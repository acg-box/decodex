//! Stateless execution sequencing over accepted durable owners.
//!
//! `ExecutionCoordinator` retains no services or lifecycle state. It consumes one persisted V16
//! decision, one V17 RuntimeSession result, one ProcessSupervisor-owned live fence, and the sole
//! ProviderAttempt writer. No production root calls this boundary, and no dispatch authorization
//! or provider gateway is reachable from it.

use decodex_core::{
	AccountId, BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationPlanKind,
	ContinuationRejection, ExecutionConsumer, ProviderAttemptConsumer,
	ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState, RoutingBlocker,
	RoutingCommandOutcome, RoutingDecisionCause, RoutingDecisionExclusion, RoutingDecisionKind,
	RoutingNoRouteReason,
};
use decodex_postgres::{
	ContinuationPlanEffect, PersistedRoutingDecision, PlanContinuation, PostgresStore,
	PrepareProviderAttemptOutcome, ProviderAttemptRejection, RouteAccount, StoreError,
};

use crate::{
	process_supervisor::FencedProcess,
	provider_attempt_service::{ProviderAttemptControl, ProviderAttemptServiceError},
};

/// Caller-owned identities for V17 fallback allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationCoordinates {
	/// Domain operation identity, distinct from the exact-command idempotency key.
	pub operation_id: String,
	/// Caller-allocated identity for one immutable continuation plan.
	pub plan_id: String,
	/// Preallocated RuntimeSession identity used only when V17 selects fallback.
	pub fallback_runtime_session_id: String,
	/// Preallocated selected-account snapshot identity used only for fallback.
	pub fallback_account_snapshot_id: String,
	/// Preallocated Context Pack identity used only for the atomic fallback shape.
	pub fallback_context_pack_id: String,
}

/// Complete input for one stateless, dispatch-disabled sequencing call.
pub struct ExecutionCommand {
	/// Exact-command idempotency key for V16.
	pub routing_idempotency_key: String,
	/// V16 operation, policy, and exact consumer coordinates.
	pub routing: RouteAccount,
	/// Exact-command idempotency key for V17.
	pub continuation_idempotency_key: String,
	/// V17 operation and fallback identities.
	pub continuation: ContinuationCoordinates,
	/// Caller-compiled fallback input for V17 canonical validation.
	pub fallback_context_pack: ContextPack,
	/// Complete ProviderAttempt input. V24 derives account, RuntimeSession, and process lineage.
	pub provider_attempt: ProviderAttemptPreparation,
}

/// Immutable V16 identity and exact consumer provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedDecisionProvenance {
	/// Identity of the immutable V16 decision.
	pub decision_id: String,
	/// Exact ordinary or managed consumer committed with the decision.
	pub consumer: ExecutionConsumer,
}

/// Pure usage wait projection. It grants no scheduler or wake mutation capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageHandoff {
	/// Identity of the persisted V16 decision.
	pub decision_id: String,
	/// Exact consumer blocked by positive current quota depletion.
	pub consumer: ExecutionConsumer,
	/// PostgreSQL-authored earliest ready instant, in Unix microseconds.
	pub earliest_ready_at_micros: i64,
	/// Complete independent 300-minute and 10,080-minute causes and provenance.
	pub causes: Vec<RoutingDecisionCause>,
	/// Complete independent positive depletion facts with exact source and timestamp lineage.
	pub quota_exclusions: Vec<RoutingDecisionExclusion>,
}

/// Pure reconciliation wait projection. It grants no retry, replay, or wake capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingReconciliationHandoff {
	/// Persisted V16 decision that retained the unresolved authority, when available.
	pub decision_id: String,
	/// Exact affected consumer.
	pub consumer: ExecutionConsumer,
	/// Complete exact account-scoped process or attempt causes.
	pub causes: Vec<RoutingDecisionCause>,
}

/// Inert prepared-attempt projection. It cannot authorize dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAttemptHandoff {
	/// Exact ProviderAttempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Current durable prepared revision.
	pub revision: i64,
	/// PostgreSQL-authored preparation or readback time, in Unix microseconds.
	pub recorded_at_micros: i64,
	/// True only when this coordinator call committed the prepared row.
	pub newly_prepared: bool,
}

