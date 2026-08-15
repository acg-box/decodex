//! Stateless execution sequencing over accepted durable owners.
//!
//! `ExecutionCoordinator` retains no services or lifecycle state. Pre-process sequences routing
//! Conversation successors, Routing Decision routing, and Continuation Planning. Post-process
//! consumes a ready generation plus exact establishment or affine resume authority into
//! ProviderAttempt preparation. No dispatch authorization or provider gateway is reachable from
//! either boundary.

use decodex_core::{
	AccountRegistryRoutingDecisionKind, BlobStore, ContextPack, ContinuationCommandOutcome,
	ContinuationPlanKind, ContinuationRejection, ConversationId, ExecutionConsumer,
	ProviderAttemptConsumer, ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState,
	RoutingCommandOutcome, RuntimeSessionId, TurnId,
};
use decodex_database::{
	BindQuickTaskContinuation, ContinuationPlanEffect, CreateQuickTaskRoutingSuccessor,
	PlanContinuation, PlanInitialThreadContinuation, PrepareProviderAttemptOutcome,
	QuickTaskInitialRoute, QuickTaskInitialRouteOutcome, QuickTaskRoutingSuccessor,
	QuickTaskRoutingSuccessorOutcome, RouteQuickTaskInitial, SqliteStore,
};
use sha2::{Digest as _, Sha256};

use crate::{
	process_supervisor::FencedProcess,
	provider_attempt_service::{
		ProviderAttemptControl, ProviderAttemptRuntimeAuthority, ProviderAttemptServiceError,
	},
};

/// Complete input for one atomic initial Account Registry route followed by initial planning.
pub(crate) struct ExecutionCommand {
	/// Exact-command key for the whole initial Account Registry route transaction.
	routing_idempotency_key: String,
	/// Exact open Conversation coordinates. The product store generates route and Turn identities.
	routing: RouteQuickTaskInitial,
}

impl ExecutionCommand {
	/// Construct first-Turn pre-process input without exposing a process operation.
	pub(crate) fn initial_thread(
		operation_key: &str,
		conversation_id: ConversationId,
		expected_conversation_revision: i64,
	) -> Self {
		Self {
			routing_idempotency_key: routing_scoped_key("route", operation_key),
			routing: RouteQuickTaskInitial { conversation_id, expected_conversation_revision },
		}
	}

	fn exact(routing_idempotency_key: String, routing: RouteQuickTaskInitial) -> Self {
		Self { routing_idempotency_key, routing }
	}
}

/// Complete non-selecting later-Turn binding followed by ordinary continuation planning.
pub(crate) struct ContinuationExecutionCommand {
	/// Exact-command key for the immutable continuation routing binding.
	binding_idempotency_key: String,
	/// Existing session and original route lineage to bind.
	binding: BindQuickTaskContinuation,
	/// Exact-command key for same-thread or same-account Context Pack planning.
	continuation_idempotency_key: String,
	/// Stable Continuation Planning operation identity.
	continuation_operation_id: String,
	/// Stable immutable Continuation Plan identity.
	continuation_plan_id: String,
	/// Preallocated same-account fallback RuntimeSession identity.
	fallback_runtime_session_id: String,
	/// Preallocated Context Pack identity.
	fallback_context_pack_id: String,
	/// Complete ordinary Quick Task Context Pack.
	fallback_context_pack: ContextPack,
}

impl ContinuationExecutionCommand {
	/// Construct one non-selecting ordinary continuation bind-and-plan sequence.
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn ordinary(
		operation_key: &str,
		conversation_id: ConversationId,
		expected_conversation_revision: i64,
		source_runtime_session_id: RuntimeSessionId,
		expected_source_runtime_session_revision: i64,
		turn_id: TurnId,
		fallback_context_pack: ContextPack,
	) -> Self {
		Self {
			binding_idempotency_key: routing_scoped_key("continuation-binding", operation_key),
			binding: BindQuickTaskContinuation {
				operation_id: routing_uuid("continuation-binding-operation", &[operation_key]),
				conversation_id,
				expected_conversation_revision,
				source_runtime_session_id,
				expected_source_runtime_session_revision,
				turn_id,
			},
			continuation_idempotency_key: routing_scoped_key("continuation", operation_key),
			continuation_operation_id: routing_uuid("continuation-operation", &[operation_key]),
			continuation_plan_id: routing_uuid("continuation-plan", &[operation_key]),
			fallback_runtime_session_id: routing_uuid("fallback-runtime-session", &[operation_key]),
			fallback_context_pack_id: routing_uuid("fallback-context-pack", &[operation_key]),
			fallback_context_pack,
		}
	}
}

/// Conversation-successor command followed by a separately committed route command.
pub(crate) struct RoutingSuccessorExecutionCommand {
	/// Exact key for Conversation-owned successor creation.
	successor_idempotency_key: String,
	/// Waiting/no-route source and expected revision.
	successor: CreateQuickTaskRoutingSuccessor,
	/// Exact key for the new Conversation's route transaction.
	routing_idempotency_key: String,
}

