//! Stateless execution sequencing over accepted durable owners.
//!
//! `ExecutionCoordinator` retains no services or lifecycle state. Pre-process commits only Routing
//! Decision routing and Continuation Planning. Post-process consumes a ready generation plus exact
//! establishment or affine resume authority into ProviderAttempt preparation. No dispatch
//! authorization or provider gateway is reachable from either boundary.

use decodex_core::{
	BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationPlanKind,
	ContinuationRejection, ExecutionConsumer, ProviderAttemptConsumer, ProviderAttemptId,
	ProviderAttemptPreparation, ProviderAttemptState, RoutingBlocker, RoutingCommandOutcome,
	RoutingDecisionKind,
};
use decodex_postgres::{
	ContinuationPlanEffect, PersistedRoutingDecision, PlanContinuation,
	PlanInitialThreadContinuation, PostgresStore, PrepareProviderAttemptOutcome, RouteAccount,
};

use crate::{
	process_supervisor::FencedProcess,
	provider_attempt_service::{
		ProviderAttemptControl, ProviderAttemptRuntimeAuthority, ProviderAttemptServiceError,
	},
};

/// Closed Continuation Planning input selected before any process operation exists.
pub(crate) enum ContinuationPlanning {
	/// Bind the exact existing starting/unfenced RuntimeSession without creating a successor.
	InitialThread { operation_id: String, plan_id: String },
	/// Atomically preserve same-thread evidence or allocate one Context-Pack successor.
	ExistingSession {
		operation_id: String,
		plan_id: String,
		fallback_runtime_session_id: String,
		fallback_account_snapshot_id: String,
		fallback_context_pack_id: String,
		fallback_context_pack: Box<ContextPack>,
	},
}

/// Complete input for one stateless pre-process route-and-plan call.
pub(crate) struct ExecutionCommand {
	/// Exact-command idempotency key for Routing Decision.
	pub routing_idempotency_key: String,
	/// Routing Decision operation, policy, and exact consumer coordinates.
	pub routing: RouteAccount,
	/// Exact-command idempotency key for Continuation Plan.
	pub continuation_idempotency_key: String,
	/// Closed initial-thread or existing-session Continuation Planning input.
	continuation: ContinuationPlanning,
}

impl ExecutionCommand {
	/// Construct first-Turn pre-process input without exposing a process operation.
	pub(crate) fn initial_thread(
		routing_idempotency_key: String,
		routing: RouteAccount,
		continuation_idempotency_key: String,
		operation_id: String,
		plan_id: String,
	) -> Self {
		Self {
			routing_idempotency_key,
			routing,
			continuation_idempotency_key,
			continuation: ContinuationPlanning::InitialThread { operation_id, plan_id },
		}
	}

	/// Construct existing-session pre-process input with one bounded fallback candidate.
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn existing_session(
		routing_idempotency_key: String,
		routing: RouteAccount,
		continuation_idempotency_key: String,
		operation_id: String,
		plan_id: String,
		fallback_runtime_session_id: String,
		fallback_account_snapshot_id: String,
		fallback_context_pack_id: String,
		fallback_context_pack: ContextPack,
	) -> Self {
		Self {
			routing_idempotency_key,
			routing,
			continuation_idempotency_key,
			continuation: ContinuationPlanning::ExistingSession {
				operation_id,
				plan_id,
				fallback_runtime_session_id,
				fallback_account_snapshot_id,
				fallback_context_pack_id,
				fallback_context_pack: Box::new(fallback_context_pack),
			},
		}
	}
}

/// Complete post-process input after one ready generation and positive establish/resume result.
pub(crate) struct PostProcessCommand {
	/// Exact accepted Routing Decision provenance.
	pub decision: PersistedDecisionProvenance,
	/// Exact accepted inert Continuation Plan.
	pub plan: ContinuationPlanEffect,
	/// Complete generic ProviderAttempt request identity and provider keys.
	pub provider_attempt: ProviderAttemptPreparation,
	/// Durable initial binding or affine same-thread resume authority.
	pub runtime_authority: ProviderAttemptRuntimeAuthority,
}

/// Immutable Routing Decision identity and exact consumer provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PersistedDecisionProvenance {
	/// Identity of the immutable Routing Decision.
	pub decision_id: String,
	/// Exact ordinary or managed consumer committed with the decision.
	pub consumer: ExecutionConsumer,
}

