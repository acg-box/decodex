//! Disabled runtime sequencing over the persisted V16 and V17 authorities.
//!
//! This boundary can persist or replay one routing decision and, only when that exact decision is
//! selected, one inert continuation plan. It has no dispatch gate, process, credential, turn,
//! approval, or scheduler capability.

use decodex_core::{
	AccountId, BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationEffectBarrierState,
	ContinuationPlanKind, ContinuationRejection, ManagedRunId, RoutingCommandOutcome,
	RoutingDecisionKind, RoutingNoRouteReason,
};
use decodex_postgres::{
	ContinuationPlanEffect, PersistedRoutingDecision, PlanContinuation, PostgresStore,
	RouteAccount, StoreError,
};

/// Caller-owned identities for V17. The selected account and all continuation lineage remain
/// PostgreSQL-derived from the exact persisted V16 decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationCoordinates {
	/// Domain operation identity for V17, distinct from its exact-command idempotency key.
	pub operation_id: String,
	/// Caller-allocated identity for the one immutable continuation plan.
	pub plan_id: String,
	/// Preallocated RuntimeSession identity used only if PostgreSQL selects fallback.
	pub fallback_runtime_session_id: String,
	/// Preallocated selected-account snapshot identity used only for fallback.
	pub fallback_account_snapshot_id: String,
	/// Preallocated Context Pack identity used only for the atomic fallback shape.
	pub fallback_context_pack_id: String,
}

/// Exact-command coordinates for one disabled orchestration invocation.
///
/// No candidate, eligibility, policy ordering, exclusion, sticky-account, quota, compatibility,
/// selection, continuation-kind, wake, credential, or dispatch fact is representable here.
pub struct DisabledRoutingCommand {
	/// Protocol-scoped exact-command idempotency key for the V16 routing command.
	pub routing_idempotency_key: String,
	/// Caller-owned operation and optimistic coordinates; candidates and evidence remain
	/// PostgreSQL-owned.
	pub routing: RouteAccount,
	/// Protocol-scoped exact-command idempotency key for the V17 continuation command.
	pub continuation_idempotency_key: String,
	/// Caller-owned V17 operation identity and preallocated fallback identities.
	pub continuation: ContinuationCoordinates,
	/// Caller-compiled fallback input submitted for canonical V17 validation; it grants no
	/// continuation-kind or dispatch authority.
	pub fallback_context_pack: ContextPack,
}

/// Immutable V16 identity coupled to the exact ManagedRun revision consumed by downstream
/// authority. V16 decisions themselves are immutable and have no independently mutable revision.
/// Constructing this public projection alone proves no persisted origin or routing authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedDecisionProvenance {
	/// Identity read back for the immutable persisted V16 decision.
	pub decision_id: String,
	/// ManagedRun identity supplied to and resolved by that decision command.
	pub managed_run_id: ManagedRunId,
	/// Exact ManagedRun revision consumed by the persisted decision.
	pub managed_run_revision: i64,
}

/// The scheduler owner's complete inert input. It does not authorize registering or firing a
/// wake; the ManagedRun revision is the exact revision bound to this immutable decision.
/// Constructing this public projection alone proves no persisted origin or scheduler authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageHandoff {
	/// Identity of the persisted V16 `waiting_usage` decision.
	pub decision_id: String,
	/// Exact ManagedRun revision bound to that immutable decision.
	pub managed_run_revision: i64,
	/// PostgreSQL-authored earliest-ready instant, in exact Unix microseconds.
	pub earliest_ready_at_micros: i64,
}

/// Provenance retained when no persisted decision identity was safely available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingAttemptProvenance {
	/// Caller-supplied semantic identity of the attempted V16 routing operation.
	pub routing_operation_id: String,
	/// Caller-supplied routing-policy lineage identity.
	pub routing_policy_id: String,
	/// Caller-supplied exact routing-policy revision expected by V16.
	pub routing_policy_revision: i64,
	/// Caller-supplied semantic identity reserved for a possible V17 operation.
	pub continuation_operation_id: String,
	/// Caller-allocated identity reserved for a possible immutable V17 plan.
	pub continuation_plan_id: String,
	/// Caller-supplied ManagedRun identity for the routing attempt.
	pub managed_run_id: ManagedRunId,
	/// Caller-supplied exact ManagedRun revision expected by V16 and V17.
	pub managed_run_revision: i64,
}

