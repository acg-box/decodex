use crate::tracker;

use super::{
	Duration, IssueTracker, PostReviewLaneClassification, PullRequestReviewState,
	RetainedReviewLane, RetainedReviewLaneBlocked, RetainedReviewRunIdentity, RunSummary,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
	issue_passes_closeout_dispatch_policy, issue_passes_dispatch_policy,
	issue_passes_retry_dispatch_policy, issue_passes_review_repair_dispatch_policy,
	issue_retry_budget_exhausted, ordinary_dispatch_blocked_by_retained_review_handoff,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IssueDispatchMode {
	Normal,
	Program,
	Retry,
	ReviewRepair,
	Closeout,
}
impl IssueDispatchMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Normal => "normal",
			Self::Program => "program",
			Self::Retry => "retry",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}

	pub(crate) fn allows_issue(
		self,
		tracker: &dyn IssueTracker,
		issue: &TrackerIssue,
		project: &ServiceConfig,
		workflow: &WorkflowDocument,
		state_store: &StateStore,
		hint: RetryIssueStateHint<'_>,
	) -> crate::prelude::Result<bool> {
		match self {
			Self::Normal => {
				let queue_label = tracker::automation_queue_label(project.service_id());

				Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, false)?
					&& !ordinary_dispatch_blocked_by_retained_review_handoff(
						project.service_id(),
						issue,
						state_store,
					)?)
			},
			Self::Program => {
				let queue_label = tracker::automation_queue_label(project.service_id());

				Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)?
					&& !ordinary_dispatch_blocked_by_retained_review_handoff(
						project.service_id(),
						issue,
						state_store,
					)?)
			},
			Self::Retry => issue_passes_retry_dispatch_policy(
				tracker,
				issue,
				project,
				workflow,
				state_store,
				hint,
			),
			Self::ReviewRepair => {
				Ok(issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?
					&& !issue_retry_budget_exhausted(workflow, state_store, &issue.id)?)
			},
			Self::Closeout => issue_passes_closeout_dispatch_policy(
				tracker,
				issue,
				project,
				workflow,
				state_store,
			),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PostReviewLaneDecision {
	Continue,
	WaitForReview,
	NeedsReviewRepair,
	ReadyToLand,
	CloseoutBlocked,
	CleanupBlocked,
	Block,
}
impl PostReviewLaneDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Continue => "continue",
			Self::WaitForReview => "wait_for_review",
			Self::NeedsReviewRepair => "needs_review_repair",
			Self::ReadyToLand => "ready_to_land",
			Self::CloseoutBlocked => "closeout_blocked",
			Self::CleanupBlocked => "cleanup_blocked",
			Self::Block => "blocked",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryKind {
	Continuation,
	Failure,
}
impl RetryKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Continuation => "continuation",
			Self::Failure => "failure",
		}
	}
}

pub(crate) enum RetryDispatchDecision {
	Blocked { excluded_issue_ids: Vec<String> },
	Dispatch(Box<RunSummary>),
	Continue,
}

#[derive(Clone, Debug)]
pub(crate) enum RunLeaseDisposition {
	RetainedReviewComplete,
	Superseded { newer_run_id: String, newer_attempt_number: i64 },
	Terminal,
	NotDispatchable,
	Stalled { idle_for: Duration },
	StalledRetainedPartialProgress { idle_for: Duration },
	StalledAlreadyNeedsAttention { idle_for: Duration },
}

pub(crate) enum RetainedReviewLaneLoad {
	Skip,
	Wait(String),
	Ready(Box<RetainedReviewLane>),
	Blocked(Box<RetainedReviewLaneBlocked>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewOrchestrationPhase {
	RequestPending,
	WaitingForAck,
	WaitingForResult,
	RepairRequired,
	PassWaitingForGates,
	WaitingForMerge,
}
impl ReviewOrchestrationPhase {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::RequestPending => "request_pending",
			Self::WaitingForAck => "waiting_for_ack",
			Self::WaitingForResult => "waiting_for_result",
			Self::RepairRequired => "repair_required",
			Self::PassWaitingForGates => "pass_waiting_for_gates",
			Self::WaitingForMerge => "waiting_for_merge",
		}
	}

