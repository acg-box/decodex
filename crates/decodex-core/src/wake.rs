//! Inert append-only scheduler facts for one exact persisted V16 `waiting_usage` decision.
//!
//! A fired transition can request only fresh authoritative routing resolution. It carries no
//! candidate, quota, eligibility, account-selection, credential, or dispatch authority.
//! These types are mechanism-neutral facts: constructing one in Rust does not prove durable-store
//! authorship or grant routing, scheduling, dispatch, or production authority.
//! Authority instants use nonnegative UTC microseconds since the Unix epoch; strict readback
//! rejects negative values.

use crate::ManagedRunId;

/// Closed resultant state of one immutable waiting-usage wake transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeState {
	/// The registered wake is waiting for its persisted earliest-ready instant and has no lease.
	Pending,
	/// The wake is held under the lease facts carried by the same immutable transition.
	Leased,
	/// The terminal wake emitted one opaque request for fresh routing resolution only.
	Fired,
	/// The terminal wake was stopped by an explicit cancellation.
	Cancelled,
	/// The terminal wake was fenced because its persisted run, policy, or decision lineage is
	/// stale.
	Superseded,
}

/// Closed operation kind of one immutable transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeTransitionKind {
	/// Creates revision one with no predecessor and leaves the wake pending.
	Registered,
	/// Acquires the first fixed-duration lease and advances the append-only lineage.
	Claimed,
	/// Acquires a new lease after the preceding lease expired, without rewriting prior history.
	Reclaimed,
	/// Terminates the wake with one opaque fresh-routing-resolution request identity.
	Fired,
	/// Terminates the wake because an explicit cancellation was accepted.
	Cancelled,
	/// Terminates the wake because authoritative lineage became stale or ambiguous.
	Superseded,
}

/// Database-derived reason a wake can no longer fire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeTerminalReason {
	/// The cancellation command explicitly ended the wake.
	ExplicitCancellation,
	/// The exact ManagedRun revision or its required waiting-usage lifecycle is no longer current.
	ManagedRunStale,
	/// The wake's exact routing-policy revision is no longer the current policy head.
	PolicyRevisionStale,
	/// Another or malformed routing-decision lineage prevents a unique authoritative continuation.
	AmbiguousDecisionLineage,
}

/// Exact database-authored lease stored on one claimed or reclaimed transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageWakeLease {
	/// Identity of the claim attempt; it is unique across persisted wake transitions.
	pub claim_id: String,
	/// Identity of the scheduler holder to which this lease was issued.
	pub holder_id: String,
	/// Opaque fence identity required with the same holder to fire this exact leased tip.
	pub lease_fence_id: String,
	/// Lease acquisition instant as nonnegative UTC microseconds since the Unix epoch, bounded
	/// through `253402300739999999` inclusive.
	pub acquired_at_micros: i64,
	/// Lease expiry instant as nonnegative UTC microseconds since the Unix epoch, exactly
	/// `60000000` microseconds after acquisition and bounded through `253402300799999999`
	/// inclusive.
	pub expires_at_micros: i64,
}

