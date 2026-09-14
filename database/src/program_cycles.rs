//! Read-only historical Program aggregates and conversation lineage.

use std::collections::HashSet;

use crate::{SqliteStore, StoreError};
use decodex_core::{
	ConversationId, ObjectiveId, ObjectiveState, ProgramClaimId, ProgramEvidenceId,
	ProgramEvidenceKind, ProgramId, ProgramObservationId, ProgramProposalId,
	ProgramReviewClassification, ProgramReviewId, ProgramState, WorkItemId, WorkItemState,
	contains_credential_material,
};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};

const MAX_LIST_ITEMS: usize = 32;
const MAX_TEXT_BYTES: usize = 4096;

/// Exact identity selected from the daemon-owned built-in Domain Pack registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainPackIdentity {
	pub pack_id: String,
	pub pack_version: String,
	pub pack_digest: String,
}

/// Exact immutable identity of one built-in Domain Pack bound to a Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramDomainPackBinding {
	pub pack_id: String,
	pub pack_version: String,
	pub pack_digest: String,
	pub bound_at_micros: i64,
}

/// Bounded Program selector row.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramSummaryRecord {
	pub program_id: ProgramId,
	pub name: String,
	pub purpose: String,
	pub state: ProgramState,
	pub revision: u64,
	pub updated_at_micros: i64,
}

/// Persisted Program charter.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramCharterRecord {
	pub program_id: ProgramId,
	pub name: String,
	pub purpose: String,
	pub non_goals: Vec<String>,
	pub review_policy: String,
	pub state: ProgramState,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted sourced Signal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramSignalRecord {
	pub signal_id: ProgramObservationId,
	pub program_id: ProgramId,
	pub predecessor_review_id: Option<ProgramReviewId>,
	pub source: String,
	pub summary: String,
	pub observed_at_micros: i64,
	pub created_at_micros: i64,
}

/// Persisted revisable Claim.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramClaimRecord {
	pub claim_id: ProgramClaimId,
	pub program_id: ProgramId,
	pub signal_id: ProgramObservationId,
	pub statement: String,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted non-executable Proposal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramProposalRecord {
	pub proposal_id: ProgramProposalId,
	pub program_id: ProgramId,
	pub claim_id: ProgramClaimId,
	pub summary: String,
	pub expected_effect: String,
	pub risk: String,
	pub evidence_need: String,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted finite Objective.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramObjectiveRecord {
	pub objective_id: ObjectiveId,
	pub program_id: ProgramId,
	pub proposal_id: ProgramProposalId,
	pub outcome: String,
	pub acceptance_criteria: Vec<String>,
	pub validation_criteria: Vec<String>,
	pub state: ObjectiveState,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted WorkItem and its optional ordinary Conversation binding.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramWorkItemRecord {
	pub work_item_id: WorkItemId,
	pub program_id: ProgramId,
	pub objective_id: ObjectiveId,
	pub title: String,
	pub instructions: String,
	pub working_directory: String,
	pub state: WorkItemState,
	pub revision: u64,
	pub conversation_id: Option<ConversationId>,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted validation or external Evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramEvidenceRecord {
	pub evidence_id: ProgramEvidenceId,
	pub program_id: ProgramId,
	pub work_item_id: WorkItemId,
	pub kind: ProgramEvidenceKind,
	pub source: String,
	pub summary: String,
	pub observed_at_micros: i64,
	pub created_at_micros: i64,
}

/// Persisted evidence-backed Program Review.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramReviewRecord {
	pub review_id: ProgramReviewId,
	pub program_id: ProgramId,
	pub work_item_id: WorkItemId,
	pub deterministic_evidence_id: ProgramEvidenceId,
	pub external_evidence_id: ProgramEvidenceId,
	pub classification: ProgramReviewClassification,
	pub rationale: String,
	pub created_at_micros: i64,
}

/// Complete causal projection for one Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramCycleRecord {
	pub program: ProgramCharterRecord,
	pub domain_pack: Option<ProgramDomainPackBinding>,
	pub signals: Vec<ProgramSignalRecord>,
	pub claims: Vec<ProgramClaimRecord>,
	pub proposals: Vec<ProgramProposalRecord>,
	pub objectives: Vec<ProgramObjectiveRecord>,
	pub work_items: Vec<ProgramWorkItemRecord>,
	pub evidence: Vec<ProgramEvidenceRecord>,
	pub reviews: Vec<ProgramReviewRecord>,
}

impl SqliteStore {
	/// Read one retained historical Program aggregate.
	pub async fn program_cycle(
		&self,
		program_id: &ProgramId,
	) -> Result<Option<ProgramCycleRecord>, StoreError> {
		let program_id = program_id.clone();
		self.run(move |connection| read_program_cycle(connection, &program_id)).await
	}

	/// List a bounded most-recent-first Program selector projection.
	pub async fn list_programs(
		&self,
		limit: usize,
	) -> Result<Vec<ProgramSummaryRecord>, StoreError> {
		if limit == 0 || limit > 64 {
			return Err(StoreError::InvalidInput("Program list bound must be within 1..=64"));
		}
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT program_id, name, purpose, state, revision, updated_at_micros
				 FROM programs ORDER BY updated_at_micros DESC, program_id DESC LIMIT ?1",
				)
				.map_err(sql_error)?;
			let rows = statement
				.query_map(params![i64::try_from(limit).unwrap_or(64)], |row| {
					Ok((
						row.get::<_, String>(0)?,
						row.get::<_, String>(1)?,
						row.get::<_, String>(2)?,
						row.get::<_, String>(3)?,
						row.get::<_, i64>(4)?,
						row.get::<_, i64>(5)?,
					))
				})
				.map_err(sql_error)?;
			rows.map(|row| {
				let (id, name, purpose, state, revision, updated) = row.map_err(sql_error)?;
				Ok(ProgramSummaryRecord {
					program_id: ProgramId::new(id).map_err(|_| incompatible("Program identity"))?,
					name,
					purpose,
					state: parse_program_state(&state)?,
					revision: positive_revision(revision)?,
					updated_at_micros: positive_time(updated)?,
				})
			})
			.collect()
		})
		.await
	}
}

