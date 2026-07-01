use std::{
	path::Path,
	process::ExitStatus,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde_json::json;
use time::OffsetDateTime;

use crate::{state, tracker::TrackerIssue};

use super::{
	ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_RETRY_KIND,
	CONTINUATION_PENDING_RUN_STATUS, CONTINUATION_RETRY_DELAY_MS, ChildExitRetryContext,
	ChildRunRef, CloseoutDispatchEligibility, FAILURE_RETRY_BASE_DELAY_MS,
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTracker,
	LaneDecisionSnapshot, PhaseGoalRecoveryContinuation, Result, RetainedPartialProgress,
	RetryEntry, RetryIssueStateHint, RetryKind, RetryQueue, ServiceConfig, StateStore,
	TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime, WorkflowDocument,
	WorktreeManager, WorktreeSpec, apply_terminal_failure_writeback, clear_worktree_retry_schedule,
	configured_public_projection_privacy_classifier, decide_lane_next_action,
	evaluate_closeout_dispatch_policy_with_inspector, handle_failure,
	issue_has_blocking_lane_decision_evidence, issue_passes_retry_retention_policy,
	issue_passes_review_repair_dispatch_policy, issue_retry_budget_exhausted,
	lane_decision_blocks_automatic_execution, mark_run_attempt_if_active,
	recover_phase_goal_continuation, refresh_issue, relative_worktree_path,
	resolve_child_exit_run_attempt, run_failure_requires_terminal_attention,
	superseded_run_disposition, worktree_has_tracked_changes, write_retry_budget_marker,
	write_terminal_guard_marker,
};

mod child_exit;
mod phase_goal;
mod retention;
mod schedule;
mod terminal;

pub(crate) use child_exit::schedule_retry_after_child_exit;
pub(in crate::orchestrator::daemon_retry) use phase_goal::recover_child_exit_phase_goal;
pub(in crate::orchestrator) use retention::retry_entry_is_temporarily_blocked;
use retention::{
	ChildExitPhaseGoalRecovery, ChildExitRetrySchedule, RetryEntryRetentionDecision,
	child_exit_retry_retention_decision,
};
pub(in crate::orchestrator) use schedule::clear_retry_schedule_and_release;
pub(crate) use schedule::{retry_delay, write_retry_schedule_for_run};
pub(in crate::orchestrator::daemon_retry) use terminal::{
	child_exit_retry_budget_attempt_count, child_exit_retry_budget_limit, child_exit_worktree_spec,
	terminalize_exhausted_child_exit_retry,
};
