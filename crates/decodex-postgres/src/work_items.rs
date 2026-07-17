//! Exact PostgreSQL WorkItem commands and non-transferable stored projections.

use serde_json::Value;
use tokio_postgres::Row;

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};
use decodex_core::{
	AgentId, ObjectiveId, ProgramId, ProjectId, WorkItem, WorkItemCorrelationId, WorkItemEdge,
	WorkItemEdgeKind, WorkItemId, WorkItemPriority, WorkItemProgramRef, WorkItemProvenance,
	WorkItemState, WorkItemTimestamp,
};

const WORK_ITEM_DOCUMENT_SELECT: &str = r#"
SELECT pg_catalog.jsonb_build_object(
	'work_item_id', item.work_item_id, 'project_id', item.project_id,
	'lead_agent_id', item.lead_agent_id, 'program_id', item.program_id,
	'objective_ids', COALESCE((SELECT pg_catalog.jsonb_agg(link.objective_id ORDER BY link.objective_id)
		FROM decodex.work_item_objectives AS link WHERE link.work_item_id=item.work_item_id),'[]'::jsonb),
	'edges', COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
		'kind',edge.kind,'related_work_item_id',edge.related_work_item_id
	) ORDER BY edge.related_work_item_id) FROM decodex.work_item_edges AS edge
		WHERE edge.work_item_id=item.work_item_id),'[]'::jsonb),
	'title',item.title,'description',item.description,'priority',item.priority,
	'acceptance_criteria',item.acceptance_criteria,'validation_criteria',item.validation_criteria,
	'state',item.state,'revision',item.revision,'last_changed_by',item.last_changed_by,
	'last_correlation_id',item.last_correlation_id,'last_provenance',item.last_provenance,
	'created_at_microseconds',(EXTRACT(EPOCH FROM item.created_at)*1000000)::bigint,
	'updated_at_microseconds',(EXTRACT(EPOCH FROM item.updated_at)*1000000)::bigint,
	'accepted_revision',(SELECT pg_catalog.max(acceptance.work_item_revision)
		FROM decodex.work_item_acceptances AS acceptance
		WHERE acceptance.work_item_id=item.work_item_id)
) FROM decodex.work_items AS item WHERE item.work_item_id=$1::pg_catalog.text::pg_catalog.uuid
"#;

/// Normalized same-Project relations supplied to a create or update command.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkItemRelations {
	/// Same-Project Objective identities.
	pub objective_ids: Vec<ObjectiveId>,
	/// Related WorkItems that must be done.
	pub depends_on_ids: Vec<WorkItemId>,
	/// Related WorkItems that must be terminal.
	pub blocked_by_ids: Vec<WorkItemId>,
}

/// Complete caller-selected input for revision-one WorkItem creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateWorkItem {
	/// Caller-selected canonical WorkItem identity.
	pub work_item_id: WorkItemId,
	/// Project that owns the WorkItem.
	pub project_id: ProjectId,
	/// Canonical Lead responsible for the WorkItem.
	pub lead_agent_id: AgentId,
	/// Optional same-Project Program association.
	pub program_id: Option<ProgramId>,
	/// Normalized same-Project relations.
	pub relations: WorkItemRelations,
	/// Human-readable WorkItem title.
	pub title: String,
	/// Bounded WorkItem description.
	pub description: String,
	/// Initial canonical priority.
	pub priority: WorkItemPriority,
	/// Criteria the Lead will later accept against an exact revision.
	pub acceptance_criteria: Vec<String>,
	/// Criteria used to validate the exact revision.
	pub validation_criteria: Vec<String>,
	/// Actor, correlation, and summary provenance for creation.
	pub provenance: WorkItemProvenance,
}

