use std::collections::BTreeSet;

use decodex_core::{
	AccountId, AccountState, CodexCapability, ExecutionConsumer, RoutingBlocker,
	RoutingCapabilityState, RoutingCommandOutcome, RoutingEvidenceEffect, RoutingMemberDisposition,
	RoutingPolicyEffect, RoutingPolicyMember, RoutingRejection, RoutingSnapshot,
	RoutingSnapshotCapabilityFact, RoutingSnapshotMember, RoutingSnapshotQuotaFact,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	PostgresStore, RoleProfileRole, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

const RESOLVE_ROUTING_SNAPSHOT_SQL: &str = "SELECT decodex.resolve_routing_snapshot_exact(\
	 $1,$2,$3::text::decodex.routing_authority_shape,$4::text::uuid,$5,$6,\
	 $7::text::decodex.provider_attempt_consumer_kind,$8::text::uuid,$9,\
	 $10::text::uuid,$11,$12::text::uuid,$13::text::uuid,$14,$15::text::uuid)";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_routing_decision_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	client.prepare(RESOLVE_ROUTING_SNAPSHOT_SQL).await?;
	Ok(1)
}

/// One explicit member supplied for complete policy replacement.
///
/// Constructing this input does not establish PostgreSQL provenance or routing authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingPolicyMemberInput {
	/// Canonical account identity that PostgreSQL must find in the complete project inventory.
	pub account_id: AccountId,
	/// Positive account revision that PostgreSQL must match before accepting the replacement.
	pub account_revision: i64,
	/// Explicit included or excluded policy state; omission is not an exclusion signal.
	pub disposition: RoutingMemberDisposition,
}

/// User-authored complete replacement request; PostgreSQL rechecks the inventory and sources.
///
/// This value represents command input only; constructing it does not prove database authorship
/// or authorize account switching, dispatch, continuation, or production routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceRoutingPolicy {
	/// Stable UUID of the routing policy to create or replace.
	pub routing_policy_id: String,
	/// Stable UUID of the project whose complete account inventory the policy must cover.
	pub project_id: String,
	/// Current positive policy revision to replace, or absence only when creating revision one.
	pub expected_revision: Option<i64>,
	/// UUID of the accepted project Policy from which this routing policy derives authority.
	pub accepted_policy_id: String,
	/// Positive, exact revision of the accepted project Policy used by this replacement.
	pub accepted_policy_revision: i64,
	/// Role that every eligible account's current RoleProfile must provide.
	pub required_role: RoleProfileRole,
	/// Positive, exact RoleProfile revision required for the specified role.
	pub required_role_profile_revision: i64,
	/// Exact canonical Codex build identity required by the policy.
	pub required_build_id: String,
	/// Complete replacement membership, with one revisioned entry for every project account.
	pub members: Vec<RoutingPolicyMemberInput>,
	/// Canonically ordered, duplicate-free set of capabilities required from every eligible
	/// member.
	pub required_capabilities: Vec<CodexCapability>,
}

/// Complete ordinary XY-1270 compatibility observation without any caller clock.
///
/// PostgreSQL supplies the ingestion time and verifies the linked authorities. Public Rust
/// construction represents an input only and does not itself prove that provenance or authorize
/// account switching, dispatch, continuation, or production routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishRoutingEvidence {
	/// UUID assigned to this immutable evidence publication.
	pub evidence_id: String,
	/// Canonical account identity observed by the evidence publication.
	pub account_id: AccountId,
	/// Positive account revision that PostgreSQL must still observe when publishing the evidence.
	pub expected_account_revision: i64,
	/// Current positive evidence revision to advance, or absence only for the first observation.
	pub expected_evidence_revision: Option<i64>,
	/// Observed RoleProfile role, checked as part of the complete compatibility fact.
	pub role: RoleProfileRole,
	/// Positive, exact revision of the observed RoleProfile.
	pub role_profile_revision: i64,
	/// Exact canonical Codex build identity observed for the process.
	pub build_id: String,
	/// UUID of the observed Codex process, present with the account and schema process facts.
	pub process_id: String,
	/// Account bound to the observed process; it must exactly equal `account_id`.
	pub process_account_id: AccountId,
	/// Exact lowercase hexadecimal schema digest observed with the same process facts.
	pub schema_fingerprint: String,
	/// Complete canonically ordered state vector containing every closed Codex capability exactly
	/// once.
	pub capabilities: Vec<(CodexCapability, RoutingCapabilityState)>,
}

impl PostgresStore {
	/// Replace the complete routing policy through the Routing Snapshot command owner.
	pub async fn replace_routing_policy(
		&self,
		idempotency_key: &str,
		request: &ReplaceRoutingPolicy,
	) -> Result<RoutingCommandOutcome<RoutingPolicyEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_policy_request(request)?;
		let account_ids = request
			.members
			.iter()
			.map(|member| member.account_id.as_str().to_owned())
			.collect::<Vec<_>>();
		let account_revisions =
			request.members.iter().map(|member| member.account_revision).collect::<Vec<_>>();
		let dispositions = request
			.members
			.iter()
			.map(|member| disposition_sql(member.disposition).to_owned())
			.collect::<Vec<_>>();
		let capabilities = request
			.required_capabilities
			.iter()
			.map(|capability| capability.as_sql().to_owned())
			.collect::<Vec<_>>();
		let role = request.required_role.as_sql();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.replace_routing_policy_exact($1,$2,$3::text::uuid,$4::text::uuid,\
				 $5,$6::text::uuid,$7,$8::text::decodex.role_profile_role,$9,$10,\
				 $11::text[]::uuid[],$12,$13::text[]::decodex.routing_member_disposition[],\
				 $14::text[]::decodex.codex_capability[])",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.routing_policy_id,
					&request.project_id,
					&request.expected_revision,
					&request.accepted_policy_id,
					&request.accepted_policy_revision,
					&role,
					&request.required_role_profile_revision,
					&request.required_build_id,
					&account_ids,
					&account_revisions,
					&dispositions,
					&capabilities,
				],
			)
			.await?;
		parse_policy_response(&response, request)
	}

	/// Publish one database-timestamped immutable compatibility/capability observation.
	pub async fn publish_routing_evidence(
		&self,
		idempotency_key: &str,
		request: &PublishRoutingEvidence,
	) -> Result<RoutingCommandOutcome<RoutingEvidenceEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_evidence_request(request)?;
		let capabilities = request
			.capabilities
			.iter()
			.map(|(capability, _)| capability.as_sql().to_owned())
			.collect::<Vec<_>>();
		let states = request
			.capabilities
			.iter()
			.map(|(_, state)| capability_state_sql(*state).to_owned())
			.collect::<Vec<_>>();
		let role = request.role.as_sql();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.publish_routing_evidence_exact($1,$2,$3::text::uuid,\
				 $4::text::uuid,$5,$6,$7::text::decodex.role_profile_role,$8,$9,\
				 $10::text::uuid,$11::text::uuid,$12,\
				 $13::text[]::decodex.codex_capability[],\
				 $14::text[]::decodex.capability_evidence_state[])",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.evidence_id,
					&request.account_id.as_str(),
					&request.expected_account_revision,
					&request.expected_evidence_revision,
					&role,
					&request.role_profile_revision,
					&request.build_id,
					&request.process_id,
					&request.process_account_id.as_str(),
					&request.schema_fingerprint,
					&capabilities,
					&states,
				],
			)
			.await?;
		parse_evidence_response(&response, request)
	}

	/// Resolve one immutable ManagedRun fact snapshot without selecting an account.
	pub async fn resolve_routing_snapshot(
		&self,
		idempotency_key: &str,
		routing_policy_id: &str,
		expected_routing_policy_revision: i64,
		consumer: &ExecutionConsumer,
	) -> Result<RoutingCommandOutcome<RoutingSnapshot>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(routing_policy_id, "routing policy identity")?;
		if expected_routing_policy_revision <= 0 || consumer.domain_revision() <= 0 {
			return Err(StoreError::InvalidInput("routing source revisions must be positive"));
		}
		if !matches!(consumer, ExecutionConsumer::ManagedRunExecution { .. }) {
			return Err(StoreError::InvalidInput(
				"split routing snapshots are reserved for ManagedRun execution",
			));
		}
		let parts = ExecutionConsumerParts::from(consumer);
		if parts.source_runtime_session_id.is_some()
			!= parts.source_runtime_session_revision.is_some()
			|| parts.source_runtime_session_revision.is_some_and(|revision| revision <= 0)
		{
			return Err(StoreError::InvalidInput(
				"source RuntimeSession identity and positive revision must be jointly present",
			));
		}
		let response = self
			.execute_exact_with_retry(
				RESOLVE_ROUTING_SNAPSHOT_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&"managed_run_project_policy",
					&routing_policy_id,
					&expected_routing_policy_revision,
					&None::<i64>,
					&parts.kind,
					&parts.conversation_id,
					&parts.conversation_revision,
					&parts.source_runtime_session_id,
					&parts.source_runtime_session_revision,
					&parts.turn_id,
					&parts.managed_run_id,
					&parts.managed_run_revision,
					&parts.managed_execution_id,
				],
			)
			.await?;
		parse_snapshot_response(
			&response,
			routing_policy_id,
			expected_routing_policy_revision,
			consumer,
		)
	}
}

