//! Mechanism-neutral facts for PostgreSQL-produced routing authority snapshots.
//!
//! Public construction supports the pure kernel and tests; it does not prove PostgreSQL
//! provenance, persistence, eligibility authority, dispatch authority, or production enablement.

use crate::{
	AccountId, AccountQuotaObservationError, AccountSelectionMode, AccountState,
	ExecutionConsumer, ObservationConfidence, QuotaWindowClass, RuntimeSessionId,
};

/// Closed persisted routing authority shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingAuthorityShape {
	/// Initial Conversation selection from current Account Registry authority with L0 lineage.
	ConversationAccountRegistry,
	/// ManagedRun selection from accepted Project policy authority with L6 lineage.
	ManagedRunProjectPolicy,
	/// Later Conversation binding to the original selected decision with L6 lineage.
	ConversationContinuation,
}
impl RoutingAuthorityShape {
	/// Return the exact stable PostgreSQL discriminator.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::ConversationAccountRegistry => "conversation_account_registry",
			Self::ManagedRunProjectPolicy => "managed_run_project_policy",
			Self::ConversationContinuation => "conversation_continuation",
		}
	}

	/// Parse one exact stable PostgreSQL discriminator.
	pub fn from_sql(value: &str) -> Option<Self> {
		Some(match value {
			"conversation_account_registry" => Self::ConversationAccountRegistry,
			"managed_run_project_policy" => Self::ManagedRunProjectPolicy,
			"conversation_continuation" => Self::ConversationContinuation,
			_ => return None,
		})
	}

	/// Report whether this shape owns an account-selection decision.
	pub const fn is_selecting(self) -> bool {
		matches!(
			self,
			Self::ConversationAccountRegistry | Self::ManagedRunProjectPolicy
		)
	}
}

/// The complete closed ordinary XY-1270 Codex capability projection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CodexCapability {
	/// JSON-RPC initialization handshake.
	Initialize,
	/// Active-account readback for one immutable process binding.
	AccountRead,
	/// Bounded thread listing.
	ThreadList,
	/// Exact-ID thread readback.
	ThreadRead,
	/// Explicit thread archival.
	ThreadArchive,
	/// Paginated rather than legacy persisted thread history.
	PaginatedHistory,
	/// Native run-local collaboration event shape.
	NativeCollaboration,
	/// Read-only thread-search availability.
	ThreadSearch,
}
impl CodexCapability {
	/// Canonical order used by evidence and snapshot matrices.
	pub const ALL: [Self; 8] = [
		Self::Initialize,
		Self::AccountRead,
		Self::ThreadList,
		Self::ThreadRead,
		Self::ThreadArchive,
		Self::PaginatedHistory,
		Self::NativeCollaboration,
		Self::ThreadSearch,
	];

	/// Closed PostgreSQL identity.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountRead => "account_read",
			Self::ThreadList => "thread_list",
			Self::ThreadRead => "thread_read",
			Self::ThreadArchive => "thread_archive",
			Self::PaginatedHistory => "paginated_history",
			Self::NativeCollaboration => "native_collaboration",
			Self::ThreadSearch => "thread_search",
		}
	}
}

/// Explicit policy disposition for one complete inventory member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingMemberDisposition {
	/// Includes the inventory member in eligibility evaluation under the persisted policy.
	Included,
	/// Retains the inventory member explicitly while excluding it from eligibility.
	Excluded,
}

/// Closed retained state for one ordinary capability observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingCapabilityState {
	/// Positive observation that the exact capability is available; applicability remains separate.
	Supported,
	/// Negative evidence that the observed schema omits the capability.
	UnsupportedSchemaMissing,
	/// Negative evidence that the exact method was not found.
	UnsupportedMethodNotFound,
	/// Negative evidence that Codex explicitly rejected the capability operation.
	UnsupportedCodexRejected,
	/// Absence of a completed probe; this never implies support.
	UnavailableNotProbed,
	/// Absence of usable evidence because the probe failed.
	UnavailableProbeFailed,
	/// Evidence that only the degraded legacy-history shape is available.
	DegradedLegacyHistoryOnly,
}

