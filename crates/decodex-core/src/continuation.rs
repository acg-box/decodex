//! Inert continuation-plan facts after one persisted routing decision.
//!
//! These values carry no routing, scheduling, credential, dispatch, or turn-replay capability.

use crate::{AccountId, ConversationId, ManagedRunId, RuntimeSessionId};

/// The two mutually exclusive continuation effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationPlanKind {
	/// Continue the exact persisted Codex thread under exact positive compatibility evidence.
	SameThread,
	/// Start a new inert RuntimeSession from one atomically linked Context Pack.
	ContextPackFallback,
}

/// Exact fail-closed ManagedRun effect-barrier state retained by a continuation plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationEffectBarrierState {
	/// Initial inert state; no effect may execute.
	Guarded,
	/// Permanently closed by one accepted ManagedRun safety input.
	Closed,
}

/// Exact positive evidence retained for a same-thread plan.
///
/// Construction of this mechanism-neutral value alone does not prove persisted provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameThreadContinuationEvidence {
	/// Identity of the persisted positive compatibility evidence selected for this plan.
	pub routing_evidence_id: String,
	/// Positive immutable revision of that persisted routing evidence.
	pub routing_evidence_revision: i64,
	/// Exact capability-schema fingerprint recorded by the compatibility evidence.
	pub schema_fingerprint: String,
	/// Identity of the compatibility experiment whose result the evidence records.
	pub experiment_id: String,
	/// Positive immutable revision of that compatibility experiment.
	pub experiment_revision: i64,
	/// Identity of the positive thread-read observation retained by the evidence.
	pub observation_id: String,
}

/// One immutable, inert, exactly-once plan produced from one selected V16 decision.
///
/// Construction of this mechanism-neutral value proves neither persistence nor production authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlan {
	/// Stable identity of this single immutable continuation plan.
	pub plan_id: String,
	/// Domain operation identity; the protocol-scoped exact-command idempotency key is separate.
	pub operation_id: String,
	/// Identity of the one persisted V16 routing decision consumed by the plan.
	pub routing_decision_id: String,
	/// ManagedRun whose identity is preserved across either continuation shape.
	pub managed_run_id: ManagedRunId,
	/// Positive immutable ManagedRun revision against which the plan was authorized.
	pub managed_run_revision: i64,
	/// Conversation whose identity is preserved across either continuation shape.
	pub conversation_id: ConversationId,
	/// RuntimeSession from which the continuation was planned.
	pub source_runtime_session_id: RuntimeSessionId,
	/// Positive immutable revision of the source RuntimeSession.
	pub source_runtime_session_revision: i64,
	/// Account selected by the consumed routing decision; this field grants no selection authority.
	pub selected_account_id: AccountId,
	/// Mutually exclusive same-thread or Context-Pack fallback shape of the plan.
	pub kind: ContinuationPlanKind,
	/// Exact persisted thread identity for a same-thread plan; absent for fallback.
	pub codex_thread_id: Option<String>,
	/// Atomically linked Context Pack identity for fallback; absent for same-thread continuation.
	pub fallback_context_pack_id: Option<String>,
	/// Atomically linked fallback RuntimeSession identity; absent for same-thread continuation.
	pub fallback_runtime_session_id: Option<RuntimeSessionId>,
	/// Exact positive compatibility evidence for same-thread continuation; absent for fallback.
	pub same_thread_evidence: Option<SameThreadContinuationEvidence>,
	/// Closed effect-barrier state retained from the same canonical persisted plan effect.
	pub effect_barrier_state: ContinuationEffectBarrierState,
	/// Positive immutable revision of the retained ManagedRun effect barrier.
	pub effect_barrier_revision: i64,
	/// Nonnegative number of submitted-turn receipts observed by the authority.
	pub submitted_turn_receipt_count: i64,
	/// Required to remain `false`; the plan never authorizes replay of a submitted turn.
	pub replay_permitted: bool,
	/// Required to remain `false`; the inert plan grants no dispatch authority.
	pub dispatch_enabled: bool,
	/// Plan creation time as positive microseconds since the Unix epoch.
	pub planned_at_micros: i64,
}

/// Closed stable rejection from the continuation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationRejection {
	/// The exact command coordinates or required input shape were invalid.
	InvalidInput,
	/// No persisted routing decision exists for the requested identity.
	MissingDecision,
	/// The persisted decision is not a selected-account decision and cannot be continued.
	DecisionNotSelected,
	/// The ManagedRun or its required revision-bound lineage is stale or incompatible.
	StaleManagedRunRevision,
	/// The decision was already consumed by a different exact command or plan identity.
	DecisionAlreadyConsumed,
	/// The fallback Context Pack failed the authority's canonical content or lineage checks.
	InvalidContextPack,
	/// A fallback Context Pack or RuntimeSession identity conflicts with persisted lineage.
	FallbackIdentityConflict,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContinuationCommandOutcome<T> {
	/// Carries the producing adapter's success payload; this variant alone proves no provenance.
	Success(T),
	/// The exact command failed closed with a stable continuation-domain rejection.
	Rejected(ContinuationRejection),
}
