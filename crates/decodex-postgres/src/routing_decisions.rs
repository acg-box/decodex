use std::collections::BTreeSet;

use decodex_core::{
	AccountId, CodexCapability, ExecutionConsumer, ObservationConfidence, QuotaWindowClass,
	RoutingBlocker, RoutingCapabilityState, RoutingCommandOutcome, RoutingDecision,
	RoutingDecisionCandidate, RoutingDecisionCause, RoutingDecisionExclusion, RoutingDecisionKind,
	RoutingDecisionQuotaFact, RoutingDecisionSnapshot, RoutingMemberDisposition,
	RoutingNoRouteReason, RoutingRejection, RoutingSnapshotCapabilityFact,
	RoutingTimestampPrecision, RoutingTimestampProvenance, decide_routing,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

/// Caller-owned operation identity and optimistic authority coordinates.
///
/// Callers cannot supply candidates or evidence through this value. Constructing it does not prove
/// PostgreSQL provenance or authorize account switching, dispatch, continuation, or production use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteAccount {
	/// Stable UUID of the semantic routing operation, distinct from the command idempotency key.
	pub operation_id: String,
	/// UUID of the routing policy lineage from which PostgreSQL must resolve the snapshot.
	pub routing_policy_id: String,
	/// Positive, exact policy revision PostgreSQL must lock for immutable snapshot selection.
	pub expected_routing_policy_revision: i64,
	/// Exact ordinary or managed execution consumer. It cannot carry an account choice.
	pub consumer: ExecutionConsumer,
}

/// Exact immutable decision read back after PostgreSQL commits the complete evidence set.
///
/// Although this public shape can be constructed in Rust, construction alone does not prove a
/// committed database origin or grant routing, credential, dispatch, continuation, or production
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedRoutingDecision {
	/// UUID of the immutable decision row created by PostgreSQL.
	pub decision_id: String,
	/// Semantic operation UUID carried through from the locked routing request lineage.
	pub operation_id: String,
	/// Exact consumer lineage committed with the immutable decision.
	pub consumer: ExecutionConsumer,
	/// PostgreSQL-owned decision instant as exact microseconds since the Unix epoch.
	pub decided_at_micros: i64,
	/// Inert pure-kernel outcome read back with its complete persisted evidence and exclusions.
	pub decision: RoutingDecision,
}

impl PostgresStore {
	/// Atomically persist one inert decision over PostgreSQL's complete locked universe.
	pub async fn route_account(
		&self,
		idempotency_key: &str,
		request: &RouteAccount,
	) -> Result<RoutingCommandOutcome<PersistedRoutingDecision>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.operation_id, "routing operation identity")?;
		validate_uuid(&request.routing_policy_id, "routing policy identity")?;
		if request.expected_routing_policy_revision <= 0 || request.consumer.domain_revision() <= 0
		{
			return Err(StoreError::InvalidInput("routing decision revisions must be positive"));
		}
		let parts = ExecutionConsumerParts::from(&request.consumer);
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
				"SELECT decodex.route_account_exact($1,$2,$3::text::uuid,$4::text::uuid,$5,\
				 $6::text::decodex.provider_attempt_consumer_kind,$7::text::uuid,$8,\
				 $9::text::uuid,$10,$11::text::uuid,$12::text::uuid,$13,$14::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.routing_policy_id,
					&request.expected_routing_policy_revision,
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
		parse_response(&response, request)
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

