#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnershipState {
	Pending,
	LeasedRun,
	Terminalizing,
	RetainedAttention,
	OrphanedLiveThread,
	ContinuationPending,
	GhostLane,
	Closed,
}
impl OwnershipState {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::LeasedRun => "leased_run",
			Self::Terminalizing => "terminalizing",
			Self::RetainedAttention => "retained_attention",
			Self::OrphanedLiveThread => "orphaned_live_thread",
			Self::ContinuationPending => "continuation_pending",
			Self::GhostLane => "ghost_lane",
			Self::Closed => "closed",
		}
	}

	pub(crate) fn from_str(value: &str) -> Option<Self> {
		Some(match value {
			"pending" => Self::Pending,
			"leased_run" => Self::LeasedRun,
			"terminalizing" => Self::Terminalizing,
			"retained_attention" => Self::RetainedAttention,
			"orphaned_live_thread" => Self::OrphanedLiveThread,
			"continuation_pending" => Self::ContinuationPending,
			"ghost_lane" => Self::GhostLane,
			"closed" => Self::Closed,
			_ => return None,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LivenessState {
	Unknown,
	ProcessAlive,
	ThreadActive,
	ProtocolRecent,
	NotRunning,
	HostBootMismatch,
	LateProtocolActivity,
}
impl LivenessState {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Unknown => "unknown",
			Self::ProcessAlive => "process_alive",
			Self::ThreadActive => "thread_active",
			Self::ProtocolRecent => "protocol_recent",
			Self::NotRunning => "not_running",
			Self::HostBootMismatch => "host_boot_mismatch",
			Self::LateProtocolActivity => "late_protocol_activity",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PolicyState {
	Allowed,
	ReviewPending,
	ReviewFindings,
	ReviewChurnExceeded,
	ContinuationRecoveryChurnExceeded,
	ArchitectureRecoveryPending,
	AuthorityBoundaryRequired,
	HumanAttentionRequired,
	RuntimeRecoveryRequired,
	RuntimeRecoveryBlocked,
}
impl PolicyState {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Allowed => "allowed",
			Self::ReviewPending => "review_pending",
			Self::ReviewFindings => "review_findings",
			Self::ReviewChurnExceeded => "review_churn_exceeded",
			Self::ContinuationRecoveryChurnExceeded => "continuation_recovery_churn_exceeded",
			Self::ArchitectureRecoveryPending => "architecture_recovery_pending",
			Self::AuthorityBoundaryRequired => "authority_boundary_required",
			Self::HumanAttentionRequired => "human_attention_required",
			Self::RuntimeRecoveryRequired => "runtime_recovery_required",
			Self::RuntimeRecoveryBlocked => "runtime_recovery_blocked",
		}
	}

	pub(crate) fn from_str(value: &str) -> Option<Self> {
		Some(match value {
			"allowed" => Self::Allowed,
			"review_pending" => Self::ReviewPending,
			"review_findings" => Self::ReviewFindings,
			"review_churn_exceeded" => Self::ReviewChurnExceeded,
			"continuation_recovery_churn_exceeded" => Self::ContinuationRecoveryChurnExceeded,
			"architecture_recovery_pending" => Self::ArchitectureRecoveryPending,
			"authority_boundary_required" => Self::AuthorityBoundaryRequired,
			"human_attention_required" => Self::HumanAttentionRequired,
			"runtime_recovery_required" => Self::RuntimeRecoveryRequired,
			"runtime_recovery_blocked" => Self::RuntimeRecoveryBlocked,
			_ => return None,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalizationState {
	None,
	BarrierStarted,
	RunControlRetired,
	ThreadArchiveRequested,
	CleanupPending,
	CleanupComplete,
}
impl TerminalizationState {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::BarrierStarted => "barrier_started",
			Self::RunControlRetired => "run_control_retired",
			Self::ThreadArchiveRequested => "thread_archive_requested",
			Self::CleanupPending => "cleanup_pending",
			Self::CleanupComplete => "cleanup_complete",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LaneStateAxes {
	pub(crate) ownership: OwnershipState,
	pub(crate) liveness: LivenessState,
	pub(crate) policy: PolicyState,
	pub(crate) terminalization: TerminalizationState,
}
impl LaneStateAxes {
	pub(crate) const fn new(
		ownership: OwnershipState,
		liveness: LivenessState,
		policy: PolicyState,
		terminalization: TerminalizationState,
	) -> Self {
		Self { ownership, liveness, policy, terminalization }
	}
}