/// Immutable operation result and historical authority for one wake transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitingUsageWakeTransition {
	/// Identity of this immutable transition in the append-only wake ledger.
	pub transition_id: String,
	/// Stable identity shared by every transition in this wake lineage.
	pub wake_id: String,
	/// Positive, contiguous wake revision; revision one is registration.
	pub revision: i64,
	/// Immediately preceding revision, absent only for registration revision one.
	pub predecessor_revision: Option<i64>,
	/// Exact preceding transition identity, absent only for registration revision one.
	pub predecessor_transition_id: Option<String>,
	/// Unique domain-operation identity durably bound to this immutable result.
	pub operation_id: String,
	/// Operation that appended this transition and determines its legal resultant shape.
	pub transition_kind: WaitingUsageWakeTransitionKind,
	/// Registration operation identity retained unchanged across the entire wake lineage.
	pub registration_operation_id: String,
	/// Exact persisted V16 `waiting_usage` decision consumed by registration.
	pub routing_decision_id: String,
	/// Immutable V16 decision revision, which is required to be exactly one.
	pub routing_decision_revision: i64,
	/// Routing-policy identity copied from the exact persisted V16 decision lineage.
	pub routing_policy_id: String,
	/// Positive routing-policy revision to which the wake remains lineage-bound.
	pub routing_policy_revision: i64,
	/// ManagedRun identity copied from the exact persisted V16 decision lineage.
	pub managed_run_id: ManagedRunId,
	/// Positive ManagedRun revision that must retain the waiting-usage lifecycle.
	pub managed_run_revision: i64,
	/// Exact V16 earliest-ready instant as nonnegative UTC microseconds since the Unix epoch,
	/// bounded through `253402300799999999` inclusive.
	pub earliest_ready_at_micros: i64,
	/// Resultant state established by this transition, not a mutable current-head readback.
	pub state: WaitingUsageWakeState,
	/// Complete lease facts for a leased state; absent for every other mutually exclusive state.
	pub lease: Option<WaitingUsageWakeLease>,
	/// Opaque fresh-resolution request identity, present only on the fired terminal transition.
	pub routing_resolution_request_id: Option<String>,
	/// Fixed `true`: firing permits only re-entry into fresh authoritative routing resolution.
	pub fresh_routing_resolution_only: bool,
	/// Fixed `false`: the persisted V16 decision and its eligibility universe cannot be reused.
	pub prior_decision_reusable: bool,
	/// Fixed `false`: this inert fact grants no production scheduling, dispatch, or execution.
	pub production_enabled: bool,
	/// Registration instant as nonnegative UTC microseconds since the Unix epoch, bounded through
	/// `253402300739999999` inclusive and unchanged across the lineage.
	pub registered_at_micros: i64,
	/// Instant this immutable revision was appended, as nonnegative UTC microseconds since the
	/// Unix epoch and bounded through `253402300739999999` inclusive.
	pub transitioned_at_micros: i64,
	/// Cause required for cancelled or superseded terminal states and absent for all other states.
	pub terminal_reason: Option<WaitingUsageWakeTerminalReason>,
}

/// Closed stable rejection from the waiting-usage wake authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeRejection {
	/// The command envelope contains a missing, malformed, zero, negative, or out-of-bound value.
	InvalidInput,
	/// No persisted V16 routing decision exists for the requested decision identity.
	MissingDecision,
	/// The exact persisted V16 decision is not a `waiting_usage` result.
	DecisionNotWaitingUsage,
	/// The bound ManagedRun revision or required waiting-usage lifecycle is no longer current.
	StaleManagedRun,
	/// The bound routing-policy revision is no longer the current policy head.
	StalePolicy,
	/// The persisted routing-decision lineage is missing, conflicting, replaced, or non-unique.
	AmbiguousDecisionLineage,
	/// An existing operation identity was reused with a different canonical domain request.
	OperationIdentityConflict,
	/// A different registration operation already owns the exact decision or ManagedRun revision.
	DecisionAlreadyRegistered,
	/// The claim identity already belongs to another immutable wake transition.
	ClaimIdentityConflict,
	/// No pending or lease-expired wake is due at the authority-selected instant.
	NoDueWake,
	/// No wake exists for the supplied wake identity.
	WakeNotFound,
	/// The expected revision and transition identity do not match the current immutable ledger tip.
	StaleWakeTip,
	/// The holder or lease fence is absent, mismatched, expired, or no longer owns the leased tip.
	LeaseLost,
	/// The wake is already fired, cancelled, or superseded and admits no successor transition.
	WakeTerminal,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WaitingUsageWakeCommandOutcome<T> {
	/// The accepted command's immutable transition-bound result.
	Success(T),
	/// A stable domain rejection that grants no wake effect or authority.
	Rejected(WaitingUsageWakeRejection),
}