fn parse_response(
	response: &[u8],
	request: &RouteAccount,
) -> Result<RoutingCommandOutcome<PersistedRoutingDecision>, StoreError> {
	let envelope: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("stored Routing Decision response bytes are malformed".into())
	})?;
	require_keys(&envelope, &["classification", "effect"])?;
	let classification = text(&envelope, "classification")?;
	let effect = envelope.get("effect").ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision effect is missing".into())
	})?;
	if classification == "stable_domain_rejection" {
		require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
		validate_digest(effect)?;
		let code = text(effect, "rejection")?;
		if text(effect, "operation")? != "route_account"
			|| !matches!(
				code,
				"malformed_input"
					| "stale_routing_policy"
					| "stale_consumer"
					| "snapshot_missing"
					| "concurrent_authority_change"
			) {
			return incompatible("stored Routing Decision rejection is unknown or cross-linked");
		}
		return Ok(RoutingCommandOutcome::Rejected(RoutingRejection {
			operation: "route_account".to_owned(),
			code: code.to_owned(),
		}));
	}
	if classification != "success" {
		return incompatible("stored Routing Decision response classification is unknown");
	}
	require_keys(
		effect,
		&[
			"capability_facts",
			"causes",
			"consumer_kind",
			"conversation_id",
			"conversation_revision",
			"decided_at_micros",
			"decision_id",
			"effect_digest",
			"effect_digest_source",
			"exclusions",
			"kind",
			"managed_execution_id",
			"managed_run_id",
			"managed_run_revision",
			"members",
			"no_route_reason",
			"operation",
			"operation_id",
			"quota_facts",
			"selected_account_id",
			"snapshot_id",
			"source_runtime_session_id",
			"source_runtime_session_revision",
			"turn_id",
			"waiting_ready_at_micros",
		],
	)?;
	validate_digest(effect)?;
	if text(effect, "operation")? != "route_account"
		|| text(effect, "operation_id")? != request.operation_id
		|| !effect_matches_consumer(effect, &request.consumer)?
	{
		return incompatible("stored Routing Decision response is cross-linked");
	}
	let snapshot_id = uuid(effect, "snapshot_id")?;
	let members = parse_members(array(effect, "members")?)?;
	let quota_facts = parse_quota_facts(array(effect, "quota_facts")?, &members)?;
	let capability_facts = parse_capability_facts(array(effect, "capability_facts")?, &members)?;
	let expected = decide_routing(&RoutingDecisionSnapshot {
		snapshot_id: snapshot_id.clone(),
		decided_at_micros: nonnegative_i64(effect, "decided_at_micros")?,
		members,
		quota_facts,
		capability_facts,
	})
	.map_err(|_| {
		StoreError::Incompatible("database-authored Routing Decision snapshot is incomplete".into())
	})?;
	let actual = parse_decision(effect, snapshot_id)?;
	if actual != expected {
		return incompatible("persisted Routing Decision differs from the pure routing kernel");
	}
	Ok(RoutingCommandOutcome::Success(PersistedRoutingDecision {
		decision_id: uuid(effect, "decision_id")?,
		operation_id: request.operation_id.clone(),
		consumer: request.consumer.clone(),
		decided_at_micros: nonnegative_i64(effect, "decided_at_micros")?,
		decision: actual,
	}))
}

fn effect_matches_consumer(
	effect: &Value,
	consumer: &ExecutionConsumer,
) -> Result<bool, StoreError> {
	if text(effect, "consumer_kind")? != consumer.as_sql() {
		return Ok(false);
	}
	match consumer {
		ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision,
			source_runtime_session_id,
			source_runtime_session_revision,
			turn_id,
		} => Ok(optional_text(effect, "conversation_id")? == Some(conversation_id.as_str())
			&& optional_positive_i64(effect, "conversation_revision")?
				== Some(*conversation_revision)
			&& optional_text(effect, "source_runtime_session_id")?
				== source_runtime_session_id.as_ref().map(decodex_core::RuntimeSessionId::as_str)
			&& optional_positive_i64(effect, "source_runtime_session_revision")?
				== *source_runtime_session_revision
			&& optional_text(effect, "turn_id")? == Some(turn_id.as_str())
			&& optional_text(effect, "managed_run_id")?.is_none()
			&& optional_positive_i64(effect, "managed_run_revision")?.is_none()
			&& optional_text(effect, "managed_execution_id")?.is_none()),
		ExecutionConsumer::ManagedRunExecution {
			managed_run_id,
			managed_run_revision,
			execution_id,
		} => Ok(optional_text(effect, "conversation_id")?.is_none()
			&& optional_positive_i64(effect, "conversation_revision")?.is_none()
			&& optional_text(effect, "source_runtime_session_id")?.is_none()
			&& optional_positive_i64(effect, "source_runtime_session_revision")?.is_none()
			&& optional_text(effect, "turn_id")?.is_none()
			&& optional_text(effect, "managed_run_id")? == Some(managed_run_id.as_str())
			&& optional_positive_i64(effect, "managed_run_revision")?
				== Some(*managed_run_revision)
			&& optional_text(effect, "managed_execution_id")? == Some(execution_id.as_str())),
	}
}

