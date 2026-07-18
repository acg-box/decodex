//! Durable inert ledger-first scheduler lifecycle over exact persisted V16 `waiting_usage` lineage.

use decodex_core::{
	ManagedRunId, WaitingUsageWakeCommandOutcome, WaitingUsageWakeLease,
	WaitingUsageWakeRejection, WaitingUsageWakeState, WaitingUsageWakeTerminalReason,
	WaitingUsageWakeTransition, WaitingUsageWakeTransitionKind,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact identities accepted when registering the persisted V16 wait.
pub struct RegisterWaitingUsageWake {
	pub operation_id: String,
	pub routing_decision_id: String,
	pub expected_managed_run_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One durable scheduler operation for claiming the next database-ordered due wake.
pub struct ClaimDueWaitingUsageWake {
	pub operation_id: String,
	pub claim_id: String,
	pub holder_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact leased ledger tip accepted when sealing one fresh-resolution request.
pub struct FireWaitingUsageWake {
	pub operation_id: String,
	pub wake_id: String,
	pub expected_revision: i64,
	pub expected_transition_id: String,
	pub holder_id: String,
	pub lease_fence_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact nonterminal ledger tip accepted for cancellation.
pub struct CancelWaitingUsageWake {
	pub operation_id: String,
	pub wake_id: String,
	pub expected_revision: i64,
	pub expected_transition_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One claim result bound to the exact appended claim, reclaim, or supersede transition.
pub struct WaitingUsageWakeClaimEffect {
	pub claimed: bool,
	pub transition: WaitingUsageWakeTransition,
}

impl PostgresStore {
	/// Register at most one immutable transition and derived head for one exact V16 decision.
	pub async fn register_waiting_usage_wake(
		&self,
		idempotency_key: &str,
		request: &RegisterWaitingUsageWake,
	) -> Result<WaitingUsageWakeCommandOutcome<WaitingUsageWakeTransition>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.operation_id, "wake registration operation identity")?;
		validate_uuid(&request.routing_decision_id, "routing decision identity")?;
		positive(request.expected_managed_run_revision, "ManagedRun revision")?;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.register_waiting_usage_wake_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.routing_decision_id,
					&request.expected_managed_run_revision,
				],
			)
			.await?;
		let outcome = parse_transition_response(&response, "register_waiting_usage_wake")?;
		if let WaitingUsageWakeCommandOutcome::Success(transition) = &outcome {
			if transition.operation_id != request.operation_id
				|| transition.registration_operation_id != request.operation_id
				|| transition.routing_decision_id != request.routing_decision_id
				|| transition.managed_run_revision != request.expected_managed_run_revision
				|| transition.transition_kind != WaitingUsageWakeTransitionKind::Registered
			{
				return incompatible("stored wake registration transition is cross-linked");
			}
			self.verify_transition_readback(transition, &response).await?;
		}
		Ok(outcome)
	}

	/// Append the claim, reclaim, or stale-lineage supersession of the earliest due head.
	pub async fn claim_due_waiting_usage_wake(
		&self,
		idempotency_key: &str,
		request: &ClaimDueWaitingUsageWake,
	) -> Result<WaitingUsageWakeCommandOutcome<WaitingUsageWakeClaimEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		for (value, label) in [
			(&request.operation_id, "wake claim operation identity"),
			(&request.claim_id, "wake claim identity"),
			(&request.holder_id, "wake lease holder identity"),
		] {
			validate_uuid(value, label)?;
		}
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.claim_due_waiting_usage_wake_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.claim_id,
					&request.holder_id,
				],
			)
			.await?;
		let (classification, effect) = parse_envelope(&response)?;
		if classification == "stable_domain_rejection" {
			return Ok(WaitingUsageWakeCommandOutcome::Rejected(parse_rejection(
				&effect,
				"claim_due_waiting_usage_wake",
			)?));
		}
		if classification != "success" {
			return incompatible("stored wake claim classification is unknown");
		}
		let transition = parse_transition(&effect, "claim_due_waiting_usage_wake", true)?;
		let claimed = boolean(&effect, "claimed")?;
		if transition.operation_id != request.operation_id
			|| claimed != matches!(
				transition.transition_kind,
				WaitingUsageWakeTransitionKind::Claimed | WaitingUsageWakeTransitionKind::Reclaimed
			)
			|| claimed
				&& transition.lease.as_ref().map_or(true, |lease| {
					lease.claim_id != request.claim_id || lease.holder_id != request.holder_id
				})
		{
			return incompatible("stored wake claim transition is cross-linked");
		}
		self.verify_transition_readback(&transition, &response).await?;
		Ok(WaitingUsageWakeCommandOutcome::Success(WaitingUsageWakeClaimEffect {
			claimed,
			transition,
		}))
	}

	/// Append one fired or stale-lineage superseded transition from an exact leased tip.
	pub async fn fire_waiting_usage_wake(
		&self,
		idempotency_key: &str,
		request: &FireWaitingUsageWake,
	) -> Result<WaitingUsageWakeCommandOutcome<WaitingUsageWakeTransition>, StoreError> {
		validate_exact_key(idempotency_key)?;
		for (value, label) in [
			(&request.operation_id, "wake fire operation identity"),
			(&request.wake_id, "wake identity"),
			(&request.expected_transition_id, "expected wake transition identity"),
			(&request.holder_id, "wake lease holder identity"),
			(&request.lease_fence_id, "wake lease fence identity"),
		] {
			validate_uuid(value, label)?;
		}
		positive(request.expected_revision, "wake revision")?;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.fire_waiting_usage_wake_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,\
				 $7::text::uuid,$8::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.wake_id,
					&request.expected_revision,
					&request.expected_transition_id,
					&request.holder_id,
					&request.lease_fence_id,
				],
			)
			.await?;
		let outcome = parse_transition_response(&response, "fire_waiting_usage_wake")?;
		if let WaitingUsageWakeCommandOutcome::Success(transition) = &outcome {
			if transition.operation_id != request.operation_id
				|| transition.wake_id != request.wake_id
				|| transition.predecessor_revision != Some(request.expected_revision)
				|| transition.predecessor_transition_id.as_deref()
					!= Some(request.expected_transition_id.as_str())
				|| !matches!(
					transition.transition_kind,
					WaitingUsageWakeTransitionKind::Fired
						| WaitingUsageWakeTransitionKind::Superseded
				)
			{
				return incompatible("stored wake fire transition is cross-linked");
			}
			self.verify_transition_readback(transition, &response).await?;
		}
		Ok(outcome)
	}

	/// Append one terminal cancellation transition from an exact nonterminal tip.
	pub async fn cancel_waiting_usage_wake(
		&self,
		idempotency_key: &str,
		request: &CancelWaitingUsageWake,
	) -> Result<WaitingUsageWakeCommandOutcome<WaitingUsageWakeTransition>, StoreError> {
		validate_exact_key(idempotency_key)?;
		for (value, label) in [
			(&request.operation_id, "wake cancellation operation identity"),
			(&request.wake_id, "wake identity"),
			(&request.expected_transition_id, "expected wake transition identity"),
		] {
			validate_uuid(value, label)?;
		}
		positive(request.expected_revision, "wake revision")?;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.cancel_waiting_usage_wake_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.wake_id,
					&request.expected_revision,
					&request.expected_transition_id,
				],
			)
			.await?;
		let outcome = parse_transition_response(&response, "cancel_waiting_usage_wake")?;
		if let WaitingUsageWakeCommandOutcome::Success(transition) = &outcome {
			if transition.operation_id != request.operation_id
				|| transition.wake_id != request.wake_id
				|| transition.predecessor_revision != Some(request.expected_revision)
				|| transition.predecessor_transition_id.as_deref()
					!= Some(request.expected_transition_id.as_str())
				|| transition.transition_kind != WaitingUsageWakeTransitionKind::Cancelled
			{
				return incompatible("stored wake cancellation transition is cross-linked");
			}
			self.verify_transition_readback(transition, &response).await?;
		}
		Ok(outcome)
	}

	async fn verify_transition_readback(
		&self,
		transition: &WaitingUsageWakeTransition,
		response: &[u8],
	) -> Result<(), StoreError> {
		let client = self.pool().get().await?;
		let row = client
			.query_opt(
				"SELECT * FROM decodex.read_waiting_usage_wake_transition_exact(\
				 $1::text::uuid,$2::text::uuid)",
				&[&transition.transition_id, &transition.operation_id],
			)
			.await?
			.ok_or_else(|| StoreError::Incompatible("wake result lost its immutable transition".into()))?;
		let lease = transition.lease.as_ref();
		let terminal_reason = transition.terminal_reason.map(terminal_reason_sql);
		let (_, effect) = parse_envelope(response)?;
		if row.get::<_, String>(0) != transition.transition_id
			|| row.get::<_, String>(1) != transition.wake_id
			|| row.get::<_, i64>(2) != transition.revision
			|| row.get::<_, Option<i64>>(3) != transition.predecessor_revision
			|| row.get::<_, Option<String>>(4).as_deref()
				!= transition.predecessor_transition_id.as_deref()
			|| row.get::<_, String>(5) != transition.operation_id
			|| row.get::<_, String>(6) != transition_kind_sql(transition.transition_kind)
			|| row.get::<_, String>(7) != transition.registration_operation_id
			|| row.get::<_, String>(8) != transition.routing_decision_id
			|| row.get::<_, i64>(9) != transition.routing_decision_revision
			|| row.get::<_, String>(10) != transition.routing_policy_id
			|| row.get::<_, i64>(11) != transition.routing_policy_revision
			|| row.get::<_, String>(12) != transition.managed_run_id.as_str()
			|| row.get::<_, i64>(13) != transition.managed_run_revision
			|| row.get::<_, i64>(14) != transition.earliest_ready_at_micros
			|| row.get::<_, String>(15) != state_sql(transition.state)
			|| row.get::<_, Option<String>>(16) != lease.map(|value| value.claim_id.clone())
			|| row.get::<_, Option<String>>(17) != lease.map(|value| value.holder_id.clone())
			|| row.get::<_, Option<String>>(18) != lease.map(|value| value.lease_fence_id.clone())
			|| row.get::<_, Option<i64>>(19) != lease.map(|value| value.acquired_at_micros)
			|| row.get::<_, Option<i64>>(20) != lease.map(|value| value.expires_at_micros)
			|| row.get::<_, i64>(21) != transition.registered_at_micros
			|| row.get::<_, i64>(22) != transition.transitioned_at_micros
			|| row.get::<_, Option<String>>(23).as_deref() != terminal_reason
			|| row.get::<_, Option<String>>(24).as_deref()
				!= transition.routing_resolution_request_id.as_deref()
			|| !row.get::<_, bool>(25)
			|| row.get::<_, bool>(26)
			|| row.get::<_, bool>(27)
			|| row.get::<_, Value>(28) != effect
			|| row.get::<_, Vec<u8>>(29).as_slice() != response
		{
			return incompatible("wake immutable transition readback differs from its command result");
		}
		Ok(())
	}
}