fn validate_pack_identity(identity: &DomainPackIdentity) -> Result<(), StoreError> {
	let valid_symbol = |value: &str| {
		value.len() >= 3
			&& value.len() <= 128
			&& value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
			&& value.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
			&& value.bytes().all(|byte| {
				byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
			}) && value.contains('.')
			&& !value.contains("..")
	};
	let version_parts = identity.pack_version.split('.').collect::<Vec<_>>();
	let valid_version = version_parts.len() == 3
		&& version_parts.iter().all(|part| {
			!part.is_empty()
				&& part.bytes().all(|byte| byte.is_ascii_digit())
				&& (part == &"0" || !part.starts_with('0'))
		});
	let valid_digest = identity.pack_digest.len() == 64
		&& identity
			.pack_digest
			.bytes()
			.all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
	if !valid_symbol(&identity.pack_id) || !valid_version || !valid_digest {
		return Err(StoreError::InvalidInput("Domain Pack identity is invalid"));
	}
	Ok(())
}

fn validate_text(value: &str, limit: usize) -> Result<(), StoreError> {
	if value.is_empty()
		|| value.len() > limit
		|| value.chars().any(char::is_control)
		|| contains_credential_material(value)
	{
		return Err(if contains_credential_material(value) {
			StoreError::CredentialRejected
		} else {
			StoreError::InvalidInput("Program text is invalid")
		});
	}
	Ok(())
}

fn validate_list(values: &[String]) -> Result<(), StoreError> {
	if values.is_empty() || values.len() > MAX_LIST_ITEMS {
		return Err(StoreError::InvalidInput("Program list is invalid"));
	}
	let mut unique = HashSet::with_capacity(values.len());
	for value in values {
		validate_text(value, MAX_TEXT_BYTES)?;
		if !unique.insert(value) {
			return Err(StoreError::InvalidInput("Program list contains duplicates"));
		}
	}
	Ok(())
}