impl RoutingSuccessorExecutionCommand {
	/// Construct the separate successor command and its follow-on initial route coordinates.
	pub(crate) fn new(
		operation_key: &str,
		source_conversation_id: ConversationId,
		expected_source_revision: i64,
	) -> Self {
		Self {
			successor_idempotency_key: routing_scoped_key("routing-successor", operation_key),
			successor: CreateQuickTaskRoutingSuccessor {
				source_conversation_id,
				expected_source_revision,
			},
			routing_idempotency_key: routing_scoped_key("successor-route", operation_key),
		}
	}
}

/// Result after the Conversation successor command commits before routing starts.
pub(crate) struct RoutingSuccessorExecutionOutcome {
	pub successor: QuickTaskRoutingSuccessor,
	pub routing: PreProcessOutcome,
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
	/// Account Registry waiting with only positive current depletion exclusions.
	Waiting,
	/// Typed unavailable state whose complete causes remain in the persisted decision.
	NoRoute,
	/// A selected decision committed, but first-session planning is not yet complete.
	EstablishmentPending,
	/// Missing, stale, rejected, mismatched, or unavailable authority.
	FailedClosed(ExecutionFailureKind),
}

/// Definite post-process refusal that proves no new provider dispatch authority escaped.
pub(crate) enum DefinitePostProcessRefusal {
	/// Input or authority was rejected and the product store returned no ProviderAttempt row.
	NoAttempt,
	/// A non-dispatchable existing attempt prevents this logical Turn from being finalized here.
	ExistingAttempt,
}