fn parse_decision(effect: &Value, snapshot_id: String) -> Result<RoutingDecision, StoreError> {
	let kind = match text(effect, "kind")? {
		"selected" => RoutingDecisionKind::Selected,
		"waiting_usage" => RoutingDecisionKind::WaitingUsage,
		"waiting_reconciliation" => RoutingDecisionKind::WaitingReconciliation,
		"no_route" => RoutingDecisionKind::NoRoute,
		_ => return incompatible("stored Routing Decision kind is unknown"),
	};
	let selected_account_id = optional_text(effect, "selected_account_id")?
		.map(AccountId::new)
		.transpose()
		.map_err(|_| StoreError::Incompatible("stored selected account is malformed".into()))?;
	let ready_at_micros = optional_i64(effect, "waiting_ready_at_micros")?;
	let no_route_reason = match optional_text(effect, "no_route_reason")? {
		Some("blocked_evidence") => Some(RoutingNoRouteReason::BlockedEvidence),
		None => None,
		Some(_) => return incompatible("stored Routing Decision no-route reason is unknown"),
	};
	Ok(RoutingDecision {
		snapshot_id,
		kind,
		selected_account_id,
		ready_at_micros,
		no_route_reason,
		exclusions: parse_exclusions(array(effect, "exclusions")?)?,
		causes: parse_causes(array(effect, "causes")?)?,
	})
}

fn parse_causes(values: &[Value]) -> Result<Vec<RoutingDecisionCause>, StoreError> {
	values
		.iter()
		.map(|value| {
			require_keys(value, &["account_id", "blocker"])?;
			Ok(RoutingDecisionCause {
				account_id: AccountId::new(text(value, "account_id")?.to_owned()).map_err(
					|_| StoreError::Incompatible("stored route-cause account is malformed".into()),
				)?,
				blocker: parse_blocker(value.get("blocker").ok_or_else(|| {
					StoreError::Incompatible("stored route cause is incomplete".into())
				})?)?,
			})
		})
		.collect()
}

fn parse_members(values: &[Value]) -> Result<Vec<RoutingDecisionCandidate>, StoreError> {
	let mut members = Vec::with_capacity(values.len());
	for (index, value) in values.iter().enumerate() {
		require_keys(value, &["account_id", "blockers", "disposition", "position", "sticky"])?;
		let position = positive_usize(value, "position")?;
		if position != index + 1 {
			return incompatible("stored Routing Decision candidate order is noncanonical");
		}
		let disposition = match text(value, "disposition")? {
			"included" => RoutingMemberDisposition::Included,
			"excluded" => RoutingMemberDisposition::Excluded,
			_ => return incompatible("stored candidate disposition is unknown"),
		};
		let blockers =
			array(value, "blockers")?.iter().map(parse_blocker).collect::<Result<Vec<_>, _>>()?;
		if (disposition == RoutingMemberDisposition::Excluded)
			!= blockers.contains(&RoutingBlocker::ExcludedByPolicy)
		{
			return incompatible("stored candidate blocker disposition is inconsistent");
		}
		members.push(RoutingDecisionCandidate {
			position,
			account_id: AccountId::new(text(value, "account_id")?.to_owned()).map_err(|_| {
				StoreError::Incompatible("stored candidate account is malformed".into())
			})?,
			disposition,
			sticky: boolean(value, "sticky")?,
			blockers,
		});
	}
	Ok(members)
}