fn read_program_cycle(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Option<ProgramCycleRecord>, StoreError> {
	let program = connection
		.query_row(
			"SELECT name, purpose, non_goals_json, review_policy, state, revision,
		 created_at_micros, updated_at_micros FROM programs WHERE program_id = ?1",
			params![program_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, i64>(5)?,
					row.get::<_, i64>(6)?,
					row.get::<_, i64>(7)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some((name, purpose, non_goals, review_policy, state, revision, created, updated)) =
		program
	else {
		return Ok(None);
	};
	let program = ProgramCharterRecord {
		program_id: program_id.clone(),
		name,
		purpose,
		non_goals: decode_list(&non_goals)?,
		review_policy,
		state: parse_program_state(&state)?,
		revision: positive_revision(revision)?,
		created_at_micros: positive_time(created)?,
		updated_at_micros: positive_time(updated)?,
	};
	let domain_pack = connection
		.query_row(
			"SELECT pack_id, pack_version, pack_digest, bound_at_micros
			 FROM program_domain_pack_bindings WHERE program_id = ?1",
			params![program_id.as_str()],
			|row| {
				Ok(ProgramDomainPackBinding {
					pack_id: row.get(0)?,
					pack_version: row.get(1)?,
					pack_digest: row.get(2)?,
					bound_at_micros: row.get(3)?,
				})
			},
		)
		.optional()
		.map_err(sql_error)?;
	if let Some(binding) = &domain_pack {
		validate_persisted_pack_binding(binding)?;
	}
	let signals = read_signals(connection, program_id)?;
	let claims = read_claims(connection, program_id)?;
	let proposals = read_proposals(connection, program_id)?;
	let objectives = read_objectives(connection, program_id)?;
	let work_items = read_work_items(connection, program_id)?;
	let evidence = read_evidence(connection, program_id)?;
	let reviews = read_reviews(connection, program_id)?;
	Ok(Some(ProgramCycleRecord {
		program,
		domain_pack,
		signals,
		claims,
		proposals,
		objectives,
		work_items,
		evidence,
		reviews,
	}))
}

fn read_signals(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramSignalRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT signal_id, predecessor_review_id, source, summary, observed_at_micros,
		 created_at_micros
		 FROM program_signals WHERE program_id = ?1 ORDER BY created_at_micros, signal_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramSignalRecord {
				signal_id: ProgramObservationId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				predecessor_review_id: row
					.get::<_, Option<String>>(1)?
					.map(ProgramReviewId::new)
					.transpose()
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				source: row.get(2)?,
				summary: row.get(3)?,
				observed_at_micros: positive_time_sql(row.get(4)?)?,
				created_at_micros: positive_time_sql(row.get(5)?)?,
			})
		},
	)
}

fn read_claims(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramClaimRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT claim_id, signal_id, statement, revision, created_at_micros, updated_at_micros
		 FROM program_claims WHERE program_id = ?1 ORDER BY created_at_micros, claim_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramClaimRecord {
				claim_id: ProgramClaimId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				signal_id: ProgramObservationId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				statement: row.get(2)?,
				revision: positive_revision_sql(row.get(3)?)?,
				created_at_micros: positive_time_sql(row.get(4)?)?,
				updated_at_micros: positive_time_sql(row.get(5)?)?,
			})
		},
	)
}

fn read_proposals(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramProposalRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT proposal_id, claim_id, summary, expected_effect, risk, evidence_need,
		 revision, created_at_micros, updated_at_micros FROM program_proposals
		 WHERE program_id = ?1 ORDER BY created_at_micros, proposal_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramProposalRecord {
				proposal_id: ProgramProposalId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				claim_id: ProgramClaimId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				summary: row.get(2)?,
				expected_effect: row.get(3)?,
				risk: row.get(4)?,
				evidence_need: row.get(5)?,
				revision: positive_revision_sql(row.get(6)?)?,
				created_at_micros: positive_time_sql(row.get(7)?)?,
				updated_at_micros: positive_time_sql(row.get(8)?)?,
			})
		},
	)
}

fn read_objectives(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramObjectiveRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT objective_id, proposal_id, outcome, acceptance_criteria_json,
		 validation_criteria_json, state, revision, created_at_micros, updated_at_micros
		 FROM program_objectives WHERE program_id = ?1 ORDER BY created_at_micros, objective_id",
		program_id.as_str(),
		|row| {
			let state = row.get::<_, String>(5)?;
			Ok(ProgramObjectiveRecord {
				objective_id: ObjectiveId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				proposal_id: ProgramProposalId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				outcome: row.get(2)?,
				acceptance_criteria: decode_list_sql(&row.get::<_, String>(3)?)?,
				validation_criteria: decode_list_sql(&row.get::<_, String>(4)?)?,
				state: parse_objective_state_sql(&state)?,
				revision: positive_revision_sql(row.get(6)?)?,
				created_at_micros: positive_time_sql(row.get(7)?)?,
				updated_at_micros: positive_time_sql(row.get(8)?)?,
			})
		},
	)
}