struct ExecutionConsumerParts<'a> {
	kind: &'static str,
	conversation_id: Option<&'a str>,
	conversation_revision: Option<i64>,
	source_runtime_session_id: Option<&'a str>,
	source_runtime_session_revision: Option<i64>,
	turn_id: Option<&'a str>,
	managed_run_id: Option<&'a str>,
	managed_run_revision: Option<i64>,
	managed_execution_id: Option<&'a str>,
}

impl<'a> From<&'a ExecutionConsumer> for ExecutionConsumerParts<'a> {
	fn from(value: &'a ExecutionConsumer) -> Self {
		match value {
			ExecutionConsumer::ConversationTurn {
				conversation_id,
				conversation_revision,
				source_runtime_session_id,
				source_runtime_session_revision,
				turn_id,
			} => Self {
				kind: value.as_sql(),
				conversation_id: Some(conversation_id.as_str()),
				conversation_revision: Some(*conversation_revision),
				source_runtime_session_id: source_runtime_session_id
					.as_ref()
					.map(decodex_core::RuntimeSessionId::as_str),
				source_runtime_session_revision: *source_runtime_session_revision,
				turn_id: Some(turn_id.as_str()),
				managed_run_id: None,
				managed_run_revision: None,
				managed_execution_id: None,
			},
			ExecutionConsumer::ManagedRunExecution {
				managed_run_id,
				managed_run_revision,
				execution_id,
			} => Self {
				kind: value.as_sql(),
				conversation_id: None,
				conversation_revision: None,
				source_runtime_session_id: None,
				source_runtime_session_revision: None,
				turn_id: None,
				managed_run_id: Some(managed_run_id.as_str()),
				managed_run_revision: Some(*managed_run_revision),
				managed_execution_id: Some(execution_id.as_str()),
			},
		}
	}
}

fn validate_policy_request(request: &ReplaceRoutingPolicy) -> Result<(), StoreError> {
	validate_uuid(&request.routing_policy_id, "routing policy identity")?;
	validate_uuid(&request.project_id, "routing policy Project identity")?;
	validate_uuid(&request.accepted_policy_id, "accepted Policy identity")?;
	if request.expected_revision.is_some_and(|revision| revision <= 0)
		|| request.accepted_policy_revision <= 0
		|| request.required_role_profile_revision <= 0
	{
		return Err(StoreError::InvalidInput("routing policy revisions must be positive"));
	}
	if !is_build_id(&request.required_build_id) {
		return Err(StoreError::InvalidInput("required Codex BuildId is malformed"));
	}
	if request.members.iter().any(|member| member.account_revision <= 0)
		|| request.members.iter().map(|member| &member.account_id).collect::<BTreeSet<_>>().len()
			!= request.members.len()
		|| !is_canonical_capability_subset(&request.required_capabilities)
	{
		return Err(StoreError::InvalidInput("routing policy inventory is malformed"));
	}
	Ok(())
}

fn validate_evidence_request(request: &PublishRoutingEvidence) -> Result<(), StoreError> {
	validate_uuid(&request.evidence_id, "routing evidence identity")?;
	validate_uuid(&request.process_id, "Codex process identity")?;
	if request.expected_account_revision <= 0
		|| request.role_profile_revision <= 0
		|| request.expected_evidence_revision.is_some_and(|revision| revision <= 0)
	{
		return Err(StoreError::InvalidInput("routing evidence revisions must be positive"));
	}
	if request.account_id != request.process_account_id
		|| !is_build_id(&request.build_id)
		|| !is_hex_digest(&request.schema_fingerprint)
		|| request.capabilities.len() != CodexCapability::ALL.len()
		|| !request
			.capabilities
			.iter()
			.zip(CodexCapability::ALL)
			.all(|((actual, _), expected)| *actual == expected)
	{
		return Err(StoreError::InvalidInput("routing evidence projection is malformed"));
	}
	Ok(())
}

fn parse_policy_response(
	response: &[u8],
	request: &ReplaceRoutingPolicy,
) -> Result<RoutingCommandOutcome<RoutingPolicyEffect>, StoreError> {
	let (classification, effect) = parse_envelope(response, "replace_routing_policy")?;
	if classification == "stable_domain_rejection" {
		return parse_rejection(&effect).map(RoutingCommandOutcome::Rejected);
	}
	require_keys(
		&effect,
		&[
			"accepted_policy_id",
			"accepted_policy_revision",
			"effect_digest",
			"effect_digest_source",
			"members",
			"operation",
			"project_id",
			"required_capabilities",
			"required_build_id",
			"required_role",
			"required_role_profile_revision",
			"routing_policy_id",
			"routing_policy_revision",
		],
	)?;
	let revision = positive_i64(&effect, "routing_policy_revision")?;
	let expected_revision = match request.expected_revision {
		Some(revision) => revision
			.checked_add(1)
			.ok_or_else(|| StoreError::Incompatible("routing policy revision overflowed".into()))?,
		None => 1,
	};
	let capabilities = parse_capabilities(required_array(&effect, "required_capabilities")?)?;
	let members =
		parse_policy_members(required_array(&effect, "members")?, request, expected_revision)?;
	if required_str(&effect, "routing_policy_id")? != request.routing_policy_id
		|| revision != expected_revision
		|| capabilities != request.required_capabilities
		|| required_str(&effect, "project_id")? != request.project_id
		|| required_str(&effect, "accepted_policy_id")? != request.accepted_policy_id
		|| positive_i64(&effect, "accepted_policy_revision")? != request.accepted_policy_revision
		|| required_str(&effect, "required_role")? != request.required_role.as_sql()
		|| positive_i64(&effect, "required_role_profile_revision")?
			!= request.required_role_profile_revision
		|| required_str(&effect, "required_build_id")? != request.required_build_id
	{
		return incompatible("routing policy result is cross-linked or reordered");
	}
	Ok(RoutingCommandOutcome::Success(RoutingPolicyEffect {
		routing_policy_id: request.routing_policy_id.clone(),
		routing_policy_revision: revision,
		project_id: request.project_id.clone(),
		accepted_policy_id: request.accepted_policy_id.clone(),
		accepted_policy_revision: request.accepted_policy_revision,
		required_role: request.required_role.as_sql().to_owned(),
		required_role_profile_revision: request.required_role_profile_revision,
		required_build_id: request.required_build_id.clone(),
		members,
		required_capabilities: capabilities,
	}))
}

