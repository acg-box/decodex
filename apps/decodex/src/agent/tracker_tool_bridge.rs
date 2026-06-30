mod inspectors;
mod model;
mod review;
mod tools;

use std::{borrow::Cow, cell::RefCell, path::Path};

use color_eyre::Report;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(crate) use self::model::{
	DynamicToolCallResponse, DynamicToolContentItem, DynamicToolSpec, LocalRepoDetails,
	PullRequestDetails, ReviewExecutionMode, ReviewHandoffContext, ReviewHandoffWritebackFailed,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition,
	TurnCompletionStatus,
};
use self::{
	inspectors::{
		GhPullRequestInspector, LocalGitRepoInspector, resolve_review_handoff_github_token,
	},
	model::{
		AuthorityDecisionOptionArgs, AuthorityDecisionRequestArgs, CommentArgs, DocsImpact,
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
};

#[cfg(test)]
use self::inspectors::{
	RepositoryIdentity, parse_github_repository_identity, parse_remote_head_symref_output,
	resolve_lane_default_branch, review_blocking_status_lines,
};
#[cfg(test)]
use crate::tracker::privacy_classifier::DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER;
use crate::{
	prelude::eyre,
	state::StateStore,
	tracker::{
		IssueTracker, TrackerIssue, privacy_classifier::PublicProjectionPrivacyClassifier,
		public_text,
	},
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

const REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK: &str =
	"Implementation completed and the PR is ready for review.";
const REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK: &str =
	"Review repair completed and the PR is ready for fresh review.";
const CLOSEOUT_PUBLIC_SUMMARY_FALLBACK: &str = "Retained closeout completed for the merged PR.";

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
impl<'a> TrackerToolBridge<'a> {
	#[cfg(test)]
	pub(crate) fn new(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
	) -> Self {
		Self {
			tracker,
			issue,
			workflow,
			review_context: None,
			state_store: None,
			public_projection_privacy_classifier: &DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
			pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
			local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
			local_issue_state_name: RefCell::new(issue.state.name.clone()),
			local_opt_out_requested: RefCell::new(
				issue.has_label(workflow.frontmatter().tracker().opt_out_label()),
			),
			manual_attention_requested: RefCell::new(false),
			manual_attention_comment_recorded: RefCell::new(false),
			manual_attention_error_class: RefCell::new(None),
			continuation_blocking_tracker_write: RefCell::new(None),
			pending_review_completion: RefCell::new(None),
			finalized_completion_path: RefCell::new(None),
		}
	}

	#[cfg(test)]
	fn with_review_handoff_inspectors(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: Option<&'a StateStore>,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store,
				public_projection_privacy_classifier:
					&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
				pull_request_inspector,
				local_repo_inspector,
			},
		)
	}

	fn with_review_handoff_options(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		options: TrackerToolBridgeOptions<'a>,
	) -> Self {
		Self {
			tracker,
			issue,
			workflow,
			review_context: Some(review_context),
			state_store: options.state_store,
			public_projection_privacy_classifier: options.public_projection_privacy_classifier,
			pull_request_inspector: options.pull_request_inspector,
			local_repo_inspector: options.local_repo_inspector,
			local_issue_state_name: RefCell::new(issue.state.name.clone()),
			local_opt_out_requested: RefCell::new(
				issue.has_label(workflow.frontmatter().tracker().opt_out_label()),
			),
			manual_attention_requested: RefCell::new(false),
			manual_attention_comment_recorded: RefCell::new(false),
			manual_attention_error_class: RefCell::new(None),
			continuation_blocking_tracker_write: RefCell::new(None),
			pending_review_completion: RefCell::new(None),
			finalized_completion_path: RefCell::new(None),
		}
	}

	#[cfg(test)]
	fn leaked_test_state_store() -> &'static StateStore {
		Box::leak(Box::new(
			StateStore::open_in_memory().expect("test runtime state store should open"),
		))
	}

	#[cfg(test)]
	pub(crate) fn with_review_handoff_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			pull_request_inspector,
			local_repo_inspector,
		)
	}

	#[cfg(test)]
	pub(crate) fn with_review_repair_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			pull_request_inspector,
			local_repo_inspector,
		)
	}

	#[cfg(test)]
	pub(crate) fn with_run_context(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			&GH_PULL_REQUEST_INSPECTOR,
			&LOCAL_GIT_REPO_INSPECTOR,
		)
	}

	#[cfg(test)]
	pub(crate) fn with_run_context_and_state_store(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: &'a StateStore,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(state_store),
				public_projection_privacy_classifier:
					&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
			},
		)
	}

	pub(crate) fn with_run_context_state_store_and_privacy_classifier(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: &'a StateStore,
		public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(state_store),
				public_projection_privacy_classifier,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
			},
		)
	}

	#[cfg(test)]
	pub(crate) fn with_review_handoff_classifier_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(Self::leaked_test_state_store()),
				public_projection_privacy_classifier,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector,
			},
		)
	}

	pub(crate) fn review_context(&self) -> Option<&ReviewHandoffContext> {
		self.review_context.as_ref()
	}

	pub(crate) fn manual_attention_error_class(&self) -> Option<String> {
		self.manual_attention_error_class.borrow().clone()
	}
}