/// Deterministic candidate-quality blocker persisted by Routing Snapshot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoutingBlocker {
	/// The persisted policy explicitly excludes this complete-inventory member.
	ExcludedByPolicy,
	/// The account observation timestamp is later than the database snapshot clock.
	AccountFromFuture,
	/// The observation is too old, or the policy-member revision differs from the locked account.
	AccountStale,
	/// The account is known but its current state is unavailable.
	AccountUnavailable,
	/// The account state is unknown and therefore cannot establish eligibility.
	AccountUnknown,
	/// Persisted account state reports depletion independently of a quota-window fact.
	AccountDepleted,
	/// Authentication evidence reports failure for the account.
	AccountAuthFailed,
	/// Required account-owned plugin readiness has not been established.
	AccountPluginUnready,
	/// Administrative account state explicitly disables the account.
	AccountDisabled,
	/// No ordinary Codex compatibility-evidence row exists for the account.
	EvidenceMissing,
	/// Compatibility evidence was ingested after the database snapshot clock.
	EvidenceFromFuture,
	/// Compatibility evidence is older than the accepted freshness window.
	EvidenceStale,
	/// Compatibility evidence names a different account revision or identity.
	EvidenceAccountMismatch,
	/// Compatibility evidence mismatches the required role or RoleProfile revision.
	EvidenceProfileMismatch,
	/// Compatibility evidence does not match the required exact Codex build.
	EvidenceBuildMismatch,
	/// The exact 300-minute quota observation is absent.
	QuotaFiveHourMissing,
	/// The 300-minute quota observation is later than the database snapshot clock.
	QuotaFiveHourFromFuture,
	/// The 300-minute quota observation is outside the freshness window.
	QuotaFiveHourStale,
	/// The 300-minute value is unknown or its confidence is not high.
	QuotaFiveHourUnknown,
	/// The 300-minute fact has a reset instant that is no longer in the future.
	QuotaFiveHourResetElapsed,
	/// The exact 300-minute fact reports zero remaining capacity.
	QuotaFiveHourDepleted,
	/// The exact 10,080-minute quota observation is absent.
	QuotaSevenDayMissing,
	/// The 10,080-minute quota observation is later than the database snapshot clock.
	QuotaSevenDayFromFuture,
	/// The 10,080-minute quota observation is outside the freshness window.
	QuotaSevenDayStale,
	/// The 10,080-minute value is unknown or its confidence is not high.
	QuotaSevenDayUnknown,
	/// The 10,080-minute fact has a reset instant that is no longer in the future.
	QuotaSevenDayResetElapsed,
	/// The exact 10,080-minute fact reports zero remaining capacity.
	QuotaSevenDayDepleted,
	/// At least one policy-required capability lacks positive applicable evidence.
	RequiredCapabilityUnsatisfied,
	/// Authentication required by the execution consumer is unavailable or unresolved.
	AuthenticationRequired,
	/// Plugin readiness required by the execution consumer is unavailable or unresolved.
	PluginUnready,
	/// A declared dependency blocks this exact execution path.
	DependencyBlocked,
	/// Required approval is absent.
	ApprovalRequired,
	/// Explicit user input is required.
	UserRequired,
	/// An external authority or readback blocks this exact execution path.
	ExternalBlocked,
	/// Usage state is unavailable without complete current positive quota depletion evidence.
	UsageUnproven,
	/// ManagedRun reconciliation lacks an exact unresolved ProcessGeneration or ProviderAttempt.
	ReconciliationUnproven,
	/// No execution-scoped independent Reviewer is available.
	ReviewerUnavailable,
	/// Independent review rejected the result.
	ReviewerFailed,
	/// Reviewer output is missing or ambiguous and grants no approval.
	ReviewerAmbiguous,
	/// ProcessGeneration authority is unresolved for this account path.
	ProcessGenerationUnresolved,
	/// No live fenced ProcessGeneration exists and no reconciliation is pending.
	ProcessGenerationUnavailable,
	/// The exact ProviderAttempt remains unresolved.
	ProviderAttemptUnresolved,
	/// The exact consumer intent already has a terminal ProviderAttempt.
	ProviderAttemptCompleted,
}
impl RoutingBlocker {
	/// Return the exact stable PostgreSQL and protocol spelling.
	pub const fn as_sql(self) -> &'static str {
		use RoutingBlocker::*;
		match self {
			ExcludedByPolicy => "excluded_by_policy",
			AccountFromFuture => "account_from_future",
			AccountStale => "account_stale",
			AccountUnavailable => "account_unavailable",
			AccountUnknown => "account_unknown",
			AccountDepleted => "account_depleted",
			AccountAuthFailed => "account_auth_failed",
			AccountPluginUnready => "account_plugin_unready",
			AccountDisabled => "account_disabled",
			EvidenceMissing => "evidence_missing",
			EvidenceFromFuture => "evidence_from_future",
			EvidenceStale => "evidence_stale",
			EvidenceAccountMismatch => "evidence_account_mismatch",
			EvidenceProfileMismatch => "evidence_profile_mismatch",
			EvidenceBuildMismatch => "evidence_build_mismatch",
			QuotaFiveHourMissing => "quota_five_hour_missing",
			QuotaFiveHourFromFuture => "quota_five_hour_from_future",
			QuotaFiveHourStale => "quota_five_hour_stale",
			QuotaFiveHourUnknown => "quota_five_hour_unknown",
			QuotaFiveHourResetElapsed => "quota_five_hour_reset_elapsed",
			QuotaFiveHourDepleted => "quota_five_hour_depleted",
			QuotaSevenDayMissing => "quota_seven_day_missing",
			QuotaSevenDayFromFuture => "quota_seven_day_from_future",
			QuotaSevenDayStale => "quota_seven_day_stale",
			QuotaSevenDayUnknown => "quota_seven_day_unknown",
			QuotaSevenDayResetElapsed => "quota_seven_day_reset_elapsed",
			QuotaSevenDayDepleted => "quota_seven_day_depleted",
			RequiredCapabilityUnsatisfied => "required_capability_unsatisfied",
			AuthenticationRequired => "authentication_required",
			PluginUnready => "plugin_unready",
			DependencyBlocked => "dependency_blocked",
			ApprovalRequired => "approval_required",
			UserRequired => "user_required",
			ExternalBlocked => "external_blocked",
			UsageUnproven => "usage_unproven",
			ReconciliationUnproven => "reconciliation_unproven",
			ReviewerUnavailable => "reviewer_unavailable",
			ReviewerFailed => "reviewer_failed",
			ReviewerAmbiguous => "reviewer_ambiguous",
			ProcessGenerationUnresolved => "process_generation_unresolved",
			ProcessGenerationUnavailable => "process_generation_unavailable",
			ProviderAttemptUnresolved => "provider_attempt_unresolved",
			ProviderAttemptCompleted => "provider_attempt_completed",
		}
	}

	/// Parse one exact stable PostgreSQL spelling.
	pub fn from_sql(value: &str) -> Option<Self> {
		use RoutingBlocker::*;
		Some(match value {
			"excluded_by_policy" => ExcludedByPolicy,
			"account_from_future" => AccountFromFuture,
			"account_stale" => AccountStale,
			"account_unavailable" => AccountUnavailable,
			"account_unknown" => AccountUnknown,
			"account_depleted" => AccountDepleted,
			"account_auth_failed" => AccountAuthFailed,
			"account_plugin_unready" => AccountPluginUnready,
			"account_disabled" => AccountDisabled,
			"evidence_missing" => EvidenceMissing,
			"evidence_from_future" => EvidenceFromFuture,
			"evidence_stale" => EvidenceStale,
			"evidence_account_mismatch" => EvidenceAccountMismatch,
			"evidence_profile_mismatch" => EvidenceProfileMismatch,
			"evidence_build_mismatch" => EvidenceBuildMismatch,
			"quota_five_hour_missing" => QuotaFiveHourMissing,
			"quota_five_hour_from_future" => QuotaFiveHourFromFuture,
			"quota_five_hour_stale" => QuotaFiveHourStale,
			"quota_five_hour_unknown" => QuotaFiveHourUnknown,
			"quota_five_hour_reset_elapsed" => QuotaFiveHourResetElapsed,
			"quota_five_hour_depleted" => QuotaFiveHourDepleted,
			"quota_seven_day_missing" => QuotaSevenDayMissing,
			"quota_seven_day_from_future" => QuotaSevenDayFromFuture,
			"quota_seven_day_stale" => QuotaSevenDayStale,
			"quota_seven_day_unknown" => QuotaSevenDayUnknown,
			"quota_seven_day_reset_elapsed" => QuotaSevenDayResetElapsed,
			"quota_seven_day_depleted" => QuotaSevenDayDepleted,
			"required_capability_unsatisfied" => RequiredCapabilityUnsatisfied,
			"authentication_required" => AuthenticationRequired,
			"plugin_unready" => PluginUnready,
			"dependency_blocked" => DependencyBlocked,
			"approval_required" => ApprovalRequired,
			"user_required" => UserRequired,
			"external_blocked" => ExternalBlocked,
			"usage_unproven" => UsageUnproven,
			"reconciliation_unproven" => ReconciliationUnproven,
			"reviewer_unavailable" => ReviewerUnavailable,
			"reviewer_failed" => ReviewerFailed,
			"reviewer_ambiguous" => ReviewerAmbiguous,
			"process_generation_unresolved" => ProcessGenerationUnresolved,
			"process_generation_unavailable" => ProcessGenerationUnavailable,
			"provider_attempt_unresolved" => ProviderAttemptUnresolved,
			"provider_attempt_completed" => ProviderAttemptCompleted,
			_ => return None,
		})
	}
}

/// One member of an immutable policy replacement readback.
///
/// Rust construction proves no database provenance and authorizes no selection or dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingPolicyMember {
	/// One-based canonical policy position used for deterministic ordering.
	pub position: usize,
	/// Canonical account identity represented exactly once in the complete inventory.
	pub account_id: AccountId,
	/// Positive account revision bound by the immutable policy member.
	pub account_revision: i64,
	/// Explicit inclusion or exclusion; absence is never interpreted as exclusion.
	pub disposition: RoutingMemberDisposition,
}

/// Immutable policy replacement effect readback.
///
/// Rust construction proves no database provenance and authorizes no selection or dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingPolicyEffect {
	/// Canonical identity of the replaced routing policy.
	pub routing_policy_id: String,
	/// Positive immutable revision created by the replacement.
	pub routing_policy_revision: i64,
	/// Project identity whose accepted Policy supplies routing requirements.
	pub project_id: String,
	/// Identity of the accepted project Policy revision source.
	pub accepted_policy_id: String,
	/// Positive accepted project Policy revision bound by this routing policy.
	pub accepted_policy_revision: i64,
	/// Required global RoleProfile role identity.
	pub required_role: String,
	/// Positive immutable revision of the required RoleProfile.
	pub required_role_profile_revision: i64,
	/// Exact content-addressed Codex build identity required by the policy.
	pub required_build_id: String,
	/// Complete canonically ordered account inventory with explicit dispositions.
	pub members: Vec<RoutingPolicyMember>,
	/// Closed policy-required capability set in canonical capability order.
	pub required_capabilities: Vec<CodexCapability>,
}

/// Immutable compatibility evidence publication readback.
///
/// Rust construction alone proves neither publication nor PostgreSQL provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingEvidenceEffect {
	/// Immutable identity of the published compatibility evidence.
	pub evidence_id: String,
	/// Account identity observed by the bound Codex process.
	pub account_id: AccountId,
	/// Positive account revision to which the observation applies.
	pub account_revision: i64,
	/// Positive per-account compatibility-evidence revision.
	pub evidence_revision: i64,
	/// Observed RoleProfile role identity.
	pub role: String,
	/// Positive RoleProfile revision used by the observed process.
	pub role_profile_revision: i64,
	/// Exact content-addressed Codex build identity that produced the evidence.
	pub build_id: String,
	/// Immutable identity of the one-account Codex process observation.
	pub process_id: String,
	/// Account identity read back from that process; it must equal `account_id`.
	pub process_account_id: AccountId,
	/// Canonical digest of the observed ordinary capability schema.
	pub schema_fingerprint: String,
	/// Recorded ingestion instant in UTC Unix microseconds.
	pub ingested_at_micros: i64,
	/// Complete canonical capability-state projection for the evidence revision.
	pub capabilities: Vec<(CodexCapability, RoutingCapabilityState)>,
}

