mod disposition;
mod handler;
mod loop_guardrail;
mod retryable_writeback;
mod review_handoff_drift;
mod terminal_writeback;
mod zero_evidence;

#[allow(unused_imports)]
pub(super) use self::{
	disposition::{
		preserve_and_promote_app_server_run_failure, preserve_manual_attention_request,
		retained_progress_source_error_class, run_failure_writeback_disposition,
	},
	handler::{
		ensure_automation_activity_label, handle_failure, retry_budget_attempts_for_current_failure,
	},
};
pub(super) use self::{
	terminal_writeback::apply_terminal_failure_writeback,
	zero_evidence::{
		AppServerZeroEvidenceStartFailure, promote_zero_evidence_app_server_start_failure,
		truncate_private_diagnostic_text,
	},
};
pub(super) use loop_guardrail::{
	git_guardrail_output, loop_guardrail_effective_status, loop_guardrail_stop_from_review_policy,
	loop_guardrail_text_hash, loop_guardrail_worktree_fingerprint,
	retryable_failure_loop_guardrail_stop, run_failure_requires_terminal_attention,
};
#[cfg(test)] pub(super) use retryable_writeback::write_retry_schedule_marker_for_runtime_retry;

use sha2::Digest;

use crate::{
	orchestrator::{
		AgentGitCredentialsUnavailable, AppServerCapabilityPreflightFailure,
		AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
		AppServerTransportFailure, AppServerTurnFailure, AuthorityBoundaryPolicyDecision,
		CodexAccountAuthFailure, Command, Display, Error, Formatter, HarnessOutcomeKind,
		IssueDispatchMode, IssueRunPlan, IssueTracker, LoopGuardrailCheckpoint,
		LoopGuardrailCheckpointInput, LoopGuardrailReason, LoopGuardrailStopRequested,
		ManualAttentionRequested, OffsetDateTime, Path, RepoGateFailure, RepoGateFailureDiagnostic,
		RepoGateFailureDisposition, Report, Result, RetainedPartialProgress,
		RetainedReviewNeedsAttention, RetryComment, RetryKind, ReviewHandoffNeedsAttention,
		ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, ServiceConfig,
		Sha256, StalledRunNeedsAttention, StateStore, TERMINAL_GUARDED_RUN_STATUS,
		TerminalFailureLifecycle, TerminalFailureOutcome, TrackerIssue, ValidationEvidenceFailure,
		WorkflowDocument, architecture_recovery_retry_next_action,
		configured_public_projection_privacy_classifier, eyre, format_retry_comment,
		format_terminal_failure_comment, json, latest_open_issue_phase_goal_before_attempt,
		loop_guardrail_architecture_recovery_decision, record_harness_outcome_best_effort,
		relative_worktree_path, repo_gate_changed_tracked_files, retry_comment_details,
		retry_delay, terminal_failure_comment_details, terminal_failure_lifecycle_event,
		terminal_failure_pr_url, terminal_failure_recovery_gate, worktree_has_tracked_changes,
		worktree_head_oid, write_retry_budget_marker, write_terminal_guard_marker,
	},
	tracker::privacy_classifier::PublicProjectionPrivacyClassifier,
};
use retryable_writeback::apply_retryable_failure_writeback;
use review_handoff_drift::handle_review_handoff_failure_drift;

pub(super) const LOOP_GUARDRAIL_CONVERGENCE_BUDGET: i64 = 3;
pub(super) const ARCHITECTURE_RECOVERY_BUDGET: usize = 1;
pub(super) const ARCHITECTURE_RECOVERY_RETRY_KIND: &str = "architecture_recovery";
pub(super) const RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE: &str = "retryable_failed_start_cleanup";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetainedReviewRepairPushFailureKind {
	Auth,
	Refspec,
	RemoteRejected,
	Failed,
}
impl RetainedReviewRepairPushFailureKind {
	pub(super) fn error_class(self) -> &'static str {
		match self {
			Self::Auth => "retained_review_repair_push_auth_failed",
			Self::Refspec => "retained_review_repair_push_refspec_failed",
			Self::RemoteRejected => "retained_review_repair_push_remote_rejected",
			Self::Failed => "retained_review_repair_push_failed",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunFailureWritebackDisposition {
	RetryableGeneric,
	RetryableStructuredRecovery,
	TerminalAttention,
}
impl RunFailureWritebackDisposition {
	pub(super) fn requires_terminal_attention(self) -> bool {
		self == Self::TerminalAttention
	}

	pub(super) fn preserves_retry_through_zero_evidence(self) -> bool {
		self == Self::RetryableStructuredRecovery
	}
}

pub(super) enum LoopGuardrailRecoveryDecision {
	Start(ArchitectureRecoveryStart),
	HumanRequired(LoopGuardrailStopRequested),
}

#[derive(Debug)]
pub(super) struct RetainedReviewRepairPushFailed {
	pub(super) issue_identifier: String,
	pub(super) run_id: String,
	pub(super) branch_name: String,
	pub(super) pr_url: Option<String>,
	pub(super) kind: RetainedReviewRepairPushFailureKind,
	pub(super) detail: String,
}
impl RetainedReviewRepairPushFailed {
	pub(super) fn error_class(&self) -> &'static str {
		self.kind.error_class()
	}

	pub(super) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			RetainedReviewRepairPushFailureKind::Auth => format!(
				"repair GitHub authentication, then rerun retained review repair for branch `{}`, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Refspec => format!(
				"inspect retained review-repair branch `{}` and push refspec, repair the branch/ref mismatch manually, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::RemoteRejected => format!(
				"inspect remote branch `{}` for non-fast-forward or protection drift, reconcile the retained PR branch, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Failed => format!(
				"inspect the retained review-repair push failure for branch `{}`, repair the remote update blocker, {recovery_gate}",
				self.branch_name
			),
		}
	}
}

impl Display for RetainedReviewRepairPushFailed {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"Run `{}` for issue `{}` could not push retained review-repair branch `{}` before handoff validation: {}",
			self.run_id, self.issue_identifier, self.branch_name, self.detail
		)
	}
}

impl Error for RetainedReviewRepairPushFailed {}

#[derive(Clone, Copy)]
pub(super) struct TerminalFailureWritebackRuntime<'a> {
	pub(super) service_id: &'a str,
	pub(super) state_store: Option<&'a StateStore>,
	pub(super) privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}

pub(super) struct ArchitectureRecoveryStart {
	pub(super) attempt_number: usize,
	pub(super) max_attempts: usize,
	pub(super) policy_decision: AuthorityBoundaryPolicyDecision,
	pub(super) detail: String,
}

pub(super) struct LoopGuardrailWorktreeFingerprint {
	pub(super) head_sha: String,
	pub(super) tracked_status_hash: String,
	pub(super) tracked_diff_hash: String,
	pub(super) effective_status_hash: String,
	pub(super) branch_delta_present: bool,
	pub(super) effective_delta_present: bool,
}

struct FailureHandlingContext<'a, T>
where
	T: IssueTracker,
{
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	worktree_path: &'a str,
	retry_budget_attempts: i64,
}