fn parse_quota_facts(
	values: &[Value],
	members: &[RoutingDecisionCandidate],
) -> Result<Vec<RoutingDecisionQuotaFact>, StoreError> {
	if values.len() != members.len() * 2 {
		return incompatible("stored Routing Decision quota matrix is incomplete");
	}
	let mut facts = Vec::with_capacity(values.len());
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
				"raw_observed_at",
				"raw_resets_at",
				"remaining_percent",
				"resets_at_micros",
				"source_id",
				"timestamp_precision",
				"window_class",
			],
		)?;
		let member = &members[index / 2];
		let position = positive_usize(value, "position")?;
		let window = match text(value, "window_class")? {
			"five_hour" if position == 1 => QuotaWindowClass::FiveHour,
			"seven_day" if position == 2 => QuotaWindowClass::SevenDay,
			_ =>
				return incompatible("stored Routing Decision quota duration identity is malformed"),
		};
		if text(value, "account_id")? != member.account_id.as_str() {
			return incompatible("stored Routing Decision quota matrix is reordered");
		}
		let revision = optional_positive_i64(value, "observation_revision")?;
		let source = optional_text(value, "source_id")?;
		let precision = optional_text(value, "timestamp_precision")?;
		let observed_raw = optional_text(value, "raw_observed_at")?;
		let reset_raw = optional_text(value, "raw_resets_at")?;
		let observed_at_micros = optional_i64(value, "observed_at_micros")?;
		let resets_at_micros = optional_i64(value, "resets_at_micros")?;
		let provenance = match (revision, source, precision, observed_raw, reset_raw) {
			(
				Some(revision),
				Some(source),
				Some("unix_microsecond"),
				Some(observed),
				Some(reset),
			) => {
				if observed_at_micros.map(|value| value.to_string()) != Some(observed.to_owned())
					|| resets_at_micros.map(|value| value.to_string()) != Some(reset.to_owned())
				{
					return incompatible(
						"stored Routing Decision raw timestamp provenance is malformed",
					);
				}
				(
					Some(timestamp_provenance(source, observed, revision)),
					Some(timestamp_provenance(source, reset, revision)),
				)
			},
			(_, None, None, None, None) => (None, None),
			_ => return incompatible("stored Routing Decision timestamp provenance is partial"),
		};
		facts.push(RoutingDecisionQuotaFact {
			account_id: member.account_id.clone(),
			window,
			duration_minutes: u16::try_from(unsigned(value, "duration_minutes")?).map_err(
				|_| StoreError::Incompatible("stored quota duration is malformed".into()),
			)?,
			observation_revision: revision,
			remaining_percent: optional_u8(value, "remaining_percent")?,
			resets_at_micros,
			observed_at_micros,
			confidence: optional_confidence(value, "confidence")?,
			observed_at_provenance: provenance.0,
			resets_at_provenance: provenance.1,
		});
	}
	Ok(facts)
}

fn parse_capability_facts(
	values: &[Value],
	members: &[RoutingDecisionCandidate],
) -> Result<Vec<RoutingSnapshotCapabilityFact>, StoreError> {
	if values.len() != members.len() * 8 {
		return incompatible("stored Routing Decision capability matrix is incomplete");
	}
	let mut result = Vec::with_capacity(values.len());
	for (index, value) in values.iter().enumerate() {
		require_keys(
			value,
			&["account_id", "applicable", "capability", "evidence_state", "position"],
		)?;
		let capability = text(value, "capability")?;
		const CAPABILITIES: [&str; 8] = [
			"initialize",
			"account_read",
			"thread_list",
			"thread_read",
			"thread_archive",
			"paginated_history",
			"native_collaboration",
			"thread_search",
		];
		if text(value, "account_id")? != members[index / 8].account_id.as_str()
			|| positive_usize(value, "position")? != index % 8 + 1
			|| capability != CAPABILITIES[index % 8]
		{
			return incompatible("stored Routing Decision capability matrix is reordered");
		}
		let evidence_state = match optional_text(value, "evidence_state")? {
			Some("supported") => Some(RoutingCapabilityState::Supported),
			Some("unsupported_schema_missing") =>
				Some(RoutingCapabilityState::UnsupportedSchemaMissing),
			Some("unsupported_method_not_found") =>
				Some(RoutingCapabilityState::UnsupportedMethodNotFound),
			Some("unsupported_codex_rejected") =>
				Some(RoutingCapabilityState::UnsupportedCodexRejected),
			Some("unavailable_not_probed") => Some(RoutingCapabilityState::UnavailableNotProbed),
			Some("unavailable_probe_failed") =>
				Some(RoutingCapabilityState::UnavailableProbeFailed),
			Some("degraded_legacy_history_only") =>
				Some(RoutingCapabilityState::DegradedLegacyHistoryOnly),
			Some(_) => return incompatible("stored Routing Decision capability state is unknown"),
			None => None,
		};
		result.push(RoutingSnapshotCapabilityFact {
			account_id: members[index / 8].account_id.clone(),
			capability: CodexCapability::ALL[index % 8],
			applicable: boolean(value, "applicable")?,
			evidence_state,
		});
	}
	Ok(result)
}