fn parse_transition_response(
	response: &[u8],
	operation: &str,
) -> Result<WaitingUsageWakeCommandOutcome<WaitingUsageWakeTransition>, StoreError> {
	let (classification, effect) = parse_envelope(response)?;
	if classification == "stable_domain_rejection" {
		return Ok(WaitingUsageWakeCommandOutcome::Rejected(parse_rejection(&effect, operation)?));
	}
	if classification != "success" {
		return incompatible("stored waiting-usage wake response classification is unknown");
	}
	Ok(WaitingUsageWakeCommandOutcome::Success(parse_transition(
		&effect, operation, false,
	)?))
}

fn parse_transition(
	effect: &Value,
	operation: &str,
	claim_effect: bool,
) -> Result<WaitingUsageWakeTransition, StoreError> {
	let ordinary_keys = [
		"activity_effects", "claim_id", "earliest_ready_at_micros", "effect_digest",
		"effect_digest_source", "fresh_routing_resolution_only", "lease_acquired_at_micros",
		"lease_expires_at_micros", "lease_fence_id", "lease_holder", "managed_run_id",
		"managed_run_revision", "operation", "operation_id", "outbox_effects",
		"predecessor_revision", "predecessor_transition_id", "prior_decision_reusable",
		"production_enabled", "registered_at_micros", "registration_operation_id", "revision",
		"routing_decision_id", "routing_decision_revision", "routing_policy_id",
		"routing_policy_revision", "routing_resolution_request_id", "state", "terminal_reason",
		"transition_id", "transition_kind", "transitioned_at_micros", "wake_id",
	];
	let claim_keys = [
		"activity_effects", "claim_id", "claimed", "earliest_ready_at_micros", "effect_digest",
		"effect_digest_source", "fresh_routing_resolution_only", "lease_acquired_at_micros",
		"lease_expires_at_micros", "lease_fence_id", "lease_holder", "managed_run_id",
		"managed_run_revision", "operation", "operation_id", "outbox_effects",
		"predecessor_revision", "predecessor_transition_id", "prior_decision_reusable",
		"production_enabled", "registered_at_micros", "registration_operation_id", "revision",
		"routing_decision_id", "routing_decision_revision", "routing_policy_id",
		"routing_policy_revision", "routing_resolution_request_id", "state", "terminal_reason",
		"transition_id", "transition_kind", "transitioned_at_micros", "wake_id",
	];
	require_keys(effect, if claim_effect { &claim_keys } else { &ordinary_keys })?;
	validate_digest(effect)?;
	if text(effect, "operation")? != operation {
		return incompatible("stored waiting-usage wake operation is cross-linked");
	}
	let state = match text(effect, "state")? {
		"pending" => WaitingUsageWakeState::Pending,
		"leased" => WaitingUsageWakeState::Leased,
		"fired" => WaitingUsageWakeState::Fired,
		"cancelled" => WaitingUsageWakeState::Cancelled,
		"superseded" => WaitingUsageWakeState::Superseded,
		_ => return incompatible("stored waiting-usage wake state is unknown"),
	};
	let transition_kind = match text(effect, "transition_kind")? {
		"registered" => WaitingUsageWakeTransitionKind::Registered,
		"claimed" => WaitingUsageWakeTransitionKind::Claimed,
		"reclaimed" => WaitingUsageWakeTransitionKind::Reclaimed,
		"fired" => WaitingUsageWakeTransitionKind::Fired,
		"cancelled" => WaitingUsageWakeTransitionKind::Cancelled,
		"superseded" => WaitingUsageWakeTransitionKind::Superseded,
		_ => return incompatible("stored waiting-usage wake transition kind is unknown"),
	};
	let claim_id = optional_uuid(effect, "claim_id")?;
	let holder_id = optional_uuid(effect, "lease_holder")?;
	let lease_fence_id = optional_uuid(effect, "lease_fence_id")?;
	let lease_acquired = optional_i64(effect, "lease_acquired_at_micros")?;
	let lease_expires = optional_i64(effect, "lease_expires_at_micros")?;
	let lease = match (claim_id, holder_id, lease_fence_id, lease_acquired, lease_expires) {
		(Some(claim_id), Some(holder_id), Some(lease_fence_id), Some(acquired_at_micros),
			Some(expires_at_micros)) if state == WaitingUsageWakeState::Leased => {
			Some(WaitingUsageWakeLease {
				claim_id,
				holder_id,
				lease_fence_id,
				acquired_at_micros,
				expires_at_micros,
			})
		}
		(None, None, None, None, None) if state != WaitingUsageWakeState::Leased => None,
		_ => return incompatible("stored waiting-usage wake lease shape is invalid"),
	};
	let resolution_id = optional_uuid(effect, "routing_resolution_request_id")?;
	let terminal_reason = match effect.get("terminal_reason") {
		Some(Value::Null) => None,
		Some(Value::String(value)) => Some(match value.as_str() {
			"explicit_cancellation" => WaitingUsageWakeTerminalReason::ExplicitCancellation,
			"managed_run_stale" => WaitingUsageWakeTerminalReason::ManagedRunStale,
			"policy_revision_stale" => WaitingUsageWakeTerminalReason::PolicyRevisionStale,
			"ambiguous_decision_lineage" => {
				WaitingUsageWakeTerminalReason::AmbiguousDecisionLineage
			}
			_ => return incompatible("stored wake terminal reason is unknown"),
		}),
		_ => return incompatible("stored wake terminal reason is malformed"),
	};
	if !matches!(
		(state, transition_kind, terminal_reason),
		(WaitingUsageWakeState::Pending, WaitingUsageWakeTransitionKind::Registered, None)
			| (WaitingUsageWakeState::Leased,
				WaitingUsageWakeTransitionKind::Claimed | WaitingUsageWakeTransitionKind::Reclaimed,
				None)
			| (WaitingUsageWakeState::Fired, WaitingUsageWakeTransitionKind::Fired, None)
			| (WaitingUsageWakeState::Cancelled, WaitingUsageWakeTransitionKind::Cancelled,
				Some(WaitingUsageWakeTerminalReason::ExplicitCancellation))
			| (WaitingUsageWakeState::Superseded, WaitingUsageWakeTransitionKind::Superseded,
				Some(WaitingUsageWakeTerminalReason::ManagedRunStale
					| WaitingUsageWakeTerminalReason::PolicyRevisionStale
					| WaitingUsageWakeTerminalReason::AmbiguousDecisionLineage))
	) {
		return incompatible("stored wake transition kind, state, and reason are inconsistent");
	}
	if (state == WaitingUsageWakeState::Fired) != resolution_id.is_some() {
		return incompatible("stored waiting-usage wake fire shape is invalid");
	}
	if !boolean(effect, "fresh_routing_resolution_only")?
		|| boolean(effect, "prior_decision_reusable")?
		|| boolean(effect, "production_enabled")?
	{
		return incompatible("stored wake unexpectedly authorizes old routing or production");
	}
	let revision = positive_i64(effect, "revision")?;
	let predecessor_revision = optional_positive_i64(effect, "predecessor_revision")?;
	let predecessor_transition_id = optional_uuid(effect, "predecessor_transition_id")?;
	if (revision == 1)
		!= (predecessor_revision.is_none() && predecessor_transition_id.is_none())
		|| revision > 1 && predecessor_revision != Some(revision - 1)
	{
		return incompatible("stored wake transition predecessor is nonmonotonic");
	}
	Ok(WaitingUsageWakeTransition {
		transition_id: uuid(effect, "transition_id")?,
		wake_id: uuid(effect, "wake_id")?,
		revision,
		predecessor_revision,
		predecessor_transition_id,
		operation_id: uuid(effect, "operation_id")?,
		transition_kind,
		registration_operation_id: uuid(effect, "registration_operation_id")?,
		routing_decision_id: uuid(effect, "routing_decision_id")?,
		routing_decision_revision: {
			let value = positive_i64(effect, "routing_decision_revision")?;
			if value != 1 {
				return incompatible("stored V16 decision revision is not immutable revision one");
			}
			value
		},
		routing_policy_id: uuid(effect, "routing_policy_id")?,
		routing_policy_revision: positive_i64(effect, "routing_policy_revision")?,
		managed_run_id: ManagedRunId::new(uuid(effect, "managed_run_id")?)
			.map_err(|_| StoreError::Incompatible("stored wake ManagedRun is malformed".into()))?,
		managed_run_revision: positive_i64(effect, "managed_run_revision")?,
		earliest_ready_at_micros: nonnegative_i64(effect, "earliest_ready_at_micros")?,
		state,
		lease,
		routing_resolution_request_id: resolution_id,
		fresh_routing_resolution_only: true,
		prior_decision_reusable: false,
		production_enabled: false,
		registered_at_micros: nonnegative_i64(effect, "registered_at_micros")?,
		transitioned_at_micros: nonnegative_i64(effect, "transitioned_at_micros")?,
		terminal_reason,
	})
}

