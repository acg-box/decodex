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

/// Exact source precision accepted by the V16 decision boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingTimestampPrecision {
	/// The raw source value is exactly representable as UTC Unix microseconds.
	UnixMicrosecond,
}

/// Retained raw authority for one exact quota timestamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingTimestampProvenance {
	pub raw_value: String,
	pub source_id: String,
	pub precision: RoutingTimestampPrecision,
	pub evidence_revision: i64,
}

/// V16 quota evidence presented to the pure kernel by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionQuotaFact {
	pub account_id: AccountId,
	pub window: QuotaWindowClass,
	pub duration_minutes: u16,
	pub observation_revision: Option<i64>,
	pub remaining_percent: Option<u8>,
	pub resets_at_micros: Option<i64>,
	pub observed_at_micros: Option<i64>,
	pub confidence: Option<ObservationConfidence>,
	pub observed_at_provenance: Option<RoutingTimestampProvenance>,
	pub resets_at_provenance: Option<RoutingTimestampProvenance>,
}

/// Closed database-authored input to the V16 pure routing kernel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionSnapshot {
	pub snapshot_id: String,
	pub decided_at_micros: i64,
	pub members: Vec<RoutingDecisionCandidate>,
	pub quota_facts: Vec<RoutingDecisionQuotaFact>,
	pub capability_facts: Vec<RoutingSnapshotCapabilityFact>,
}

/// One candidate in the complete database-authored V16 universe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionCandidate {
	pub position: usize,
	pub account_id: AccountId,
	pub disposition: RoutingMemberDisposition,
	pub sticky: bool,
	pub blockers: Vec<RoutingBlocker>,
}

/// Stable semantic outcome persisted by V16.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingDecisionKind {
	Selected,
	WaitingUsage,
	NoRoute,
}

/// Stable reason for a typed no-route result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingNoRouteReason {
	BlockedEvidence,
}

/// One normalized, duration-typed depletion exclusion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecisionExclusion {
	pub account_id: AccountId,
	pub member_position: usize,
	pub window: QuotaWindowClass,
	pub duration_minutes: u16,
	pub observation_revision: i64,
	pub remaining_percent: u8,
	pub observed_at_micros: i64,
	pub resets_at_micros: i64,
	pub confidence: ObservationConfidence,
	pub observed_at_provenance: RoutingTimestampProvenance,
	pub resets_at_provenance: RoutingTimestampProvenance,
}

/// Inert deterministic V16 result. It carries no execution capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecision {
	pub snapshot_id: String,
	pub kind: RoutingDecisionKind,
	pub selected_account_id: Option<AccountId>,
	pub ready_at_micros: Option<i64>,
	pub no_route_reason: Option<RoutingNoRouteReason>,
	pub exclusions: Vec<RoutingDecisionExclusion>,
}

/// Structural failure of a supposedly closed database-authored snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingKernelError {
	MalformedSnapshot,
	IncompleteEvidence,
}

/// Select from one closed database-authored snapshot without I/O, clocks, or mechanisms.
pub fn decide_routing(
	snapshot: &RoutingDecisionSnapshot,
) -> Result<RoutingDecision, RoutingKernelError> {
	let members = &snapshot.members;
	if members.is_empty()
		|| members.iter().enumerate().any(|(index, member)| member.position != index + 1)
		|| members.iter().enumerate().any(|(index, member)| {
			members[..index].iter().any(|prior| prior.account_id == member.account_id)
		}) || members.iter().filter(|member| member.sticky).count() > 1
	{
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

	let included = members
		.iter()
		.enumerate()
		.filter(|(_, member)| member.disposition == RoutingMemberDisposition::Included)
		.collect::<Vec<_>>();
	if included.is_empty() {
		return Ok(no_route(&snapshot.snapshot_id));
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
				return Ok(no_route(&snapshot.snapshot_id));
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
		});
	}

	if included.iter().any(|(_, member)| {
		member.blockers.is_empty()
			|| member.blockers.iter().any(|blocker| !is_depletion_blocker(*blocker))
	}) {
		return Ok(no_route(&snapshot.snapshot_id));
	}
	let mut exclusions = Vec::new();
	let mut earliest_ready = None;
	for (index, member) in included {
		if !facts_by_member[index]
			.iter()
			.all(|fact| quota_fact_current(fact, snapshot.decided_at_micros))
		{
			return Ok(no_route(&snapshot.snapshot_id));
		}
		let account_exclusions =
			depletion_exclusions(member, &facts_by_member[index], snapshot.decided_at_micros)?;
		if account_exclusions.is_empty() {
			return Ok(no_route(&snapshot.snapshot_id));
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
	})
}

fn no_route(snapshot_id: &str) -> RoutingDecision {
	RoutingDecision {
		snapshot_id: snapshot_id.to_owned(),
		kind: RoutingDecisionKind::NoRoute,
		selected_account_id: None,
		ready_at_micros: None,
		no_route_reason: Some(RoutingNoRouteReason::BlockedEvidence),
		exclusions: Vec::new(),
	}
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