fn read_work_items(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramWorkItemRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT item.work_item_id, item.objective_id, item.title, item.instructions,
		 item.working_directory, item.state, item.revision, execution.conversation_id,
		 item.created_at_micros, item.updated_at_micros FROM program_work_items AS item
		 LEFT JOIN program_work_item_executions AS execution USING (work_item_id)
		 WHERE item.program_id = ?1 ORDER BY item.created_at_micros, item.work_item_id",
		program_id.as_str(),
		|row| {
			let state = row.get::<_, String>(5)?;
			Ok(ProgramWorkItemRecord {
				work_item_id: WorkItemId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				objective_id: ObjectiveId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				title: row.get(2)?,
				instructions: row.get(3)?,
				working_directory: row.get(4)?,
				state: parse_work_item_state_sql(&state)?,
				revision: positive_revision_sql(row.get(6)?)?,
				conversation_id: row
					.get::<_, Option<String>>(7)?
					.map(ConversationId::new)
					.transpose()
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				created_at_micros: positive_time_sql(row.get(8)?)?,
				updated_at_micros: positive_time_sql(row.get(9)?)?,
			})
		},
	)
}

fn read_evidence(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramEvidenceRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT evidence_id, work_item_id, kind, source, summary, observed_at_micros,
		 created_at_micros FROM program_evidence WHERE program_id = ?1
		 ORDER BY created_at_micros, evidence_id",
		program_id.as_str(),
		|row| {
			let kind = row.get::<_, String>(2)?;
			Ok(ProgramEvidenceRecord {
				evidence_id: ProgramEvidenceId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				work_item_id: WorkItemId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				kind: parse_evidence_kind_sql(&kind)?,
				source: row.get(3)?,
				summary: row.get(4)?,
				observed_at_micros: positive_time_sql(row.get(5)?)?,
				created_at_micros: positive_time_sql(row.get(6)?)?,
			})
		},
	)
}