/// One database-produced member in canonical policy order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotMember {
	/// One-based canonical policy position retained in the immutable snapshot.
	pub position: usize,
	/// Canonical account identity represented by this complete-inventory member.
	pub account_id: AccountId,
	/// Explicit policy disposition; excluded members remain present as evidence.
	pub disposition: RoutingMemberDisposition,
	/// Positive account revision observed under the snapshot lock boundary.
	pub account_revision: i64,
	/// Credential-negative account label retained for exact source comparison.
	pub display_label: String,
	/// Account state observed under the snapshot's database clock and locks.
	pub account_state: AccountState,
	/// Canonical six-digit RFC 3339 UTC account-observation timestamp.
	pub account_observed_at_utc: String,
	/// Compatibility-evidence identity, or `None` when no evidence exists.
	pub evidence_id: Option<String>,
	/// Positive evidence revision, absent exactly when `evidence_id` is absent.
	pub evidence_revision: Option<i64>,
	/// Evidence-bound account revision, absent with the evidence identity.
	pub evidence_account_revision: Option<i64>,
	/// Evidence-bound RoleProfile role, absent with the evidence identity.
	pub evidence_role: Option<String>,
	/// Evidence-bound RoleProfile revision, absent with the evidence identity.
	pub evidence_role_profile_revision: Option<i64>,
	/// Evidence-bound exact Codex build, absent with the evidence identity.
	pub evidence_build_id: Option<String>,
	/// Bound process identity, present exactly when `evidence_id` is present and absent with it.
	pub process_id: Option<String>,
	/// Schema digest, present exactly when `evidence_id` is present and absent with it.
	pub schema_fingerprint: Option<String>,
	/// Whether persisted RuntimeSession affinity prefers this independently eligible member.
	pub sticky: bool,
	/// Complete deterministic blockers; an empty list alone grants no dispatch authority.
	pub blockers: Vec<RoutingBlocker>,
}

/// Closed Account Registry quota observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountRegistryQuotaObservation {
	/// No quota observation exists for the account and window.
	Missing,
	/// Current quota use and its observation and reset instants.
	Current {
		/// Used percentage in `0..=100`.
		used_percent: u8,
		/// Observation instant in the closed UTC Unix microsecond product range.
		observed_at_micros: i64,
		/// Later reset instant in the closed UTC Unix microsecond product range.
		resets_at_micros: i64,
	},
	/// The quota observation failed with one closed Account Registry error.
	ObservationError {
		/// Exact closed error returned by the account observation owner.
		error: AccountQuotaObservationError,
		/// Failure instant in the closed UTC Unix microsecond product range.
		observed_at_micros: i64,
	},
}

/// One Account Registry quota fact.
///
/// At the persistence adapter boundary, only 300 and 10,080 are valid
/// `duration_minutes` values, and each value must agree with `window`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistryQuotaFact {
	/// Account identity to which this duration-typed fact belongs.
	pub account_id: AccountId,
	/// Closed quota-window class.
	pub window: QuotaWindowClass,
	/// Exact quota-window duration in minutes.
	pub duration_minutes: u16,
	/// Closed observation for this account and quota window.
	pub observation: AccountRegistryQuotaObservation,
}

/// One Account Registry routing candidate in canonical position order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistryRoutingMember {
	/// One-based canonical position used for deterministic selection.
	pub position: usize,
	/// Canonical account identity represented by this member.
	pub account_id: AccountId,
	/// Positive Account Registry revision observed for the account.
	pub account_revision: i64,
	/// Canonical unique Account Registry account blockers in strict enum order.
	pub blockers: Vec<RoutingBlocker>,
}

/// Immutable Account Registry routing snapshot for an initial Conversation selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistryRoutingSnapshot {
	/// Immutable snapshot identity.
	pub snapshot_id: String,
	/// Positive Account Registry routing revision resolved by the snapshot.
	pub routing_revision: i64,
	/// Account-selection mode applied to the complete member inventory.
	pub mode: AccountSelectionMode,
	/// Positive task RoleProfile revision used for classification.
	pub task_role_profile_revision: i64,
	/// Resolution instant in the closed UTC Unix microsecond product range.
	pub resolved_at_micros: i64,
	/// Complete account inventory in canonical position order.
	pub members: Vec<AccountRegistryRoutingMember>,
	/// Complete two-window Account Registry quota matrix for every member.
	pub quota_facts: Vec<AccountRegistryQuotaFact>,
}

/// Immutable lineage binding for a later Conversation routing continuation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationContinuationRoutingBinding {
	/// RuntimeSession identity that owns the original selection.
	pub source_session_id: RuntimeSessionId,
	/// Positive revision of the source RuntimeSession.
	pub source_session_revision: i64,
	/// Immutable identity of the initial routing decision.
	pub initial_decision_id: String,
	/// Immutable account-snapshot identity bound by the initial decision.
	pub account_snapshot_id: String,
	/// Positive revision of the bound account snapshot.
	pub account_snapshot_revision: i64,
	/// Immutable profile-snapshot identity bound by the initial decision.
	pub profile_snapshot_id: String,
	/// Positive revision of the bound profile snapshot.
	pub profile_snapshot_revision: i64,
}

/// One exact duration-owned quota fact. Missing observations retain explicit null facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotQuotaFact {
	/// Account identity to which this duration-typed fact belongs.
	pub account_id: AccountId,
	/// Closed quota-window class; five-hour and seven-day facts never substitute.
	pub window: QuotaWindowClass,
	/// Exact window duration in minutes: 300 or 10,080 as selected by `window`.
	pub duration_minutes: u16,
	/// Positive source observation revision, or `None` for an explicit missing fact.
	pub observation_revision: Option<i64>,
	/// Remaining percentage in `0..=100`, or `None` when the value is unknown.
	pub remaining_percent: Option<u8>,
	/// Exact reset instant in UTC Unix microseconds, or `None` when unavailable.
	pub resets_at_micros: Option<i64>,
	/// Exact observation instant in UTC Unix microseconds, absent with its revision.
	pub observed_at_micros: Option<i64>,
	/// Observation confidence, absent exactly when no observation revision exists.
	pub confidence: Option<ObservationConfidence>,
}

/// One cell in the complete member-by-ordinary-capability matrix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotCapabilityFact {
	/// Account identity for this matrix cell.
	pub account_id: AccountId,
	/// Canonically ordered ordinary Codex capability represented by the cell.
	pub capability: CodexCapability,
	/// Whether the accepted policy requires this capability for this member.
	pub applicable: bool,
	/// Retained observation state, or `None` when compatibility evidence supplies no cell.
	pub evidence_state: Option<RoutingCapabilityState>,
}

/// Complete immutable PostgreSQL classification readback for the later Routing Decision pure
/// kernel.
///
/// The public value is mechanism-neutral; only the adapter and transaction establish provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshot {
	/// Immutable database snapshot identity; constructing this string does not prove provenance.
	pub snapshot_id: String,
	/// Routing-policy identity whose exact revision supplied member order and requirements.
	pub routing_policy_id: String,
	/// Positive immutable routing-policy revision bound by the snapshot.
	pub routing_policy_revision: i64,
	/// Accepted project Policy identity transitively bound through the routing policy.
	pub accepted_policy_id: String,
	/// Positive accepted project Policy revision used for classification.
	pub accepted_policy_revision: i64,
	/// Required global RoleProfile role identity.
	pub required_role: String,
	/// Positive immutable revision of the required RoleProfile.
	pub required_role_profile_revision: i64,
	/// Exact content-addressed Codex build required for compatibility.
	pub required_build_id: String,
	/// Exact ordinary or managed execution consumer for which the snapshot was resolved.
	pub consumer: ExecutionConsumer,
	/// RuntimeSession identity supplying L6 affinity, absent only for initial L0 routing.
	pub runtime_session_id: Option<RuntimeSessionId>,
	/// Positive RuntimeSession revision, jointly absent only for initial L0 routing.
	pub runtime_session_revision: Option<i64>,
	/// Immutable account-snapshot identity, jointly absent only for initial L0 routing.
	pub account_snapshot_id: Option<String>,
	/// Positive account source revision, jointly absent only for initial L0 routing.
	pub account_snapshot_source_revision: Option<i64>,
	/// Immutable profile-snapshot identity, jointly absent only for initial L0 routing.
	pub profile_snapshot_id: Option<String>,
	/// Positive profile source revision, jointly absent only for initial L0 routing.
	pub profile_snapshot_source_revision: Option<i64>,
	/// PostgreSQL resolution instant in UTC Unix microseconds.
	pub resolved_at_micros: i64,
	/// Complete account inventory in canonical policy order.
	pub members: Vec<RoutingSnapshotMember>,
	/// Complete two-window quota matrix for every member.
	pub quota_facts: Vec<RoutingSnapshotQuotaFact>,
	/// Complete member-by-ordinary-capability matrix.
	pub capability_facts: Vec<RoutingSnapshotCapabilityFact>,
}

