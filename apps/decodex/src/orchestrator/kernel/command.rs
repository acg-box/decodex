#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandIntentKind {
	ContinueAttempt,
	WaitExternal,
	ScheduleRetry,
	ResumeRetainedLane,
	RequestManualIntervention,
	LandReadyPullRequest,
	FinishTerminalCleanup,
	RequestExternalReview,
	ProbeExternalReviewAcknowledgement,
	ResendExternalReviewRequest,
	StartReviewRepair,
	StartRetainedLanding,
	StartRetainedCloseout,
	FinishRetainedCleanup,
	SyncReviewLifecycleAuthority,
	ObserveLoopGuardrailCheckpoint,
	ClearLoopGuardrailCheckpoint,
}
impl CommandIntentKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ContinueAttempt => "continue_attempt",
			Self::WaitExternal => "wait_external",
			Self::ScheduleRetry => "schedule_retry",
			Self::ResumeRetainedLane => "resume_retained_lane",
			Self::RequestManualIntervention => "request_manual_intervention",
			Self::LandReadyPullRequest => "land_ready_pull_request",
			Self::FinishTerminalCleanup => "finish_terminal_cleanup",
			Self::RequestExternalReview => "request_external_review",
			Self::ProbeExternalReviewAcknowledgement => "probe_external_review_acknowledgement",
			Self::ResendExternalReviewRequest => "resend_external_review_request",
			Self::StartReviewRepair => "start_review_repair",
			Self::StartRetainedLanding => "start_retained_landing",
			Self::StartRetainedCloseout => "start_retained_closeout",
			Self::FinishRetainedCleanup => "finish_retained_cleanup",
			Self::SyncReviewLifecycleAuthority => "sync_review_lifecycle_authority",
			Self::ObserveLoopGuardrailCheckpoint => "observe_loop_guardrail_checkpoint",
			Self::ClearLoopGuardrailCheckpoint => "clear_loop_guardrail_checkpoint",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandFact {
	ActiveOwnedWorkPresent,
	AuthorityComplete,
	ExternalSignalStillPending,
	HumanInterventionRecorded,
	IssueStillOwned,
	LandingSequenceStarted,
	NoContradictoryAuthority,
	NoHumanAttentionSignal,
	PostReviewLifecyclePresent,
	ReadyToLandPrerequisitesSatisfied,
	RetainedLaneReusable,
	RetainedLaneResumed,
	RetryBudgetAvailable,
	RetryScheduled,
	TerminalCleanupCompleted,
	TerminalCleanupPending,
	ExternalReviewRequested,
	ExternalReviewRequestPresent,
	ExternalReviewAcknowledgementObserved,
	ExternalReviewAcknowledgementPending,
	ExternalReviewRequestRetryAvailable,
	ReviewRepairStarted,
	RetainedLandingStarted,
	RetainedCloseoutStarted,
	RetainedCleanupCompleted,
	ReviewLifecycleAuthorityCurrent,
	OpenTrackerBlockersPresent,
	OpenTrackerBlockersResolved,
	LoopGuardrailCheckpointObserved,
	LoopGuardrailCheckpointCleared,
}
impl CommandFact {
	#[allow(dead_code)]
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ActiveOwnedWorkPresent => "active_owned_work_present",
			Self::AuthorityComplete => "authority_complete",
			Self::ExternalSignalStillPending => "external_signal_still_pending",
			Self::HumanInterventionRecorded => "human_intervention_recorded",
			Self::IssueStillOwned => "issue_still_owned",
			Self::LandingSequenceStarted => "landing_sequence_started",
			Self::NoContradictoryAuthority => "no_contradictory_authority",
			Self::NoHumanAttentionSignal => "no_human_attention_signal",
			Self::PostReviewLifecyclePresent => "post_review_lifecycle_present",
			Self::ReadyToLandPrerequisitesSatisfied => "ready_to_land_prerequisites_satisfied",
			Self::RetainedLaneReusable => "retained_lane_reusable",
			Self::RetainedLaneResumed => "retained_lane_resumed",
			Self::RetryBudgetAvailable => "retry_budget_available",
			Self::RetryScheduled => "retry_scheduled",
			Self::TerminalCleanupCompleted => "terminal_cleanup_completed",
			Self::TerminalCleanupPending => "terminal_cleanup_pending",
			Self::ExternalReviewRequested => "external_review_requested",
			Self::ExternalReviewRequestPresent => "external_review_request_present",
			Self::ExternalReviewAcknowledgementObserved =>
				"external_review_acknowledgement_observed",
			Self::ExternalReviewAcknowledgementPending => "external_review_acknowledgement_pending",
			Self::ExternalReviewRequestRetryAvailable => "external_review_request_retry_available",
			Self::ReviewRepairStarted => "review_repair_started",
			Self::RetainedLandingStarted => "retained_landing_started",
			Self::RetainedCloseoutStarted => "retained_closeout_started",
			Self::RetainedCleanupCompleted => "retained_cleanup_completed",
			Self::ReviewLifecycleAuthorityCurrent => "review_lifecycle_authority_current",
			Self::OpenTrackerBlockersPresent => "open_tracker_blockers_present",
			Self::OpenTrackerBlockersResolved => "open_tracker_blockers_resolved",
			Self::LoopGuardrailCheckpointObserved => "loop_guardrail_checkpoint_observed",
			Self::LoopGuardrailCheckpointCleared => "loop_guardrail_checkpoint_cleared",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandIntent {
	pub(crate) kind: CommandIntentKind,
	pub(crate) idempotency_key: String,
	pub(crate) preconditions: Vec<CommandFact>,
	pub(crate) expected_postconditions: Vec<CommandFact>,
}
impl CommandIntent {
	pub(crate) fn new(
		kind: CommandIntentKind,
		idempotency_key: impl Into<String>,
		preconditions: Vec<CommandFact>,
		expected_postconditions: Vec<CommandFact>,
	) -> Self {
		Self {
			kind,
			idempotency_key: idempotency_key.into(),
			preconditions,
			expected_postconditions,
		}
	}
}