fn parse_evidence_response(
	response: &[u8],
	request: &PublishRoutingEvidence,
) -> Result<RoutingCommandOutcome<RoutingEvidenceEffect>, StoreError> {
	let (classification, effect) = parse_envelope(response, "publish_routing_evidence")?;
	if classification == "stable_domain_rejection" {
		return parse_rejection(&effect).map(RoutingCommandOutcome::Rejected);
	}
	require_keys(
		&effect,
		&[
			"account_id",
			"account_revision",
			"build_id",
			"capabilities",
			"effect_digest",
			"effect_digest_source",
			"evidence_id",
			"evidence_revision",
			"ingested_at_micros",
			"operation",
			"process_account_id",
			"process_id",
			"role",
			"role_profile_revision",
			"schema_fingerprint",
			"states",
		],
	)?;
	let capabilities = parse_capabilities(required_array(&effect, "capabilities")?)?;
	let states = required_array(&effect, "states")?
		.iter()
		.map(parse_capability_state)
		.collect::<Result<Vec<_>, _>>()?;
	if capabilities
		.iter()
		.copied()
		.zip(states.iter().copied())
		.ne(request.capabilities.iter().copied())
		|| required_str(&effect, "evidence_id")? != request.evidence_id
		|| required_str(&effect, "account_id")? != request.account_id.as_str()
		|| positive_i64(&effect, "account_revision")? != request.expected_account_revision
		|| required_str(&effect, "role")? != request.role.as_sql()
		|| positive_i64(&effect, "role_profile_revision")? != request.role_profile_revision
		|| required_str(&effect, "build_id")? != request.build_id
		|| required_str(&effect, "process_id")? != request.process_id
		|| required_str(&effect, "process_account_id")? != request.process_account_id.as_str()
		|| required_str(&effect, "schema_fingerprint")? != request.schema_fingerprint
	{
		return incompatible("routing evidence result is cross-linked or reordered");
	}
	let evidence_revision = positive_i64(&effect, "evidence_revision")?;
	let expected_evidence_revision = match request.expected_evidence_revision {
		Some(revision) => revision.checked_add(1).ok_or_else(|| {
			StoreError::Incompatible("routing evidence revision overflowed".into())
		})?,
		None => 1,
	};
	if evidence_revision != expected_evidence_revision {
		return incompatible("routing evidence revision is not the requested successor");
	}
	Ok(RoutingCommandOutcome::Success(RoutingEvidenceEffect {
		evidence_id: request.evidence_id.clone(),
		account_id: request.account_id.clone(),
		account_revision: request.expected_account_revision,
		evidence_revision,
		role: request.role.as_sql().to_owned(),
		role_profile_revision: request.role_profile_revision,
		build_id: request.build_id.clone(),
		process_id: request.process_id.clone(),
		process_account_id: request.process_account_id.clone(),
		schema_fingerprint: request.schema_fingerprint.clone(),
		ingested_at_micros: required_timestamp_micros(&effect, "ingested_at_micros")?,
		capabilities: capabilities.into_iter().zip(states).collect(),
	}))
}

fn parse_policy_members(
	values: &[Value],
	request: &ReplaceRoutingPolicy,
	expected_routing_revision: i64,
) -> Result<Vec<RoutingPolicyMember>, StoreError> {
	if values.len() != request.members.len() {
		return incompatible("routing policy result member cardinality is invalid");
	}
	values
		.iter()
		.zip(&request.members)
		.enumerate()
		.map(|(index, (value, expected))| {
			require_keys(
				value,
				&[
					"account_id",
					"account_revision",
					"disposition",
					"position",
					"routing_policy_id",
					"routing_policy_revision",
				],
			)?;
			let account_id = AccountId::new(required_str(value, "account_id")?).map_err(|_| {
				StoreError::Incompatible("routing policy account identity is malformed".into())
			})?;
			let disposition = parse_disposition(required_str(value, "disposition")?)?;
			let revision = positive_i64(value, "account_revision")?;
			if required_usize(value, "position")? != index + 1
				|| account_id != expected.account_id
				|| revision != expected.account_revision
				|| disposition != expected.disposition
				|| required_str(value, "routing_policy_id")? != request.routing_policy_id
				|| positive_i64(value, "routing_policy_revision")? != expected_routing_revision
			{
				return incompatible("routing policy result members are reordered or cross-linked");
			}
			Ok(RoutingPolicyMember {
				position: index + 1,
				account_id,
				account_revision: revision,
				disposition,
			})
		})
		.collect()
}

const ROUTING_SNAPSHOT_EFFECT_KEYS: &[&str] = &[
	"accepted_policy_id",
	"accepted_policy_revision",
	"account_snapshot_id",
	"account_snapshot_source_revision",
	"blockers",
	"capability_facts",
	"effect_digest",
	"effect_digest_source",
	"consumer_kind",
	"conversation_id",
	"conversation_revision",
	"turn_id",
	"managed_run_id",
	"managed_run_revision",
	"managed_execution_id",
	"members",
	"operation",
	"profile_snapshot_id",
	"profile_snapshot_source_revision",
	"quota_facts",
	"required_build_id",
	"required_role",
	"required_role_profile_revision",
	"resolved_at_micros",
	"routing_policy_id",
	"routing_policy_revision",
	"runtime_session_id",
	"runtime_session_revision",
	"snapshot_id",
];

struct RoutingSnapshotLineage {
	runtime_session_id: Option<decodex_core::RuntimeSessionId>,
	runtime_session_revision: Option<i64>,
	account_snapshot_id: Option<String>,
	account_snapshot_source_revision: Option<i64>,
	profile_snapshot_id: Option<String>,
	profile_snapshot_source_revision: Option<i64>,
	lineage_is_l0: bool,
}