/// Inert prepared-attempt projection. It cannot authorize dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedAttemptHandoff {
	/// Exact ProviderAttempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Current durable prepared revision.
	pub revision: i64,
	/// True only when this coordinator call committed the prepared row.
	pub newly_prepared: bool,
}

/// Closed fail-closed classification without database or provider detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionFailureKind {
	/// Continuation Plan returned the rejection inspected by the production caller.
	ContinuationRejected(ContinuationRejection),
	/// Every other fail-closed routing, persistence, or validation result.
	Other,
}

/// One dispatch-disabled result from stateless pre-process routing and planning.
#[allow(clippy::large_enum_variant)] // The closed handoff remains one typed by-value authority result.
pub(crate) enum PreProcessOutcome {
	/// Routing Decision and Continuation Plan are committed; no process or ProviderAttempt
	/// operation has occurred.
	Planned {
		/// Exact Routing Decision provenance.
		decision: PersistedDecisionProvenance,
		/// Exact Continuation Plan result.
		plan: ContinuationPlanEffect,
	},
	/// Pure positive quota depletion. No wake is registered here.
	WaitingUsage,
	/// Pure unresolved ProcessGeneration or ProviderAttempt authority.
	WaitingReconciliation,
	/// Typed unavailable state whose complete causes remain in the persisted decision.
	NoRoute,
	/// Missing, stale, rejected, mismatched, or unavailable authority.
	FailedClosed(ExecutionFailureKind),
}

/// Definite post-process refusal that proves no new provider dispatch authority escaped.
pub(crate) enum DefinitePostProcessRefusal {
	/// Input or authority was rejected and PostgreSQL returned no ProviderAttempt row.
	NoAttempt,
	/// A non-dispatchable existing attempt prevents this logical Turn from being finalized here.
	ExistingAttempt,
}

/// Closed dispatch disposition after post-process ProviderAttempt preparation.
pub(crate) enum PostProcessOutcome {
	/// This call freshly prepared the attempt and retains one-use authorization input.
	FreshPrepared {
		attempt: PreparedAttemptHandoff,
		fresh_preparation: decodex_postgres::FreshPreparedProviderAttempt,
	},
	/// The exact preparation already exists at the supplied prepared revision.
	PreparedReplay { attempt: PreparedAttemptHandoff },
	/// A known pre-effect refusal. The caller may exact-finalize a Turn only after checking that
	/// no prepared attempt exists for this disposition.
	DefiniteRejection(DefinitePostProcessRefusal),
	/// Existing effect authority or preparation persistence is unresolved.
	EffectOrPersistenceAmbiguity,
}

/// Zero-sized coordinator. All durable state belongs to the sequenced owners.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ExecutionCoordinator;