/// Stable V16 rejection classifications admitted by the strict adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingAuthorityRejection {
	/// Exact-command coordinates or input were malformed.
	MalformedInput,
	/// Routing-policy authority changed.
	StaleRoutingPolicy,
	/// Conversation or ManagedRun authority changed.
	StaleConsumer,
	/// No immutable V14 snapshot matched the exact consumer.
	SnapshotMissing,
	/// A locked account, policy, quota, or RuntimeSession authority changed.
	ConcurrentAuthorityChange,
}

/// Closed fail-closed classification without database or provider detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionFailureKind {
	/// V16 returned a recognized stable rejection.
	RoutingRejected(RoutingAuthorityRejection),
	/// V17 returned a recognized stable rejection.
	ContinuationRejected(ContinuationRejection),
	/// A successful V16 readback violated its closed decision shape.
	InvalidPersistedDecision,
	/// A successful V17 readback violated consumer, account, or inertness lineage.
	InvalidPersistedPlan,
	/// ProviderAttempt input did not name the same exact consumer as V16 and V17.
	ConsumerCrossLink,
	/// ProviderAttempt rejected an exact identity or immutable consumer binding.
	ProviderAttemptRejected(ProviderAttemptRejection),
	/// Store input or credential-shaped content was rejected.
	InvalidCommand,
	/// Optimistic authority changed.
	StaleAuthority,
	/// An exact-command or semantic identity conflicted.
	ExactCommandConflict,
	/// Persisted authority or host integrity was incompatible.
	PersistedAuthorityIncompatible,
	/// Persisted authority was unavailable.
	PersistedAuthorityUnavailable,
}

/// Typed failure with the most exact persisted decision provenance available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionFailure {
	/// Exact affected consumer.
	pub consumer: ExecutionConsumer,
	/// Persisted V16 identity when it was safely read before the failure.
	pub decision: Option<PersistedDecisionProvenance>,
	/// Closed failure classification.
	pub kind: ExecutionFailureKind,
}

/// One dispatch-disabled result from the stateless coordinator.
pub enum ExecutionOutcome {
	/// ProviderAttempt binding is durable. No variant grants dispatch authority.
	Prepared {
		/// Exact V16 decision provenance.
		decision: PersistedDecisionProvenance,
		/// Exact V17 result and verified fallback pack, when fallback was selected.
		plan: ContinuationPlanEffect,
		/// Fresh or replayed prepared projection with the dispatch-capable token consumed.
		attempt: PreparedAttemptHandoff,
	},
	/// Pure positive quota depletion. No wake is registered here.
	WaitingUsage(WaitingUsageHandoff),
	/// Pure unresolved ProcessGeneration or ProviderAttempt authority.
	WaitingReconciliation(WaitingReconciliationHandoff),
	/// Typed unavailable state with every exact persisted cause.
	NoRoute {
		/// Exact V16 decision provenance.
		decision: PersistedDecisionProvenance,
		/// Stable persisted no-route reason.
		reason: RoutingNoRouteReason,
		/// Complete exact causes. Mixed causes remain in this variant.
		causes: Vec<RoutingDecisionCause>,
	},
	/// Missing, stale, rejected, mismatched, or unavailable authority.
	FailedClosed(ExecutionFailure),
}

/// Zero-sized coordinator. All durable state belongs to the sequenced owners.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionCoordinator;