/// Stable V16 rejection classifications admitted by the strict PostgreSQL adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingAuthorityRejection {
	/// The V16 authority rejected malformed exact-command coordinates or input.
	MalformedInput,
	/// The requested routing-policy revision was not the current locked authority.
	StaleRoutingPolicy,
	/// The requested ManagedRun revision or routing lineage was stale.
	StaleManagedRun,
	/// PostgreSQL could not resolve the required immutable routing snapshot.
	SnapshotMissing,
	/// The locked routing authority changed before the exact command could commit.
	ConcurrentAuthorityChange,
}

/// Closed fail-closed classifications. They carry no adapter or database error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisabledRoutingFailureKind {
	/// V16 returned a recognized stable routing-domain rejection.
	RoutingRejected(RoutingAuthorityRejection),
	/// V17 returned a stable continuation-domain rejection.
	ContinuationRejected(ContinuationRejection),
	/// Successful V16 readback violated the closed decision shape required by its kind.
	InvalidPersistedDecision,
	/// Successful V17 readback violated persisted lineage, inertness, or plan-shape requirements.
	InvalidPersistedPlan,
	/// The store rejected command input or credential-shaped content at the invoked boundary.
	InvalidCommand,
	/// The store reported an optimistic revision conflict.
	StaleAuthority,
	/// An idempotency key or semantic operation identity conflicted with an existing command.
	ExactCommandConflict,
	/// A store incompatibility, unsafe authority or host path, or blob failure was classified
	/// fail-closed.
	PersistedAuthorityIncompatible,
	/// Any other store failure was classified as persisted-authority unavailability.
	PersistedAuthorityUnavailable,
}

/// Typed inert failure with the most exact persisted provenance available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisabledRoutingFailure {
	/// Caller-supplied attempt coordinates captured before the V16 command.
	pub attempt: RoutingAttemptProvenance,
	/// Persisted V16 identity when successful decision readback preceded the later failure.
	pub decision: Option<PersistedDecisionProvenance>,
	/// Closed failure classification without underlying adapter or database error text.
	pub kind: DisabledRoutingFailureKind,
}

/// Exactly one closed outcome for one persisted V16 decision attempt.
/// Construction of this public result alone proves no persisted origin or execution authority.
// No production root consumes this disabled boundary; preserve its public by-value authority shape.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisabledRoutingOutcome {
	/// One exact V17 plan. Both plan kinds are already selected and sealed by PostgreSQL.
	Planned {
		/// Caller-supplied coordinates captured before persistence.
		attempt: RoutingAttemptProvenance,
		/// Strictly checked committed V17 plan readback and any verified fallback pack.
		effect: ContinuationPlanEffect,
	},
	/// Scheduler-owned future work represented without any wake lifecycle capability.
	WaitingUsage {
		/// Caller-supplied coordinates captured before persistence.
		attempt: RoutingAttemptProvenance,
		/// Persisted decision lineage and earliest-ready fact for the separate scheduler owner.
		handoff: WaitingUsageHandoff,
	},
	/// A persisted blocked-evidence decision that cannot advance execution.
	NoRoute {
		/// Caller-supplied coordinates captured before persistence.
		attempt: RoutingAttemptProvenance,
		/// Exact persisted V16 decision and ManagedRun lineage.
		decision: PersistedDecisionProvenance,
		/// Stable reason supplied by the persisted no-route decision.
		reason: RoutingNoRouteReason,
	},
	/// Missing, stale, rejected, mismatched, ambiguous, or unavailable authority.
	FailedClosed(DisabledRoutingFailure),
}

/// Explicitly disabled orchestration composition. Its only successful effects are persisted V16
/// decision readback and an inert V17 plan; no enabled counterpart or dispatch token exists.
#[derive(Clone)]
pub struct DisabledRoutingOrchestration {
	store: PostgresStore,
	blob_store: BlobStore,
}

