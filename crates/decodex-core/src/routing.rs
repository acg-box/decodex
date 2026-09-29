//! Account Registry routing and shared Codex capability identities.
//!
//! Public construction supports the pure kernel and tests; it does not prove durable-store
//! provenance, persistence, eligibility authority, dispatch authority, or production enablement.

use crate::{AccountId, AccountQuotaObservationError, AccountSelectionMode, QuotaWindowClass};

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

	/// Closed durable-store identity.
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
	/// Return the exact stable durable-store and protocol spelling.
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

	/// Parse one exact stable durable-store spelling.
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

/// Closed Account Registry quota observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountRegistryQuotaObservation {
	/// A positive observation confirms that the optional five-hour limit does not apply.
	NotApplicable {
		/// Exact positive observation time; routing still checks freshness.
		observed_at_micros: i64,
	},
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

/// One exact account-scoped cause retained by a non-selected route projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionCause {
	/// Account path to which the cause applies.
	pub account_id: AccountId,
	/// Exact persisted blocker without lossy category collapse.
	pub blocker: RoutingBlocker,
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
/// - `Waiting`: `selected_account_id` is `None`, `exclusions` is non-empty, and `causes` is empty.
/// - `NoRoute`: `selected_account_id` is `None`, `causes` is non-empty, and `exclusions` contains
///   every positive current depletion found during evaluation.
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
	let facts_by_member = validated_account_registry_quota_facts(snapshot, decided_at_micros)?;
	let evaluated_member_indexes = account_registry_evaluated_member_indexes(snapshot)?;
	let evaluations = evaluated_member_indexes
		.iter()
		.map(|member_index| {
			classify_account_registry_member(
				&snapshot.members[*member_index],
				&facts_by_member[*member_index],
				decided_at_micros,
			)
		})
		.collect::<Vec<_>>();

	let mut exclusions = Vec::new();
	let mut causes = Vec::new();
	for (member_index, evaluation) in evaluated_member_indexes.iter().zip(&evaluations) {
		let member = &snapshot.members[*member_index];
		match evaluation {
			AccountRegistryMemberCapacity::KnownAvailable => {
				return Ok(account_registry_selected_decision(
					snapshot, member, exclusions, causes,
				));
			},
			AccountRegistryMemberCapacity::Unknown { causes: soft_causes } => {
				causes.extend(soft_causes.iter().cloned());
			},
			AccountRegistryMemberCapacity::Blocked {
				causes: member_causes,
				exclusions: member_exclusions,
			} => {
				causes.extend(member_causes.iter().cloned());
				exclusions.extend(member_exclusions.iter().cloned());
			},
		}
	}

	// Unknown capacity is a fallback, not evidence of depletion. Balanced mode prefers complete
	// current non-depletion evidence, then preserves configured order among unknown candidates.
	// Fixed mode evaluates one member, so this pass admits that member unless a hard blocker or a
	// current positive depletion was observed.
	exclusions.clear();
	causes.clear();
	for (member_index, evaluation) in evaluated_member_indexes.iter().zip(&evaluations) {
		let member = &snapshot.members[*member_index];
		match evaluation {
			AccountRegistryMemberCapacity::Unknown { .. } => {
				return Ok(account_registry_selected_decision(
					snapshot, member, exclusions, causes,
				));
			},
			AccountRegistryMemberCapacity::KnownAvailable => unreachable!("selected in first pass"),
			AccountRegistryMemberCapacity::Blocked {
				causes: member_causes,
				exclusions: member_exclusions,
			} => {
				causes.extend(member_causes.iter().cloned());
				exclusions.extend(member_exclusions.iter().cloned());
			},
		}
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

fn account_registry_selected_decision(
	snapshot: &AccountRegistryRoutingSnapshot,
	member: &AccountRegistryRoutingMember,
	exclusions: Vec<AccountRegistryRoutingExclusion>,
	causes: Vec<RoutingDecisionCause>,
) -> AccountRegistryRoutingDecision {
	AccountRegistryRoutingDecision {
		snapshot_id: snapshot.snapshot_id.clone(),
		kind: AccountRegistryRoutingDecisionKind::Selected,
		selected_account_id: Some(member.account_id.clone()),
		exclusions,
		causes,
	}
}

fn validated_account_registry_quota_facts(
	snapshot: &AccountRegistryRoutingSnapshot,
	decided_at_micros: i64,
) -> Result<Vec<[&AccountRegistryQuotaFact; 2]>, AccountRegistryRoutingKernelError> {
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
		return Err(AccountRegistryRoutingKernelError::InvalidTaskRoleProfileRevision {
			task_role_profile_revision: snapshot.task_role_profile_revision,
		});
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
			return Err(AccountRegistryRoutingKernelError::InvalidMemberAccountRevision {
				account_id: member.account_id.clone(),
				account_revision: member.account_revision,
			});
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
			return Err(AccountRegistryRoutingKernelError::QuotaFactWindowDurationMismatch {
				account_id: fact.account_id.clone(),
				window: fact.window,
				expected_duration_minutes,
				duration_minutes: fact.duration_minutes,
			});
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
				fact.account_id == member.account_id && fact.window == QuotaWindowClass::FiveHour
			})
			.ok_or_else(|| AccountRegistryRoutingKernelError::MissingQuotaFact {
				account_id: member.account_id.clone(),
				window: QuotaWindowClass::FiveHour,
			})?;
		let seven_day = snapshot
			.quota_facts
			.iter()
			.find(|fact| {
				fact.account_id == member.account_id && fact.window == QuotaWindowClass::SevenDay
			})
			.ok_or_else(|| AccountRegistryRoutingKernelError::MissingQuotaFact {
				account_id: member.account_id.clone(),
				window: QuotaWindowClass::SevenDay,
			})?;
		facts_by_member.push([five_hour, seven_day]);
	}

	let mut fact_index = 0;
	for member in &snapshot.members {
		for expected_window in [QuotaWindowClass::FiveHour, QuotaWindowClass::SevenDay] {
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
		AccountRegistryQuotaObservation::NotApplicable { observed_at_micros } => {
			if fact.window != QuotaWindowClass::FiveHour {
				return Err(AccountRegistryRoutingKernelError::QuotaFactWindowDurationMismatch {
					account_id: fact.account_id.clone(),
					window: fact.window,
					expected_duration_minutes: 300,
					duration_minutes: fact.duration_minutes,
				});
			}
			if *observed_at_micros <= 0 || !account_registry_timestamp_is_valid(*observed_at_micros)
			{
				return Err(AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
					account_id: fact.account_id.clone(),
					window: fact.window,
					observed_at_micros: *observed_at_micros,
				});
			}
		},
		AccountRegistryQuotaObservation::Current {
			used_percent,
			observed_at_micros,
			resets_at_micros,
		} => {
			if *used_percent > 100 {
				return Err(AccountRegistryRoutingKernelError::InvalidQuotaFactUsedPercent {
					account_id: fact.account_id.clone(),
					window: fact.window,
					used_percent: *used_percent,
				});
			}
			if !account_registry_timestamp_is_valid(*observed_at_micros) {
				return Err(AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
					account_id: fact.account_id.clone(),
					window: fact.window,
					observed_at_micros: *observed_at_micros,
				});
			}
			if !account_registry_timestamp_is_valid(*resets_at_micros)
				|| *resets_at_micros <= *observed_at_micros
			{
				return Err(AccountRegistryRoutingKernelError::InvalidQuotaFactResetsAtMicros {
					account_id: fact.account_id.clone(),
					window: fact.window,
					observed_at_micros: *observed_at_micros,
					resets_at_micros: *resets_at_micros,
				});
			}
		},
		AccountRegistryQuotaObservation::ObservationError { observed_at_micros, .. } =>
			if !account_registry_timestamp_is_valid(*observed_at_micros) {
				return Err(AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
					account_id: fact.account_id.clone(),
					window: fact.window,
					observed_at_micros: *observed_at_micros,
				});
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
		if let Some(first_index) =
			member.blockers[..index].iter().position(|prior| *prior == blocker)
		{
			return Err(AccountRegistryRoutingKernelError::DuplicateMemberBlocker {
				account_id: member.account_id.clone(),
				member_position: member.position,
				blocker,
				first_blocker_position: first_index + 1,
				duplicate_blocker_position: index + 1,
			});
		}
		if let Some((previous_blocker, previous_rank)) = previous
			&& previous_rank >= rank
		{
			return Err(AccountRegistryRoutingKernelError::NonCanonicalMemberBlocker {
				account_id: member.account_id.clone(),
				member_position: member.position,
				blocker_position: index + 1,
				previous_blocker,
				blocker,
			});
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

enum AccountRegistryMemberCapacity {
	KnownAvailable,
	Unknown { causes: Vec<RoutingDecisionCause> },
	Blocked { causes: Vec<RoutingDecisionCause>, exclusions: Vec<AccountRegistryRoutingExclusion> },
}

fn classify_account_registry_member(
	member: &AccountRegistryRoutingMember,
	facts: &[&AccountRegistryQuotaFact; 2],
	decided_at_micros: i64,
) -> AccountRegistryMemberCapacity {
	let mut hard_causes = member
		.blockers
		.iter()
		.copied()
		.map(|blocker| RoutingDecisionCause { account_id: member.account_id.clone(), blocker })
		.collect::<Vec<_>>();
	let mut unknown_causes = Vec::new();
	let mut exclusions = Vec::new();
	let mut known_available_windows = 0_u8;

	for fact in facts {
		match &fact.observation {
			AccountRegistryQuotaObservation::NotApplicable { observed_at_micros } =>
				if *observed_at_micros > decided_at_micros {
					hard_causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_from_future_blocker(fact.window),
					});
				} else if decided_at_micros - *observed_at_micros
					> ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS
				{
					unknown_causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_stale_blocker(fact.window),
					});
				} else {
					known_available_windows += 1;
				},
			AccountRegistryQuotaObservation::Missing => unknown_causes.push(RoutingDecisionCause {
				account_id: member.account_id.clone(),
				blocker: account_registry_missing_blocker(fact.window),
			}),
			AccountRegistryQuotaObservation::ObservationError { .. } => {
				unknown_causes.push(RoutingDecisionCause {
					account_id: member.account_id.clone(),
					blocker: account_registry_unknown_blocker(fact.window),
				});
			},
			AccountRegistryQuotaObservation::Current {
				used_percent,
				observed_at_micros,
				resets_at_micros,
			} =>
				if *observed_at_micros > decided_at_micros {
					hard_causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_from_future_blocker(fact.window),
					});
				} else if decided_at_micros - *observed_at_micros
					> ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS
				{
					unknown_causes.push(RoutingDecisionCause {
						account_id: member.account_id.clone(),
						blocker: account_registry_stale_blocker(fact.window),
					});
				} else if *resets_at_micros <= decided_at_micros {
					unknown_causes.push(RoutingDecisionCause {
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
				} else {
					known_available_windows += 1;
				},
		}
	}

	if !hard_causes.is_empty() {
		hard_causes.extend(unknown_causes);
		AccountRegistryMemberCapacity::Blocked { causes: hard_causes, exclusions }
	} else if !exclusions.is_empty() {
		AccountRegistryMemberCapacity::Blocked { causes: Vec::new(), exclusions }
	} else if known_available_windows == 2 {
		AccountRegistryMemberCapacity::KnownAvailable
	} else {
		AccountRegistryMemberCapacity::Unknown { causes: unknown_causes }
	}
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