/// Complete optimistic replacement of mutable WorkItem structure and ordinary lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateWorkItem {
	/// Canonical identity of the WorkItem to replace.
	pub work_item_id: WorkItemId,
	/// Owning Project identity.
	pub project_id: ProjectId,
	/// Revision that must still be current.
	pub expected_revision: u64,
	/// Replacement optional same-Project Program association.
	pub program_id: Option<ProgramId>,
	/// Replacement normalized same-Project relations.
	pub relations: WorkItemRelations,
	/// Replacement title.
	pub title: String,
	/// Replacement bounded description.
	pub description: String,
	/// Replacement canonical priority.
	pub priority: WorkItemPriority,
	/// Replacement acceptance criteria.
	pub acceptance_criteria: Vec<String>,
	/// Replacement validation criteria.
	pub validation_criteria: Vec<String>,
	/// Requested ordinary lifecycle state.
	pub target_state: WorkItemState,
	/// Actor, correlation, and summary provenance for the update.
	pub provenance: WorkItemProvenance,
}

/// Exact-revision Lead acceptance input. PostgreSQL snapshots the criteria and authors time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptWorkItem {
	/// Immutable canonical acceptance identity.
	pub acceptance_id: String,
	/// Canonical WorkItem identity.
	pub work_item_id: WorkItemId,
	/// Owning Project identity.
	pub project_id: ProjectId,
	/// Exact review revision being accepted.
	pub expected_revision: u64,
	/// Canonical Lead and correlation provenance.
	pub provenance: WorkItemProvenance,
	/// Provenance for the snapshotted criteria.
	pub criteria_provenance: String,
	/// Bounded summary of the acceptance evidence.
	pub evidence_summary: String,
	/// Provenance for the acceptance evidence.
	pub evidence_provenance: String,
}

/// Persisted WorkItem with normalized edges and a non-authoritative acceptance observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredWorkItem {
	/// Canonical WorkItem projection.
	pub work_item: WorkItem,
	/// Normalized typed dependency and blocker edges.
	pub edges: Vec<WorkItemEdge>,
	/// Latest accepted exact revision, if any. This is readback, not a completion permit.
	pub accepted_revision: Option<u64>,
}

/// Typed current-state blocker persisted by an exact readiness transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkItemReadinessBlockerKind {
	/// The owning Project is not active.
	ProjectInactive,
	/// The canonical Lead is not active in the Project.
	LeadInactive,
	/// The associated Program is not active.
	ProgramInactive,
	/// An associated Objective is not active.
	ObjectiveInactive,
	/// A dependency has not reached `done`.
	DependencyIncomplete,
	/// A blocker has not reached a terminal state.
	BlockerActive,
	/// The current dependency graph contains a cycle.
	DependencyCycle,
}

/// Inspectable blocker fact. It is not reusable transition authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemReadinessBlocker {
	/// Typed reason readiness was denied.
	pub kind: WorkItemReadinessBlockerKind,
	/// Related authority identity, when the blocker has one.
	pub subject_id: Option<String>,
	/// Current state observed by the readiness transaction.
	pub observed_state: String,
}

/// Complete committed exact-command effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemCommandEffect {
	/// Committed canonical WorkItem projection.
	pub work_item: StoredWorkItem,
	/// Current blockers recorded by readiness, otherwise empty.
	pub readiness_blockers: Vec<WorkItemReadinessBlocker>,
	/// Append-only activity sequence emitted by the command.
	pub activity_sequence: i64,
	/// Exact credential-negative activity payload.
	pub activity_payload: Value,
	/// Transactional outbox row identity.
	pub outbox_id: i64,
	/// Exact credential-negative outbox payload.
	pub outbox_payload: Value,
}

/// Stable WorkItem domain rejection committed into the exact receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkItemRejection {
	/// The target WorkItem does not exist.
	MissingTarget,
	/// Creation targeted an existing WorkItem.
	DuplicateTarget,
	/// The expected revision is no longer current.
	StaleRevision,
	/// The requested lifecycle transition is not allowed.
	IllegalTransition,
	/// Current Project, Lead, or association authority is invalid.
	InvalidAuthority,
	/// A bounded or typed command value is invalid.
	InvalidInput,
	/// A relation is not valid for the owning Project.
	InvalidRelation,
	/// A normalized relation was supplied more than once.
	DuplicateRelation,
	/// The candidate dependency graph contains a cycle.
	DependencyCycle,
	/// The exact revision already has an acceptance.
	DuplicateAcceptance,
}