/// Closed dispatch disposition after post-process ProviderAttempt preparation.
pub(crate) enum PostProcessOutcome {
	/// This call freshly prepared the attempt and retains one-use authorization input.
	FreshPrepared {
		attempt: PreparedAttemptHandoff,
		fresh_preparation: decodex_database::FreshPreparedProviderAttempt,
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
	/// Atomically route one initial Conversation, then plan only when selection committed.
	pub(crate) async fn pre_process(
		&self,
		store: &SqliteStore,
		command: &ExecutionCommand,
	) -> PreProcessOutcome {
		let route = match store
			.route_quick_task_initial(&command.routing_idempotency_key, &command.routing)
			.await
		{
			Ok(QuickTaskInitialRouteOutcome::Fresh(route))
			| Ok(QuickTaskInitialRouteOutcome::Replayed(route)) => route,
			Ok(
				QuickTaskInitialRouteOutcome::Rejected(_)
				| QuickTaskInitialRouteOutcome::ReplayedRejection(_),
			)
			| Err(_) => return failed(ExecutionFailureKind::Other),
		};
		match route.decision.kind {
			AccountRegistryRoutingDecisionKind::Selected =>
				self.plan_selected_initial(store, route).await,
			AccountRegistryRoutingDecisionKind::Waiting
				if route.decision.selected_account_id.is_none()
					&& route.decision.causes.is_empty()
					&& !route.decision.exclusions.is_empty() =>
				PreProcessOutcome::Waiting,
			AccountRegistryRoutingDecisionKind::NoRoute
				if route.decision.selected_account_id.is_none()
					&& !route.decision.causes.is_empty() =>
				PreProcessOutcome::NoRoute,
			_ => failed(ExecutionFailureKind::Other),
		}
	}

	/// Resume establishment from the committed selected decision without routing again.
	pub(crate) async fn resume_establishment(
		&self,
		store: &SqliteStore,
		conversation_id: &ConversationId,
	) -> PreProcessOutcome {
		let route = match store.read_quick_task_initial_route(conversation_id).await {
			Ok(Some(route))
				if route.decision.kind == AccountRegistryRoutingDecisionKind::Selected =>
				route,
			Ok(_) | Err(_) => return failed(ExecutionFailureKind::Other),
		};
		self.plan_selected_initial(store, route).await
	}

	/// Create one routing successor, then route the committed successor in a separate command.
	pub(crate) async fn successor_to_route(
		&self,
		store: &SqliteStore,
		command: &RoutingSuccessorExecutionCommand,
	) -> Result<RoutingSuccessorExecutionOutcome, ExecutionFailureKind> {
		let successor = match store
			.create_quick_task_routing_successor(
				&command.successor_idempotency_key,
				&command.successor,
			)
			.await
		{
			Ok(QuickTaskRoutingSuccessorOutcome::Fresh(successor))
			| Ok(QuickTaskRoutingSuccessorOutcome::Replayed(successor)) => successor,
			Ok(QuickTaskRoutingSuccessorOutcome::Rejected { .. }) | Err(_) => {
				return Err(ExecutionFailureKind::Other);
			},
		};
		let routing = self
			.pre_process(
				store,
				&ExecutionCommand::exact(
					command.routing_idempotency_key.clone(),
					RouteQuickTaskInitial {
						conversation_id: successor.successor_conversation_id.clone(),
						expected_conversation_revision: successor.successor_revision,
					},
				),
			)
			.await;
		Ok(RoutingSuccessorExecutionOutcome { successor, routing })
	}

	/// Bind immutable original route lineage, then plan same-thread or same-account Context Pack.
	pub(crate) async fn continuation_bind_to_plan(
		&self,
		store: &SqliteStore,
		blob_store: &BlobStore,
		command: &ContinuationExecutionCommand,
	) -> PreProcessOutcome {
		let binding = match store
			.bind_quick_task_continuation(&command.binding_idempotency_key, &command.binding)
			.await
		{
			Ok(RoutingCommandOutcome::Success(binding)) => binding,
			Ok(RoutingCommandOutcome::Rejected(_)) | Err(_) => {
				return failed(ExecutionFailureKind::Other);
			},
		};
		let decision = PersistedDecisionProvenance {
			decision_id: binding.decision_id.clone(),
			consumer: binding.consumer.clone(),
		};
		let request = PlanContinuation {
			operation_id: command.continuation_operation_id.clone(),
			routing_decision_id: binding.decision_id,
			expected_consumer_revision: binding.consumer.domain_revision(),
			plan_id: command.continuation_plan_id.clone(),
			fallback_runtime_session_id: command.fallback_runtime_session_id.clone(),
			fallback_account_snapshot_id: binding.account_snapshot_id,
			fallback_context_pack_id: command.fallback_context_pack_id.clone(),
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
			Ok(ContinuationCommandOutcome::Success(plan)) => plan,
			Ok(ContinuationCommandOutcome::Rejected(rejection)) => {
				return failed(ExecutionFailureKind::ContinuationRejected(rejection));
			},
			Err(_) => return failed(ExecutionFailureKind::Other),
		};
		if plan.plan.routing_decision_id != decision.decision_id
			|| plan.plan.consumer != decision.consumer
			|| plan.plan.replay_permitted
			|| plan.plan.dispatch_enabled
			|| !valid_plan_shape(&plan)
		{
			return failed(ExecutionFailureKind::Other);
		}
		PreProcessOutcome::Planned { decision, plan }
	}

	async fn plan_selected_initial(
		&self,
		store: &SqliteStore,
		route: QuickTaskInitialRoute,
	) -> PreProcessOutcome {
		let Some(selected_account_id) = route.decision.selected_account_id.clone() else {
			return failed(ExecutionFailureKind::Other);
		};
		if route.decision.kind != AccountRegistryRoutingDecisionKind::Selected {
			return failed(ExecutionFailureKind::Other);
		}
		let consumer = route.consumer.clone();
		let decision = PersistedDecisionProvenance {
			decision_id: route.decision_id.clone(),
			consumer: consumer.clone(),
		};
		let continuation_idempotency_key =
			routing_scoped_key("initial-continuation", &route.decision_id);
		let request = PlanInitialThreadContinuation {
			operation_id: routing_uuid("initial-continuation-operation", &[&route.decision_id]),
			routing_decision_id: route.decision_id.clone(),
			expected_conversation_revision: consumer.domain_revision(),
			plan_id: routing_uuid("initial-continuation-plan", &[&route.decision_id]),
		};
		let plan = match store
			.plan_initial_thread_continuation(&continuation_idempotency_key, &request)
			.await
		{
			Ok(ContinuationCommandOutcome::Success(effect)) => effect,
			Ok(ContinuationCommandOutcome::Rejected(_)) | Err(_) => {
				return PreProcessOutcome::EstablishmentPending;
			},
		};
		if plan.plan.routing_decision_id != route.decision_id
			|| plan.plan.consumer != consumer
			|| plan.plan.selected_account_id != selected_account_id
			|| plan.plan.replay_permitted
			|| plan.plan.dispatch_enabled
			|| !valid_plan_shape(&plan)
		{
			return PreProcessOutcome::EstablishmentPending;
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

fn routing_scoped_key(scope: &str, key: &str) -> String {
	format!("ordinary-{scope}:{}", routing_digest(&[key]))
}

fn routing_digest(parts: &[&str]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part.len().to_be_bytes());
		digest.update(part.as_bytes());
	}
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn routing_uuid(scope: &str, parts: &[&str]) -> String {
	let digest = Sha256::digest(
		format!("decodex/ordinary-task/{scope}/{}", routing_digest(parts)).as_bytes(),
	);
	let mut bytes = [0_u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15],
	)
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
				&& effect.uncertain_predecessor_attempt_id.is_none()
				&& effect.runtime_session.is_some()
				&& effect.fallback_context_pack.is_none(),
		ContinuationPlanKind::SameThread =>
			effect.plan.codex_thread_id.is_some()
				&& effect.plan.fallback_context_pack_id.is_none()
				&& effect.plan.fallback_runtime_session_id.is_none()
				&& effect.plan.same_thread_evidence.is_some()
				&& effect.uncertain_predecessor_attempt_id.is_none()
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