fn parse_snapshot_response(
	response: &[u8],
	routing_policy_id: &str,
	routing_revision: i64,
	consumer: &ExecutionConsumer,
) -> Result<RoutingCommandOutcome<RoutingSnapshot>, StoreError> {
	let (classification, effect) = parse_envelope(response, "resolve_routing_snapshot")?;
	if classification == "stable_domain_rejection" {
		return parse_rejection(&effect).map(RoutingCommandOutcome::Rejected);
	}
	require_keys(&effect, ROUTING_SNAPSHOT_EFFECT_KEYS)?;
	if required_str(&effect, "routing_policy_id")? != routing_policy_id
		|| positive_i64(&effect, "routing_policy_revision")? != routing_revision
		|| !effect_matches_consumer(&effect, consumer)?
	{
		return incompatible("routing snapshot result is cross-linked");
	}
	let snapshot_id = required_uuid(&effect, "snapshot_id")?;
	let accepted_policy_id = required_uuid(&effect, "accepted_policy_id")?;
	let accepted_policy_revision = positive_i64(&effect, "accepted_policy_revision")?;
	let required_role = required_role(&effect, "required_role")?.to_owned();
	let required_role_profile_revision = positive_i64(&effect, "required_role_profile_revision")?;
	let required_build_id = required_build_id(&effect, "required_build_id")?.to_owned();
	let lineage = RoutingSnapshotLineage::parse(&effect)?;
	let resolved_at_micros = required_timestamp_micros(&effect, "resolved_at_micros")?;
	let members = parse_members(required_array(&effect, "members")?, &snapshot_id)?;
	lineage.validate(consumer, &members, required_role_profile_revision)?;
	validate_member_facts(
		&members,
		&required_role,
		required_role_profile_revision,
		&required_build_id,
	)?;
	let quota_facts = parse_quota_facts(
		required_array(&effect, "quota_facts")?,
		&snapshot_id,
		&members,
		resolved_at_micros,
	)?;
	let capability_facts = parse_capability_facts(
		required_array(&effect, "capability_facts")?,
		&snapshot_id,
		&members,
	)?;
	validate_blocker_projection(required_array(&effect, "blockers")?, &snapshot_id, &members)?;
	Ok(RoutingCommandOutcome::Success(RoutingSnapshot {
		snapshot_id,
		routing_policy_id: routing_policy_id.to_owned(),
		routing_policy_revision: routing_revision,
		accepted_policy_id,
		accepted_policy_revision,
		required_role,
		required_role_profile_revision,
		required_build_id,
		consumer: consumer.clone(),
		runtime_session_id: lineage.runtime_session_id,
		runtime_session_revision: lineage.runtime_session_revision,
		account_snapshot_id: lineage.account_snapshot_id,
		account_snapshot_source_revision: lineage.account_snapshot_source_revision,
		profile_snapshot_id: lineage.profile_snapshot_id,
		profile_snapshot_source_revision: lineage.profile_snapshot_source_revision,
		resolved_at_micros,
		members,
		quota_facts,
		capability_facts,
	}))
}

impl RoutingSnapshotLineage {
	fn parse(effect: &Value) -> Result<Self, StoreError> {
		let runtime_session_id = optional_uuid(effect, "runtime_session_id")?
			.map(decodex_core::RuntimeSessionId::new)
			.transpose()
			.map_err(|_| {
				StoreError::Incompatible("stored RuntimeSession identity is invalid".into())
			})?;
		let runtime_session_revision = optional_positive_i64(effect, "runtime_session_revision")?;
		let account_snapshot_id = optional_uuid(effect, "account_snapshot_id")?;
		let account_snapshot_source_revision =
			optional_positive_i64(effect, "account_snapshot_source_revision")?;
		let profile_snapshot_id = optional_uuid(effect, "profile_snapshot_id")?;
		let profile_snapshot_source_revision =
			optional_positive_i64(effect, "profile_snapshot_source_revision")?;
		let lineage_presence = [
			runtime_session_id.is_some(),
			runtime_session_revision.is_some(),
			account_snapshot_id.is_some(),
			account_snapshot_source_revision.is_some(),
			profile_snapshot_id.is_some(),
			profile_snapshot_source_revision.is_some(),
		];
		let lineage_is_l0 = lineage_presence.iter().all(|present| !*present);
		let lineage_is_l6 = lineage_presence.iter().all(|present| *present);
		if !lineage_is_l0 && !lineage_is_l6 {
			return incompatible("routing snapshot source lineage is partial");
		}
		Ok(Self {
			runtime_session_id,
			runtime_session_revision,
			account_snapshot_id,
			account_snapshot_source_revision,
			profile_snapshot_id,
			profile_snapshot_source_revision,
			lineage_is_l0,
		})
	}

	fn validate(
		&self,
		consumer: &ExecutionConsumer,
		members: &[RoutingSnapshotMember],
		required_role_profile_revision: i64,
	) -> Result<(), StoreError> {
		let sticky_members = members.iter().filter(|member| member.sticky).collect::<Vec<_>>();
		match (consumer, self.lineage_is_l0) {
			(
				ExecutionConsumer::ConversationTurn {
					source_runtime_session_id,
					source_runtime_session_revision,
					..
				},
				true,
			) if source_runtime_session_id.is_none()
				&& source_runtime_session_revision.is_none()
				&& sticky_members.is_empty() =>
				Ok(()),
			(_, false) if sticky_members.len() == 1 => {
				let sticky = sticky_members[0];
				if Some(sticky.account_revision) != self.account_snapshot_source_revision
					|| self.profile_snapshot_source_revision != Some(required_role_profile_revision)
				{
					return incompatible("routing sticky snapshot revisions are cross-linked");
				}
				Ok(())
			},
			_ => incompatible("routing snapshot lineage and sticky shape disagree"),
		}
	}
}

fn effect_matches_consumer(
	effect: &Value,
	consumer: &ExecutionConsumer,
) -> Result<bool, StoreError> {
	if required_str(effect, "consumer_kind")? != consumer.as_sql() {
		return Ok(false);
	}
	match consumer {
		ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision,
			source_runtime_session_id,
			source_runtime_session_revision,
			turn_id,
		} => Ok(optional_uuid(effect, "conversation_id")?.as_deref()
			== Some(conversation_id.as_str())
			&& optional_positive_i64(effect, "conversation_revision")?
				== Some(*conversation_revision)
			&& optional_uuid(effect, "runtime_session_id")?.as_deref()
				== source_runtime_session_id.as_ref().map(decodex_core::RuntimeSessionId::as_str)
			&& optional_positive_i64(effect, "runtime_session_revision")?
				== *source_runtime_session_revision
			&& optional_uuid(effect, "turn_id")?.as_deref() == Some(turn_id.as_str())
			&& optional_uuid(effect, "managed_run_id")?.is_none()
			&& optional_positive_i64(effect, "managed_run_revision")?.is_none()
			&& optional_uuid(effect, "managed_execution_id")?.is_none()),
		ExecutionConsumer::ManagedRunExecution {
			managed_run_id,
			managed_run_revision,
			execution_id,
		} => Ok(optional_uuid(effect, "conversation_id")?.is_none()
			&& optional_positive_i64(effect, "conversation_revision")?.is_none()
			&& optional_uuid(effect, "turn_id")?.is_none()
			&& optional_uuid(effect, "managed_run_id")?.as_deref()
				== Some(managed_run_id.as_str())
			&& optional_positive_i64(effect, "managed_run_revision")?
				== Some(*managed_run_revision)
			&& optional_uuid(effect, "managed_execution_id")?.as_deref()
				== Some(execution_id.as_str())),
	}
}

fn parse_envelope(bytes: &[u8], expected_operation: &str) -> Result<(String, Value), StoreError> {
	let document: Value = serde_json::from_slice(bytes).map_err(|_| {
		StoreError::Incompatible("stored Routing Snapshot response bytes are malformed".into())
	})?;
	require_keys(&document, &["classification", "effect"])?;
	let classification = required_str(&document, "classification")?;
	if !matches!(classification, "success" | "stable_domain_rejection") {
		return incompatible("stored Routing Snapshot response classification is unknown");
	}
	let effect = required_object_value(&document, "effect")?;
	verify_effect_digest(effect)?;
	if required_str(effect, "operation")? != expected_operation {
		return incompatible("stored Routing Snapshot response operation is cross-linked");
	}
	Ok((classification.to_owned(), effect.clone()))
}

