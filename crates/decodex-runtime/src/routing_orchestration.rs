//! Disabled runtime sequencing over the persisted V16 and V17 authorities.
//!
//! This boundary can persist or replay one routing decision and, only when that exact decision is
//! selected, one inert continuation plan. It has no dispatch gate, process, credential, turn,
//! approval, or scheduler capability.

use decodex_core::{
	AccountId, BlobStore, ContextPack, ContinuationCommandOutcome,
	ContinuationEffectBarrierState, ContinuationPlanKind, ContinuationRejection, ManagedRunId,
	RoutingCommandOutcome, RoutingDecisionKind, RoutingNoRouteReason,
};
use decodex_postgres::{
	ContinuationPlanEffect, PersistedRoutingDecision, PlanContinuation, PostgresStore, RouteAccount,
	StoreError,
};

/// Caller-owned identities for V17. The selected account and all continuation lineage remain
/// PostgreSQL-derived from the exact persisted V16 decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationCoordinates {
	pub operation_id: String,
	pub plan_id: String,
	pub fallback_runtime_session_id: String,
	pub fallback_account_snapshot_id: String,
	pub fallback_context_pack_id: String,
}

/// Exact-command coordinates for one disabled orchestration invocation.
///
/// No candidate, eligibility, policy ordering, exclusion, sticky-account, quota, compatibility,
/// selection, continuation-kind, wake, credential, or dispatch fact is representable here.
pub struct DisabledRoutingCommand {
	pub routing_idempotency_key: String,
	pub routing: RouteAccount,
	pub continuation_idempotency_key: String,
	pub continuation: ContinuationCoordinates,
	pub fallback_context_pack: ContextPack,
}

/// Immutable V16 identity coupled to the exact ManagedRun revision consumed by downstream
/// authority. V16 decisions themselves are immutable and have no independently mutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedDecisionProvenance {
	pub decision_id: String,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
}

/// The scheduler owner's complete inert input. It does not authorize registering or firing a
/// wake; the ManagedRun revision is the exact revision bound to this immutable decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageHandoff {
	pub decision_id: String,
	pub managed_run_revision: i64,
	pub earliest_ready_at_micros: i64,
}

/// Provenance retained when no persisted decision identity was safely available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingAttemptProvenance {
	pub routing_operation_id: String,
	pub routing_policy_id: String,
	pub routing_policy_revision: i64,
	pub continuation_operation_id: String,
	pub continuation_plan_id: String,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
}

/// Stable V16 rejection classifications admitted by the strict PostgreSQL adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingAuthorityRejection {
	MalformedInput,
	StaleRoutingPolicy,
	StaleManagedRun,
	SnapshotMissing,
	ConcurrentAuthorityChange,
}

/// Closed fail-closed classifications. They carry no adapter or database error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisabledRoutingFailureKind {
	RoutingRejected(RoutingAuthorityRejection),
	ContinuationRejected(ContinuationRejection),
	InvalidPersistedDecision,
	InvalidPersistedPlan,
	InvalidCommand,
	StaleAuthority,
	ExactCommandConflict,
	PersistedAuthorityIncompatible,
	PersistedAuthorityUnavailable,
}

/// Typed inert failure with the most exact persisted provenance available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisabledRoutingFailure {
	pub attempt: RoutingAttemptProvenance,
	pub decision: Option<PersistedDecisionProvenance>,
	pub kind: DisabledRoutingFailureKind,
}

/// Exactly one closed outcome for one persisted V16 decision attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisabledRoutingOutcome {
	/// One exact V17 plan. Both plan kinds are already selected and sealed by PostgreSQL.
	Planned {
		attempt: RoutingAttemptProvenance,
		effect: ContinuationPlanEffect,
	},
	/// Scheduler-owned future work represented without any wake lifecycle capability.
	WaitingUsage {
		attempt: RoutingAttemptProvenance,
		handoff: WaitingUsageHandoff,
	},
	/// A persisted blocked-evidence decision that cannot advance execution.
	NoRoute {
		attempt: RoutingAttemptProvenance,
		decision: PersistedDecisionProvenance,
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
	pub const fn new(store: PostgresStore, blob_store: BlobStore) -> Self {
		Self { store, blob_store }
	}

	/// Sequence one exact V16 command and, for `selected` only, one exact V17 command.
	pub async fn orchestrate(
		&self,
		command: &DisabledRoutingCommand,
	) -> DisabledRoutingOutcome {
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
					"concurrent_authority_change" => {
						RoutingAuthorityRejection::ConcurrentAuthorityChange
					},
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
				self
					.plan_selected(command, attempt, decision, &persisted, &selected_account_id)
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
			fallback_runtime_session_id: command
				.continuation
				.fallback_runtime_session_id
				.clone(),
			fallback_account_snapshot_id: command
				.continuation
				.fallback_account_snapshot_id
				.clone(),
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
		(ContinuationEffectBarrierState::Guarded, 1)
			| (ContinuationEffectBarrierState::Closed, 2)
	) && plan.submitted_turn_receipt_count >= 0
}

fn valid_plan_shape(effect: &ContinuationPlanEffect) -> bool {
	match effect.plan.kind {
		ContinuationPlanKind::SameThread => {
			effect.plan.codex_thread_id.is_some()
				&& effect.plan.fallback_context_pack_id.is_none()
				&& effect.plan.fallback_runtime_session_id.is_none()
				&& effect.plan.same_thread_evidence.is_some()
				&& effect.fallback_context_pack.is_none()
		},
		ContinuationPlanKind::ContextPackFallback => {
			effect.plan.codex_thread_id.is_none()
				&& effect.plan.fallback_context_pack_id.is_some()
				&& effect.plan.fallback_runtime_session_id.is_some()
				&& effect.plan.same_thread_evidence.is_none()
				&& effect.fallback_context_pack.is_some()
		},
	}
}

fn classify_store_error(error: &StoreError) -> DisabledRoutingFailureKind {
	match error {
		StoreError::InvalidInput(_) | StoreError::CredentialRejected => {
			DisabledRoutingFailureKind::InvalidCommand
		},
		StoreError::RevisionConflict { .. } => DisabledRoutingFailureKind::StaleAuthority,
		StoreError::IdempotencyConflict | StoreError::OperationIdConflict => {
			DisabledRoutingFailureKind::ExactCommandConflict
		},
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