impl DisabledRoutingOrchestration {
	/// Retain the PostgreSQL and blob adapters for the disabled sequencer; construction grants no
	/// dispatch or scheduling capability.
	pub const fn new(store: PostgresStore, blob_store: BlobStore) -> Self {
		Self { store, blob_store }
	}

	/// Sequence one exact V16 command and, for `selected` only, one exact V17 command.
	pub async fn orchestrate(&self, command: &DisabledRoutingCommand) -> DisabledRoutingOutcome {
		let attempt = RoutingAttemptProvenance {
			routing_operation_id: command.routing.operation_id.clone(),
			routing_policy_id: command.routing.routing_policy_id.clone(),
			routing_policy_revision: command.routing.expected_routing_policy_revision,
			continuation_operation_id: command.continuation.operation_id.clone(),
			continuation_plan_id: command.continuation.plan_id.clone(),
			managed_run_id: command.routing.managed_run_id.clone(),
			managed_run_revision: command.routing.expected_managed_run_revision,
		};
		let persisted = match self
			.store
			.route_account(&command.routing_idempotency_key, &command.routing)
			.await
		{
			Ok(RoutingCommandOutcome::Success(persisted)) => persisted,
			Ok(RoutingCommandOutcome::Rejected(rejection)) => {
				let kind = match rejection.code.as_str() {
					"malformed_input" => RoutingAuthorityRejection::MalformedInput,
					"stale_routing_policy" => RoutingAuthorityRejection::StaleRoutingPolicy,
					"stale_managed_run" => RoutingAuthorityRejection::StaleManagedRun,
					"snapshot_missing" => RoutingAuthorityRejection::SnapshotMissing,
					"concurrent_authority_change" =>
						RoutingAuthorityRejection::ConcurrentAuthorityChange,
					_ => {
						return failed(
							attempt,
							None,
							DisabledRoutingFailureKind::PersistedAuthorityIncompatible,
						);
					},
				};
				return failed(attempt, None, DisabledRoutingFailureKind::RoutingRejected(kind));
			},
			Err(error) => return failed(attempt, None, classify_store_error(&error)),
		};
		let decision = PersistedDecisionProvenance {
			decision_id: persisted.decision_id.clone(),
			managed_run_id: command.routing.managed_run_id.clone(),
			managed_run_revision: command.routing.expected_managed_run_revision,
		};

		match persisted.decision.kind {
			RoutingDecisionKind::Selected => {
				let Some(selected_account_id) = persisted.decision.selected_account_id.clone()
				else {
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				};
				if persisted.decision.ready_at_micros.is_some()
					|| persisted.decision.no_route_reason.is_some()
				{
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				}
				self.plan_selected(command, attempt, decision, &persisted, &selected_account_id)
					.await
			},
			RoutingDecisionKind::WaitingUsage => {
				let Some(earliest_ready_at_micros) = persisted.decision.ready_at_micros else {
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				};
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.no_route_reason.is_some()
					|| earliest_ready_at_micros < 0
				{
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				}
				DisabledRoutingOutcome::WaitingUsage {
					attempt,
					handoff: WaitingUsageHandoff {
						decision_id: decision.decision_id,
						managed_run_revision: decision.managed_run_revision,
						earliest_ready_at_micros,
					},
				}
			},
			RoutingDecisionKind::NoRoute => {
				let Some(reason) = persisted.decision.no_route_reason else {
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				};
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.ready_at_micros.is_some()
				{
					return failed(
						attempt,
						Some(decision),
						DisabledRoutingFailureKind::InvalidPersistedDecision,
					);
				}
				DisabledRoutingOutcome::NoRoute { attempt, decision, reason }
			},
		}
	}