impl ExecutionCoordinator {
	/// Sequence accepted owners without retaining state or authorizing provider dispatch.
	///
	/// This method is crate-private until the aggregate gate and a separate enablement amendment.
	pub(crate) async fn coordinate(
		&self,
		store: &PostgresStore,
		blob_store: &BlobStore,
		attempts: &ProviderAttemptControl,
		process: &FencedProcess,
		command: &ExecutionCommand,
	) -> ExecutionOutcome {
		let consumer = command.routing.consumer.clone();
		if !same_consumer(&consumer, &command.provider_attempt.consumer) {
			return failed(consumer, None, ExecutionFailureKind::ConsumerCrossLink);
		}

		let persisted = match store
			.route_account(&command.routing_idempotency_key, &command.routing)
			.await
		{
			Ok(RoutingCommandOutcome::Success(persisted)) => persisted,
			Ok(RoutingCommandOutcome::Rejected(rejection)) => {
				let kind = match rejection.code.as_str() {
					"malformed_input" => RoutingAuthorityRejection::MalformedInput,
					"stale_routing_policy" => RoutingAuthorityRejection::StaleRoutingPolicy,
					"stale_consumer" => RoutingAuthorityRejection::StaleConsumer,
					"snapshot_missing" => RoutingAuthorityRejection::SnapshotMissing,
					"concurrent_authority_change" =>
						RoutingAuthorityRejection::ConcurrentAuthorityChange,
					_ =>
						return failed(
							consumer,
							None,
							ExecutionFailureKind::PersistedAuthorityIncompatible,
						),
				};
				return failed(
					consumer,
					None,
					ExecutionFailureKind::RoutingRejected(kind),
				);
			},
			Err(error) => return failed(consumer, None, classify_store_error(&error)),
		};
		if persisted.consumer != consumer {
			return failed(
				consumer,
				None,
				ExecutionFailureKind::InvalidPersistedDecision,
			);
		}
		let decision = PersistedDecisionProvenance {
			decision_id: persisted.decision_id.clone(),
			consumer: consumer.clone(),
		};

		match persisted.decision.kind {
			RoutingDecisionKind::Selected =>
				self.plan_and_prepare(
					store,
					blob_store,
					attempts,
					process,
					command,
					consumer,
					decision,
					&persisted,
				)
				.await,
			RoutingDecisionKind::WaitingUsage => {
				let Some(earliest_ready_at_micros) = persisted.decision.ready_at_micros else {
					return failed(
						consumer,
						Some(decision),
						ExecutionFailureKind::InvalidPersistedDecision,
					);
				};
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.no_route_reason.is_some()
					|| earliest_ready_at_micros < 0
					|| persisted.decision.causes.is_empty()
					|| persisted.decision.exclusions.is_empty()
					|| persisted.decision.causes.len() != persisted.decision.exclusions.len()
					|| persisted
						.decision
						.causes
						.iter()
						.any(|cause| {
							!matches!(
								cause.blocker,
								RoutingBlocker::QuotaFiveHourDepleted
									| RoutingBlocker::QuotaSevenDayDepleted
							)
						})
				{
					return failed(
						consumer,
						Some(decision),
						ExecutionFailureKind::InvalidPersistedDecision,
					);
				}
				ExecutionOutcome::WaitingUsage(WaitingUsageHandoff {
					decision_id: decision.decision_id,
					consumer,
					earliest_ready_at_micros,
					causes: persisted.decision.causes,
					quota_exclusions: persisted.decision.exclusions,
				})
			},
			RoutingDecisionKind::WaitingReconciliation => {
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.ready_at_micros.is_some()
					|| persisted.decision.no_route_reason.is_some()
					|| persisted.decision.causes.is_empty()
					|| !persisted.decision.exclusions.is_empty()
					|| persisted.decision.causes.iter().any(|cause| {
						!matches!(
							cause.blocker,
							RoutingBlocker::ProcessGenerationUnresolved
								| RoutingBlocker::ProviderAttemptUnresolved
						)
					})
				{
					return failed(
						consumer,
						Some(decision),
						ExecutionFailureKind::InvalidPersistedDecision,
					);
				}
				ExecutionOutcome::WaitingReconciliation(WaitingReconciliationHandoff {
					decision_id: decision.decision_id,
					consumer,
					causes: persisted.decision.causes,
				})
			},
			RoutingDecisionKind::NoRoute => {
				let Some(reason) = persisted.decision.no_route_reason else {
					return failed(
						consumer,
						Some(decision),
						ExecutionFailureKind::InvalidPersistedDecision,
					);
				};
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.ready_at_micros.is_some()
					|| !persisted.decision.exclusions.is_empty()
					|| persisted.decision.causes.is_empty()
				{
					return failed(
						consumer,
						Some(decision),
						ExecutionFailureKind::InvalidPersistedDecision,
					);
				}
				ExecutionOutcome::NoRoute {
					decision,
					reason,
					causes: persisted.decision.causes,
				}
			},
		}
	}