fn parse_envelope(response: &[u8]) -> Result<(String, Value), StoreError> {
	let envelope: Value = serde_json::from_slice(response)
		.map_err(|_| StoreError::Incompatible("stored wake response bytes are malformed".into()))?;
	require_keys(&envelope, &["classification", "effect"])?;
	let classification = text(&envelope, "classification")?.to_owned();
	let effect = envelope
		.get("effect")
		.cloned()
		.ok_or_else(|| StoreError::Incompatible("stored wake effect is missing".into()))?;
	Ok((classification, effect))
}

fn parse_rejection(effect: &Value, operation: &str) -> Result<WaitingUsageWakeRejection, StoreError> {
	require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
	validate_digest(effect)?;
	if text(effect, "operation")? != operation {
		return incompatible("stored wake rejection operation is cross-linked");
	}
	match text(effect, "rejection")? {
		"invalid_input" => Ok(WaitingUsageWakeRejection::InvalidInput),
		"missing_decision" => Ok(WaitingUsageWakeRejection::MissingDecision),
		"decision_not_waiting_usage" => Ok(WaitingUsageWakeRejection::DecisionNotWaitingUsage),
		"stale_managed_run" | "managed_run_stale" => Ok(WaitingUsageWakeRejection::StaleManagedRun),
		"stale_policy" | "policy_revision_stale" => Ok(WaitingUsageWakeRejection::StalePolicy),
		"ambiguous_decision_lineage" => Ok(WaitingUsageWakeRejection::AmbiguousDecisionLineage),
		"operation_identity_conflict" => Ok(WaitingUsageWakeRejection::OperationIdentityConflict),
		"decision_already_registered" => Ok(WaitingUsageWakeRejection::DecisionAlreadyRegistered),
		"claim_identity_conflict" => Ok(WaitingUsageWakeRejection::ClaimIdentityConflict),
		"no_due_wake" => Ok(WaitingUsageWakeRejection::NoDueWake),
		"wake_not_found" => Ok(WaitingUsageWakeRejection::WakeNotFound),
		"stale_wake_tip" => Ok(WaitingUsageWakeRejection::StaleWakeTip),
		"lease_lost" => Ok(WaitingUsageWakeRejection::LeaseLost),
		"wake_terminal" => Ok(WaitingUsageWakeRejection::WakeTerminal),
		_ => incompatible("stored waiting-usage wake rejection is unknown"),
	}
}