impl DynamicToolHandler for TrackerToolBridge<'_> {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		self.build_tool_specs()
	}

	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse {
		self.handle_call_inner(tool_name, arguments)
	}

	fn classify_turn_completion(
		&self,
		_final_output: &str,
	) -> crate::prelude::Result<TurnCompletionStatus> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let manual_attention_requested = *self.manual_attention_requested.borrow();
		let manual_attention_comment_recorded = *self.manual_attention_comment_recorded.borrow();
		let review_completion = self.pending_review_completion.borrow().clone();

		match (manual_attention_requested, manual_attention_comment_recorded, review_completion) {
			(false, false, None) => {
				if let Some(review_policy_stop) =
					self.review_policy_stop_requested(review_context)?
				{
					return Err(Report::new(review_policy_stop));
				}
				if let Some(reason) = self.continuation_blocking_write_reason()? {
					eyre::bail!(
						"Run `{}` changed issue `{}` via {} without recording a terminal path. Continuation turns may only yield cleanly while the leased issue remains active.",
						review_context.run_id,
						self.issue.identifier,
						reason
					);
				}

				if review_context.mode == ReviewExecutionMode::Closeout {
					eyre::bail!(
						"Run `{}` reached a clean continuation boundary for retained closeout on issue `{}`, but closeout is a deterministic tail. Finish the same turn with `{}` plus `{}` or take the manual-attention path instead of yielding another clean continuation boundary.",
						review_context.run_id,
						self.issue.identifier,
						ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
						ISSUE_TERMINAL_FINALIZE_TOOL_NAME
					);
				}

				Ok(TurnCompletionStatus::Continue)
			},
			(false, false, Some(_)) | (true, true, None) => {
				self.validate_turn_completion("")?;

				Ok(TurnCompletionStatus::Complete)
			},
			(true, false, None) => eyre::bail!(
				"Run `{}` requested human attention with label `{}`, but issue `{}` never recorded the required explanatory comment.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label(),
				self.issue.identifier
			),
			(true, _, Some(_)) => eyre::bail!(
				"Run `{}` recorded both `issue_review_handoff` and label `{}`. Use exactly one final handoff path.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
			(false, true, None) | (false, true, Some(_)) => eyre::bail!(
				"Run `{}` recorded a human-attention comment for issue `{}`, but never recorded label `{}`.",
				review_context.run_id,
				self.issue.identifier,
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
		}
	}

	fn has_terminal_completion_signal(&self) -> bool {
		self.completion_disposition().is_ok()
	}

	fn validate_turn_completion(&self, _final_output: &str) -> crate::prelude::Result<()> {
		let completion_path = self.completion_disposition()?;
		let Some(finalized_path) = *self.finalized_completion_path.borrow() else {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` completed, but issue `{}` never called `{}` for terminal path `{}`.",
				review_context.run_id,
				self.issue.identifier,
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				completion_path.as_str()
			);
		};

		if finalized_path != completion_path {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` finalized terminal path `{}`, but the recorded terminal path resolved to `{}` at turn completion.",
				review_context.run_id,
				finalized_path.as_str(),
				completion_path.as_str()
			);
		}

		Ok(())
	}
}

struct TrackerToolBridgeOptions<'a> {
	state_store: Option<&'a StateStore>,
	public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
	pull_request_inspector: &'a dyn PullRequestInspector,
	local_repo_inspector: &'a dyn LocalRepoInspector,
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

fn normalize_summary(summary: &str) -> String {
	summary.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_progress_list(items: Vec<String>) -> Vec<String> {
	items.into_iter().map(|item| normalize_summary(&item)).filter(|item| !item.is_empty()).collect()
}

fn normalize_optional_progress_field(value: Option<String>) -> Option<String> {
	value.and_then(|value| {
		let normalized = normalize_summary(&value);

		(!normalized.is_empty()).then_some(normalized)
	})
}

fn public_summary_or_fallback<'a>(summary: &'a str, fallback: &'static str) -> Cow<'a, str> {
	if public_text::validate_public_text_field("summary", summary).is_ok() {
		Cow::Borrowed(summary)
	} else {
		Cow::Borrowed(fallback)
	}
}

fn format_review_handoff_comment(
	review_context: &ReviewHandoffContext,
	pending_review_handoff: &PendingReviewAction,
	summary: &str,
) -> String {
	format!(
		"decodex run completed and is ready for review\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt}` (not retry-budget count)\n- finished_at: `{finished_at}`\n- branch: `{branch}`\n- pr_url: `{pr_url}`\n- worktree_path: `{worktree_path}`\n- validation_result: `passed`\n- summary: {summary}",
		run_id = review_context.run_id,
		attempt = review_context.attempt_number,
		finished_at = current_timestamp(),
		branch = review_context.branch_name,
		pr_url = pending_review_handoff.pr_url,
		worktree_path = review_context.worktree_path,
		summary = summary,
	)
}

fn format_review_repair_comment(
	review_context: &ReviewHandoffContext,
	pending_review_repair: &PendingReviewAction,
	summary: &str,
) -> String {
	format!(
		"decodex review repair completed and requested fresh review\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt}` (not retry-budget count)\n- finished_at: `{finished_at}`\n- branch: `{branch}`\n- pr_url: `{pr_url}`\n- worktree_path: `{worktree_path}`\n- validation_result: `passed`\n- summary: {summary}",
		run_id = review_context.run_id,
		attempt = review_context.attempt_number,
		finished_at = current_timestamp(),
		branch = review_context.branch_name,
		pr_url = pending_review_repair.pr_url,
		worktree_path = review_context.worktree_path,
		summary = summary,
	)
}

#[cfg(test)] mod tests;
