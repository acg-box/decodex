use std::collections::BTreeSet;

use decodex_core::{
	AccountId, CodexCapability, ManagedRunId, ObservationConfidence, QuotaWindowClass,
	RoutingBlocker, RoutingCapabilityState, RoutingCommandOutcome, RoutingDecision,
	RoutingDecisionCandidate, RoutingDecisionExclusion, RoutingDecisionKind,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteAccount {
	pub operation_id: String,
	pub routing_policy_id: String,
	pub expected_routing_policy_revision: i64,
	pub managed_run_id: ManagedRunId,
	pub expected_managed_run_revision: i64,
}

/// Exact immutable decision read back after PostgreSQL commits the complete evidence set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedRoutingDecision {
	pub decision_id: String,
	pub operation_id: String,
	pub decided_at_micros: i64,
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
		if request.expected_routing_policy_revision <= 0
			|| request.expected_managed_run_revision <= 0
		{
			return Err(StoreError::InvalidInput("routing decision revisions must be positive"));
		}
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.route_account_exact($1,$2,$3::text::uuid,$4::text::uuid,$5,\
				 $6::text::uuid,$7)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.routing_policy_id,
					&request.expected_routing_policy_revision,
					&request.managed_run_id.as_str(),
					&request.expected_managed_run_revision,
				],
			)
			.await?;
		parse_response(&response, request)
	}
}