/// Stable exact-command domain rejection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingRejection {
	/// Stable exact-command operation name that produced the rejection.
	pub operation: String,
	/// Stable typed domain-rejection code; it carries no routing choice.
	pub code: String,
}

/// Closed exact-command result. No variant carries routing selection authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingCommandOutcome<T> {
	/// Exact-command success containing an inert typed effect or readback.
	Success(T),
	/// Stable domain rejection with no persisted routing selection authority.
	Rejected(RoutingRejection),
}

/// Exact source precision accepted by the Routing Decision boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingTimestampPrecision {
	/// The raw source value is exactly representable as UTC Unix microseconds.
	UnixMicrosecond,
}

/// Retained raw-lineage shape for one exact quota timestamp.
///
/// Only validated adapter readback establishes that the values came from persisted authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingTimestampProvenance {
	/// Unrounded canonical decimal UTC Unix-microsecond source value.
	pub raw_value: String,
	/// Credential-negative identity of the observation source.
	pub source_id: String,
	/// Exact source precision; unsupported precision must not be normalized.
	pub precision: RoutingTimestampPrecision,
	/// Positive evidence revision that supplied this timestamp.
	pub evidence_revision: i64,
}

/// Routing Decision quota evidence presented to the pure kernel by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionQuotaFact {
	/// Account identity to which this Routing Decision quota fact belongs.
	pub account_id: AccountId,
	/// Closed duration-typed quota-window class.
	pub window: QuotaWindowClass,
	/// Exact window duration in minutes: 300 or 10,080 according to `window`.
	pub duration_minutes: u16,
	/// Positive authoritative observation revision, or `None` for explicit absence.
	pub observation_revision: Option<i64>,
	/// Remaining percentage in `0..=100`, or `None` when unknown.
	pub remaining_percent: Option<u8>,
	/// Exact reset instant in UTC Unix microseconds, whether future or elapsed, or explicit
	/// absence. An elapsed value is classified by the matching `Quota*ResetElapsed` blocker.
	pub resets_at_micros: Option<i64>,
	/// Exact observation instant in UTC Unix microseconds, or explicit absence.
	pub observed_at_micros: Option<i64>,
	/// Source confidence, absent when no authoritative observation exists.
	pub confidence: Option<ObservationConfidence>,
	/// Exact observed-time provenance, or `None` when provenance is unavailable.
	pub observed_at_provenance: Option<RoutingTimestampProvenance>,
	/// Exact reset-time provenance, or `None` when provenance is unavailable.
	pub resets_at_provenance: Option<RoutingTimestampProvenance>,
}

/// Closed input shape consumed by the Routing Decision pure routing kernel.
///
/// PostgreSQL supplies the authoritative production value; arbitrary Rust construction does not
/// establish completeness, persistence, or routing authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionSnapshot {
	/// Immutable snapshot identity whose text alone does not prove database authorship.
	pub snapshot_id: String,
	/// Decision instant in UTC Unix microseconds, supplied by PostgreSQL in authoritative use.
	pub decided_at_micros: i64,
	/// Complete canonically ordered candidate universe supplied to the pure kernel.
	pub members: Vec<RoutingDecisionCandidate>,
	/// Complete two-window quota matrix ordered by member and duration.
	pub quota_facts: Vec<RoutingDecisionQuotaFact>,
	/// Complete member-by-capability matrix in canonical capability order.
	pub capability_facts: Vec<RoutingSnapshotCapabilityFact>,
}

/// One candidate shape in the complete Routing Decision universe.
///
/// The enclosing validated snapshot, not public construction, establishes database authorship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionCandidate {
	/// One-based canonical persisted-policy position used for deterministic ordering.
	pub position: usize,
	/// Canonical account identity represented exactly once in the closed universe.
	pub account_id: AccountId,
	/// Explicit policy disposition; excluded members remain visible but ineligible.
	pub disposition: RoutingMemberDisposition,
	/// Persisted affinity preference that applies only after independent eligibility.
	pub sticky: bool,
	/// Complete current blockers; an empty vector is necessary but grants no execution authority.
	pub blockers: Vec<RoutingBlocker>,
}

/// Stable semantic outcome shape persisted by Routing Decision after transactional validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingDecisionKind {
	/// One independently eligible account was deterministically chosen.
	Selected,
	/// Every otherwise eligible account is blocked only by exact usage depletion.
	WaitingUsage,
	/// Every otherwise eligible path is blocked only by unresolved process or attempt authority.
	WaitingReconciliation,
	/// No account is routable, without implying a future wake instant.
	NoRoute,
}

/// Stable reason for a typed no-route result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingNoRouteReason {
	/// Complete evidence contains a non-depletion blocker or unusable unknown fact.
	BlockedEvidence,
}

/// One normalized, duration-typed depletion-exclusion shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionExclusion {
	/// Account identity excluded by this exact depletion fact.
	pub account_id: AccountId,
	/// One-based canonical member position tied to the persisted snapshot member.
	pub member_position: usize,
	/// Duration-typed quota window whose exact fact caused exclusion.
	pub window: QuotaWindowClass,
	/// Exact duration in minutes: 300 or 10,080 according to `window`.
	pub duration_minutes: u16,
	/// Positive authoritative quota observation revision.
	pub observation_revision: i64,
	/// Exact remaining percentage; normalized depletion exclusions require zero.
	pub remaining_percent: u8,
	/// Exact observation instant in UTC Unix microseconds.
	pub observed_at_micros: i64,
	/// Exact future-ready instant in UTC Unix microseconds.
	pub resets_at_micros: i64,
	/// Source confidence; persisted depletion exclusions require high confidence.
	pub confidence: ObservationConfidence,
	/// Exact raw/source/revision lineage for `observed_at_micros`.
	pub observed_at_provenance: RoutingTimestampProvenance,
	/// Exact raw/source/revision lineage for `resets_at_micros`.
	pub resets_at_provenance: RoutingTimestampProvenance,
}

/// One exact account-scoped cause retained by a non-selected route projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionCause {
	/// Account path to which the cause applies.
	pub account_id: AccountId,
	/// Exact persisted blocker without lossy category collapse.
	pub blocker: RoutingBlocker,
}

/// Inert deterministic Routing Decision result shape.
///
/// Construction does not prove persistence and carries no routing, dispatch, or execution
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecision {
	/// Immutable source snapshot identity; constructing it does not establish authority.
	pub snapshot_id: String,
	/// Mutually exclusive semantic decision shape.
	pub kind: RoutingDecisionKind,
	/// Selected account only for `Selected`; otherwise `None`.
	pub selected_account_id: Option<AccountId>,
	/// Exact earliest ready UTC Unix microsecond only for `WaitingUsage`.
	pub ready_at_micros: Option<i64>,
	/// Typed reason only for `NoRoute`; selected and waiting shapes use `None`.
	pub no_route_reason: Option<RoutingNoRouteReason>,
	/// Complete normalized depletion lineage required by the selected or waiting shape.
	pub exclusions: Vec<RoutingDecisionExclusion>,
	/// Complete included-member causes for waits, or complete-universe causes for `NoRoute`.
	pub causes: Vec<RoutingDecisionCause>,
}

/// Structural failure of a supposedly closed database-authored snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingKernelError {
	/// The supposedly closed universe has invalid identity, order, or matrix shape.
	MalformedSnapshot,
	/// Required exact evidence or provenance is missing or internally inconsistent.
	IncompleteEvidence,
}