impl ExecutionCoordinator {
	/// Persist only one Routing Decision and one inert Continuation Plan.
	#[allow(clippy::too_many_lines)]
	pub(crate) async fn pre_process(
		&self,
		store: &PostgresStore,
		blob_store: &BlobStore,
		command: &ExecutionCommand,
	) -> PreProcessOutcome {
		let consumer = command.routing.consumer.clone();
		let persisted =
			match store.route_account(&command.routing_idempotency_key, &command.routing).await {
				Ok(RoutingCommandOutcome::Success(persisted)) => persisted,
				Ok(RoutingCommandOutcome::Rejected(_)) | Err(_) => {
					return failed(ExecutionFailureKind::Other);
				},
			};
		if persisted.consumer != consumer {
			return failed(ExecutionFailureKind::Other);
		}
		let decision = PersistedDecisionProvenance {
			decision_id: persisted.decision_id.clone(),
			consumer: consumer.clone(),
		};

		match persisted.decision.kind {
			RoutingDecisionKind::Selected =>
				self.plan_selected(store, blob_store, command, consumer, decision, &persisted).await,
			RoutingDecisionKind::WaitingUsage => {
				let Some(earliest_ready_at_micros) = persisted.decision.ready_at_micros else {
					return failed(ExecutionFailureKind::Other);
				};
				if persisted.decision.selected_account_id.is_some()
					|| persisted.decision.no_route_reason.is_some()
					|| earliest_ready_at_micros < 0
					|| persisted.decision.causes.is_empty()
					|| persisted.decision.exclusions.is_empty()
					|| persisted.decision.causes.len() != persisted.decision.exclusions.len()
					|| persisted.decision.causes.iter().any(|cause| {
						!matches!(
							cause.blocker,
							RoutingBlocker::QuotaFiveHourDepleted
								| RoutingBlocker::QuotaSevenDayDepleted
						)
					}) {
					return failed(ExecutionFailureKind::Other);
				}
				PreProcessOutcome::WaitingUsage
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
					}) {
					return failed(ExecutionFailureKind::Other);
				}
				PreProcessOutcome::WaitingReconciliation
			},
			RoutingDecisionKind::NoRoute => {
				if persisted.decision.no_route_reason.is_none()
					|| persisted.decision.selected_account_id.is_some()
					|| persisted.decision.ready_at_micros.is_some()
					|| !persisted.decision.exclusions.is_empty()
					|| persisted.decision.causes.is_empty()
				{
					return failed(ExecutionFailureKind::Other);
				}
				PreProcessOutcome::NoRoute
			},
		}
	}

	async fn plan_selected(
		&self,
		store: &PostgresStore,
		blob_store: &BlobStore,
		command: &ExecutionCommand,
		consumer: ExecutionConsumer,
		decision: PersistedDecisionProvenance,
		persisted: &PersistedRoutingDecision,
	) -> PreProcessOutcome {
		let Some(selected_account_id) = persisted.decision.selected_account_id.clone() else {
			return failed(ExecutionFailureKind::Other);
		};
		if persisted.decision.ready_at_micros.is_some()
			|| persisted.decision.no_route_reason.is_some()
			|| !persisted.decision.causes.is_empty()
		{
			return failed(ExecutionFailureKind::Other);
		}
		let planned = match &command.continuation {
			ContinuationPlanning::InitialThread { operation_id, plan_id } => {
				let request = PlanInitialThreadContinuation {
					operation_id: operation_id.clone(),
					routing_decision_id: persisted.decision_id.clone(),
					expected_conversation_revision: consumer.domain_revision(),
					plan_id: plan_id.clone(),
				};
				store
					.plan_initial_thread_continuation(
						&command.continuation_idempotency_key,
						&request,
					)
					.await
			},
			ContinuationPlanning::ExistingSession {
				operation_id,
				plan_id,
				fallback_runtime_session_id,
				fallback_account_snapshot_id,
				fallback_context_pack_id,
				fallback_context_pack,
			} => {
				let request = PlanContinuation {
					operation_id: operation_id.clone(),
					routing_decision_id: persisted.decision_id.clone(),
					expected_consumer_revision: consumer.domain_revision(),
					plan_id: plan_id.clone(),
					fallback_runtime_session_id: fallback_runtime_session_id.clone(),
					fallback_account_snapshot_id: fallback_account_snapshot_id.clone(),
					fallback_context_pack_id: fallback_context_pack_id.clone(),
				};
				store
					.plan_continuation(
						blob_store,
						&command.continuation_idempotency_key,
						&request,
						fallback_context_pack,
					)
					.await
			},
		};
		let plan = match planned {
			Ok(ContinuationCommandOutcome::Success(effect)) => effect,
			Ok(ContinuationCommandOutcome::Rejected(rejection)) => {
				return failed(ExecutionFailureKind::ContinuationRejected(rejection));
			},
			Err(_) => return failed(ExecutionFailureKind::Other),
		};
		if plan.plan.routing_decision_id != persisted.decision_id
			|| plan.plan.consumer != consumer
			|| plan.plan.selected_account_id != selected_account_id
			|| plan.plan.replay_permitted
			|| plan.plan.dispatch_enabled
			|| !valid_plan_shape(&plan)
		{
			return failed(ExecutionFailureKind::Other);
		}

		PreProcessOutcome::Planned { decision, plan }
	}

	/// Consume one ready generation plus exact establish/resume authority into preparation only.
	pub(crate) async fn post_process(
		&self,
		attempts: &ProviderAttemptControl,
		process: &FencedProcess,
		command: PostProcessCommand,
	) -> PostProcessOutcome {
		let PostProcessCommand { decision, plan, provider_attempt, runtime_authority } = command;
		let consumer = decision.consumer.clone();
		if !same_consumer(&consumer, &provider_attempt.consumer)
			|| plan.plan.consumer != consumer
			|| plan.plan.routing_decision_id != decision.decision_id
			|| plan.plan.replay_permitted
			|| plan.plan.dispatch_enabled
			|| !valid_plan_shape(&plan)
		{
			return PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::NoAttempt);
		}
		let attempt = match attempts
			.prepare(&plan, process, &provider_attempt, runtime_authority)
			.await
		{
			Ok(attempt) => attempt,
			Err(error) => {
				return match error {
					ProviderAttemptServiceError::AuthorityConflict =>
						PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::NoAttempt),
					ProviderAttemptServiceError::ProductState
					| ProviderAttemptServiceError::EvidenceUnavailable =>
						PostProcessOutcome::EffectOrPersistenceAmbiguity,
				};
			},
		};
		match attempt {
			PrepareProviderAttemptOutcome::Fresh(fresh) => {
				let attempt = PreparedAttemptHandoff {
					attempt_id: fresh.attempt_id().clone(),
					revision: fresh.revision(),
					newly_prepared: true,
				};
				PostProcessOutcome::FreshPrepared { attempt, fresh_preparation: fresh }
			},
			PrepareProviderAttemptOutcome::Replayed(actual)
				if actual.state == ProviderAttemptState::Prepared =>
				PostProcessOutcome::PreparedReplay {
					attempt: PreparedAttemptHandoff {
						attempt_id: provider_attempt.attempt_id.clone(),
						revision: actual.revision,
						newly_prepared: false,
					},
				},
			PrepareProviderAttemptOutcome::Replayed(actual)
				if matches!(
					actual.state,
					ProviderAttemptState::DispatchAuthorized | ProviderAttemptState::Unknown
				) =>
				PostProcessOutcome::EffectOrPersistenceAmbiguity,
			PrepareProviderAttemptOutcome::Replayed(_) =>
				PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::ExistingAttempt),
			PrepareProviderAttemptOutcome::Rejected { actual, .. } => {
				// A generation race after Routing Decision selected one account does not prove that
				// every otherwise eligible route is blocked only by reconciliation. A rejected
				// attempt can also describe a cross-linked identity whose projection is not
				// attributable to this consumer. Only exact replay or a persisted Routing Decision
				// can project an unresolved or completed attempt.
				if actual.revision == 0 {
					PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::NoAttempt)
				} else {
					PostProcessOutcome::DefiniteRejection(
						DefinitePostProcessRefusal::ExistingAttempt,
					)
				}
			},
		}
	}
}