/// Exact command outcome parsed from PostgreSQL-owned immutable response bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkItemCommandOutcome {
	/// Committed exact-command effect.
	Success(Box<WorkItemCommandEffect>),
	/// Stable domain rejection committed into the exact receipt.
	Rejected(WorkItemRejection),
}

impl PostgresStore {
	/// Create one canonical inbox WorkItem through the command-complete V11 owner.
	pub async fn create_work_item(
		&self,
		idempotency_key: &str,
		create: &CreateWorkItem,
	) -> Result<WorkItemCommandOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		let relations = relation_parameters(&create.relations);
		let program_id = create.program_id.as_ref().map(ToString::to_string);
		let priority = create.priority.as_str();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.create_work_item_exact($1,$2,$3::text::uuid,$4::text::uuid,\
				 $5::text::uuid,$6::text::uuid,$7::text[]::uuid[],$8::text[]::uuid[],\
				 $9::text[]::uuid[],$10,$11,$12::text::decodex.work_item_priority,\
				 $13,$14,$15::text::uuid,$16::text::uuid,$17)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&create.work_item_id.as_str(),
					&create.project_id.as_str(),
					&create.lead_agent_id.as_str(),
					&program_id,
					&relations.objectives,
					&relations.depends_on,
					&relations.blocked_by,
					&create.title,
					&create.description,
					&priority,
					&create.acceptance_criteria,
					&create.validation_criteria,
					&create.provenance.actor_id().as_str(),
					&create.provenance.correlation_id().as_str(),
					&create.provenance.summary(),
				],
			)
			.await?;
		parse_command_response(&response)
	}

	/// Replace mutable WorkItem structure and perform one ordinary lifecycle transition.
	pub async fn update_work_item(
		&self,
		idempotency_key: &str,
		update: &UpdateWorkItem,
	) -> Result<WorkItemCommandOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		let expected_revision = positive_revision(update.expected_revision)?;
		let relations = relation_parameters(&update.relations);
		let program_id = update.program_id.as_ref().map(ToString::to_string);
		let priority = update.priority.as_str();
		let target_state = update.target_state.as_str();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.update_work_item_exact($1,$2,$3::text::uuid,$4::text::uuid,$5,\
				 $6::text::uuid,$7::text[]::uuid[],$8::text[]::uuid[],$9::text[]::uuid[],\
				 $10,$11,$12::text::decodex.work_item_priority,$13,$14,\
				 $15::text::decodex.work_item_state,$16::text::uuid,$17::text::uuid,$18)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&update.work_item_id.as_str(),
					&update.project_id.as_str(),
					&expected_revision,
					&program_id,
					&relations.objectives,
					&relations.depends_on,
					&relations.blocked_by,
					&update.title,
					&update.description,
					&priority,
					&update.acceptance_criteria,
					&update.validation_criteria,
					&target_state,
					&update.provenance.actor_id().as_str(),
					&update.provenance.correlation_id().as_str(),
					&update.provenance.summary(),
				],
			)
			.await?;
		parse_command_response(&response)
	}

	/// Re-read every readiness input and atomically persist blockers or enter `ready`.
	pub async fn assess_work_item_readiness(
		&self,
		idempotency_key: &str,
		work_item_id: &WorkItemId,
		project_id: &ProjectId,
		expected_revision: u64,
		provenance: &WorkItemProvenance,
	) -> Result<WorkItemCommandOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		let expected_revision = positive_revision(expected_revision)?;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.assess_work_item_readiness_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,$7::text::uuid,$8)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&work_item_id.as_str(),
					&project_id.as_str(),
					&expected_revision,
					&provenance.actor_id().as_str(),
					&provenance.correlation_id().as_str(),
					&provenance.summary(),
				],
			)
			.await?;
		parse_command_response(&response)
	}

	/// Persist immutable canonical-Lead acceptance without changing WorkItem lifecycle or revision.
	pub async fn accept_work_item(
		&self,
		idempotency_key: &str,
		acceptance: &AcceptWorkItem,
	) -> Result<WorkItemCommandOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&acceptance.acceptance_id, "invalid WorkItem acceptance identity")?;
		let expected_revision = positive_revision(acceptance.expected_revision)?;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.accept_work_item_exact($1,$2,$3::text::uuid,$4::text::uuid,\
				 $5::text::uuid,$6,$7::text::uuid,$8::text::uuid,$9,$10,$11,$12)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&acceptance.acceptance_id,
					&acceptance.work_item_id.as_str(),
					&acceptance.project_id.as_str(),
					&expected_revision,
					&acceptance.provenance.actor_id().as_str(),
					&acceptance.provenance.correlation_id().as_str(),
					&acceptance.provenance.summary(),
					&acceptance.criteria_provenance,
					&acceptance.evidence_summary,
					&acceptance.evidence_provenance,
				],
			)
			.await?;
		parse_command_response(&response)
	}

	/// Read one current canonical WorkItem projection.
	pub async fn read_work_item(
		&self,
		work_item_id: &WorkItemId,
	) -> Result<Option<StoredWorkItem>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		client
			.query_opt(WORK_ITEM_DOCUMENT_SELECT, &[&work_item_id.as_str()])
			.await?
			.map(document_from_row)
			.transpose()
	}

	/// Query one deterministic bounded Project page ordered by WorkItem identity.
	pub async fn query_work_items(
		&self,
		project_id: &ProjectId,
		state: Option<WorkItemState>,
		after: Option<&WorkItemId>,
		limit: usize,
	) -> Result<Vec<StoredWorkItem>, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput("WorkItem query limit must be 1..=256"));
		}
		let state = state.map(|value| value.as_str());
		let after = after.map(WorkItemId::as_str);
		let limit = i64::try_from(limit)
			.map_err(|_| StoreError::InvalidInput("WorkItem query limit is invalid"))?;
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let statement = WORK_ITEM_DOCUMENT_SELECT.replace(
			"WHERE item.work_item_id=$1::pg_catalog.text::pg_catalog.uuid",
			"WHERE item.project_id=$1::pg_catalog.text::pg_catalog.uuid \
			 AND ($2::text IS NULL OR item.state=$2::text::decodex.work_item_state) \
			 AND ($3::text IS NULL OR item.work_item_id>$3::text::uuid) \
			 ORDER BY item.work_item_id LIMIT $4",
		);
		let rows =
			client.query(&statement, &[&project_id.as_str(), &state, &after, &limit]).await?;
		rows.into_iter().map(document_from_row).collect()
	}

	/// Inert future running/resume check. The transaction returns no permit or authority object.
	pub async fn guard_work_item_running_resume(
		&self,
		work_item_id: &WorkItemId,
		project_id: &ProjectId,
		expected_revision: u64,
	) -> Result<(), StoreError> {
		let expected_revision = positive_revision(expected_revision)?;
		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		transaction
			.query_one(
				"SELECT decodex.guard_work_item_running_resume($1::text::uuid,$2::text::uuid,$3)",
				&[&work_item_id.as_str(), &project_id.as_str(), &expected_revision],
			)
			.await?;
		transaction.commit().await?;
		Ok(())
	}
}