/// Select from one closed database-authored snapshot without I/O, clocks, or mechanisms.
pub fn decide_routing(
	snapshot: &RoutingDecisionSnapshot,
) -> Result<RoutingDecision, RoutingKernelError> {
	let members = &snapshot.members;
	let facts_by_member = validated_quota_facts(snapshot)?;

	let included = members
		.iter()
		.enumerate()
		.filter(|(_, member)| member.disposition == RoutingMemberDisposition::Included)
		.collect::<Vec<_>>();
	let complete_universe = members.iter().enumerate().collect::<Vec<_>>();
	let no_route_causes = routing_causes(&complete_universe);
	if included.is_empty() {
		return no_route(&snapshot.snapshot_id, no_route_causes);
	}

	let sticky = included.iter().copied().find(|(_, member)| member.sticky);
	let selected = sticky
		.filter(|(index, member)| {
			member.blockers.is_empty()
				&& account_available(&facts_by_member[*index], snapshot.decided_at_micros)
		})
		.or_else(|| {
			included.iter().copied().find(|(index, member)| {
				member.blockers.is_empty()
					&& account_available(&facts_by_member[*index], snapshot.decided_at_micros)
			})
		});
	if let Some((selected_index, selected_member)) = selected {
		let mut exclusions = Vec::new();
		for (index, member) in included.iter().copied() {
			if index >= selected_index {
				break;
			}
			let account_exclusions =
				depletion_exclusions(member, &facts_by_member[index], snapshot.decided_at_micros)?;
			let required =
				member.blockers.iter().filter(|blocker| is_depletion_blocker(**blocker)).count();
			if account_exclusions.len() != required {
				return no_route(&snapshot.snapshot_id, no_route_causes);
			}
			exclusions.extend(account_exclusions);
		}
		return Ok(RoutingDecision {
			snapshot_id: snapshot.snapshot_id.clone(),
			kind: RoutingDecisionKind::Selected,
			selected_account_id: Some(selected_member.account_id.clone()),
			ready_at_micros: None,
			no_route_reason: None,
			exclusions,
			causes: Vec::new(),
		});
	}

	let causes = routing_causes(&included);
	if causes.is_empty() {
		return Err(RoutingKernelError::IncompleteEvidence);
	}
	let only_depletion = included.iter().all(|(_, member)| {
		!member.blockers.is_empty()
			&& member.blockers.iter().all(|blocker| is_depletion_blocker(*blocker))
	});
	if !only_depletion {
		if included.iter().all(|(_, member)| {
			!member.blockers.is_empty()
				&& member.blockers.iter().all(|blocker| is_reconciliation_blocker(*blocker))
		}) {
			return Ok(RoutingDecision {
				snapshot_id: snapshot.snapshot_id.clone(),
				kind: RoutingDecisionKind::WaitingReconciliation,
				selected_account_id: None,
				ready_at_micros: None,
				no_route_reason: None,
				exclusions: Vec::new(),
				causes,
			});
		}
		return no_route(&snapshot.snapshot_id, no_route_causes);
	}
	let mut exclusions = Vec::new();
	let mut earliest_ready = None;
	for (index, member) in included {
		if !facts_by_member[index]
			.iter()
			.all(|fact| quota_fact_current(fact, snapshot.decided_at_micros))
		{
			return no_route(&snapshot.snapshot_id, no_route_causes.clone());
		}
		let account_exclusions =
			depletion_exclusions(member, &facts_by_member[index], snapshot.decided_at_micros)?;
		if account_exclusions.is_empty() {
			return no_route(&snapshot.snapshot_id, no_route_causes.clone());
		}
		let ready = account_exclusions
			.iter()
			.map(|exclusion| exclusion.resets_at_micros)
			.max()
			.ok_or(RoutingKernelError::IncompleteEvidence)?;
		earliest_ready = Some(earliest_ready.map_or(ready, |current: i64| current.min(ready)));
		exclusions.extend(account_exclusions);
	}
	Ok(RoutingDecision {
		snapshot_id: snapshot.snapshot_id.clone(),
		kind: RoutingDecisionKind::WaitingUsage,
		selected_account_id: None,
		ready_at_micros: earliest_ready,
		no_route_reason: None,
		exclusions,
		causes,
	})
}

fn validated_quota_facts(
	snapshot: &RoutingDecisionSnapshot,
) -> Result<Vec<Vec<&RoutingDecisionQuotaFact>>, RoutingKernelError> {
	let members = &snapshot.members;
	if members.is_empty()
		|| members.iter().enumerate().any(|(index, member)| member.position != index + 1)
		|| members.iter().enumerate().any(|(index, member)| {
			members[..index].iter().any(|prior| prior.account_id == member.account_id)
		}) || members.iter().filter(|member| member.sticky).count() > 1
		|| members.iter().any(|member| {
			(member.disposition == RoutingMemberDisposition::Excluded)
				!= member.blockers.contains(&RoutingBlocker::ExcludedByPolicy)
		}) {
		return Err(RoutingKernelError::MalformedSnapshot);
	}
	let mut facts_by_member = Vec::with_capacity(members.len());
	for member in members {
		let facts = snapshot
			.quota_facts
			.iter()
			.filter(|fact| fact.account_id == member.account_id)
			.collect::<Vec<_>>();
		if facts.len() != 2
			|| facts[0].window != QuotaWindowClass::FiveHour
			|| facts[0].duration_minutes != 300
			|| facts[1].window != QuotaWindowClass::SevenDay
			|| facts[1].duration_minutes != 10_080
		{
			return Err(RoutingKernelError::MalformedSnapshot);
		}
		facts_by_member.push(facts);
	}
	if snapshot.quota_facts.len() != members.len() * 2 {
		return Err(RoutingKernelError::MalformedSnapshot);
	}
	if snapshot.capability_facts.len() != members.len() * CodexCapability::ALL.len()
		|| snapshot.capability_facts.iter().enumerate().any(|(index, fact)| {
			fact.account_id != members[index / CodexCapability::ALL.len()].account_id
				|| fact.capability != CodexCapability::ALL[index % CodexCapability::ALL.len()]
		}) {
		return Err(RoutingKernelError::MalformedSnapshot);
	}
	Ok(facts_by_member)
}

fn no_route(
	snapshot_id: &str,
	causes: Vec<RoutingDecisionCause>,
) -> Result<RoutingDecision, RoutingKernelError> {
	if causes.is_empty() {
		return Err(RoutingKernelError::IncompleteEvidence);
	}
	Ok(RoutingDecision {
		snapshot_id: snapshot_id.to_owned(),
		kind: RoutingDecisionKind::NoRoute,
		selected_account_id: None,
		ready_at_micros: None,
		no_route_reason: Some(RoutingNoRouteReason::BlockedEvidence),
		exclusions: Vec::new(),
		causes,
	})
}

fn routing_causes(members: &[(usize, &RoutingDecisionCandidate)]) -> Vec<RoutingDecisionCause> {
	members
		.iter()
		.flat_map(|(_, member)| {
			member.blockers.iter().copied().map(|blocker| RoutingDecisionCause {
				account_id: member.account_id.clone(),
				blocker,
			})
		})
		.collect()
}

fn account_available(facts: &[&RoutingDecisionQuotaFact], decided_at_micros: i64) -> bool {
	facts.iter().all(|fact| {
		quota_fact_current(fact, decided_at_micros)
			&& fact.remaining_percent.is_some_and(|remaining| remaining > 0)
	})
}

fn quota_fact_current(fact: &RoutingDecisionQuotaFact, decided_at_micros: i64) -> bool {
	fact.confidence == Some(ObservationConfidence::High)
		&& fact.remaining_percent.is_some()
		&& fact.observation_revision.is_some_and(|revision| {
			fact.observed_at_provenance
				.as_ref()
				.is_some_and(|value| provenance_complete(value, revision, fact.observed_at_micros))
				&& fact.resets_at_provenance.as_ref().is_some_and(|value| {
					provenance_complete(value, revision, fact.resets_at_micros)
				})
		}) && fact.observed_at_micros.is_some_and(|observed| {
		observed <= decided_at_micros && decided_at_micros - observed <= 300_000_000
	}) && fact.resets_at_micros.is_some_and(|resets| resets > decided_at_micros)
}

fn is_depletion_blocker(blocker: RoutingBlocker) -> bool {
	matches!(blocker, RoutingBlocker::QuotaFiveHourDepleted | RoutingBlocker::QuotaSevenDayDepleted)
}