fn parse_exclusions(values: &[Value]) -> Result<Vec<RoutingDecisionExclusion>, StoreError> {
	values
		.iter()
		.map(|value| {
			require_keys(
				value,
				&[
					"account_id",
					"confidence",
					"duration_minutes",
					"member_position",
					"observation_revision",
					"observed_at_micros",
					"raw_observed_at",
					"raw_resets_at",
					"reason",
					"remaining_percent",
					"resets_at_micros",
					"source_id",
					"timestamp_precision",
					"window_class",
				],
			)?;
			if text(value, "reason")? != "usage_depleted"
				|| unsigned(value, "remaining_percent")? != 0
				|| text(value, "timestamp_precision")? != "unix_microsecond"
			{
				return incompatible(
					"stored Routing Decision exclusion is not exact depletion evidence",
				);
			}
			let revision = positive_i64(value, "observation_revision")?;
			let source = text(value, "source_id")?;
			let observed_at_micros = nonnegative_i64(value, "observed_at_micros")?;
			let resets_at_micros = nonnegative_i64(value, "resets_at_micros")?;
			if text(value, "raw_observed_at")? != observed_at_micros.to_string()
				|| text(value, "raw_resets_at")? != resets_at_micros.to_string()
			{
				return incompatible("stored exclusion raw timestamp provenance is malformed");
			}
			Ok(RoutingDecisionExclusion {
				account_id: AccountId::new(text(value, "account_id")?.to_owned()).map_err(
					|_| StoreError::Incompatible("stored exclusion account is malformed".into()),
				)?,
				member_position: positive_usize(value, "member_position")?,
				window: match text(value, "window_class")? {
					"five_hour" => QuotaWindowClass::FiveHour,
					"seven_day" => QuotaWindowClass::SevenDay,
					_ => return incompatible("stored exclusion window is unknown"),
				},
				duration_minutes: u16::try_from(unsigned(value, "duration_minutes")?).map_err(
					|_| StoreError::Incompatible("stored exclusion duration is malformed".into()),
				)?,
				observation_revision: revision,
				remaining_percent: 0,
				observed_at_micros,
				resets_at_micros,
				confidence: match text(value, "confidence")? {
					"high" => ObservationConfidence::High,
					_ => return incompatible("stored exclusion confidence is not high"),
				},
				observed_at_provenance: timestamp_provenance(
					source,
					text(value, "raw_observed_at")?,
					revision,
				),
				resets_at_provenance: timestamp_provenance(
					source,
					text(value, "raw_resets_at")?,
					revision,
				),
			})
		})
		.collect()
}

fn timestamp_provenance(source: &str, raw: &str, revision: i64) -> RoutingTimestampProvenance {
	RoutingTimestampProvenance {
		raw_value: raw.to_owned(),
		source_id: source.to_owned(),
		precision: RoutingTimestampPrecision::UnixMicrosecond,
		evidence_revision: revision,
	}
}

