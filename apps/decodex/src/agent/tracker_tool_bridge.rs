mod review;
mod tools;

use std::{
	borrow::Cow,
	cell::RefCell,
	env,
	error::Error,
	fmt::{Display, Formatter},
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::Report;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[cfg(test)]
use crate::tracker::privacy_classifier::DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER;
use crate::{
	config::ReviewLevel,
	github,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolSpec {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) namespace: Option<String>,
	pub(crate) description: String,
	#[serde(rename = "deferLoading", default, skip_serializing_if = "std::ops::Not::not")]
	pub(crate) defer_loading: bool,
	#[serde(rename = "inputSchema")]
	pub(crate) input_schema: Value,
	pub(crate) name: String,
}
impl DynamicToolSpec {
	pub(crate) fn new(
		name: impl Into<String>,
		description: impl Into<String>,
		input_schema: Value,
	) -> Self {
		Self {
			namespace: None,
			description: description.into(),
			defer_loading: false,
			input_schema,
			name: name.into(),
		}
	}

	pub(crate) fn deferred(mut self) -> Self {
		self.defer_loading = true;

		self
	}
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffContext {
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: String,
	pub(crate) run_id: String,
	pub(crate) service_id: String,
	pub(crate) worktree_path: String,
	pub(crate) cwd: PathBuf,
	pub(crate) github_token_env_var: Option<String>,
	pub(crate) github_command_path: Option<PathBuf>,
	pub(crate) review_level: ReviewLevel,
	pub(crate) mode: ReviewExecutionMode,
	pub(crate) recorded_pr_url: Option<String>,
}
impl ReviewHandoffContext {
	pub(crate) fn decodex_review_checkpoint_enabled(&self) -> bool {
		self.review_level.requires_review_checkpoint()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffWritebackFailed {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) pr_url: String,
	pub(crate) success_state: String,
	pub(crate) source: String,
}
impl Display for ReviewHandoffWritebackFailed {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"Run `{}` failed to finalize the review handoff for issue `{}` around target state `{}` and PR `{}`: {}",
			self.run_id, self.issue_identifier, self.success_state, self.pr_url, self.source
		)
	}
}

impl Error for ReviewHandoffWritebackFailed {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestDetails {
	base_ref_name: String,
	head_ref_name: String,
	head_ref_oid: String,
	head_repository_name: String,
	head_repository_owner: String,
	is_draft: bool,
	state: String,
	url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalRepoDetails {
	default_branch: String,
	head_oid: String,
	head_tree_oid: String,
	repository_name: String,
	repository_owner: String,
	review_blocking_changes: Vec<String>,
}
impl LocalRepoDetails {
	fn review_worktree_clean(&self) -> bool {
		self.review_blocking_changes.is_empty()
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolCallResponse {
	#[serde(rename = "contentItems")]
	pub(crate) content_items: Vec<DynamicToolContentItem>,
	pub(crate) success: bool,
}
impl DynamicToolCallResponse {
	pub(crate) fn success(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: true }
	}

	pub(crate) fn failure(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: false }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyStopRequested {
	pub(crate) head_sha: String,
	pub(crate) issue_identifier: String,
	pub(crate) fingerprint: Option<String>,
	pub(crate) nonclean_rounds: Option<i64>,
	pub(crate) reason: ReviewPolicyStopReason,
	pub(crate) run_id: String,
}
impl Display for ReviewPolicyStopRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self.reason {
			ReviewPolicyStopReason::Exhausted => write!(
				f,
				"Run `{}` for issue `{}` exhausted the runtime-owned review convergence budget at HEAD `{}` after {} non-clean rounds{}.",
				self.run_id,
				self.issue_identifier,
				self.head_sha,
				self.nonclean_rounds.unwrap_or_default(),
				self.fingerprint.as_ref().map_or_else(String::new, |fingerprint| format!(
					" for finding fingerprint `{fingerprint}`"
				))
			),
			ReviewPolicyStopReason::ArchitectureReviewRequired => write!(
				f,
				"Run `{}` for issue `{}` recorded `needs_architecture_review` at HEAD `{}` and now requires human architecture review.",
				self.run_id, self.issue_identifier, self.head_sha
			),
			ReviewPolicyStopReason::Blocked => write!(
				f,
				"Run `{}` for issue `{}` recorded `blocked` at HEAD `{}` and now requires human intervention.",
				self.run_id, self.issue_identifier, self.head_sha
			),
		}
	}
}

impl Error for ReviewPolicyStopRequested {}

struct TrackerToolBridgeOptions<'a> {
	state_store: Option<&'a StateStore>,
	public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
	pull_request_inspector: &'a dyn PullRequestInspector,
	local_repo_inspector: &'a dyn LocalRepoInspector,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingReviewAction {
	pr_url: String,
	summary: String,
}

struct GhPullRequestInspector;
impl PullRequestInspector for GhPullRequestInspector {
	fn inspect_pull_request(
		&self,
		cwd: &Path,
		pr_url: &str,
		github_token: &str,
		gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		let mut command = github::gh_command_with_config(gh_command_path);

		command.args([
			"pr",
			"view",
			pr_url,
			"--json",
			"url,baseRefName,headRefName,headRefOid,state,isDraft,headRepository,headRepositoryOwner",
		]);
		command.current_dir(cwd);

		github::configure_gh_command(&mut command, github_token);

		let output = command
			.output()
			.map_err(|error| format!("Failed to inspect pull request `{pr_url}`: {error}"))?;

		if !output.status.success() {
			let stderr = String::from_utf8_lossy(&output.stderr);

			return Err(format!("Failed to inspect pull request `{pr_url}`: {}", stderr.trim()));
		}

		let response: PullRequestViewResponse =
			serde_json::from_slice(&output.stdout).map_err(|error| {
				format!("Failed to parse pull request details for `{pr_url}`: {error}")
			})?;
		let Some(head_repository) = response.head_repository else {
			return Err(format!(
				"Pull request `{pr_url}` does not expose a head repository for review handoff validation."
			));
		};

		Ok(PullRequestDetails {
			base_ref_name: response.base_ref_name,
			head_ref_name: response.head_ref_name,
			head_ref_oid: response.head_ref_oid,
			head_repository_name: head_repository.name,
			head_repository_owner: response.head_repository_owner.login,
			is_draft: response.is_draft,
			state: response.state,
			url: response.url,
		})
	}
}

struct LocalGitRepoInspector;
impl LocalRepoInspector for LocalGitRepoInspector {
	fn inspect_local_repo(&self, cwd: &Path) -> std::result::Result<LocalRepoDetails, String> {
		let head_oid =
			run_command_for_stdout("git", &["rev-parse", "HEAD"], cwd, "inspect lane HEAD")?;
		let head_tree_oid = run_command_for_stdout(
			"git",
			&["rev-parse", "HEAD^{tree}"],
			cwd,
			"inspect lane HEAD tree",
		)?;
		let worktree_status = run_command_for_stdout_allow_empty(
			"git",
			&["status", "--porcelain=v1", "--untracked-files=all"],
			cwd,
			"inspect review-blocking worktree status",
		)?;
		let default_branch = resolve_lane_default_branch(cwd)?;
		let origin_url = run_command_for_stdout(
			"git",
			&["config", "--get", "remote.origin.url"],
			cwd,
			"inspect lane origin repository",
		)?;
		let repository = parse_github_repository_identity(origin_url.trim())?;

		Ok(LocalRepoDetails {
			default_branch: default_branch
				.strip_prefix("origin/")
				.unwrap_or(default_branch.as_str())
				.to_owned(),
			head_oid,
			head_tree_oid,
			repository_name: repository.name,
			repository_owner: repository.owner,
			review_blocking_changes: review_blocking_status_lines(&worktree_status),
		})
	}
}

#[derive(Debug, Deserialize)]
struct ScopeArgs {
	issue_id: Option<String>,

	issue_identifier: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	kind: String,
	error_class: Option<String>,
	next_action: Option<String>,
	#[serde(default)]
	blockers: Vec<String>,
	#[serde(default)]
	evidence: Vec<String>,
	failed_command: Option<String>,
	raw_error: Option<String>,
	summary: Option<String>,
	decision_request: Option<AuthorityDecisionRequestArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityDecisionRequestArgs {
	boundary_check_id: i64,
	decision_request_id: String,
	reason_code: String,
	boundary_type: String,
	proposed_change: String,
	why_exceeds_authority: String,
	#[serde(default)]
	options: Vec<AuthorityDecisionOptionArgs>,
	recommendation: String,
	resume_condition: String,
	#[serde(default)]
	retained_worktree_evidence: Vec<String>,
	#[serde(default)]
	retained_diff_evidence: Vec<String>,
	#[serde(default)]
	recovery_attempt_context: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityDecisionOptionArgs {
	label: String,
	description: String,
}

#[derive(Debug, Deserialize)]
struct ReviewHandoffArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	pr_url: String,
	summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgressCheckpointArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	phase: String,
	docs_impact: String,
	focus: String,
	next_action: String,
	#[serde(default)]
	blockers: Vec<String>,
	#[serde(default)]
	evidence: Vec<String>,
	#[serde(default)]
	verification: Vec<String>,
	head_sha: Option<String>,
	branch: Option<String>,
	pr_url: Option<String>,
}

#[derive(Debug)]
struct NormalizedProgressCheckpoint {
	phase: ExecutionProgressPhase,
	docs_impact: DocsImpact,
	focus: String,
	next_action: String,
	blockers: Vec<String>,
	evidence: Vec<String>,
	verification: Vec<String>,
	head_sha: Option<String>,
	branch: Option<String>,
	pr_url: Option<String>,
}
impl NormalizedProgressCheckpoint {
	fn public_branch(&self, review_context: &ReviewHandoffContext) -> String {
		self.branch.clone().unwrap_or_else(|| review_context.branch_name.clone())
	}
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	label: String,
}

#[derive(Debug, Deserialize)]
struct TerminalFinalizeArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointArgs {
	#[serde(flatten)]
	scope: ScopeArgs,
	reviewer: Option<String>,
	status: String,
	head_sha: String,
	review_contract: Option<ReviewCheckpointContractArgs>,
	review_cost_control: Option<ReviewCostControlArgs>,
	checks: Option<ReviewCheckpointChecksArgs>,
	#[serde(default)]
	evidence: Vec<String>,
	#[serde(default)]
	accepted_findings: Vec<ReviewCheckpointFindingArgs>,
	#[serde(default)]
	rejected_findings: Vec<ReviewCheckpointRejectedFindingArgs>,
	#[serde(default)]
	finding_routes: Vec<ReviewCheckpointFindingRouteArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCostControlArgs {
	review_class: String,
	risk_class: String,
	changed_surface_count: u64,
	#[serde(default)]
	changed_surface_summary: Vec<String>,
	#[serde(default)]
	high_risk_surfaces: Vec<String>,
	current_head_evidence: bool,
	validation_backed: bool,
	#[serde(default)]
	validation_current: bool,
	#[serde(default)]
	evidence_sufficient: bool,
	reviewer_judgment: String,
	fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointContractArgs {
	workflow_policy_source: String,
	review_type: String,
	risk_tier: String,
	objective: String,
	#[serde(default)]
	scope: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	required_checks: Vec<String>,
	#[serde(default)]
	allowed_expansion_triggers: Vec<String>,
	#[serde(default)]
	validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointChecksArgs {
	intended_behavior: String,
	regression_risk: String,
	missing_tests: String,
	docs_config_drift: String,
	migration_fallout: String,
	operator_facing_fallout: String,
	loop_decision_contract: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointFindingArgs {
	severity: String,
	summary: String,
	#[serde(default)]
	evidence: Vec<String>,
	kind: Option<String>,
	file: Option<String>,
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	guidance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointRejectedFindingArgs {
	severity: String,
	summary: String,
	rejection_reason: String,
	#[serde(default)]
	evidence: Vec<String>,
	kind: Option<String>,
	file: Option<String>,
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointFindingRouteArgs {
	route: String,
	severity: String,
	summary: String,
	#[serde(default)]
	evidence: Vec<String>,
	resolver: String,
	next_action: String,
	risk_tier: Option<String>,
	finding_source: Option<String>,
	finding_index: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewCheckpointLineRangeArgs {
	start: u64,
	end: u64,
}

#[derive(Debug, Serialize)]
struct NormalizedReviewCheckpointPayload {
	reviewer: String,
	review_contract: NormalizedReviewCheckpointContract,
	review_contract_hash: String,
	review_cost_control: NormalizedReviewCostControl,
	reviewed_head: ReviewCheckpointHeadBinding,
	checks: ReviewCheckpointChecksArgs,
	evidence: Vec<String>,
	accepted_findings: Vec<NormalizedReviewCheckpointFinding>,
	rejected_findings: Vec<NormalizedRejectedReviewCheckpointFinding>,
	finding_routes: Vec<NormalizedReviewCheckpointFindingRoute>,
	finding_route_summary: ReviewCheckpointFindingRouteSummary,
	finding_policy: ReviewFindingPolicyState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedReviewCheckpointContract {
	workflow_policy_source: String,
	review_type: String,
	risk_tier: String,
	objective: String,
	scope: Vec<String>,
	non_goals: Vec<String>,
	required_checks: Vec<String>,
	allowed_expansion_triggers: Vec<String>,
	validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedReviewCostControl {
	review_class: String,
	risk_class: String,
	compact_eligible: bool,
	changed_surface_count: u64,
	changed_surface_summary: Vec<String>,
	high_risk_surfaces: Vec<String>,
	current_head_evidence: bool,
	validation_backed: bool,
	validation_current: bool,
	evidence_sufficient: bool,
	reviewer_judgment: String,
	fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReviewCheckpointHeadBinding {
	head_sha: String,
	head_tree_oid: String,
	review_worktree_clean: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedReviewCheckpointFinding {
	severity: String,
	summary: String,
	#[serde(default)]
	evidence: Vec<String>,
	kind: String,
	file: Option<String>,
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	guidance: String,
	fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedRejectedReviewCheckpointFinding {
	severity: String,
	summary: String,
	rejection_reason: String,
	#[serde(default)]
	evidence: Vec<String>,
	kind: String,
	file: Option<String>,
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedReviewCheckpointFindingRoute {
	route: String,
	severity: String,
	risk_tier: String,
	summary: String,
	#[serde(default)]
	evidence: Vec<String>,
	resolver: String,
	next_action: String,
	finding_source: String,
	finding_index: Option<u64>,
	finding_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReviewCheckpointFindingRouteSummary {
	route_counts: Vec<ReviewCheckpointFindingRouteCount>,
	next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReviewCheckpointFindingRouteCount {
	route: String,
	count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
struct ReviewFindingPolicyState {
	schema: String,
	phase: String,
	status: String,
	head_sha: String,
	nonclean_rounds: i64,
	active_fingerprints: Vec<String>,
	stop_fingerprint: Option<String>,
	findings: Vec<ReviewFindingPolicyRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct ReviewFindingPolicyRecord {
	fingerprint: String,
	kind: String,
	title: String,
	body: String,
	file: Option<String>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	first_seen_head: String,
	last_seen_head: String,
	status: String,
	repeat_count: i64,
	repair_evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PullRequestViewResponse {
	#[serde(rename = "baseRefName")]
	base_ref_name: String,
	#[serde(rename = "headRefName")]
	head_ref_name: String,
	#[serde(rename = "headRefOid")]
	head_ref_oid: String,
	#[serde(rename = "headRepository")]
	head_repository: Option<PullRequestRepositoryResponse>,
	#[serde(rename = "headRepositoryOwner")]
	head_repository_owner: PullRequestRepositoryOwnerResponse,
	#[serde(rename = "isDraft")]
	is_draft: bool,
	state: String,
	url: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestRepositoryResponse {
	name: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestRepositoryOwnerResponse {
	login: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepositoryIdentity {
	name: String,
	owner: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReviewPolicyState {
	phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: String,
	nonclean_rounds: i64,
	details_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewExecutionMode {
	Handoff,
	Repair,
	Closeout,
}
impl ReviewExecutionMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TurnCompletionStatus {
	Continue,
	Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunCompletionDisposition {
	ManualAttention,
	ReviewHandoff,
	ReviewRepair,
	Closeout,
}
impl RunCompletionDisposition {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ManualAttention => "manual_attention",
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub(crate) enum DynamicToolContentItem {
	#[serde(rename = "inputText")]
	InputText { text: String },
}
impl DynamicToolContentItem {
	fn text(text: String) -> Self {
		Self::InputText { text }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewPolicyStopReason {
	Exhausted,
	ArchitectureReviewRequired,
	Blocked,
}
impl ReviewPolicyStopReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::Exhausted => "review_policy_exhausted",
			Self::ArchitectureReviewRequired => "architecture_review_required",
			Self::Blocked => "review_policy_blocked",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionProgressPhase {
	Probing,
	Implementing,
	Verifying,
	Blocked,
	ReadyForReview,
	ReviewRepair,
	ReadyToLand,
	Closeout,
}
impl ExecutionProgressPhase {
	fn as_str(self) -> &'static str {
		match self {
			Self::Probing => "probing",
			Self::Implementing => "implementing",
			Self::Verifying => "verifying",
			Self::Blocked => "blocked",
			Self::ReadyForReview => "ready_for_review",
			Self::ReviewRepair => "review_repair",
			Self::ReadyToLand => "ready_to_land",
			Self::Closeout => "closeout",
		}
	}

	fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"probing" => Ok(Self::Probing),
			"implementing" => Ok(Self::Implementing),
			"verifying" => Ok(Self::Verifying),
			"blocked" => Ok(Self::Blocked),
			"ready_for_review" => Ok(Self::ReadyForReview),
			"review_repair" => Ok(Self::ReviewRepair),
			"ready_to_land" => Ok(Self::ReadyToLand),
			"closeout" => Ok(Self::Closeout),
			other => Err(format!(
				"`issue_progress_checkpoint` phase must be `probing`, `implementing`, `verifying`, `blocked`, `ready_for_review`, `review_repair`, `ready_to_land`, or `closeout`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocsImpact {
	None,
	UpdateRequired,
	ResearchRequired,
	DriftRequired,
}
impl DocsImpact {
	fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::UpdateRequired => "update_required",
			Self::ResearchRequired => "research_required",
			Self::DriftRequired => "drift_required",
		}
	}

	fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"none" => Ok(Self::None),
			"update_required" => Ok(Self::UpdateRequired),
			"research_required" => Ok(Self::ResearchRequired),
			"drift_required" => Ok(Self::DriftRequired),
			other => Err(format!(
				"`issue_progress_checkpoint` docs_impact must be `none`, `update_required`, `research_required`, or `drift_required`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewPolicyPhase {
	Handoff,
	Repair,
}
impl ReviewPolicyPhase {
	fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
		}
	}

	fn for_mode(mode: ReviewExecutionMode) -> Option<Self> {
		match mode {
			ReviewExecutionMode::Handoff => Some(Self::Handoff),
			ReviewExecutionMode::Repair => Some(Self::Repair),
			ReviewExecutionMode::Closeout => None,
		}
	}

	fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"handoff" => Ok(Self::Handoff),
			"repair" => Ok(Self::Repair),
			other => Err(format!(
				"Unsupported review policy phase `{other}` in the run activity marker."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewPolicyStatus {
	Clean,
	Findings,
	NeedsArchitectureReview,
	Blocked,
}
impl ReviewPolicyStatus {
	fn as_str(self) -> &'static str {
		match self {
			Self::Clean => "clean",
			Self::Findings => "findings",
			Self::NeedsArchitectureReview => "needs_architecture_review",
			Self::Blocked => "blocked",
		}
	}

	fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"clean" => Ok(Self::Clean),
			"findings" => Ok(Self::Findings),
			"needs_architecture_review" => Ok(Self::NeedsArchitectureReview),
			"blocked" => Ok(Self::Blocked),
			other => Err(format!(
				"`issue_review_checkpoint` status must be `clean`, `findings`, `needs_architecture_review`, or `blocked`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingReviewCompletion {
	Handoff(PendingReviewAction),
	Repair(PendingReviewAction),
	Closeout(PendingReviewAction),
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

fn review_blocking_status_lines(status: &str) -> Vec<String> {
	status
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.filter(|line| !is_ignorable_runtime_status_line(line))
		.map(ToOwned::to_owned)
		.collect()
}

fn is_ignorable_runtime_status_line(line: &str) -> bool {
	let Some(path) = line.strip_prefix("?? ") else {
		return false;
	};

	path == ".decodex-run-activity"
		|| path.starts_with(".decodex-run-activity/")
		|| path == ".decodex-run-control"
		|| path.starts_with(".decodex-run-control/")
}

fn resolve_review_handoff_github_token(
	review_context: &ReviewHandoffContext,
) -> std::result::Result<String, String> {
	let Some(env_var) = review_context.github_token_env_var.as_deref() else {
		return Err(String::from(
			"`github.token_env_var` must be configured for PR-backed review handoff validation.",
		));
	};
	let value = env::var(env_var).map_err(|error| {
		format!(
			"Failed to read environment variable `{env_var}` referenced by `github.token_env_var`: {error}"
		)
	})?;

	if value.trim().is_empty() {
		return Err(format!(
			"Environment variable `{env_var}` referenced by `github.token_env_var` must not be blank."
		));
	}

	Ok(value)
}

fn run_command_for_stdout(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	let stdout = run_command_stdout(command, args, cwd, purpose)?;
	let value = stdout.trim();

	if value.is_empty() {
		return Err(format!("Failed to {purpose} with `{command}`: command returned no output."));
	}

	Ok(value.to_owned())
}

fn run_command_for_stdout_allow_empty(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	run_command_stdout(command, args, cwd, purpose)
}

fn run_command_stdout(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	let output = Command::new(command)
		.args(args)
		.current_dir(cwd)
		.output()
		.map_err(|error| format!("Failed to {purpose} with `{command}`: {error}"))?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		if detail.is_empty() {
			return Err(format!("Failed to {purpose} with `{command}`."));
		}

		return Err(format!("Failed to {purpose} with `{command}`: {detail}"));
	}

	let stdout = String::from_utf8_lossy(&output.stdout);

	Ok(stdout.into_owned())
}

fn resolve_lane_default_branch(cwd: &Path) -> std::result::Result<String, String> {
	if let Some(default_branch) = resolve_lane_default_branch_from_local_cache(cwd)? {
		return Ok(default_branch);
	}

	let remote_default_branch = resolve_lane_default_branch_from_remote(cwd);

	if let Ok(Some(default_branch)) = remote_default_branch.as_ref() {
		return Ok(default_branch.clone());
	}

	match remote_default_branch {
		Err(error) => Err(error),
		Ok(None) => Err(String::from(
			"Failed to inspect lane default branch with `git`: neither remote `origin` nor local `origin/HEAD` exposed a default branch.",
		)),
		Ok(Some(_)) => unreachable!("handled authoritative remote branch above"),
	}
}

fn resolve_lane_default_branch_from_remote(
	cwd: &Path,
) -> std::result::Result<Option<String>, String> {
	let remote_probe = Command::new("git")
		.args(["ls-remote", "--symref", "origin", "HEAD"])
		.current_dir(cwd)
		.output()
		.map_err(|error| format!("Failed to inspect lane default branch with `git`: {error}"))?;

	if !remote_probe.status.success() {
		let stderr = String::from_utf8_lossy(&remote_probe.stderr);
		let stdout = String::from_utf8_lossy(&remote_probe.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		if detail.is_empty() {
			return Err(String::from("Failed to inspect lane default branch with `git`."));
		}

		return Err(format!("Failed to inspect lane default branch with `git`: {detail}"));
	}

	Ok(parse_remote_head_symref_output(String::from_utf8_lossy(&remote_probe.stdout).as_ref()))
}

fn resolve_lane_default_branch_from_local_cache(
	cwd: &Path,
) -> std::result::Result<Option<String>, String> {
	let symbolic_ref = Command::new("git")
		.args(["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
		.current_dir(cwd)
		.output()
		.map_err(|error| format!("Failed to inspect lane default branch with `git`: {error}"))?;

	if !symbolic_ref.status.success() {
		return Ok(None);
	}

	let stdout = String::from_utf8_lossy(&symbolic_ref.stdout);
	let default_branch = stdout.trim();

	if default_branch.is_empty() {
		return Ok(None);
	}

	Ok(Some(default_branch.strip_prefix("origin/").unwrap_or(default_branch).to_owned()))
}

fn parse_remote_head_symref_output(stdout: &str) -> Option<String> {
	stdout.lines().find_map(|line| {
		let line = line.trim();

		line.strip_prefix("ref: refs/heads/")
			.and_then(|remainder| remainder.strip_suffix("\tHEAD"))
			.map(str::to_owned)
	})
}

fn parse_github_repository_identity(
	remote_url: &str,
) -> std::result::Result<RepositoryIdentity, String> {
	let path = if let Some(path) = remote_url.strip_prefix("git@github.com:") {
		path
	} else {
		parse_github_remote_with_authority(remote_url)?
	};
	let path = path.strip_suffix(".git").unwrap_or(path);
	let mut parts = path.split('/');
	let Some(owner) = parts.next() else {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	};
	let Some(name) = parts.next() else {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	};

	if owner.is_empty() || name.is_empty() || parts.next().is_some() {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	}

	Ok(RepositoryIdentity { name: name.to_owned(), owner: owner.to_owned() })
}

fn parse_github_remote_with_authority(remote_url: &str) -> std::result::Result<&str, String> {
	let rest = remote_url
		.strip_prefix("https://")
		.or_else(|| remote_url.strip_prefix("http://"))
		.or_else(|| remote_url.strip_prefix("ssh://"))
		.ok_or_else(|| format!("Unsupported GitHub remote URL `{remote_url}`."))?;
	let (authority, path) = rest
		.split_once('/')
		.ok_or_else(|| format!("Unsupported GitHub remote URL `{remote_url}`."))?;
	let authority = authority.rsplit('@').next().unwrap_or(authority);
	let host = authority.split_once(':').map(|(host, _)| host).unwrap_or(authority);

	if host != "github.com" {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	}

	Ok(path)
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

#[cfg(test)]
mod tests;