	#[allow(clippy::too_many_arguments)]
	async fn plan_and_prepare(
		&self,
		store: &PostgresStore,
		blob_store: &BlobStore,
		attempts: &ProviderAttemptControl,
		process: &FencedProcess,
		command: &ExecutionCommand,
		consumer: ExecutionConsumer,
		decision: PersistedDecisionProvenance,
		persisted: &PersistedRoutingDecision,
	) -> ExecutionOutcome {
		let Some(selected_account_id) = persisted.decision.selected_account_id.clone() else {
			return failed(
				consumer,
				Some(decision),
				ExecutionFailureKind::InvalidPersistedDecision,
			);
		};
		if persisted.decision.ready_at_micros.is_some()
			|| persisted.decision.no_route_reason.is_some()
			|| !persisted.decision.causes.is_empty()
		{
			return failed(
				consumer,
				Some(decision),
				ExecutionFailureKind::InvalidPersistedDecision,
			);
		}
		let request = PlanContinuation {
			operation_id: command.continuation.operation_id.clone(),
			routing_decision_id: persisted.decision_id.clone(),
			expected_consumer_revision: consumer.domain_revision(),
			plan_id: command.continuation.plan_id.clone(),
			fallback_runtime_session_id: command.continuation.fallback_runtime_session_id.clone(),
			fallback_account_snapshot_id: command.continuation.fallback_account_snapshot_id.clone(),
			fallback_context_pack_id: command.continuation.fallback_context_pack_id.clone(),
		};
		let plan = match store
			.plan_continuation(
				blob_store,
				&command.continuation_idempotency_key,
				&request,
				&command.fallback_context_pack,
			)
			.await
		{
			Ok(ContinuationCommandOutcome::Success(effect)) => effect,
			Ok(ContinuationCommandOutcome::Rejected(rejection)) =>
				return failed(
					consumer,
					Some(decision),
					ExecutionFailureKind::ContinuationRejected(rejection),
				),
			Err(error) =>
				return failed(consumer, Some(decision), classify_store_error(&error)),
		};
		if plan.plan.routing_decision_id != persisted.decision_id
			|| plan.plan.consumer != consumer
			|| plan.plan.selected_account_id != selected_account_id
			|| plan.plan.replay_permitted
			|| plan.plan.dispatch_enabled
			|| !valid_plan_shape(&plan)
		{
			return failed(
				consumer,
				Some(decision),
				ExecutionFailureKind::InvalidPersistedPlan,
			);
		}

		let attempt = match attempts.prepare(&plan, process, &command.provider_attempt).await {
			Ok(attempt) => attempt,
			Err(error) =>
				return failed(
					consumer,
					Some(decision),
					classify_attempt_service_error(error),
				),
		};
		match attempt {
			PrepareProviderAttemptOutcome::Fresh(fresh) =>
				ExecutionOutcome::Prepared {
					decision,
					plan,
					attempt: PreparedAttemptHandoff {
						attempt_id: fresh.attempt_id().clone(),
						revision: fresh.revision(),
						recorded_at_micros: fresh.prepared_at_micros(),
						newly_prepared: true,
					},
				},
			PrepareProviderAttemptOutcome::Replayed(actual)
				if actual.state == ProviderAttemptState::Prepared =>
				ExecutionOutcome::Prepared {
					decision,
					plan,
					attempt: PreparedAttemptHandoff {
						attempt_id: command.provider_attempt.attempt_id.clone(),
						revision: actual.revision,
						recorded_at_micros: actual.recorded_at_micros,
						newly_prepared: false,
					},
				},
			PrepareProviderAttemptOutcome::Replayed(actual)
				if matches!(
					actual.state,
					ProviderAttemptState::DispatchAuthorized | ProviderAttemptState::Unknown
				) =>
				ExecutionOutcome::WaitingReconciliation(WaitingReconciliationHandoff {
					decision_id: decision.decision_id,
					consumer,
					causes: vec![RoutingDecisionCause {
						account_id: selected_account_id,
						blocker: RoutingBlocker::ProviderAttemptUnresolved,
					}],
				}),
			PrepareProviderAttemptOutcome::Replayed(actual) if actual.state.is_terminal() =>
				ExecutionOutcome::NoRoute {
					decision,
					reason: RoutingNoRouteReason::BlockedEvidence,
					causes: vec![RoutingDecisionCause {
						account_id: selected_account_id,
						blocker: RoutingBlocker::ProviderAttemptCompleted,
					}],
				},
			PrepareProviderAttemptOutcome::Replayed(_) =>
				failed(
					consumer,
					Some(decision),
					ExecutionFailureKind::PersistedAuthorityIncompatible,
				),
			PrepareProviderAttemptOutcome::Rejected { rejection, .. } => {
				// A generation race after V16 selected one account does not prove that every
				// otherwise eligible route is blocked only by reconciliation. A rejected
				// attempt can also describe a cross-linked identity whose projection is not
				// attributable to this consumer. Only exact replay or a persisted V16 decision
				// can project an unresolved or completed attempt.
				failed(
					consumer,
					Some(decision),
					ExecutionFailureKind::ProviderAttemptRejected(rejection),
				)
			},
		}
	}
}