fn parse_blocker(value: &Value) -> Result<RoutingBlocker, StoreError> {
	let value = value
		.as_str()
		.ok_or_else(|| StoreError::Incompatible("stored blocker is malformed".into()))?;
	RoutingBlocker::from_sql(value)
		.ok_or_else(|| StoreError::Incompatible("stored blocker is unknown".into()))
}

fn validate_digest(effect: &Value) -> Result<(), StoreError> {
	let source = text(effect, "effect_digest_source")?;
	let digest = text(effect, "effect_digest")?;
	let parsed: Value = serde_json::from_str(source).map_err(|_| {
		StoreError::Incompatible("stored Routing Decision digest source is malformed".into())
	})?;
	let mut projected = effect.clone();
	let object = projected.as_object_mut().ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision effect is malformed".into())
	})?;
	object.remove("effect_digest");
	object.remove("effect_digest_source");
	if parsed != projected || hex_sha256(source.as_bytes()) != digest {
		return incompatible("stored Routing Decision effect digest is invalid");
	}
	Ok(())
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value.as_object().ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision object is malformed".into())
	})?;
	let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
	let expected = expected.iter().copied().collect::<BTreeSet<_>>();
	if actual == expected {
		Ok(())
	} else {
		incompatible("stored Routing Decision object has missing or unknown keys")
	}
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value.get(key).and_then(Value::as_array).map(Vec::as_slice).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision array is malformed".into())
	})
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored Routing Decision text is malformed".into()))
}
fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value)),
		_ => incompatible("stored Routing Decision optional text is malformed"),
	}
}
fn boolean(value: &Value, key: &str) -> Result<bool, StoreError> {
	value.get(key).and_then(Value::as_bool).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision boolean is malformed".into())
	})
}
fn unsigned(value: &Value, key: &str) -> Result<u64, StoreError> {
	value.get(key).and_then(Value::as_u64).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision unsigned integer is malformed".into())
	})
}
fn positive_usize(value: &Value, key: &str) -> Result<usize, StoreError> {
	usize::try_from(unsigned(value, key)?).ok().filter(|value| *value > 0).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision position is malformed".into())
	})
}
fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value.get(key).and_then(Value::as_i64).filter(|value| *value > 0).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision revision is malformed".into())
	})
}
fn nonnegative_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value.get(key).and_then(Value::as_i64).filter(|value| *value >= 0).ok_or_else(|| {
		StoreError::Incompatible("stored Routing Decision timestamp is malformed".into())
	})
}
fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value.as_i64().filter(|value| *value >= 0).map(Some).ok_or_else(|| {
			StoreError::Incompatible("stored Routing Decision optional integer is malformed".into())
		}),
		None => incompatible("stored Routing Decision optional integer is missing"),
	}
}
fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match optional_i64(value, key)? {
		Some(value) if value > 0 => Ok(Some(value)),
		Some(_) => incompatible("stored Routing Decision optional revision is malformed"),
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
			.ok_or_else(|| {
				StoreError::Incompatible("stored Routing Decision percent is malformed".into())
			}),
		None => incompatible("stored Routing Decision percent is missing"),
	}
}
fn optional_confidence(
	value: &Value,
	key: &str,
) -> Result<Option<ObservationConfidence>, StoreError> {
	match optional_text(value, key)? {
		Some("unknown") => Ok(Some(ObservationConfidence::Unknown)),
		Some("low") => Ok(Some(ObservationConfidence::Low)),
		Some("high") => Ok(Some(ObservationConfidence::High)),
		Some(_) => incompatible("stored Routing Decision confidence is unknown"),
		None => Ok(None),
	}
}
fn uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?;
	if is_uuid(value) {
		Ok(value.to_owned())
	} else {
		incompatible("stored Routing Decision UUID is malformed")
	}
}
fn validate_uuid(value: &str, field: &'static str) -> Result<(), StoreError> {
	if is_uuid(value) { Ok(()) } else { Err(StoreError::InvalidInput(field)) }
}
fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}
fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
fn incompatible<T>(reason: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(reason.to_owned()))
}
