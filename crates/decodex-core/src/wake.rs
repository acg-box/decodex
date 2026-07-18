//! Inert append-only scheduler facts for one exact persisted V16 `waiting_usage` decision.
//!
//! A fired transition can request only fresh authoritative routing resolution. It carries no
//! candidate, quota, eligibility, account-selection, credential, or dispatch authority.

use crate::ManagedRunId;

/// Closed resultant state of one immutable waiting-usage wake transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeState {
	Pending,
	Leased,
	Fired,
	Cancelled,
	Superseded,
}

/// Closed operation kind of one immutable transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeTransitionKind {
	Registered,
	Claimed,
	Reclaimed,
	Fired,
	Cancelled,
	Superseded,
}

/// Database-derived reason a wake can no longer fire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeTerminalReason {
	ExplicitCancellation,
	ManagedRunStale,
	PolicyRevisionStale,
	AmbiguousDecisionLineage,
}

/// Exact database-authored lease stored on one claimed or reclaimed transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageWakeLease {
	pub claim_id: String,
	pub holder_id: String,
	pub lease_fence_id: String,
	pub acquired_at_micros: i64,
	pub expires_at_micros: i64,
}

/// Immutable operation result and historical authority for one wake transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageWakeTransition {
	pub transition_id: String,
	pub wake_id: String,
	pub revision: i64,
	pub predecessor_revision: Option<i64>,
	pub predecessor_transition_id: Option<String>,
	pub operation_id: String,
	pub transition_kind: WaitingUsageWakeTransitionKind,
	pub registration_operation_id: String,
	pub routing_decision_id: String,
	pub routing_decision_revision: i64,
	pub routing_policy_id: String,
	pub routing_policy_revision: i64,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
	pub earliest_ready_at_micros: i64,
	pub state: WaitingUsageWakeState,
	pub lease: Option<WaitingUsageWakeLease>,
	pub routing_resolution_request_id: Option<String>,
	pub fresh_routing_resolution_only: bool,
	pub prior_decision_reusable: bool,
	pub production_enabled: bool,
	pub registered_at_micros: i64,
	pub transitioned_at_micros: i64,
	pub terminal_reason: Option<WaitingUsageWakeTerminalReason>,
}

/// Closed stable rejection from the waiting-usage wake authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeRejection {
	InvalidInput,
	MissingDecision,
	DecisionNotWaitingUsage,
	StaleManagedRun,
	StalePolicy,
	AmbiguousDecisionLineage,
	OperationIdentityConflict,
	DecisionAlreadyRegistered,
	ClaimIdentityConflict,
	NoDueWake,
	WakeNotFound,
	StaleWakeTip,
	LeaseLost,
	WakeTerminal,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeCommandOutcome<T> {
	Success(T),
	Rejected(WaitingUsageWakeRejection),
}