fn verify_effect_digest(effect: &Value) -> Result<(), StoreError> {
	let source = required_str(effect, "effect_digest_source")?;
	let digest = required_str(effect, "effect_digest")?;
	if !is_hex_digest(digest) || hex_sha256(source.as_bytes()) != digest {
		return incompatible(
			"stored Routing Snapshot effect digest does not match its exact source bytes",
		);
	}
	let source_value: Value = serde_json::from_str(source).map_err(|_| {
		StoreError::Incompatible("stored Routing Snapshot digest source is malformed".into())
	})?;
	let mut projected = effect
		.as_object()
		.ok_or_else(|| {
			StoreError::Incompatible("stored Routing Snapshot effect is not an object".into())
		})?
		.clone();
	projected.remove("effect_digest");
	projected.remove("effect_digest_source");
	if source_value != Value::Object(projected) {
		return incompatible(
			"stored Routing Snapshot digest source differs from the closed effect projection",
		);
	}
	Ok(())
}

fn parse_rejection(effect: &Value) -> Result<RoutingRejection, StoreError> {
	require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
	let operation = required_str(effect, "operation")?;
	let code = required_str(effect, "rejection")?;
	let known = match operation {
		"replace_routing_policy" => matches!(
			code,
			"invalid_inventory"
				| "inventory_revision_mismatch"
				| "accepted_policy_mismatch"
				| "role_profile_mismatch"
				| "stale_revision"
		),
		"publish_routing_evidence" => matches!(
			code,
			"invalid_evidence"
				| "account_provenance_mismatch"
				| "role_profile_mismatch"
				| "invalid_capability_projection"
				| "duplicate_evidence_id"
				| "duplicate_process_id"
				| "stale_evidence_revision"
		),
		"resolve_routing_snapshot" => matches!(
			code,
			"malformed_input"
				| "routing_authority_mismatch"
				| "conversation_mismatch"
				| "managed_run_mismatch"
				| "sticky_provenance_mismatch"
		),
		_ => false,
	};
	if !known {
		return incompatible("stored Routing Snapshot rejection code is unknown");
	}
	Ok(RoutingRejection { operation: operation.to_owned(), code: code.to_owned() })
}

fn parse_members(
	values: &[Value],
	snapshot_id: &str,
) -> Result<Vec<RoutingSnapshotMember>, StoreError> {
	let mut result = Vec::with_capacity(values.len());
	let mut accounts = BTreeSet::new();
	for (index, value) in values.iter().enumerate() {
		require_keys(
			value,
			&[
				"account_id",
				"account_observed_at_utc",
				"account_revision",
				"account_state",
				"blockers",
				"display_label",
				"disposition",
				"evidence_account_revision",
				"evidence_build_id",
				"evidence_id",
				"evidence_revision",
				"evidence_role",
				"evidence_role_profile_revision",
				"position",
				"process_id",
				"schema_fingerprint",
				"snapshot_id",
				"sticky",
			],
		)?;
		if required_str(value, "snapshot_id")? != snapshot_id
			|| required_usize(value, "position")? != index + 1
		{
			return incompatible("routing snapshot member identities are reordered or gapped");
		}
		let account_id = AccountId::new(required_str(value, "account_id")?).map_err(|_| {
			StoreError::Incompatible("stored routing account identity is invalid".into())
		})?;
		if !accounts.insert(account_id.clone()) {
			return incompatible("routing snapshot repeats an account");
		}
		let disposition = parse_disposition(required_str(value, "disposition")?)?;
		let blockers = required_array(value, "blockers")?
			.iter()
			.map(parse_blocker)
			.collect::<Result<Vec<_>, _>>()?;
		if blockers.windows(2).any(|pair| pair[0] >= pair[1])
			|| (disposition == RoutingMemberDisposition::Excluded)
				!= blockers.contains(&RoutingBlocker::ExcludedByPolicy)
		{
			return incompatible("routing member blocker order or disposition is inconsistent");
		}
		let evidence_id = optional_uuid(value, "evidence_id")?;
		let evidence_revision = optional_positive_i64(value, "evidence_revision")?;
		let evidence_account_revision = optional_positive_i64(value, "evidence_account_revision")?;
		let evidence_role = optional_role(value, "evidence_role")?;
		let evidence_role_profile_revision =
			optional_positive_i64(value, "evidence_role_profile_revision")?;
		let evidence_build_id = optional_build_id(value, "evidence_build_id")?;
		let process_id = optional_uuid(value, "process_id")?;
		let schema_fingerprint = optional_digest(value, "schema_fingerprint")?;
		if [
			evidence_revision.is_some(),
			evidence_account_revision.is_some(),
			evidence_role.is_some(),
			evidence_role_profile_revision.is_some(),
			evidence_build_id.is_some(),
			process_id.is_some(),
			schema_fingerprint.is_some(),
		]
		.into_iter()
		.any(|present| present != evidence_id.is_some())
		{
			return incompatible("routing evidence provenance is partial");
		}
		result.push(RoutingSnapshotMember {
			position: index + 1,
			account_id,
			disposition,
			account_revision: positive_i64(value, "account_revision")?,
			display_label: required_str(value, "display_label")?.to_owned(),
			account_state: parse_account_state(required_str(value, "account_state")?)?,
			account_observed_at_utc: required_utc_microsecond(value, "account_observed_at_utc")?
				.to_owned(),
			evidence_id,
			evidence_revision,
			evidence_account_revision,
			evidence_role,
			evidence_role_profile_revision,
			evidence_build_id,
			process_id,
			schema_fingerprint,
			sticky: required_bool(value, "sticky")?,
			blockers,
		});
	}
	Ok(result)
}