struct RelationParameters {
	objectives: Vec<String>,
	depends_on: Vec<String>,
	blocked_by: Vec<String>,
}

fn relation_parameters(relations: &WorkItemRelations) -> RelationParameters {
	let mut objectives =
		relations.objective_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
	let mut depends_on =
		relations.depends_on_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
	let mut blocked_by =
		relations.blocked_by_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
	objectives.sort();
	depends_on.sort();
	blocked_by.sort();
	RelationParameters { objectives, depends_on, blocked_by }
}

fn parse_command_response(response: &[u8]) -> Result<WorkItemCommandOutcome, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact WorkItem response bytes are invalid".into())
	})?;
	match document.get("classification").and_then(Value::as_str) {
		Some("stable_domain_rejection") => {
			return rejection_from_document(&document).map(WorkItemCommandOutcome::Rejected);
		},
		Some("success") => {},
		_ => {
			return Err(StoreError::Incompatible("exact WorkItem classification is invalid".into()));
		},
	}
	let effect = required_value(&document, "effect")?;
	let work_item = stored_from_value(required_value(effect, "work_item")?)?;
	let readiness_blockers =
		effect.get("readiness_blockers").map(blockers_from_value).transpose()?.unwrap_or_default();
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;
	let activity_payload = required_value(effect, "activity_payload")?.clone();
	let outbox_payload = required_value(effect, "outbox_payload")?.clone();
	let event_kind = outbox_payload.get("event_kind").and_then(Value::as_str);
	if activity_payload.get("kind").and_then(Value::as_str) != Some("work_item")
		|| activity_payload.get("work_item") != effect.get("work_item")
		|| outbox_payload.get("activity_sequence").and_then(Value::as_i64)
			!= Some(activity_sequence)
		|| outbox_payload.get("aggregate_kind").and_then(Value::as_str) != Some("work_item")
		|| outbox_payload.get("aggregate_id").and_then(Value::as_str)
			!= Some(work_item.work_item.id().as_str())
		|| outbox_payload.get("revision").and_then(Value::as_i64)
			!= i64::try_from(work_item.work_item.revision()).ok()
		|| outbox_payload.get("payload") != Some(&activity_payload)
		|| !matches!(event_kind, Some("work_item_created" | "work_item_updated"
			| "work_item_readiness_blocked" | "work_item_ready" | "work_item_accepted"))
	{
		return Err(StoreError::Incompatible("exact WorkItem audit effect is inconsistent".into()));
	}
	Ok(WorkItemCommandOutcome::Success(Box::new(WorkItemCommandEffect {
		work_item,
		readiness_blockers,
		activity_sequence,
		activity_payload,
		outbox_id,
		outbox_payload,
	})))
}