fn is_reconciliation_blocker(blocker: RoutingBlocker) -> bool {
	matches!(
		blocker,
		RoutingBlocker::ProcessGenerationUnresolved | RoutingBlocker::ProviderAttemptUnresolved
	)
}

fn provenance_complete(
	value: &RoutingTimestampProvenance,
	revision: i64,
	canonical_micros: Option<i64>,
) -> bool {
	value.evidence_revision == revision
		&& !value.source_id.is_empty()
		&& value.raw_value.parse::<i64>().ok() == canonical_micros
		&& canonical_micros
			.is_some_and(|micros| micros >= 0 && value.raw_value == micros.to_string())
}

fn depletion_exclusions(
	member: &RoutingDecisionCandidate,
	facts: &[&RoutingDecisionQuotaFact],
	decided_at_micros: i64,
) -> Result<Vec<RoutingDecisionExclusion>, RoutingKernelError> {
	let mut result = Vec::new();
	for fact in facts {
		let depletion_blocker = match fact.window {
			QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourDepleted,
			QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayDepleted,
		};
		if fact.remaining_percent != Some(0) || !member.blockers.contains(&depletion_blocker) {
			continue;
		}
		let (
			Some(observed),
			Some(resets),
			Some(revision),
			Some(observed_at_micros),
			Some(resets_at_micros),
			Some(confidence),
		) = (
			fact.observed_at_provenance.clone(),
			fact.resets_at_provenance.clone(),
			fact.observation_revision,
			fact.observed_at_micros,
			fact.resets_at_micros,
			fact.confidence,
		)
		else {
			continue;
		};
		if !provenance_complete(&observed, revision, Some(observed_at_micros))
			|| !provenance_complete(&resets, revision, Some(resets_at_micros))
		{
			return Err(RoutingKernelError::IncompleteEvidence);
		}
		if confidence != ObservationConfidence::High
			|| observed_at_micros > decided_at_micros
			|| decided_at_micros - observed_at_micros > 300_000_000
			|| resets_at_micros <= decided_at_micros
		{
			continue;
		}
		result.push(RoutingDecisionExclusion {
			account_id: member.account_id.clone(),
			member_position: member.position,
			window: fact.window,
			duration_minutes: fact.duration_minutes,
			observation_revision: revision,
			remaining_percent: 0,
			observed_at_micros,
			resets_at_micros,
			confidence,
			observed_at_provenance: observed,
			resets_at_provenance: resets,
		});
	}
	Ok(result)
}

/// Closed outcome kind for Account Registry routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountRegistryRoutingDecisionKind {
	/// One evaluated account with no cause or exclusion was selected.
	Selected,
	/// Every evaluated account was blocked only by positive current depletion.
	Waiting,
	/// At least one evaluated account had a retained routing cause.
	NoRoute,
}

/// One exact current-depletion exclusion produced by Account Registry routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistryRoutingExclusion {
	/// Account identity excluded by this quota fact.
	pub account_id: AccountId,
	/// One-based canonical position of the excluded member.
	pub member_position: usize,
	/// Quota window that caused the exclusion.
	pub window: QuotaWindowClass,
	/// Exact duration in minutes for `window`.
	pub duration_minutes: u16,
	/// Validated used percentage; depletion exclusions contain 100.
	pub used_percent: u8,
	/// Validated quota observation instant in UTC Unix microseconds.
	pub observed_at_micros: i64,
	/// Validated future quota reset instant in UTC Unix microseconds.
	pub resets_at_micros: i64,
}

/// The deterministic result of applying Account Registry routing to one snapshot.
///
/// Field rules by `kind`:
/// - `Selected`: `selected_account_id` is `Some`; causes and exclusions contain complete
///   classifications for evaluated preceding members and may be empty.
/// - `Waiting`: `selected_account_id` is `None`, `exclusions` is non-empty, and `causes`
///   is empty.
/// - `NoRoute`: `selected_account_id` is `None`, `causes` is non-empty, and `exclusions`
///   contains every positive current depletion found during evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistryRoutingDecision {
	/// Exact identity copied from the validated source snapshot.
	pub snapshot_id: String,
	/// Mutually exclusive semantic decision kind.
	pub kind: AccountRegistryRoutingDecisionKind,
	/// Selected account exactly for `Selected`; otherwise `None`.
	pub selected_account_id: Option<AccountId>,
	/// Complete evaluated positive current depletions in member and window order.
	pub exclusions: Vec<AccountRegistryRoutingExclusion>,
	/// Complete evaluated member and quota causes in deterministic order.
	pub causes: Vec<RoutingDecisionCause>,
}

/// Structural failure of an Account Registry routing snapshot or decision instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountRegistryRoutingKernelError {
	/// The supplied decision instant is outside the closed timestamp product range.
	InvalidDecidedAtMicros {
		/// Invalid decision instant in UTC Unix microseconds.
		decided_at_micros: i64,
	},
	/// The Account Registry routing revision is not positive.
	InvalidRoutingRevision {
		/// Invalid Account Registry routing revision.
		routing_revision: i64,
	},
	/// The task RoleProfile revision is not positive.
	InvalidTaskRoleProfileRevision {
		/// Invalid task RoleProfile revision.
		task_role_profile_revision: i64,
	},
	/// The snapshot resolution instant is invalid or later than the decision instant.
	InvalidResolvedAtMicros {
		/// Invalid resolution instant in UTC Unix microseconds.
		resolved_at_micros: i64,
	},
	/// The Account Registry member inventory is empty.
	EmptyMembers,
	/// One account identity occurs at more than one member position.
	DuplicateMember {
		/// Repeated account identity.
		account_id: AccountId,
		/// First one-based position containing the account.
		first_position: usize,
		/// Later one-based position containing the account.
		duplicate_position: usize,
	},
	/// A member position does not match the canonical one-based sequence.
	NonCanonicalMember {
		/// Account identity at the invalid position.
		account_id: AccountId,
		/// Position supplied by the member.
		member_position: usize,
		/// Position required by canonical member order.
		expected_member_position: usize,
	},
	/// A member carries a non-positive Account Registry revision.
	InvalidMemberAccountRevision {
		/// Account identity with the invalid revision.
		account_id: AccountId,
		/// Invalid Account Registry revision.
		account_revision: i64,
	},
	/// A member blocker does not belong to the closed Account Registry account subset.
	ForbiddenMemberBlocker {
		/// Account identity owning the forbidden blocker.
		account_id: AccountId,
		/// One-based canonical member position.
		member_position: usize,
		/// One-based position of the forbidden blocker.
		blocker_position: usize,
		/// Blocker outside the Account Registry account subset.
		blocker: RoutingBlocker,
	},
	/// One Account Registry member blocker occurs more than once.
	DuplicateMemberBlocker {
		/// Account identity owning the duplicate blocker.
		account_id: AccountId,
		/// One-based canonical member position.
		member_position: usize,
		/// Repeated Account Registry account blocker.
		blocker: RoutingBlocker,
		/// One-based first position of the blocker.
		first_blocker_position: usize,
		/// One-based later position of the blocker.
		duplicate_blocker_position: usize,
	},
	/// Account Registry member blockers are outside strict canonical enum order.
	NonCanonicalMemberBlocker {
		/// Account identity owning the reordered blocker.
		account_id: AccountId,
		/// One-based canonical member position.
		member_position: usize,
		/// One-based position of the reordered blocker.
		blocker_position: usize,
		/// Canonically later blocker supplied immediately before this blocker.
		previous_blocker: RoutingBlocker,
		/// Canonically earlier blocker supplied after it.
		blocker: RoutingBlocker,
	},
	/// More than one quota fact exists for one account and window.
	DuplicateQuotaFact {
		/// Account identity owning the duplicate fact.
		account_id: AccountId,
		/// Window repeated by the duplicate fact.
		window: QuotaWindowClass,
	},
	/// A required quota fact is absent for one member and window.
	MissingQuotaFact {
		/// Account identity missing the fact.
		account_id: AccountId,
		/// Required window that is absent.
		window: QuotaWindowClass,
	},
	/// A quota fact names an account outside the member inventory.
	ExtraQuotaFact {
		/// Unknown account identity named by the fact.
		account_id: AccountId,
		/// Window named by the extra fact.
		window: QuotaWindowClass,
	},
	/// A complete quota fact occurs outside canonical member and window order.
	NonCanonicalQuotaFact {
		/// One-based position of the misplaced fact.
		fact_position: usize,
		/// Account identity supplied at this position.
		account_id: AccountId,
		/// Window supplied at this position.
		window: QuotaWindowClass,
		/// Account identity required at this position.
		expected_account_id: AccountId,
		/// Window required at this position.
		expected_window: QuotaWindowClass,
	},
	/// A quota fact duration does not match its closed window.
	QuotaFactWindowDurationMismatch {
		/// Account identity owning the invalid fact.
		account_id: AccountId,
		/// Closed window named by the fact.
		window: QuotaWindowClass,
		/// Exact duration required for the window.
		expected_duration_minutes: u16,
		/// Invalid duration supplied by the fact.
		duration_minutes: u16,
	},
	/// A current quota observation has a used percentage above 100.
	InvalidQuotaFactUsedPercent {
		/// Account identity owning the invalid observation.
		account_id: AccountId,
		/// Window owning the invalid observation.
		window: QuotaWindowClass,
		/// Invalid used percentage.
		used_percent: u8,
	},
	/// A current or failed observation instant is outside the closed timestamp product range.
	InvalidQuotaFactObservedAtMicros {
		/// Account identity owning the invalid observation.
		account_id: AccountId,
		/// Window owning the invalid observation.
		window: QuotaWindowClass,
		/// Invalid observation instant in UTC Unix microseconds.
		observed_at_micros: i64,
	},
	/// A current quota reset instant is invalid or not later than its observation instant.
	InvalidQuotaFactResetsAtMicros {
		/// Account identity owning the invalid observation.
		account_id: AccountId,
		/// Window owning the invalid observation.
		window: QuotaWindowClass,
		/// Validated observation instant in UTC Unix microseconds.
		observed_at_micros: i64,
		/// Invalid reset instant in UTC Unix microseconds.
		resets_at_micros: i64,
	},
	/// Fixed selection names no exact member in the snapshot.
	FixedTargetAbsent {
		/// Fixed account identity absent from the member inventory.
		account_id: AccountId,
	},
}