fn validate_digest(effect: &Value) -> Result<(), StoreError> {
	let source = text(effect, "effect_digest_source")?;
	let expected = text(effect, "effect_digest")?;
	let actual = format!("{:x}", Sha256::digest(source.as_bytes()));
	if expected.len() != 64 || expected != actual {
		return incompatible("stored waiting-usage wake effect digest is invalid");
	}
	let parsed: Value = serde_json::from_str(source)
		.map_err(|_| StoreError::Incompatible("stored wake digest source is malformed".into()))?;
	let mut projected = effect.clone();
	let object = projected
		.as_object_mut()
		.ok_or_else(|| StoreError::Incompatible("stored wake effect is malformed".into()))?;
	object.remove("effect_digest");
	object.remove("effect_digest_source");
	if projected != parsed {
		return incompatible("stored wake effect differs from its digest source");
	}
	Ok(())
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value
		.as_object()
		.ok_or_else(|| StoreError::Incompatible("stored wake object is malformed".into()))?;
	if object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key)) {
		Ok(())
	} else {
		incompatible("stored wake object has missing or unknown keys")
	}
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value.get(key).and_then(Value::as_str).ok_or_else(|| {
		StoreError::Incompatible(format!("stored wake {key} is malformed"))
	})
}

fn boolean(value: &Value, key: &str) -> Result<bool, StoreError> {
	value.get(key).and_then(Value::as_bool).ok_or_else(|| {
		StoreError::Incompatible(format!("stored wake {key} is malformed"))
	})
}