fn same_consumer(execution: &ExecutionConsumer, attempt: &ProviderAttemptConsumer) -> bool {
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
		ContinuationPlanKind::InitialThread =>
			effect.plan.codex_thread_id.is_none()
				&& effect.plan.fallback_context_pack_id.is_none()
				&& effect.plan.fallback_runtime_session_id.is_none()
				&& effect.plan.same_thread_evidence.is_none()
				&& effect.runtime_session.is_some()
				&& effect.fallback_context_pack.is_none(),
		ContinuationPlanKind::SameThread =>
			effect.plan.codex_thread_id.is_some()
				&& effect.plan.fallback_context_pack_id.is_none()
				&& effect.plan.fallback_runtime_session_id.is_none()
				&& effect.plan.same_thread_evidence.is_some()
				&& effect.runtime_session.is_none()
				&& effect.fallback_context_pack.is_none(),
		ContinuationPlanKind::ContextPackFallback =>
			effect.plan.codex_thread_id.is_none()
				&& effect.plan.fallback_context_pack_id.is_some()
				&& effect.plan.fallback_runtime_session_id.is_some()
				&& effect.plan.same_thread_evidence.is_none()
				&& effect.runtime_session.as_ref().is_some_and(|session| {
					effect.plan.fallback_runtime_session_id.as_ref()
						== Some(&session.runtime_session_id)
				}) && effect.fallback_context_pack.is_some(),
	}
}

fn failed(kind: ExecutionFailureKind) -> PreProcessOutcome {
	PreProcessOutcome::FailedClosed(kind)
}