fn same_consumer(
	execution: &ExecutionConsumer,
	attempt: &ProviderAttemptConsumer,
) -> bool {
	match (execution, attempt) {
		(
			ExecutionConsumer::ConversationTurn { conversation_id, turn_id, .. },
			ProviderAttemptConsumer::ConversationTurn {
				conversation_id: attempt_conversation_id,
				turn_id: attempt_turn_id,
			},
		) => conversation_id == attempt_conversation_id && turn_id == attempt_turn_id,
		(
			ExecutionConsumer::ManagedRunExecution {
				managed_run_id,
				managed_run_revision,
				execution_id,
			},
			ProviderAttemptConsumer::ManagedRunExecution {
				managed_run_id: attempt_managed_run_id,
				managed_run_revision: attempt_managed_run_revision,
				execution_id: attempt_execution_id,
			},
		) =>
			managed_run_id == attempt_managed_run_id
				&& managed_run_revision == attempt_managed_run_revision
				&& execution_id == attempt_execution_id,
		_ => false,
	}
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

fn classify_attempt_service_error(error: ProviderAttemptServiceError) -> ExecutionFailureKind {
	match error {
		ProviderAttemptServiceError::AuthorityConflict =>
			ExecutionFailureKind::PersistedAuthorityIncompatible,
		ProviderAttemptServiceError::ProductState
		| ProviderAttemptServiceError::EvidenceUnavailable =>
			ExecutionFailureKind::PersistedAuthorityUnavailable,
	}
}

fn classify_store_error(error: &StoreError) -> ExecutionFailureKind {
	match error {
		StoreError::InvalidInput(_) | StoreError::CredentialRejected =>
			ExecutionFailureKind::InvalidCommand,
		StoreError::RevisionConflict { .. } => ExecutionFailureKind::StaleAuthority,
		StoreError::IdempotencyConflict | StoreError::OperationIdConflict =>
			ExecutionFailureKind::ExactCommandConflict,
		StoreError::Incompatible(_)
		| StoreError::UnsafeAuthority(_)
		| StoreError::UnsafeHostPath
		| StoreError::Blob(_) => ExecutionFailureKind::PersistedAuthorityIncompatible,
		_ => ExecutionFailureKind::PersistedAuthorityUnavailable,
	}
}

fn failed(
	consumer: ExecutionConsumer,
	decision: Option<PersistedDecisionProvenance>,
	kind: ExecutionFailureKind,
) -> ExecutionOutcome {
	ExecutionOutcome::FailedClosed(ExecutionFailure { consumer, decision, kind })
}