fn parse_quota_facts(
	values: &[Value],
	snapshot_id: &str,
	members: &[RoutingSnapshotMember],
	resolved_at_micros: i64,
) -> Result<Vec<RoutingSnapshotQuotaFact>, StoreError> {
	if members.len().checked_mul(2) != Some(values.len()) {
		return incompatible("routing snapshot quota pair cardinality is invalid");
	}
	let mut result = Vec::with_capacity(values.len());
	for (index, value) in values.iter().enumerate() {
		require_keys(
			value,
			&[
				"account_id",
				"confidence",
				"duration_minutes",
				"observation_revision",
				"observed_at_micros",
				"position",
				"remaining_percent",
				"resets_at_micros",
				"snapshot_id",
				"window_class",
			],
		)?;
		let member = &members[index / 2];
		let pair = index % 2;
		let duration = required_u16(value, "duration_minutes")?;
		if required_str(value, "snapshot_id")? != snapshot_id
			|| required_str(value, "account_id")? != member.account_id.as_str()
			|| required_usize(value, "position")? != pair + 1
			|| (pair == 0
				&& (duration != 300 || required_str(value, "window_class")? != "five_hour"))
			|| (pair == 1
				&& (duration != 10080 || required_str(value, "window_class")? != "seven_day"))
		{
			return incompatible("routing snapshot quota facts are reordered or cross-linked");
		}
		let revision = optional_positive_i64(value, "observation_revision")?;
		let remaining = optional_u8(value, "remaining_percent")?;
		let resets = optional_nonnegative_i64(value, "resets_at_micros")?;
		let observed = optional_nonnegative_i64(value, "observed_at_micros")?;
		let confidence = optional_confidence(value, "confidence")?;
		if revision.is_none()
			&& (remaining.is_some()
				|| resets.is_some()
				|| observed.is_some()
				|| confidence.is_some())
		{
			return incompatible("missing quota fact contains defaulted observation values");
		}
		if revision.is_some() && (observed.is_none() || confidence.is_none()) {
			return incompatible("present quota fact omits observation provenance");
		}
		let window_blockers = if pair == 0 {
			[
				RoutingBlocker::QuotaFiveHourMissing,
				RoutingBlocker::QuotaFiveHourFromFuture,
				RoutingBlocker::QuotaFiveHourStale,
				RoutingBlocker::QuotaFiveHourUnknown,
				RoutingBlocker::QuotaFiveHourResetElapsed,
				RoutingBlocker::QuotaFiveHourDepleted,
			]
		} else {
			[
				RoutingBlocker::QuotaSevenDayMissing,
				RoutingBlocker::QuotaSevenDayFromFuture,
				RoutingBlocker::QuotaSevenDayStale,
				RoutingBlocker::QuotaSevenDayUnknown,
				RoutingBlocker::QuotaSevenDayResetElapsed,
				RoutingBlocker::QuotaSevenDayDepleted,
			]
		};
		let expected_blocker = if revision.is_none() {
			Some(window_blockers[0])
		} else {
			let observed = observed.ok_or_else(|| {
				StoreError::Incompatible("present quota fact omits observation time".into())
			})?;
			if observed > resolved_at_micros {
				Some(window_blockers[1])
			} else if resolved_at_micros - observed > 300_000_000 {
				Some(window_blockers[2])
			} else if remaining.is_none()
				|| confidence != Some(decodex_core::ObservationConfidence::High)
			{
				Some(window_blockers[3])
			} else if resets.is_some_and(|reset| reset <= resolved_at_micros) {
				Some(window_blockers[4])
			} else if remaining == Some(0) {
				Some(window_blockers[5])
			} else {
				None
			}
		};
		for blocker in window_blockers {
			if member.blockers.contains(&blocker) != (expected_blocker == Some(blocker)) {
				return incompatible("routing quota blocker is inconsistent with its exact fact");
			}
		}
		result.push(RoutingSnapshotQuotaFact {
			account_id: member.account_id.clone(),
			window: if pair == 0 {
				decodex_core::QuotaWindowClass::FiveHour
			} else {
				decodex_core::QuotaWindowClass::SevenDay
			},
			duration_minutes: duration,
			observation_revision: revision,
			remaining_percent: remaining,
			resets_at_micros: resets,
			observed_at_micros: observed,
			confidence,
		});
	}
	Ok(result)
}

fn validate_member_facts(
	members: &[RoutingSnapshotMember],
	required_role: &str,
	required_role_profile_revision: i64,
	required_build_id: &str,
) -> Result<(), StoreError> {
	for member in members {
		let state_blocker = match member.account_state {
			AccountState::Unavailable => Some(RoutingBlocker::AccountUnavailable),
			AccountState::Unknown => Some(RoutingBlocker::AccountUnknown),
			AccountState::Depleted => Some(RoutingBlocker::AccountDepleted),
			AccountState::AuthFailed => Some(RoutingBlocker::AccountAuthFailed),
			AccountState::PluginUnready => Some(RoutingBlocker::AccountPluginUnready),
			AccountState::Available => None,
		};
		for blocker in [
			RoutingBlocker::AccountUnavailable,
			RoutingBlocker::AccountUnknown,
			RoutingBlocker::AccountDepleted,
			RoutingBlocker::AccountAuthFailed,
			RoutingBlocker::AccountPluginUnready,
		] {
			if member.blockers.contains(&blocker) != (state_blocker == Some(blocker)) {
				return incompatible("routing account-state blocker is inconsistent with its fact");
			}
		}
		if member.blockers.contains(&RoutingBlocker::EvidenceMissing)
			!= member.evidence_id.is_none()
		{
			return incompatible(
				"routing evidence-missing blocker is inconsistent with provenance",
			);
		}
		if let Some(account_revision) = member.evidence_account_revision {
			if member.blockers.contains(&RoutingBlocker::EvidenceAccountMismatch)
				!= (account_revision != member.account_revision)
			{
				return incompatible("routing evidence account blocker is inconsistent");
			}
			let profile_mismatch = member.evidence_role.as_deref() != Some(required_role)
				|| member.evidence_role_profile_revision != Some(required_role_profile_revision);
			if member.blockers.contains(&RoutingBlocker::EvidenceProfileMismatch)
				!= profile_mismatch
			{
				return incompatible("routing evidence profile blocker is inconsistent");
			}
			if member.blockers.contains(&RoutingBlocker::EvidenceBuildMismatch)
				!= (member.evidence_build_id.as_deref() != Some(required_build_id))
			{
				return incompatible("routing evidence BuildId blocker is inconsistent");
			}
		} else if member.blockers.contains(&RoutingBlocker::EvidenceAccountMismatch)
			|| member.blockers.contains(&RoutingBlocker::EvidenceProfileMismatch)
			|| member.blockers.contains(&RoutingBlocker::EvidenceBuildMismatch)
		{
			return incompatible("missing routing evidence has mismatch blockers");
		}
	}
	Ok(())
}

fn parse_capability_facts(
	values: &[Value],
	snapshot_id: &str,
	members: &[RoutingSnapshotMember],
) -> Result<Vec<RoutingSnapshotCapabilityFact>, StoreError> {
	if members.len().checked_mul(8) != Some(values.len()) {
		return incompatible("routing capability matrix cardinality is invalid");
	}
	let mut result = Vec::with_capacity(values.len());
	for (index, value) in values.iter().enumerate() {
		require_keys(
			value,
			&[
				"account_id",
				"applicable",
				"capability",
				"evidence_state",
				"position",
				"snapshot_id",
			],
		)?;
		let member = &members[index / 8];
		let cell = index % 8;
		let capability = parse_capability(required_str(value, "capability")?)?;
		if required_str(value, "snapshot_id")? != snapshot_id
			|| required_str(value, "account_id")? != member.account_id.as_str()
			|| required_usize(value, "position")? != cell + 1
			|| capability != CodexCapability::ALL[cell]
		{
			return incompatible("routing capability matrix is reordered or cross-linked");
		}
		let evidence_state = match value.get("evidence_state") {
			Some(Value::Null) => None,
			Some(value) => Some(parse_capability_state(value)?),
			_ => return incompatible("capability state is missing"),
		};
		result.push(RoutingSnapshotCapabilityFact {
			account_id: member.account_id.clone(),
			capability,
			applicable: required_bool(value, "applicable")?,
			evidence_state,
		});
	}
	for (member_index, member) in members.iter().enumerate() {
		let cells = &result[member_index * 8..member_index * 8 + 8];
		if cells.iter().any(|cell| cell.evidence_state.is_some() != member.evidence_id.is_some()) {
			return incompatible("routing capability evidence provenance is partial");
		}
		let unsatisfied = cells.iter().any(|cell| {
			cell.applicable && cell.evidence_state != Some(RoutingCapabilityState::Supported)
		});
		if unsatisfied != member.blockers.contains(&RoutingBlocker::RequiredCapabilityUnsatisfied) {
			return incompatible("routing capability blocker is inconsistent with the matrix");
		}
	}
	Ok(result)
}