fn parse_response(
	response: &[u8],
	request: &RouteAccount,
) -> Result<RoutingCommandOutcome<PersistedRoutingDecision>, StoreError> {
	let envelope: Value = serde_json::from_slice(response)
		.map_err(|_| StoreError::Incompatible("stored V16 response bytes are malformed".into()))?;
	require_keys(&envelope, &["classification", "effect"])?;
	let classification = text(&envelope, "classification")?;
	let effect = envelope
		.get("effect")
		.ok_or_else(|| StoreError::Incompatible("stored V16 effect is missing".into()))?;
	if classification == "stable_domain_rejection" {
		require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
		validate_digest(effect)?;
		let code = text(effect, "rejection")?;
		if text(effect, "operation")? != "route_account"
			|| !matches!(
				code,
				"malformed_input"
					| "stale_routing_policy"
					| "stale_managed_run"
					| "snapshot_missing"
					| "concurrent_authority_change"
			) {
			return incompatible("stored V16 rejection is unknown or cross-linked");
		}
		return Ok(RoutingCommandOutcome::Rejected(RoutingRejection {
			operation: "route_account".to_owned(),
			code: code.to_owned(),
		}));
	}
	if classification != "completed_success" {
		return incompatible("stored V16 response classification is unknown");
	}
	require_keys(
		effect,
		&[
			"capability_facts",
			"decided_at_micros",
			"decision_id",
			"effect_digest",
			"effect_digest_source",
			"exclusions",
			"kind",
			"members",
			"no_route_reason",
			"operation",
			"operation_id",
			"quota_facts",
			"selected_account_id",
			"snapshot_id",
			"waiting_ready_at_micros",
		],
	)?;
	validate_digest(effect)?;
	if text(effect, "operation")? != "route_account"
		|| text(effect, "operation_id")? != request.operation_id
	{
		return incompatible("stored V16 response is cross-linked");
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
	.map_err(|_| StoreError::Incompatible("database-authored V16 snapshot is incomplete".into()))?;
	let actual = parse_decision(effect, snapshot_id)?;
	if actual != expected {
		return incompatible("persisted V16 decision differs from the pure routing kernel");
	}
	Ok(RoutingCommandOutcome::Success(PersistedRoutingDecision {
		decision_id: uuid(effect, "decision_id")?,
		operation_id: request.operation_id.clone(),
		decided_at_micros: nonnegative_i64(effect, "decided_at_micros")?,
		decision: actual,
	}))
}

fn parse_decision(effect: &Value, snapshot_id: String) -> Result<RoutingDecision, StoreError> {
	let kind = match text(effect, "kind")? {
		"selected" => RoutingDecisionKind::Selected,
		"waiting_usage" => RoutingDecisionKind::WaitingUsage,
		"no_route" => RoutingDecisionKind::NoRoute,
		_ => return incompatible("stored V16 decision kind is unknown"),
	};
	let selected_account_id = optional_text(effect, "selected_account_id")?
		.map(AccountId::new)
		.transpose()
		.map_err(|_| StoreError::Incompatible("stored selected account is malformed".into()))?;
	let ready_at_micros = optional_i64(effect, "waiting_ready_at_micros")?;
	let no_route_reason = match optional_text(effect, "no_route_reason")? {
		Some("blocked_evidence") => Some(RoutingNoRouteReason::BlockedEvidence),
		None => None,
		Some(_) => return incompatible("stored V16 no-route reason is unknown"),
	};
	Ok(RoutingDecision {
		snapshot_id,
		kind,
		selected_account_id,
		ready_at_micros,
		no_route_reason,
		exclusions: parse_exclusions(array(effect, "exclusions")?)?,
	})
}

fn parse_members(values: &[Value]) -> Result<Vec<RoutingDecisionCandidate>, StoreError> {
	let mut members = Vec::with_capacity(values.len());
	for (index, value) in values.iter().enumerate() {
		require_keys(value, &["account_id", "blockers", "disposition", "position", "sticky"])?;
		let position = positive_usize(value, "position")?;
		if position != index + 1 {
			return incompatible("stored V16 candidate order is noncanonical");
		}
		members.push(RoutingDecisionCandidate {
			position,
			account_id: AccountId::new(text(value, "account_id")?.to_owned()).map_err(|_| {
				StoreError::Incompatible("stored candidate account is malformed".into())
			})?,
			disposition: match text(value, "disposition")? {
				"included" => RoutingMemberDisposition::Included,
				"excluded" => RoutingMemberDisposition::Excluded,
				_ => return incompatible("stored candidate disposition is unknown"),
			},
			sticky: boolean(value, "sticky")?,
			blockers: array(value, "blockers")?
				.iter()
				.map(parse_blocker)
				.collect::<Result<Vec<_>, _>>()?,
		});
	}
	Ok(members)
}

fn parse_quota_facts(
	values: &[Value],
	members: &[RoutingDecisionCandidate],
) -> Result<Vec<RoutingDecisionQuotaFact>, StoreError> {
	if values.len() != members.len() * 2 {
		return incompatible("stored V16 quota matrix is incomplete");
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
			_ => return incompatible("stored V16 quota duration identity is malformed"),
		};
		if text(value, "account_id")? != member.account_id.as_str() {
			return incompatible("stored V16 quota matrix is reordered");
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
					return incompatible("stored V16 raw timestamp provenance is malformed");
				}
				(
					Some(timestamp_provenance(source, observed, revision)),
					Some(timestamp_provenance(source, reset, revision)),
				)
			},
			(_, None, None, None, None) => (None, None),
			_ => return incompatible("stored V16 timestamp provenance is partial"),
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
		return incompatible("stored V16 capability matrix is incomplete");
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
			return incompatible("stored V16 capability matrix is reordered");
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
			Some(_) => return incompatible("stored V16 capability state is unknown"),
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
				return incompatible("stored V16 exclusion is not exact depletion evidence");
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
	use RoutingBlocker::*;
	Ok(match value {
		"excluded_by_policy" => ExcludedByPolicy,
		"account_from_future" => AccountFromFuture,
		"account_stale" => AccountStale,
		"account_unavailable" => AccountUnavailable,
		"account_unknown" => AccountUnknown,
		"account_depleted" => AccountDepleted,
		"account_auth_failed" => AccountAuthFailed,
		"account_plugin_unready" => AccountPluginUnready,
		"account_disabled" => AccountDisabled,
		"evidence_missing" => EvidenceMissing,
		"evidence_from_future" => EvidenceFromFuture,
		"evidence_stale" => EvidenceStale,
		"evidence_account_mismatch" => EvidenceAccountMismatch,
		"evidence_profile_mismatch" => EvidenceProfileMismatch,
		"evidence_build_mismatch" => EvidenceBuildMismatch,
		"quota_five_hour_missing" => QuotaFiveHourMissing,
		"quota_five_hour_from_future" => QuotaFiveHourFromFuture,
		"quota_five_hour_stale" => QuotaFiveHourStale,
		"quota_five_hour_unknown" => QuotaFiveHourUnknown,
		"quota_five_hour_reset_elapsed" => QuotaFiveHourResetElapsed,
		"quota_five_hour_depleted" => QuotaFiveHourDepleted,
		"quota_seven_day_missing" => QuotaSevenDayMissing,
		"quota_seven_day_from_future" => QuotaSevenDayFromFuture,
		"quota_seven_day_stale" => QuotaSevenDayStale,
		"quota_seven_day_unknown" => QuotaSevenDayUnknown,
		"quota_seven_day_reset_elapsed" => QuotaSevenDayResetElapsed,
		"quota_seven_day_depleted" => QuotaSevenDayDepleted,
		"required_capability_unsatisfied" => RequiredCapabilityUnsatisfied,
		_ => return incompatible("stored blocker is unknown"),
	})
}