const MAX_ACCOUNT_REGISTRY_TIMESTAMP_MICROS: i64 = 253_402_300_799_999_999;
const ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS: i64 = 300_000_000;

/// Select an account at one closed-range UTC Unix microsecond instant without I/O or clocks.
pub fn decide_account_registry_routing(
	snapshot: &AccountRegistryRoutingSnapshot,
	decided_at_micros: i64,
) -> Result<AccountRegistryRoutingDecision, AccountRegistryRoutingKernelError> {
	let facts_by_member =
		validated_account_registry_quota_facts(snapshot, decided_at_micros)?;
	let evaluated_member_indexes = account_registry_evaluated_member_indexes(snapshot)?;
	let mut exclusions = Vec::new();
	let mut causes = Vec::new();

	for member_index in evaluated_member_indexes {
		let member = &snapshot.members[member_index];
		let (member_causes, member_exclusions) = classify_account_registry_member(
			member,
			&facts_by_member[member_index],
			decided_at_micros,
		);
		if member_causes.is_empty() && member_exclusions.is_empty() {
			return Ok(AccountRegistryRoutingDecision {
				snapshot_id: snapshot.snapshot_id.clone(),
				kind: AccountRegistryRoutingDecisionKind::Selected,
				selected_account_id: Some(member.account_id.clone()),
				exclusions,
				causes,
			});
		}
		causes.extend(member_causes);
		exclusions.extend(member_exclusions);
	}

	let kind = if causes.is_empty() {
		AccountRegistryRoutingDecisionKind::Waiting
	} else {
		AccountRegistryRoutingDecisionKind::NoRoute
	};
	Ok(AccountRegistryRoutingDecision {
		snapshot_id: snapshot.snapshot_id.clone(),
		kind,
		selected_account_id: None,
		exclusions,
		causes,
	})
}

fn validated_account_registry_quota_facts<'a>(
	snapshot: &'a AccountRegistryRoutingSnapshot,
	decided_at_micros: i64,
) -> Result<Vec<[&'a AccountRegistryQuotaFact; 2]>, AccountRegistryRoutingKernelError> {
	if !account_registry_timestamp_is_valid(decided_at_micros) {
		return Err(AccountRegistryRoutingKernelError::InvalidDecidedAtMicros {
			decided_at_micros,
		});
	}
	if snapshot.routing_revision <= 0 {
		return Err(AccountRegistryRoutingKernelError::InvalidRoutingRevision {
			routing_revision: snapshot.routing_revision,
		});
	}
	if snapshot.task_role_profile_revision <= 0 {
		return Err(
			AccountRegistryRoutingKernelError::InvalidTaskRoleProfileRevision {
				task_role_profile_revision: snapshot.task_role_profile_revision,
			},
		);
	}
	if !account_registry_timestamp_is_valid(snapshot.resolved_at_micros)
		|| snapshot.resolved_at_micros > decided_at_micros
	{
		return Err(AccountRegistryRoutingKernelError::InvalidResolvedAtMicros {
			resolved_at_micros: snapshot.resolved_at_micros,
		});
	}
	if snapshot.members.is_empty() {
		return Err(AccountRegistryRoutingKernelError::EmptyMembers);
	}
	for (index, member) in snapshot.members.iter().enumerate() {
		let expected_member_position = index + 1;
		if member.position != expected_member_position {
			return Err(AccountRegistryRoutingKernelError::NonCanonicalMember {
				account_id: member.account_id.clone(),
				member_position: member.position,
				expected_member_position,
			});
		}
		if member.account_revision <= 0 {
			return Err(
				AccountRegistryRoutingKernelError::InvalidMemberAccountRevision {
					account_id: member.account_id.clone(),
					account_revision: member.account_revision,
				},
			);
		}
		if let Some(first) =
			snapshot.members[..index].iter().find(|prior| prior.account_id == member.account_id)
		{
			return Err(AccountRegistryRoutingKernelError::DuplicateMember {
				account_id: member.account_id.clone(),
				first_position: first.position,
				duplicate_position: member.position,
			});
		}
		validate_account_registry_member_blockers(member)?;
	}

	for (index, fact) in snapshot.quota_facts.iter().enumerate() {
		if !snapshot.members.iter().any(|member| member.account_id == fact.account_id) {
			return Err(AccountRegistryRoutingKernelError::ExtraQuotaFact {
				account_id: fact.account_id.clone(),
				window: fact.window,
			});
		}
		let expected_duration_minutes = account_registry_window_duration(fact.window);
		if fact.duration_minutes != expected_duration_minutes {
			return Err(
				AccountRegistryRoutingKernelError::QuotaFactWindowDurationMismatch {
					account_id: fact.account_id.clone(),
					window: fact.window,
					expected_duration_minutes,
					duration_minutes: fact.duration_minutes,
				},
			);
		}
		if snapshot.quota_facts[..index]
			.iter()
			.any(|prior| prior.account_id == fact.account_id && prior.window == fact.window)
		{
			return Err(AccountRegistryRoutingKernelError::DuplicateQuotaFact {
				account_id: fact.account_id.clone(),
				window: fact.window,
			});
		}
		validate_account_registry_quota_observation(fact)?;
	}

	let mut facts_by_member = Vec::with_capacity(snapshot.members.len());
	for member in &snapshot.members {
		let five_hour = snapshot
			.quota_facts
			.iter()
			.find(|fact| {
				fact.account_id == member.account_id
					&& fact.window == QuotaWindowClass::FiveHour
			})
			.ok_or_else(|| AccountRegistryRoutingKernelError::MissingQuotaFact {
				account_id: member.account_id.clone(),
				window: QuotaWindowClass::FiveHour,
			})?;
		let seven_day = snapshot
			.quota_facts
			.iter()
			.find(|fact| {
				fact.account_id == member.account_id
					&& fact.window == QuotaWindowClass::SevenDay
			})
			.ok_or_else(|| AccountRegistryRoutingKernelError::MissingQuotaFact {
				account_id: member.account_id.clone(),
				window: QuotaWindowClass::SevenDay,
			})?;
		facts_by_member.push([five_hour, seven_day]);
	}

	let mut fact_index = 0;
	for member in &snapshot.members {
		for expected_window in
			[QuotaWindowClass::FiveHour, QuotaWindowClass::SevenDay]
		{
			let fact = &snapshot.quota_facts[fact_index];
			if fact.account_id != member.account_id || fact.window != expected_window {
				return Err(AccountRegistryRoutingKernelError::NonCanonicalQuotaFact {
					fact_position: fact_index + 1,
					account_id: fact.account_id.clone(),
					window: fact.window,
					expected_account_id: member.account_id.clone(),
					expected_window,
				});
			}
			fact_index += 1;
		}
	}

	Ok(facts_by_member)
}