fn uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?.to_owned();
	validate_uuid(&value, "stored wake UUID")?;
	Ok(value)
}

fn optional_uuid(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => {
			validate_uuid(value, "stored wake UUID")?;
			Ok(Some(value.clone()))
		}
		_ => incompatible("stored optional wake UUID is malformed"),
	}
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let value = value.get(key).and_then(Value::as_i64).ok_or_else(|| {
		StoreError::Incompatible(format!("stored wake {key} is malformed"))
	})?;
	positive(value, "stored wake revision")?;
	Ok(value)
}

fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_i64()
			.filter(|value| *value > 0)
			.map(Some)
			.ok_or_else(|| StoreError::Incompatible("stored optional wake revision is malformed".into())),
		None => incompatible("stored optional wake revision is missing"),
	}
}

fn nonnegative_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let value = value.get(key).and_then(Value::as_i64).ok_or_else(|| {
		StoreError::Incompatible(format!("stored wake {key} is malformed"))
	})?;
	if value < 0 {
		return incompatible("stored wake timestamp is negative");
	}
	Ok(value)
}

fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_i64()
			.filter(|value| *value >= 0)
			.map(Some)
			.ok_or_else(|| StoreError::Incompatible("stored optional wake timestamp is malformed".into())),
		None => incompatible("stored optional wake timestamp is missing"),
	}
}