fn validate_digest(effect: &Value) -> Result<(), StoreError> {
	let source = text(effect, "effect_digest_source")?;
	let digest = text(effect, "effect_digest")?;
	let parsed: Value = serde_json::from_str(source)
		.map_err(|_| StoreError::Incompatible("stored V16 digest source is malformed".into()))?;
	let mut projected = effect.clone();
	let object = projected
		.as_object_mut()
		.ok_or_else(|| StoreError::Incompatible("stored V16 effect is malformed".into()))?;
	object.remove("effect_digest");
	object.remove("effect_digest_source");
	if parsed != projected || hex_sha256(source.as_bytes()) != digest {
		return incompatible("stored V16 effect digest is invalid");
	}
	Ok(())
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value
		.as_object()
		.ok_or_else(|| StoreError::Incompatible("stored V16 object is malformed".into()))?;
	let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
	let expected = expected.iter().copied().collect::<BTreeSet<_>>();
	if actual == expected {
		Ok(())
	} else {
		incompatible("stored V16 object has missing or unknown keys")
	}
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value
		.get(key)
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.ok_or_else(|| StoreError::Incompatible("stored V16 array is malformed".into()))
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored V16 text is malformed".into()))
}
fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value)),
		_ => incompatible("stored V16 optional text is malformed"),
	}
}
fn boolean(value: &Value, key: &str) -> Result<bool, StoreError> {
	value
		.get(key)
		.and_then(Value::as_bool)
		.ok_or_else(|| StoreError::Incompatible("stored V16 boolean is malformed".into()))
}
fn unsigned(value: &Value, key: &str) -> Result<u64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_u64)
		.ok_or_else(|| StoreError::Incompatible("stored V16 unsigned integer is malformed".into()))
}
fn positive_usize(value: &Value, key: &str) -> Result<usize, StoreError> {
	usize::try_from(unsigned(value, key)?)
		.ok()
		.filter(|value| *value > 0)
		.ok_or_else(|| StoreError::Incompatible("stored V16 position is malformed".into()))
}
fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value > 0)
		.ok_or_else(|| StoreError::Incompatible("stored V16 revision is malformed".into()))
}
fn nonnegative_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value >= 0)
		.ok_or_else(|| StoreError::Incompatible("stored V16 timestamp is malformed".into()))
}
fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value.as_i64().filter(|value| *value >= 0).map(Some).ok_or_else(|| {
			StoreError::Incompatible("stored V16 optional integer is malformed".into())
		}),
		None => incompatible("stored V16 optional integer is missing"),
	}
}
fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match optional_i64(value, key)? {
		Some(value) if value > 0 => Ok(Some(value)),
		Some(_) => incompatible("stored V16 optional revision is malformed"),
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
			.ok_or_else(|| StoreError::Incompatible("stored V16 percent is malformed".into())),
		None => incompatible("stored V16 percent is missing"),
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
		Some(_) => incompatible("stored V16 confidence is unknown"),
		None => Ok(None),
	}
}
fn uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?;
	if is_uuid(value) { Ok(value.to_owned()) } else { incompatible("stored V16 UUID is malformed") }
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