	async fn plan_selected(
		&self,
		command: &DisabledRoutingCommand,
		attempt: RoutingAttemptProvenance,
		decision: PersistedDecisionProvenance,
		persisted: &PersistedRoutingDecision,
		selected_account_id: &AccountId,
	) -> DisabledRoutingOutcome {
		let request = PlanContinuation {
			operation_id: command.continuation.operation_id.clone(),
			routing_decision_id: persisted.decision_id.clone(),
			expected_managed_run_revision: command.routing.expected_managed_run_revision,
			plan_id: command.continuation.plan_id.clone(),
			fallback_runtime_session_id: command.continuation.fallback_runtime_session_id.clone(),
			fallback_account_snapshot_id: command.continuation.fallback_account_snapshot_id.clone(),
			fallback_context_pack_id: command.continuation.fallback_context_pack_id.clone(),
		};
		let effect = match self
			.store
			.plan_continuation(
				&self.blob_store,
				&command.continuation_idempotency_key,
				&request,
				&command.fallback_context_pack,
			)
			.await
		{
			Ok(ContinuationCommandOutcome::Success(effect)) => effect,
			Ok(ContinuationCommandOutcome::Rejected(rejection)) => {
				return failed(
					attempt,
					Some(decision),
					DisabledRoutingFailureKind::ContinuationRejected(rejection),
				);
			},
			Err(error) => {
				return failed(attempt, Some(decision), classify_store_error(&error));
			},
		};
		let plan = &effect.plan;
		if plan.routing_decision_id != persisted.decision_id
			|| plan.managed_run_id != command.routing.managed_run_id
			|| plan.managed_run_revision != command.routing.expected_managed_run_revision
			|| plan.selected_account_id != *selected_account_id
			|| plan.replay_permitted
			|| plan.dispatch_enabled
			|| !valid_effect_barrier_lineage(&effect)
			|| !valid_plan_shape(&effect)
		{
			return failed(
				attempt,
				Some(decision),
				DisabledRoutingFailureKind::InvalidPersistedPlan,
			);
		}
		DisabledRoutingOutcome::Planned { attempt, effect }
	}
}

fn valid_effect_barrier_lineage(effect: &ContinuationPlanEffect) -> bool {
	let plan = &effect.plan;
	matches!(
		(plan.effect_barrier_state, plan.effect_barrier_revision),
		(ContinuationEffectBarrierState::Guarded, 1) | (ContinuationEffectBarrierState::Closed, 2)
	) && plan.submitted_turn_receipt_count >= 0
}

fn valid_plan_shape(effect: &ContinuationPlanEffect) -> bool {
	match effect.plan.kind {
		ContinuationPlanKind::SameThread =>
			effect.plan.codex_thread_id.is_some()
				&& effect.plan.fallback_context_pack_id.is_none()
				&& effect.plan.fallback_runtime_session_id.is_none()
				&& effect.plan.same_thread_evidence.is_some()
				&& effect.fallback_context_pack.is_none(),
		ContinuationPlanKind::ContextPackFallback =>
			effect.plan.codex_thread_id.is_none()
				&& effect.plan.fallback_context_pack_id.is_some()
				&& effect.plan.fallback_runtime_session_id.is_some()
				&& effect.plan.same_thread_evidence.is_none()
				&& effect.fallback_context_pack.is_some(),
	}
}

fn classify_store_error(error: &StoreError) -> DisabledRoutingFailureKind {
	match error {
		StoreError::InvalidInput(_) | StoreError::CredentialRejected =>
			DisabledRoutingFailureKind::InvalidCommand,
		StoreError::RevisionConflict { .. } => DisabledRoutingFailureKind::StaleAuthority,
		StoreError::IdempotencyConflict | StoreError::OperationIdConflict =>
			DisabledRoutingFailureKind::ExactCommandConflict,
		StoreError::Incompatible(_)
		| StoreError::UnsafeAuthority(_)
		| StoreError::UnsafeHostPath
		| StoreError::Blob(_) => DisabledRoutingFailureKind::PersistedAuthorityIncompatible,
		_ => DisabledRoutingFailureKind::PersistedAuthorityUnavailable,
	}
}

fn failed(
	attempt: RoutingAttemptProvenance,
	decision: Option<PersistedDecisionProvenance>,
	kind: DisabledRoutingFailureKind,
) -> DisabledRoutingOutcome {
	DisabledRoutingOutcome::FailedClosed(DisabledRoutingFailure { attempt, decision, kind })
}