fn validate_blocker_projection(
	values: &[Value],
	snapshot_id: &str,
	members: &[RoutingSnapshotMember],
) -> Result<(), StoreError> {
	let expected = members
		.iter()
		.flat_map(|member| {
			member
				.blockers
				.iter()
				.enumerate()
				.map(move |(position, blocker)| (&member.account_id, position + 1, *blocker))
		})
		.collect::<Vec<_>>();
	if values.len() != expected.len() {
		return incompatible("routing blocker projection cardinality is invalid");
	}
	for (index, (value, (account, position, blocker))) in values.iter().zip(expected).enumerate() {
		require_keys(value, &["account_id", "blocker", "position", "snapshot_id"])?;
		if required_str(value, "snapshot_id")? != snapshot_id
			|| required_str(value, "account_id")? != account.as_str()
			|| parse_blocker(
				value
					.get("blocker")
					.ok_or_else(|| StoreError::Incompatible("blocker is missing".into()))?,
			)? != blocker
			|| required_usize(value, "position")? != position
		{
			return incompatible(&format!("routing blocker projection is reordered at {index}"));
		}
	}
	Ok(())
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value.as_object().ok_or_else(|| {
		StoreError::Incompatible("stored Routing Snapshot object is malformed".into())
	})?;
	let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
	let expected = expected.iter().copied().collect::<BTreeSet<_>>();
	if actual == expected {
		Ok(())
	} else {
		incompatible("stored Routing Snapshot object has missing or unknown keys")
	}
}
fn required_object_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	let value = value.get(key).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Snapshot field is missing".into())
	})?;
	if value.is_object() {
		Ok(value)
	} else {
		incompatible("stored Routing Snapshot object field is malformed")
	}
}
fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value.get(key).and_then(Value::as_array).map(Vec::as_slice).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Snapshot array is malformed".into())
	})
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored Routing Snapshot text is malformed".into()))
}
fn required_utc_microsecond<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	let value = required_str(value, key)?;
	if value.len() == 27
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			4 | 7 => byte == b'-',
			10 => byte == b'T',
			13 | 16 => byte == b':',
			19 => byte == b'.',
			26 => byte == b'Z',
			_ => byte.is_ascii_digit(),
		}) {
		Ok(value)
	} else {
		incompatible("stored Routing Snapshot UTC microsecond timestamp is malformed")
	}
}
fn required_bool(value: &Value, key: &str) -> Result<bool, StoreError> {
	value.get(key).and_then(Value::as_bool).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Snapshot boolean is malformed".into())
	})
}
fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let value =
		value.get(key).and_then(Value::as_i64).filter(|value| *value > 0).ok_or_else(|| {
			StoreError::Incompatible("stored Routing Snapshot revision is malformed".into())
		})?;
	Ok(value)
}
fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value.get(key).and_then(Value::as_i64).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Snapshot integer is malformed".into())
	})
}
fn required_timestamp_micros(value: &Value, key: &str) -> Result<i64, StoreError> {
	required_i64(value, key).and_then(|value| {
		if (0..=253_402_300_799_999_999).contains(&value) {
			Ok(value)
		} else {
			incompatible("stored Routing Snapshot timestamp is outside the exact product range")
		}
	})
}
fn required_usize(value: &Value, key: &str) -> Result<usize, StoreError> {
	value
		.get(key)
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.filter(|value| *value > 0)
		.ok_or_else(|| {
			StoreError::Incompatible("stored Routing Snapshot position is malformed".into())
		})
}
fn required_u16(value: &Value, key: &str) -> Result<u16, StoreError> {
	value.get(key).and_then(Value::as_u64).and_then(|value| u16::try_from(value).ok()).ok_or_else(
		|| StoreError::Incompatible("stored Routing Snapshot duration is malformed".into()),
	)
}
fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value.as_i64().map(Some).ok_or_else(|| {
			StoreError::Incompatible("stored Routing Snapshot optional integer is malformed".into())
		}),
		None => incompatible("stored Routing Snapshot optional integer is missing"),
	}
}
fn optional_nonnegative_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match optional_i64(value, key)? {
		Some(value) if (0..=253_402_300_799_999_999).contains(&value) => Ok(Some(value)),
		Some(_) => incompatible("stored Routing Snapshot optional timestamp is malformed"),
		None => Ok(None),
	}
}
fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match optional_i64(value, key)? {
		Some(value) if value > 0 => Ok(Some(value)),
		Some(_) => incompatible("stored Routing Snapshot optional revision is malformed"),
		None => Ok(None),
	}
}
fn optional_u8(value: &Value, key: &str) -> Result<Option<u8>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_u64()
			.and_then(|value| u8::try_from(value).ok())
			.filter(|value| *value <= 100)
			.map(Some)
			.ok_or_else(|| StoreError::Incompatible("stored quota percent is malformed".into())),
		None => incompatible("stored quota percent is missing"),
	}
}
fn optional_uuid(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) if is_uuid(value) => Ok(Some(value.clone())),
		_ => incompatible("stored optional UUID is malformed"),
	}
}
fn optional_digest(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) if is_hex_digest(value) => Ok(Some(value.clone())),
		_ => incompatible("stored optional digest is malformed"),
	}
}
fn optional_build_id(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) if is_build_id(value) => Ok(Some(value.clone())),
		_ => incompatible("stored optional BuildId is malformed"),
	}
}
fn optional_role(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value))
			if matches!(value.as_str(), "advisor" | "lead" | "task" | "reviewer") =>
			Ok(Some(value.clone())),
		_ => incompatible("stored optional RoleProfile role is malformed"),
	}
}
fn optional_confidence(
	value: &Value,
	key: &str,
) -> Result<Option<decodex_core::ObservationConfidence>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => match value.as_str() {
			"unknown" => Ok(Some(decodex_core::ObservationConfidence::Unknown)),
			"low" => Ok(Some(decodex_core::ObservationConfidence::Low)),
			"high" => Ok(Some(decodex_core::ObservationConfidence::High)),
			_ => incompatible("stored quota confidence is unknown"),
		},
		_ => incompatible("stored quota confidence is malformed"),
	}
}
fn required_uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = required_str(value, key)?;
	if is_uuid(value) { Ok(value.to_owned()) } else { incompatible("stored UUID is malformed") }
}
fn required_role<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	let value = required_str(value, key)?;
	if matches!(value, "advisor" | "lead" | "task" | "reviewer") {
		Ok(value)
	} else {
		incompatible("stored RoleProfile role is malformed")
	}
}
fn required_build_id<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	let value = required_str(value, key)?;
	if is_build_id(value) { Ok(value) } else { incompatible("stored BuildId is malformed") }
}