fn positive(value: i64, label: &'static str) -> Result<(), StoreError> {
	if value <= 0 {
		Err(StoreError::InvalidInput(label))
	} else {
		Ok(())
	}
}

fn validate_uuid(value: &str, label: &'static str) -> Result<(), StoreError> {
	if uuid::Uuid::parse_str(value).is_err() {
		Err(StoreError::InvalidInput(label))
	} else {
		Ok(())
	}
}

fn state_sql(state: WaitingUsageWakeState) -> &'static str {
	match state {
		WaitingUsageWakeState::Pending => "pending",
		WaitingUsageWakeState::Leased => "leased",
		WaitingUsageWakeState::Fired => "fired",
		WaitingUsageWakeState::Cancelled => "cancelled",
		WaitingUsageWakeState::Superseded => "superseded",
	}
}

fn transition_kind_sql(kind: WaitingUsageWakeTransitionKind) -> &'static str {
	match kind {
		WaitingUsageWakeTransitionKind::Registered => "registered",
		WaitingUsageWakeTransitionKind::Claimed => "claimed",
		WaitingUsageWakeTransitionKind::Reclaimed => "reclaimed",
		WaitingUsageWakeTransitionKind::Fired => "fired",
		WaitingUsageWakeTransitionKind::Cancelled => "cancelled",
		WaitingUsageWakeTransitionKind::Superseded => "superseded",
	}
}

fn terminal_reason_sql(reason: WaitingUsageWakeTerminalReason) -> &'static str {
	match reason {
		WaitingUsageWakeTerminalReason::ExplicitCancellation => "explicit_cancellation",
		WaitingUsageWakeTerminalReason::ManagedRunStale => "managed_run_stale",
		WaitingUsageWakeTerminalReason::PolicyRevisionStale => "policy_revision_stale",
		WaitingUsageWakeTerminalReason::AmbiguousDecisionLineage => "ambiguous_decision_lineage",
	}
}

fn incompatible<T>(message: &'static str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(message.into()))
}