fn document_from_row(row: Row) -> Result<StoredWorkItem, StoreError> {
	stored_from_value(&row.get::<_, Value>(0))
}

fn stored_from_value(value: &Value) -> Result<StoredWorkItem, StoreError> {
	let work_item_id = WorkItemId::new(required_str(value, "work_item_id")?).map_err(domain)?;
	let project_id = ProjectId::new(required_str(value, "project_id")?).map_err(domain)?;
	let lead_agent_id = AgentId::new(required_str(value, "lead_agent_id")?).map_err(domain)?;
	let program = optional_str(value, "program_id")?
		.map(|id| ProgramId::new(id).map(|id| WorkItemProgramRef::new(id, project_id.clone())))
		.transpose()
		.map_err(domain)?;
	let objective_ids = required_array(value, "objective_ids")?;
	let objectives = objective_ids
		.iter()
		.map(|id| -> Result<_, StoreError> {
			let id = id.as_str().ok_or_else(incompatible)?;
			let id = ObjectiveId::new(id).map_err(domain)?;
			Ok(decodex_core::WorkItemObjectiveRef::new(id, project_id.clone()))
		})
		.collect::<Result<Vec<_>, _>>()?;
	let provenance = WorkItemProvenance::new(
		AgentId::new(required_str(value, "last_changed_by")?).map_err(domain)?,
		WorkItemCorrelationId::new(required_str(value, "last_correlation_id")?).map_err(domain)?,
		required_str(value, "last_provenance")?,
	)
	.map_err(domain)?;
	let work_item = WorkItem::from_stored(
		work_item_id.clone(),
		project_id.clone(),
		lead_agent_id,
		program,
		objectives,
		required_str(value, "title")?.to_owned(),
		required_str(value, "description")?.to_owned(),
		priority_from_sql(required_str(value, "priority")?)?,
		string_array(value, "acceptance_criteria")?,
		string_array(value, "validation_criteria")?,
		state_from_sql(required_str(value, "state")?)?,
		positive_u64(value, "revision")?,
		WorkItemTimestamp::from_unix_microseconds(required_i64(value, "created_at_microseconds")?)
			.map_err(domain)?,
		WorkItemTimestamp::from_unix_microseconds(required_i64(value, "updated_at_microseconds")?)
			.map_err(domain)?,
		provenance,
	)
	.map_err(domain)?;
	let edges = required_array(value, "edges")?
		.iter()
		.map(|edge| -> Result<_, StoreError> {
			WorkItemEdge::new(
				edge_kind_from_sql(required_str(edge, "kind")?)?,
				work_item_id.clone(),
				project_id.clone(),
				WorkItemId::new(required_str(edge, "related_work_item_id")?).map_err(domain)?,
				project_id.clone(),
			)
			.map_err(domain)
		})
		.collect::<Result<Vec<_>, _>>()?;
	let accepted_revision = optional_positive_u64(value, "accepted_revision")?;
	if accepted_revision.is_some_and(|revision| revision > work_item.revision()) {
		return Err(incompatible());
	}
	Ok(StoredWorkItem { work_item, edges, accepted_revision })
}

