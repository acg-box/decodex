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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameThreadContinuationEvidence {
	pub routing_evidence_id: String,
	pub routing_evidence_revision: i64,
	pub schema_fingerprint: String,
	pub experiment_id: String,
	pub experiment_revision: i64,
	pub observation_id: String,
}

/// One immutable, inert, exactly-once plan produced from one selected V16 decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlan {
	pub plan_id: String,
	pub operation_id: String,
	pub routing_decision_id: String,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
	pub conversation_id: ConversationId,
	pub source_runtime_session_id: RuntimeSessionId,
	pub source_runtime_session_revision: i64,
	pub selected_account_id: AccountId,
	pub kind: ContinuationPlanKind,
	pub codex_thread_id: Option<String>,
	pub fallback_context_pack_id: Option<String>,
	pub fallback_runtime_session_id: Option<RuntimeSessionId>,
	pub same_thread_evidence: Option<SameThreadContinuationEvidence>,
	pub effect_barrier_state: ContinuationEffectBarrierState,
	pub effect_barrier_revision: i64,
	pub submitted_turn_receipt_count: i64,
	pub replay_permitted: bool,
	pub dispatch_enabled: bool,
	pub planned_at_micros: i64,
}

/// Closed stable rejection from the continuation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationRejection {
	InvalidInput,
	MissingDecision,
	DecisionNotSelected,
	StaleManagedRunRevision,
	DecisionAlreadyConsumed,
	InvalidContextPack,
	FallbackIdentityConflict,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContinuationCommandOutcome<T> {
	Success(T),
	Rejected(ContinuationRejection),
}