#[cfg(test)]
mod optional_quota_tests {
	use super::{
		ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS, AccountId, AccountRegistryMemberCapacity,
		AccountRegistryQuotaFact, AccountRegistryQuotaObservation, AccountRegistryRoutingMember,
		QuotaWindowClass, classify_account_registry_member,
		validate_account_registry_quota_observation,
	};

	#[test]
	fn optional_quota_routing_preserves_absence_freshness_without_fabricated_capacity() {
		let id = AccountId::new("10000000-0000-4000-8000-000000000001").expect("account");
		let now = ACCOUNT_REGISTRY_QUOTA_FRESHNESS_MICROS + 10;
		let member = AccountRegistryRoutingMember {
			account_id: id.clone(),
			position: 0,
			account_revision: 1,
			blockers: vec![],
		};
		let mut five = AccountRegistryQuotaFact {
			account_id: id.clone(),
			window: QuotaWindowClass::FiveHour,
			duration_minutes: 300,
			observation: AccountRegistryQuotaObservation::NotApplicable { observed_at_micros: now },
		};
		let mut weekly = AccountRegistryQuotaFact {
			account_id: id,
			window: QuotaWindowClass::SevenDay,
			duration_minutes: 10080,
			observation: AccountRegistryQuotaObservation::Current {
				used_percent: 8,
				observed_at_micros: now,
				resets_at_micros: now + 60_000_000,
			},
		};
		assert!(validate_account_registry_quota_observation(&five).is_ok());
		assert!(matches!(
			classify_account_registry_member(&member, &[&five, &weekly], now),
			AccountRegistryMemberCapacity::KnownAvailable
		));
		five.observation = AccountRegistryQuotaObservation::NotApplicable { observed_at_micros: 1 };
		assert!(matches!(
			classify_account_registry_member(&member, &[&five, &weekly], now),
			AccountRegistryMemberCapacity::Unknown { .. }
		));
		five.observation =
			AccountRegistryQuotaObservation::NotApplicable { observed_at_micros: now + 1 };
		assert!(
			matches!(classify_account_registry_member(&member,&[&five,&weekly],now),AccountRegistryMemberCapacity::Blocked { exclusions,.. } if exclusions.is_empty())
		);
		five.observation =
			AccountRegistryQuotaObservation::NotApplicable { observed_at_micros: now };
		weekly.observation = AccountRegistryQuotaObservation::Current {
			used_percent: 100,
			observed_at_micros: now,
			resets_at_micros: now + 60_000_000,
		};
		assert!(
			matches!(classify_account_registry_member(&member,&[&five,&weekly],now),AccountRegistryMemberCapacity::Blocked { exclusions,.. } if exclusions.len()==1)
		);
		weekly.observation =
			AccountRegistryQuotaObservation::NotApplicable { observed_at_micros: now };
		assert!(validate_account_registry_quota_observation(&weekly).is_err());
	}
}