fn blockers_from_value(value: &Value) -> Result<Vec<WorkItemReadinessBlocker>, StoreError> {
	value
		.as_array()
		.ok_or_else(incompatible)?
		.iter()
		.map(|value| {
			Ok(WorkItemReadinessBlocker {
				kind: blocker_kind_from_sql(required_str(value, "kind")?)?,
				subject_id: optional_str(value, "subject_id")?
					.map(|id| WorkItemId::new(id).map(|id| id.to_string()).map_err(domain))
					.transpose()?,
				observed_state: required_str(value, "observed_state")?.to_owned(),
			})
		})
		.collect()
}

fn rejection_from_document(value: &Value) -> Result<WorkItemRejection, StoreError> {
	let code = value.get("code").and_then(Value::as_str);
	let effect = required_value(value, "effect")?;
	if effect.get("changed").and_then(Value::as_bool) != Some(false)
		|| effect.get("code").and_then(Value::as_str) != code
	{
		return Err(StoreError::Incompatible("exact WorkItem rejection effect is inconsistent".into()));
	}
	match code {
		Some("missing_target") => Ok(WorkItemRejection::MissingTarget),
		Some("duplicate_target") => Ok(WorkItemRejection::DuplicateTarget),
		Some("stale_revision") => Ok(WorkItemRejection::StaleRevision),
		Some("illegal_transition") => Ok(WorkItemRejection::IllegalTransition),
		Some("invalid_authority") => Ok(WorkItemRejection::InvalidAuthority),
		Some("invalid_input") => Ok(WorkItemRejection::InvalidInput),
		Some("invalid_relation") => Ok(WorkItemRejection::InvalidRelation),
		Some("duplicate_relation") => Ok(WorkItemRejection::DuplicateRelation),
		Some("dependency_cycle") => Ok(WorkItemRejection::DependencyCycle),
		Some("duplicate_acceptance") => Ok(WorkItemRejection::DuplicateAcceptance),
		_ => Err(StoreError::Incompatible("exact WorkItem rejection code is invalid".into())),
	}
}

fn priority_from_sql(value: &str) -> Result<WorkItemPriority, StoreError> {
	match value {
		"urgent" => Ok(WorkItemPriority::Urgent),
		"high" => Ok(WorkItemPriority::High),
		"medium" => Ok(WorkItemPriority::Medium),
		"low" => Ok(WorkItemPriority::Low),
		"none" => Ok(WorkItemPriority::None),
		_ => Err(incompatible()),
	}
}

fn state_from_sql(value: &str) -> Result<WorkItemState, StoreError> {
	match value {
		"inbox" => Ok(WorkItemState::Inbox),
		"planned" => Ok(WorkItemState::Planned),
		"ready" => Ok(WorkItemState::Ready),
		"running" => Ok(WorkItemState::Running),
		"review" => Ok(WorkItemState::Review),
		"blocked" => Ok(WorkItemState::Blocked),
		"done" => Ok(WorkItemState::Done),
		"canceled" => Ok(WorkItemState::Canceled),
		_ => Err(incompatible()),
	}
}

fn edge_kind_from_sql(value: &str) -> Result<WorkItemEdgeKind, StoreError> {
	match value {
		"depends_on" => Ok(WorkItemEdgeKind::DependsOn),
		"blocked_by" => Ok(WorkItemEdgeKind::BlockedBy),
		_ => Err(incompatible()),
	}
}

