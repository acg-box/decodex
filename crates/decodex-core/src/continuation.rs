//! Inert continuation-plan facts after one persisted routing decision.
//!
//! These values carry no routing, scheduling, credential, dispatch, or turn-replay capability.

use crate::{
	AccountId, ConversationId, ExecutionConsumer, ProviderAttemptId, ProviderEvidenceId,
	RuntimeSessionId,
};

/// The mutually exclusive continuation effects available to runtime callers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationPlanKind {
	/// Establish the thread on the exact existing unfenced RuntimeSession.
	InitialThread,
	/// Continue the exact persisted Codex thread under exact positive compatibility evidence.
	SameThread,
	/// Start a new inert RuntimeSession from one atomically linked Context Pack.
	ContextPackFallback,
}

/// Exact positive evidence retained for a same-thread plan.
///
/// Construction of this mechanism-neutral value alone does not prove persisted provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SameThreadContinuationEvidence {
	/// Positive V15 causal experiment evidence retained for a ManagedRun.
	CausalExperiment {
		/// Identity of the persisted positive compatibility evidence selected for this plan.
		routing_evidence_id: String,
		/// Positive immutable revision of that persisted routing evidence.
		routing_evidence_revision: i64,
		/// Exact capability-schema fingerprint recorded by the compatibility evidence.
		schema_fingerprint: String,
		/// Identity of the compatibility experiment whose result the evidence records.
		experiment_id: String,
		/// Positive immutable revision of that compatibility experiment.
		experiment_revision: i64,
		/// Identity of the positive thread-read observation retained by the evidence.
		observation_id: String,
	},
	/// Positive exact-thread readback retained from the original ordinary ProviderAttempt.
	ProviderAttempt {
		/// Original attempt that owns the positive result.
		attempt_id: ProviderAttemptId,
		/// Positive attempt revision at which the evidence became terminal.
		attempt_revision: i64,
		/// Exact positive evidence identity.
		evidence_id: ProviderEvidenceId,
	},
}

/// One immutable, inert, exactly-once plan produced from one selected Routing Decision.
///
/// Construction of this mechanism-neutral value proves neither persistence nor production
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlan {
	/// Stable identity of this single immutable continuation plan.
	pub plan_id: String,
	/// Domain operation identity; the protocol-scoped exact-command idempotency key is separate.
	pub operation_id: String,
	/// Identity of the one persisted Routing Decision consumed by the plan.
	pub routing_decision_id: String,
	/// Exact ordinary or managed consumer preserved from the Routing Decision.
	pub consumer: ExecutionConsumer,
	/// Conversation whose identity is preserved by the continuation.
	pub conversation_id: ConversationId,
	/// RuntimeSession from which the continuation was planned.
	pub source_runtime_session_id: RuntimeSessionId,
	/// Positive immutable revision of the source RuntimeSession.
	pub source_runtime_session_revision: i64,
	/// Account selected by the consumed routing decision; this field grants no selection
	/// authority.
	pub selected_account_id: AccountId,
	/// Mutually exclusive initial-thread, same-thread, or Context-Pack fallback shape.
	pub kind: ContinuationPlanKind,
	/// Exact persisted thread identity for a same-thread plan; absent otherwise.
	pub codex_thread_id: Option<String>,
	/// Atomically linked Context Pack identity for fallback; absent otherwise.
	pub fallback_context_pack_id: Option<String>,
	/// Atomically linked fallback RuntimeSession identity; absent otherwise.
	pub fallback_runtime_session_id: Option<RuntimeSessionId>,
	/// Exact positive compatibility evidence for same-thread continuation; absent otherwise.
	pub same_thread_evidence: Option<SameThreadContinuationEvidence>,
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
	/// The Conversation or ManagedRun revision-bound lineage is stale or incompatible.
	StaleConsumerRevision,
	/// The decision was already consumed by a different exact command or plan identity.
	DecisionAlreadyConsumed,
	/// Exact same-thread proof is absent or incompatible. The receipt is stable and replayable.
	SameThreadUnavailable,
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