fn read_reviews(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Vec<ProgramReviewRecord>, StoreError> {
	query_rows(
		connection,
		"SELECT review_id, work_item_id, deterministic_evidence_id, external_evidence_id,
		 classification, rationale, created_at_micros FROM program_reviews
		 WHERE program_id = ?1 ORDER BY created_at_micros, review_id",
		program_id.as_str(),
		|row| {
			let classification = row.get::<_, String>(4)?;
			Ok(ProgramReviewRecord {
				review_id: ProgramReviewId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				work_item_id: WorkItemId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				deterministic_evidence_id: ProgramEvidenceId::new(row.get::<_, String>(2)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				external_evidence_id: ProgramEvidenceId::new(row.get::<_, String>(3)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				classification: parse_review_classification_sql(&classification)?,
				rationale: row.get(5)?,
				created_at_micros: positive_time_sql(row.get(6)?)?,
			})
		},
	)
}

fn validate_persisted_pack_binding(binding: &ProgramDomainPackBinding) -> Result<(), StoreError> {
	validate_pack_identity(&DomainPackIdentity {
		pack_id: binding.pack_id.clone(),
		pack_version: binding.pack_version.clone(),
		pack_digest: binding.pack_digest.clone(),
	})
	.map_err(|_| incompatible("Program Domain Pack identity"))?;
	positive_time(binding.bound_at_micros)?;
	Ok(())
}

fn query_rows<T>(
	connection: &Connection,
	sql: &str,
	program_id: &str,
	mut map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, StoreError> {
	let mut statement = connection.prepare(sql).map_err(sql_error)?;
	let rows = statement.query_map(params![program_id], |row| map(row)).map_err(sql_error)?;
	rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
}

fn decode_list(value: &str) -> Result<Vec<String>, StoreError> {
	let values: Vec<String> =
		serde_json::from_str(value).map_err(|_| incompatible("Program list"))?;
	validate_list(&values).map_err(|_| incompatible("Program list"))?;
	Ok(values)
}

fn decode_list_sql(value: &str) -> rusqlite::Result<Vec<String>> {
	decode_list(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn parse_program_state(value: &str) -> Result<ProgramState, StoreError> {
	match value {
		"active" => Ok(ProgramState::Active),
		"paused" => Ok(ProgramState::Paused),
		"retired" => Ok(ProgramState::Retired),
		_ => Err(incompatible("Program state")),
	}
}

fn parse_objective_state_sql(value: &str) -> rusqlite::Result<ObjectiveState> {
	match value {
		"active" => Ok(ObjectiveState::Active),
		"achieved" => Ok(ObjectiveState::Achieved),
		"abandoned" => Ok(ObjectiveState::Abandoned),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

pub(crate) fn parse_work_item_state_sql(value: &str) -> rusqlite::Result<WorkItemState> {
	match value {
		"ready" => Ok(WorkItemState::Ready),
		"running" => Ok(WorkItemState::Running),
		"done" => Ok(WorkItemState::Done),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn parse_evidence_kind_sql(value: &str) -> rusqlite::Result<ProgramEvidenceKind> {
	match value {
		"deterministic_validation" => Ok(ProgramEvidenceKind::DeterministicValidation),
		"external" => Ok(ProgramEvidenceKind::External),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn parse_review_classification_sql(value: &str) -> rusqlite::Result<ProgramReviewClassification> {
	match value {
		"outcome_progress" => Ok(ProgramReviewClassification::OutcomeProgress),
		"knowledge_progress" => Ok(ProgramReviewClassification::KnowledgeProgress),
		"capability_progress" => Ok(ProgramReviewClassification::CapabilityProgress),
		"no_material_change" => Ok(ProgramReviewClassification::NoMaterialChange),
		"regression" => Ok(ProgramReviewClassification::Regression),
		"unknown" => Ok(ProgramReviewClassification::Unknown),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn positive_revision(value: i64) -> Result<u64, StoreError> {
	u64::try_from(value)
		.ok()
		.filter(|value| *value > 0)
		.ok_or_else(|| incompatible("Program revision"))
}

fn positive_revision_sql(value: i64) -> rusqlite::Result<u64> {
	positive_revision(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn positive_time(value: i64) -> Result<i64, StoreError> {
	(value > 0).then_some(value).ok_or_else(|| incompatible("Program timestamp"))
}

fn positive_time_sql(value: i64) -> rusqlite::Result<i64> {
	positive_time(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn sql_error(_error: rusqlite::Error) -> StoreError {
	StoreError::Database(crate::DatabaseError::Unavailable)
}

fn incompatible(subject: &str) -> StoreError {
	StoreError::Incompatible(subject.to_owned())
}

#[cfg(test)]
mod historical_tests {
	use crate::{OrdinaryTaskConversationProjection, SqliteStore};
	use decodex_core::{ConversationId, DecodexRoot, ProgramId, ProgramState, WorkItemState};

	#[tokio::test]
	async fn historical_program_reads_preserve_review_lineage_and_conversation_binding_after_reopen()
	 {
		let directory = tempfile::tempdir().expect("temporary database");
		let root = DecodexRoot::new(directory.path().canonicalize().expect("absolute path"))
			.expect("root");
		let store = SqliteStore::open(&root.paths()).expect("store");
		store
			.run(|connection| {
				connection
					.execute_batch(include_str!("../tests/fixtures/historical_program.sql"))
					.expect("released schema fixture");
				Ok(())
			})
			.await
			.expect("fixture installed");
		let id = ProgramId::new("10000000-0000-4000-8000-000000000001").expect("program");
		let record =
			store.program_cycle(&id).await.expect("historical query").expect("retained program");
		assert_eq!(record.program.state, ProgramState::Retired);
		assert_eq!(record.signals.len(), 2);
		assert_eq!(
			record.signals[1].predecessor_review_id.as_ref(),
			Some(&record.reviews[0].review_id)
		);
		assert_eq!(record.reviews[0].rationale, "Historical rationale");
		assert_eq!(record.evidence.len(), 2);
		assert_eq!(record.work_items[0].state, WorkItemState::Done);
		assert_eq!(record.domain_pack.as_ref().expect("pack binding").pack_digest, "a".repeat(64));
		let conversation =
			ConversationId::new("10000000-0000-4000-8000-000000000010").expect("conversation");
		assert_eq!(record.work_items[0].conversation_id.as_ref(), Some(&conversation));
		let projections = store
			.read_ordinary_task_conversations(Some(&conversation), None, 1)
			.await
			.expect("historical conversation");
		let OrdinaryTaskConversationProjection::Current(projection) = &projections[0] else {
			panic!("readable historical conversation");
		};
		assert_eq!(projection.title, "Historical work title");
		let context = projection.program_work_item.as_ref().expect("historical lineage");
		assert_eq!(context.program_id, id);
		assert_eq!(context.instructions, "Historical instructions");
		let listing = store.list_programs(64).await.expect("historical list");
		assert_eq!(listing.len(), 1);
		assert!(store.list_programs(0).await.is_err());
		let missing =
			ProgramId::new("10000000-0000-4000-8000-000000000099").expect("absent program");
		assert_eq!(store.program_cycle(&missing).await.expect("absent read"), None);
		store.close();
		let reopened = SqliteStore::open(&root.paths()).expect("reopened schema");
		assert_eq!(
			reopened.program_cycle(&id).await.expect("reopened historical read"),
			Some(record)
		);
		assert_eq!(reopened.list_programs(64).await.expect("reopened list"), listing);
		assert_eq!(
			reopened
				.read_ordinary_task_conversations(Some(&conversation), None, 1)
				.await
				.expect("reopened lineage"),
			projections
		);
	}
}