fn blocker_kind_from_sql(value: &str) -> Result<WorkItemReadinessBlockerKind, StoreError> {
	match value {
		"project_inactive" => Ok(WorkItemReadinessBlockerKind::ProjectInactive),
		"lead_inactive" => Ok(WorkItemReadinessBlockerKind::LeadInactive),
		"program_inactive" => Ok(WorkItemReadinessBlockerKind::ProgramInactive),
		"objective_inactive" => Ok(WorkItemReadinessBlockerKind::ObjectiveInactive),
		"dependency_incomplete" => Ok(WorkItemReadinessBlockerKind::DependencyIncomplete),
		"blocker_active" => Ok(WorkItemReadinessBlockerKind::BlockerActive),
		"dependency_cycle" => Ok(WorkItemReadinessBlockerKind::DependencyCycle),
		_ => Err(incompatible()),
	}
}

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value.get(key).ok_or_else(incompatible)
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value.get(key).and_then(Value::as_str).ok_or_else(incompatible)
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value.clone())),
		_ => Err(incompatible()),
	}
}

fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value.get(key).and_then(Value::as_array).map(Vec::as_slice).ok_or_else(incompatible)
}

fn string_array(value: &Value, key: &str) -> Result<Vec<String>, StoreError> {
	required_array(value, key)?
		.iter()
		.map(|value| value.as_str().map(ToOwned::to_owned).ok_or_else(incompatible))
		.collect()
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value.get(key).and_then(Value::as_i64).ok_or_else(incompatible)
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let value = required_i64(value, key)?;
	if value > 0 { Ok(value) } else { Err(incompatible()) }
}

fn positive_u64(value: &Value, key: &str) -> Result<u64, StoreError> {
	u64::try_from(positive_i64(value, key)?).map_err(|_| incompatible())
}

fn optional_positive_u64(value: &Value, key: &str) -> Result<Option<u64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(_) => positive_u64(value, key).map(Some),
		None => Err(incompatible()),
	}
}

fn positive_revision(value: u64) -> Result<i64, StoreError> {
	i64::try_from(value)
		.ok()
		.filter(|value| *value > 0)
		.ok_or(StoreError::InvalidInput("WorkItem revision must be positive and fit bigint"))
}

fn validate_uuid(value: &str, message: &'static str) -> Result<(), StoreError> {
	if WorkItemId::new(value).is_ok() { Ok(()) } else { Err(StoreError::InvalidInput(message)) }
}

fn incompatible() -> StoreError {
	StoreError::Incompatible("stored WorkItem projection is invalid".into())
}

fn domain(error: impl std::fmt::Display) -> StoreError {
	StoreError::Incompatible(format!("stored WorkItem domain is invalid: {error}"))
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::{WorkItemReadinessBlockerKind, blockers_from_value, stored_from_value};

	#[test]
	fn stored_projection_reconstructs_canonical_work_item() {
		let value = json!({
			"work_item_id":"51000000-0000-4000-8000-000000000001",
			"project_id":"21000000-0000-4000-8000-000000000001",
			"lead_agent_id":"31000000-0000-4000-8000-000000000001",
			"program_id":null,"objective_ids":[],"edges":[],"title":"Persist WorkItems",
			"description":"Exact PostgreSQL authority.","priority":"high",
			"acceptance_criteria":["Stored"],"validation_criteria":["Validated"],
			"state":"inbox","revision":1,
			"last_changed_by":"31000000-0000-4000-8000-000000000001",
			"last_correlation_id":"61000000-0000-4000-8000-000000000001",
			"last_provenance":"XY-1343","created_at_microseconds":1,
			"updated_at_microseconds":1,"accepted_revision":null
		});
		let stored = stored_from_value(&value).unwrap();
		assert_eq!(stored.work_item.revision(), 1);
		assert_eq!(stored.accepted_revision, None);
	}

	#[test]
	fn readiness_blockers_are_typed_and_order_preserving() {
		let blockers = blockers_from_value(&json!([{
			"kind":"dependency_incomplete",
			"subject_id":"51000000-0000-4000-8000-000000000002",
			"observed_state":"planned"
		}]))
		.unwrap();
		assert_eq!(blockers[0].kind, WorkItemReadinessBlockerKind::DependencyIncomplete);
	}
}