fn validate_account_registry_quota_observation(
	fact: &AccountRegistryQuotaFact,
) -> Result<(), AccountRegistryRoutingKernelError> {
	match &fact.observation {
		AccountRegistryQuotaObservation::Missing => {},
		AccountRegistryQuotaObservation::Current {
			used_percent,
			observed_at_micros,
			resets_at_micros,
		} => {
			if *used_percent > 100 {
				return Err(
					AccountRegistryRoutingKernelError::InvalidQuotaFactUsedPercent {
						account_id: fact.account_id.clone(),
						window: fact.window,
						used_percent: *used_percent,
					},
				);
			}
			if !account_registry_timestamp_is_valid(*observed_at_micros) {
				return Err(
					AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
						account_id: fact.account_id.clone(),
						window: fact.window,
						observed_at_micros: *observed_at_micros,
					},
				);
			}
			if !account_registry_timestamp_is_valid(*resets_at_micros)
				|| *resets_at_micros <= *observed_at_micros
			{
				return Err(
					AccountRegistryRoutingKernelError::InvalidQuotaFactResetsAtMicros {
						account_id: fact.account_id.clone(),
						window: fact.window,
						observed_at_micros: *observed_at_micros,
						resets_at_micros: *resets_at_micros,
					},
				);
			}
		},
		AccountRegistryQuotaObservation::ObservationError { observed_at_micros, .. } => {
			if !account_registry_timestamp_is_valid(*observed_at_micros) {
				return Err(
					AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
						account_id: fact.account_id.clone(),
						window: fact.window,
						observed_at_micros: *observed_at_micros,
					},
				);
			}
		},
	}
	Ok(())
}

fn validate_account_registry_member_blockers(
	member: &AccountRegistryRoutingMember,
) -> Result<(), AccountRegistryRoutingKernelError> {
	let mut previous = None;
	for (index, blocker) in member.blockers.iter().copied().enumerate() {
		let Some(rank) = account_registry_member_blocker_rank(blocker) else {
			return Err(AccountRegistryRoutingKernelError::ForbiddenMemberBlocker {
				account_id: member.account_id.clone(),
				member_position: member.position,
				blocker_position: index + 1,
				blocker,
			});
		};
		if let Some(first_index) = member.blockers[..index]
			.iter()
			.position(|prior| *prior == blocker)
		{
			return Err(AccountRegistryRoutingKernelError::DuplicateMemberBlocker {
				account_id: member.account_id.clone(),
				member_position: member.position,
				blocker,
				first_blocker_position: first_index + 1,
				duplicate_blocker_position: index + 1,
			});
		}
		if let Some((previous_blocker, previous_rank)) = previous {
			if previous_rank >= rank {
				return Err(AccountRegistryRoutingKernelError::NonCanonicalMemberBlocker {
					account_id: member.account_id.clone(),
					member_position: member.position,
					blocker_position: index + 1,
					previous_blocker,
					blocker,
				});
			}
		}
		previous = Some((blocker, rank));
	}
	Ok(())
}

fn account_registry_evaluated_member_indexes(
	snapshot: &AccountRegistryRoutingSnapshot,
) -> Result<Vec<usize>, AccountRegistryRoutingKernelError> {
	match &snapshot.mode {
		AccountSelectionMode::Balanced => Ok((0..snapshot.members.len()).collect()),
		AccountSelectionMode::Fixed(account_id) => snapshot
			.members
			.iter()
			.position(|member| member.account_id == *account_id)
			.map(|index| vec![index])
			.ok_or_else(|| AccountRegistryRoutingKernelError::FixedTargetAbsent {
				account_id: account_id.clone(),
			}),
	}
}

fn classify_account_registry_member(
	member: &AccountRegistryRoutingMember,
	facts: &[&AccountRegistryQuotaFact; 2],
	decided_at_micros: i64,
) -> (Vec<RoutingDecisionCause>, Vec<AccountRegistryRoutingExclusion>) {
	let mut causes = member
		.blockers
		.iter()
		.copied()
		.map(|blocker| RoutingDecisionCause {
			account_id: member.account_id.clone(),
			blocker,
		})
		.collect::<Vec<_>>();
	let mut exclusions = Vec::new();

	for fact in facts {
		match &fact.observation {
			AccountRegistryQuotaObservation::Missing => causes.push(RoutingDecisionCause {
				account_id: member.account_id.clone(),
				blocker: account_registry_missing_blocker(fact.window),
			}),
			AccountRegistryQuotaObservation::ObservationError { .. } => {
				causes.push(RoutingDecisionCause {
					account_id: member.account_id.clone(),
					blocker: account_registry_unknown_blocker(fact.window),
				});
			},
			AccountRegistryQuotaObservation::Current {
				used_percent,
				observed_at_micros,
				resets_at_micros,
			} => {
				if *observed_at_micros > decided_at_micros {
					causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_from_future_blocker(fact.window),
					});
				} else if decided_at_micros - *observed_at_micros
					> ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS
				{
					causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_stale_blocker(fact.window),
					});
				} else if *resets_at_micros <= decided_at_micros {
					causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_reset_elapsed_blocker(fact.window),
					});
				} else if *used_percent >= 100 {
					exclusions.push(AccountRegistryRoutingExclusion {
						account_id: member.account_id.clone(),
						member_position: member.position,
						window: fact.window,
						duration_minutes: fact.duration_minutes,
						used_percent: *used_percent,
						observed_at_micros: *observed_at_micros,
						resets_at_micros: *resets_at_micros,
					});
				}
			},
		}
	}

	(causes, exclusions)
}

const fn account_registry_window_duration(window: QuotaWindowClass) -> u16 {
	match window {
		QuotaWindowClass::FiveHour => 300,
		QuotaWindowClass::SevenDay => 10_080,
	}
}

const fn account_registry_missing_blocker(window: QuotaWindowClass) -> RoutingBlocker {
	match window {
		QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourMissing,
		QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayMissing,
	}
}

const fn account_registry_unknown_blocker(window: QuotaWindowClass) -> RoutingBlocker {
	match window {
		QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourUnknown,
		QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayUnknown,
	}
}

const fn account_registry_from_future_blocker(window: QuotaWindowClass) -> RoutingBlocker {
	match window {
		QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourFromFuture,
		QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayFromFuture,
	}
}

const fn account_registry_stale_blocker(window: QuotaWindowClass) -> RoutingBlocker {
	match window {
		QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourStale,
		QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayStale,
	}
}

const fn account_registry_reset_elapsed_blocker(window: QuotaWindowClass) -> RoutingBlocker {
	match window {
		QuotaWindowClass::FiveHour => RoutingBlocker::QuotaFiveHourResetElapsed,
		QuotaWindowClass::SevenDay => RoutingBlocker::QuotaSevenDayResetElapsed,
	}
}

const fn account_registry_member_blocker_rank(blocker: RoutingBlocker) -> Option<u8> {
	Some(match blocker {
		RoutingBlocker::AccountFromFuture => 0,
		RoutingBlocker::AccountStale => 1,
		RoutingBlocker::AccountUnavailable => 2,
		RoutingBlocker::AccountUnknown => 3,
		RoutingBlocker::AccountDepleted => 4,
		RoutingBlocker::AccountAuthFailed => 5,
		RoutingBlocker::AccountPluginUnready => 6,
		RoutingBlocker::AccountDisabled => 7,
		_ => return None,
	})
}

const fn account_registry_timestamp_is_valid(timestamp_micros: i64) -> bool {
	timestamp_micros >= 0 && timestamp_micros <= MAX_ACCOUNT_REGISTRY_TIMESTAMP_MICROS
}