fn parse_capabilities(values: &[Value]) -> Result<Vec<CodexCapability>, StoreError> {
	let result = values
		.iter()
		.map(|value| {
			value
				.as_str()
				.ok_or_else(|| StoreError::Incompatible("capability identity is malformed".into()))
				.and_then(parse_capability)
		})
		.collect::<Result<Vec<_>, _>>()?;
	if is_canonical_capability_subset(&result) {
		Ok(result)
	} else {
		incompatible("capability identities are duplicated or reordered")
	}
}
fn parse_capability(value: &str) -> Result<CodexCapability, StoreError> {
	CodexCapability::ALL
		.into_iter()
		.find(|capability| capability.as_sql() == value)
		.ok_or_else(|| StoreError::Incompatible("capability identity is unknown".into()))
}
fn parse_capability_state(value: &Value) -> Result<RoutingCapabilityState, StoreError> {
	let value = value
		.as_str()
		.ok_or_else(|| StoreError::Incompatible("capability state is malformed".into()))?;
	match value {
		"supported" => Ok(RoutingCapabilityState::Supported),
		"unsupported_schema_missing" => Ok(RoutingCapabilityState::UnsupportedSchemaMissing),
		"unsupported_method_not_found" => Ok(RoutingCapabilityState::UnsupportedMethodNotFound),
		"unsupported_codex_rejected" => Ok(RoutingCapabilityState::UnsupportedCodexRejected),
		"unavailable_not_probed" => Ok(RoutingCapabilityState::UnavailableNotProbed),
		"unavailable_probe_failed" => Ok(RoutingCapabilityState::UnavailableProbeFailed),
		"degraded_legacy_history_only" => Ok(RoutingCapabilityState::DegradedLegacyHistoryOnly),
		_ => incompatible("capability state is unknown"),
	}
}
fn capability_state_sql(value: RoutingCapabilityState) -> &'static str {
	match value {
		RoutingCapabilityState::Supported => "supported",
		RoutingCapabilityState::UnsupportedSchemaMissing => "unsupported_schema_missing",
		RoutingCapabilityState::UnsupportedMethodNotFound => "unsupported_method_not_found",
		RoutingCapabilityState::UnsupportedCodexRejected => "unsupported_codex_rejected",
		RoutingCapabilityState::UnavailableNotProbed => "unavailable_not_probed",
		RoutingCapabilityState::UnavailableProbeFailed => "unavailable_probe_failed",
		RoutingCapabilityState::DegradedLegacyHistoryOnly => "degraded_legacy_history_only",
	}
}
fn parse_disposition(value: &str) -> Result<RoutingMemberDisposition, StoreError> {
	match value {
		"included" => Ok(RoutingMemberDisposition::Included),
		"excluded" => Ok(RoutingMemberDisposition::Excluded),
		_ => incompatible("routing disposition is unknown"),
	}
}
fn disposition_sql(value: RoutingMemberDisposition) -> &'static str {
	match value {
		RoutingMemberDisposition::Included => "included",
		RoutingMemberDisposition::Excluded => "excluded",
	}
}
fn parse_account_state(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		"disabled" => Err(StoreError::Incompatible(
			"stored account state encodes administrative enablement".into(),
		)),
		_ => incompatible("stored account state is unknown"),
	}
}
fn parse_blocker(value: &Value) -> Result<RoutingBlocker, StoreError> {
	let value = value
		.as_str()
		.ok_or_else(|| StoreError::Incompatible("routing blocker is malformed".into()))?;
	match value {
		"excluded_by_policy" => Ok(RoutingBlocker::ExcludedByPolicy),
		"account_from_future" => Ok(RoutingBlocker::AccountFromFuture),
		"account_stale" => Ok(RoutingBlocker::AccountStale),
		"account_unavailable" => Ok(RoutingBlocker::AccountUnavailable),
		"account_unknown" => Ok(RoutingBlocker::AccountUnknown),
		"account_depleted" => Ok(RoutingBlocker::AccountDepleted),
		"account_auth_failed" => Ok(RoutingBlocker::AccountAuthFailed),
		"account_plugin_unready" => Ok(RoutingBlocker::AccountPluginUnready),
		"account_disabled" => Ok(RoutingBlocker::AccountDisabled),
		"evidence_missing" => Ok(RoutingBlocker::EvidenceMissing),
		"evidence_from_future" => Ok(RoutingBlocker::EvidenceFromFuture),
		"evidence_stale" => Ok(RoutingBlocker::EvidenceStale),
		"evidence_account_mismatch" => Ok(RoutingBlocker::EvidenceAccountMismatch),
		"evidence_profile_mismatch" => Ok(RoutingBlocker::EvidenceProfileMismatch),
		"evidence_build_mismatch" => Ok(RoutingBlocker::EvidenceBuildMismatch),
		"quota_five_hour_missing" => Ok(RoutingBlocker::QuotaFiveHourMissing),
		"quota_five_hour_from_future" => Ok(RoutingBlocker::QuotaFiveHourFromFuture),
		"quota_five_hour_stale" => Ok(RoutingBlocker::QuotaFiveHourStale),
		"quota_five_hour_unknown" => Ok(RoutingBlocker::QuotaFiveHourUnknown),
		"quota_five_hour_reset_elapsed" => Ok(RoutingBlocker::QuotaFiveHourResetElapsed),
		"quota_five_hour_depleted" => Ok(RoutingBlocker::QuotaFiveHourDepleted),
		"quota_seven_day_missing" => Ok(RoutingBlocker::QuotaSevenDayMissing),
		"quota_seven_day_from_future" => Ok(RoutingBlocker::QuotaSevenDayFromFuture),
		"quota_seven_day_stale" => Ok(RoutingBlocker::QuotaSevenDayStale),
		"quota_seven_day_unknown" => Ok(RoutingBlocker::QuotaSevenDayUnknown),
		"quota_seven_day_reset_elapsed" => Ok(RoutingBlocker::QuotaSevenDayResetElapsed),
		"quota_seven_day_depleted" => Ok(RoutingBlocker::QuotaSevenDayDepleted),
		"required_capability_unsatisfied" => Ok(RoutingBlocker::RequiredCapabilityUnsatisfied),
		"authentication_required" => Ok(RoutingBlocker::AuthenticationRequired),
		"plugin_unready" => Ok(RoutingBlocker::PluginUnready),
		"dependency_blocked" => Ok(RoutingBlocker::DependencyBlocked),
		"approval_required" => Ok(RoutingBlocker::ApprovalRequired),
		"user_required" => Ok(RoutingBlocker::UserRequired),
		"external_blocked" => Ok(RoutingBlocker::ExternalBlocked),
		"usage_unproven" => Ok(RoutingBlocker::UsageUnproven),
		"reconciliation_unproven" => Ok(RoutingBlocker::ReconciliationUnproven),
		"reviewer_unavailable" => Ok(RoutingBlocker::ReviewerUnavailable),
		"reviewer_failed" => Ok(RoutingBlocker::ReviewerFailed),
		"reviewer_ambiguous" => Ok(RoutingBlocker::ReviewerAmbiguous),
		"process_generation_unresolved" => Ok(RoutingBlocker::ProcessGenerationUnresolved),
		"process_generation_unavailable" => Ok(RoutingBlocker::ProcessGenerationUnavailable),
		"provider_attempt_unresolved" => Ok(RoutingBlocker::ProviderAttemptUnresolved),
		"provider_attempt_completed" => Ok(RoutingBlocker::ProviderAttemptCompleted),
		_ => incompatible("routing blocker is unknown"),
	}
}

fn is_canonical_capability_subset(values: &[CodexCapability]) -> bool {
	let mut last = None;
	for value in values {
		let Some(index) = CodexCapability::ALL.iter().position(|candidate| candidate == value)
		else {
			return false;
		};
		if last.is_some_and(|last| index <= last) {
			return false;
		}
		last = Some(index);
	}
	true
}
fn is_build_id(value: &str) -> bool {
	value.len() == 71 && value.starts_with("sha256:") && is_hex(&value[7..])
}
fn is_hex_digest(value: &str) -> bool {
	value.len() == 64 && is_hex(value)
}
fn is_hex(value: &str) -> bool {
	value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}
fn validate_uuid(value: &str, field: &'static str) -> Result<(), StoreError> {
	if is_uuid(value) { Ok(()) } else { Err(StoreError::InvalidInput(field)) }
}
fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
fn incompatible<T>(reason: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(reason.to_owned()))
}