	pub(crate) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"request_pending" => Ok(Self::RequestPending),
			"waiting_for_ack" => Ok(Self::WaitingForAck),
			"waiting_for_result" => Ok(Self::WaitingForResult),
			"repair_required" => Ok(Self::RepairRequired),
			"pass_waiting_for_gates" => Ok(Self::PassWaitingForGates),
			"waiting_for_merge" => Ok(Self::WaitingForMerge),
			other => Err(format!(
				"Unknown review orchestration phase `{other}` in retained review marker."
			)),
		}
	}
}

pub(crate) enum PostReviewLaneStateLoad {
	Classification(PostReviewLaneClassification),
	ReviewState(PullRequestReviewState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoopGuardrailReason {
	ValidationRepeat,
	NoEffectiveDiff,
	RemainingDeltaUnchanged,
	ReviewChurn,
	ReviewHandoffStateDrift,
	DependencyProgramStale,
	UncoveredDirection,
	AmbiguousRetainedProgress,
}
impl LoopGuardrailReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::ValidationRepeat => "validation_repeat",
			Self::NoEffectiveDiff => "no_effective_diff",
			Self::RemainingDeltaUnchanged => "remaining_delta_unchanged",
			Self::ReviewChurn => "review_churn",
			Self::ReviewHandoffStateDrift => "review_handoff_state_drift",
			Self::DependencyProgramStale => "dependency_program_stale",
			Self::UncoveredDirection => "uncovered_direction",
			Self::AmbiguousRetainedProgress => "ambiguous_retained_progress",
		}
	}

	pub(crate) fn from_error_class(error_class: &str) -> Option<Self> {
		match error_class {
			"validation_repeat" | "validation_failure_repeated" => Some(Self::ValidationRepeat),
			"no_effective_diff" => Some(Self::NoEffectiveDiff),
			"remaining_delta_unchanged" => Some(Self::RemainingDeltaUnchanged),
			"review_churn" | "review_policy_exhausted" => Some(Self::ReviewChurn),
			"review_handoff_state_drift" | "review_handoff_rebind_required" => {
				Some(Self::ReviewHandoffStateDrift)
			},
			"dependency_program_stale" | "dependency_blocked" => Some(Self::DependencyProgramStale),
			"uncovered_direction" | "research_contract_required" => Some(Self::UncoveredDirection),
			"ambiguous_retained_progress" | "ownership_ambiguous" => {
				Some(Self::AmbiguousRetainedProgress)
			},
			_ => None,
		}
	}

	pub(crate) fn terminal_next_action(self, recovery_gate: &str) -> String {
		match self {
			Self::ValidationRepeat => format!(
				"inspect the repeated validation failure, preserved worktree, and prior repair attempts; change repair strategy or route the issue to architecture/research review manually, {recovery_gate}"
			),
			Self::NoEffectiveDiff => format!(
				"inspect the retained worktree and retry evidence; do not continue automatic repair until a human identifies a concrete next diff or resets the lane, {recovery_gate}"
			),
			Self::RemainingDeltaUnchanged => format!(
				"inspect the unchanged remaining delta and validation evidence; decide the next bounded repair manually before requeueing, {recovery_gate}"
			),
			Self::ReviewChurn => format!(
				"inspect the repeated review findings and current head; decide the next repair or architecture review manually before requeueing, {recovery_gate}"
			),
			Self::ReviewHandoffStateDrift => format!(
				"inspect the retained review handoff marker, clean review checkpoint, PR head, and issue state; restore or rebind the post-review lifecycle before clearing attention, {recovery_gate}"
			),
			Self::DependencyProgramStale => format!(
				"inspect the dependency blocker and Execution Program readiness evidence; refresh dependencies or split/research the program before requeueing, {recovery_gate}"
			),
			Self::UncoveredDirection => format!(
				"capture the missing direction in a research or decision contract before continuing execution, {recovery_gate}"
			),
			Self::AmbiguousRetainedProgress => format!(
				"inspect retained partial progress and ownership evidence; choose resume, reset, or manual repair explicitly before clearing the guard, {recovery_gate}"
			),
		}
	}
}

pub(crate) struct SelectedIssueRunCandidate {
	pub(crate) issue: TrackerIssue,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_identity: Option<RetainedReviewRunIdentity>,
}
impl SelectedIssueRunCandidate {
	pub(crate) fn new(issue: TrackerIssue, dispatch_mode: IssueDispatchMode) -> Self {
		Self { issue, dispatch_mode, preferred_run_identity: None }
	}
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RetryIssueStateHint<'a> {
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
}
