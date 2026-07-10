mod construction;
mod dynamic_handler;
mod inspectors;
mod model;
mod review;
mod text;
mod tools;

pub(crate) use self::model::{
	DynamicToolCallResponse, DynamicToolContentItem, DynamicToolSpec, LocalRepoDetails,
	PullRequestDetails, ReviewExecutionMode, ReviewHandoffContext, ReviewHandoffWritebackFailed,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition,
	TurnCompletionStatus,
};

use std::{cell::RefCell, path::Path};

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[cfg(test)]
use self::inspectors::{
	RepositoryIdentity, parse_github_repository_identity, parse_remote_head_symref_output,
	resolve_lane_default_branch, review_blocking_status_lines,
};
use self::{
	inspectors::{
		GhPullRequestInspector, LocalGitRepoInspector, resolve_review_handoff_github_token,
	},
	model::{
		AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs, CommentArgs,
		ExecutionProgressPhase, LabelArgs, NormalizedProgressCheckpoint,
		NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointContract,
		NormalizedReviewCheckpointFinding, NormalizedReviewCheckpointFindingRoute,
		NormalizedReviewCheckpointPayload, NormalizedReviewCostControl, PendingReviewAction,
		PendingReviewCompletion, ProgressCheckpointArgs, ReviewCheckpointArgs,
		ReviewCheckpointChecksArgs, ReviewCheckpointContractArgs, ReviewCheckpointFindingArgs,
		ReviewCheckpointFindingRouteArgs, ReviewCheckpointFindingRouteCount,
		ReviewCheckpointFindingRouteSummary, ReviewCheckpointHeadBinding,
		ReviewCheckpointLineRangeArgs, ReviewCheckpointRejectedFindingArgs, ReviewCostControlArgs,
		ReviewFindingPolicyRecord, ReviewFindingPolicyState, ReviewHandoffArgs, ReviewPolicyPhase,
		ReviewPolicyState, ReviewPolicyStatus, ScopeArgs, TerminalFinalizeArgs, TransitionArgs,
	},
	text::{
		format_review_handoff_comment, format_review_repair_comment,
		normalize_optional_progress_field, normalize_progress_list, normalize_summary,
		public_summary_or_fallback,
	},
};
use crate::{
	state::StateStore,
	tracker::{IssueTracker, TrackerIssue, privacy_classifier::PublicProjectionPrivacyClassifier},
	workflow::WorkflowDocument,
};

pub(crate) const ISSUE_TRANSITION_TOOL_NAME: &str = "issue_transition";
pub(crate) const ISSUE_COMMENT_TOOL_NAME: &str = "issue_comment";
pub(crate) const ISSUE_LABEL_ADD_TOOL_NAME: &str = "issue_label_add";
pub(crate) const ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME: &str = "issue_progress_checkpoint";
pub(crate) const ISSUE_REVIEW_CHECKPOINT_TOOL_NAME: &str = "issue_review_checkpoint";
pub(crate) const ISSUE_REVIEW_HANDOFF_TOOL_NAME: &str = "issue_review_handoff";
pub(crate) const ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME: &str = "issue_review_repair_complete";
pub(crate) const ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME: &str = "issue_closeout_complete";
pub(crate) const ISSUE_TERMINAL_FINALIZE_TOOL_NAME: &str = "issue_terminal_finalize";
pub(crate) const REVIEW_POLICY_CONVERGENCE_BUDGET: i64 = 3;

pub(super) const REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK: &str =
	"Implementation completed and the PR is ready for review.";
pub(super) const REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK: &str =
	"Review repair completed and the PR is ready for fresh review.";
pub(super) const CLOSEOUT_PUBLIC_SUMMARY_FALLBACK: &str =
	"Retained closeout completed for the merged PR.";

static GH_PULL_REQUEST_INSPECTOR: GhPullRequestInspector = GhPullRequestInspector;
static LOCAL_GIT_REPO_INSPECTOR: LocalGitRepoInspector = LocalGitRepoInspector;

pub(crate) trait DynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec>;
	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse;
	fn handle_call_with_namespace(
		&self,
		namespace: Option<&str>,
		tool_name: &str,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let _ = namespace;

		self.handle_call(tool_name, arguments)
	}
	fn classify_turn_completion(
		&self,
		final_output: &str,
	) -> crate::prelude::Result<TurnCompletionStatus> {
		self.validate_turn_completion(final_output)?;

		Ok(TurnCompletionStatus::Complete)
	}
	fn has_terminal_completion_signal(&self) -> bool {
		false
	}
	fn validate_turn_completion(&self, _final_output: &str) -> crate::prelude::Result<()> {
		Ok(())
	}
}

pub(crate) trait PullRequestInspector {
	fn inspect_pull_request(
		&self,
		cwd: &Path,
		pr_url: &str,
		github_token: &str,
		gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String>;
}

pub(crate) trait LocalRepoInspector {
	fn inspect_local_repo(&self, cwd: &Path) -> std::result::Result<LocalRepoDetails, String>;
}

pub(crate) struct TrackerToolBridge<'a> {
	tracker: &'a dyn IssueTracker,
	issue: &'a TrackerIssue,
	workflow: &'a WorkflowDocument,
	review_context: Option<ReviewHandoffContext>,
	state_store: Option<&'a StateStore>,
	public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
	pull_request_inspector: &'a dyn PullRequestInspector,
	local_repo_inspector: &'a dyn LocalRepoInspector,
	local_issue_state_name: RefCell<String>,
	local_opt_out_requested: RefCell<bool>,
	manual_attention_requested: RefCell<bool>,
	manual_attention_comment_recorded: RefCell<bool>,
	manual_attention_error_class: RefCell<Option<String>>,
	continuation_blocking_tracker_write: RefCell<Option<String>>,
	pending_review_completion: RefCell<Option<PendingReviewCompletion>>,
	finalized_completion_path: RefCell<Option<RunCompletionDisposition>>,
}
pub(super) fn summarize_review_blocking_changes(review_blocking_changes: &[String]) -> String {
	const MAX_REVIEW_BLOCKING_CHANGES: usize = 5;

	let mut summary = review_blocking_changes
		.iter()
		.take(MAX_REVIEW_BLOCKING_CHANGES)
		.cloned()
		.collect::<Vec<_>>();

	if review_blocking_changes.len() > MAX_REVIEW_BLOCKING_CHANGES {
		summary.push(format!(
			"and {} more",
			review_blocking_changes.len() - MAX_REVIEW_BLOCKING_CHANGES
		));
	}

	if summary.is_empty() { String::from("(none)") } else { summary.join("; ") }
}

pub(crate) fn dynamic_tool_identifier_is_valid(identifier: &str) -> bool {
	!identifier.is_empty()
		&& identifier.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

pub(crate) fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

#[cfg(test)] mod tests;
