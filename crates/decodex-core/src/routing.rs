//! Mechanism-neutral facts for PostgreSQL-produced routing authority snapshots.

use crate::{
	AccountId, AccountState, ManagedRunId, ObservationConfidence, QuotaWindowClass,
	RuntimeSessionId,
};

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
	Included,
	Excluded,
}

/// Closed retained state for one ordinary capability observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingCapabilityState {
	Supported,
	UnsupportedSchemaMissing,
	UnsupportedMethodNotFound,
	UnsupportedCodexRejected,
	UnavailableNotProbed,
	UnavailableProbeFailed,
	DegradedLegacyHistoryOnly,
}

/// Deterministic candidate-quality blocker persisted by V14.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoutingBlocker {
	ExcludedByPolicy,
	AccountFromFuture,
	AccountStale,
	AccountUnavailable,
	AccountUnknown,
	AccountDepleted,
	AccountAuthFailed,
	AccountPluginUnready,
	AccountDisabled,
	EvidenceMissing,
	EvidenceFromFuture,
	EvidenceStale,
	EvidenceAccountMismatch,
	EvidenceProfileMismatch,
	EvidenceBuildMismatch,
	QuotaFiveHourMissing,
	QuotaFiveHourFromFuture,
	QuotaFiveHourStale,
	QuotaFiveHourUnknown,
	QuotaFiveHourResetElapsed,
	QuotaFiveHourDepleted,
	QuotaSevenDayMissing,
	QuotaSevenDayFromFuture,
	QuotaSevenDayStale,
	QuotaSevenDayUnknown,
	QuotaSevenDayResetElapsed,
	QuotaSevenDayDepleted,
	RequiredCapabilityUnsatisfied,
}

/// Immutable policy replacement effect. It authorizes no selection or dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingPolicyMember {
	pub position: usize,
	pub account_id: AccountId,
	pub account_revision: i64,
	pub disposition: RoutingMemberDisposition,
}

/// Immutable policy replacement effect. It authorizes no selection or dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingPolicyEffect {
	pub routing_policy_id: String,
	pub routing_policy_revision: i64,
	pub project_id: String,
	pub accepted_policy_id: String,
	pub accepted_policy_revision: i64,
	pub required_role: String,
	pub required_role_profile_revision: i64,
	pub required_build_id: String,
	pub members: Vec<RoutingPolicyMember>,
	pub required_capabilities: Vec<CodexCapability>,
}

/// Immutable compatibility evidence publication effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingEvidenceEffect {
	pub evidence_id: String,
	pub account_id: AccountId,
	pub account_revision: i64,
	pub evidence_revision: i64,
	pub role: String,
	pub role_profile_revision: i64,
	pub build_id: String,
	pub process_id: String,
	pub process_account_id: AccountId,
	pub schema_fingerprint: String,
	pub ingested_at_micros: i64,
	pub capabilities: Vec<(CodexCapability, RoutingCapabilityState)>,
}

/// One database-produced member in canonical policy order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotMember {
	pub position: usize,
	pub account_id: AccountId,
	pub disposition: RoutingMemberDisposition,
	pub account_revision: i64,
	pub display_label: String,
	pub account_state: AccountState,
	pub account_observed_at_utc: String,
	pub evidence_id: Option<String>,
	pub evidence_revision: Option<i64>,
	pub evidence_account_revision: Option<i64>,
	pub evidence_role: Option<String>,
	pub evidence_role_profile_revision: Option<i64>,
	pub evidence_build_id: Option<String>,
	pub process_id: Option<String>,
	pub schema_fingerprint: Option<String>,
	pub sticky: bool,
	pub blockers: Vec<RoutingBlocker>,
}

/// One exact duration-owned quota fact. Missing observations retain explicit null facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotQuotaFact {
	pub account_id: AccountId,
	pub window: QuotaWindowClass,
	pub duration_minutes: u16,
	pub observation_revision: Option<i64>,
	pub remaining_percent: Option<u8>,
	pub resets_at_micros: Option<i64>,
	pub observed_at_micros: Option<i64>,
	pub confidence: Option<ObservationConfidence>,
}

/// One cell in the complete member-by-ordinary-capability matrix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshotCapabilityFact {
	pub account_id: AccountId,
	pub capability: CodexCapability,
	pub applicable: bool,
	pub evidence_state: Option<RoutingCapabilityState>,
}

/// Complete immutable PostgreSQL classification input for the later V16 pure kernel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingSnapshot {
	pub snapshot_id: String,
	pub routing_policy_id: String,
	pub routing_policy_revision: i64,
	pub accepted_policy_id: String,
	pub accepted_policy_revision: i64,
	pub required_role: String,
	pub required_role_profile_revision: i64,
	pub required_build_id: String,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub runtime_session_revision: i64,
	pub account_snapshot_id: String,
	pub account_snapshot_source_revision: i64,
	pub profile_snapshot_id: String,
	pub profile_snapshot_source_revision: i64,
	pub resolved_at_micros: i64,
	pub members: Vec<RoutingSnapshotMember>,
	pub quota_facts: Vec<RoutingSnapshotQuotaFact>,
	pub capability_facts: Vec<RoutingSnapshotCapabilityFact>,
}

/// Stable exact-command domain rejection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingRejection {
	pub operation: String,
	pub code: String,
}

/// Closed exact-command result. No variant carries routing selection authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingCommandOutcome<T> {
	Success(T),
	Rejected(RoutingRejection),
}
