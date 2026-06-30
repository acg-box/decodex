use super::{
	Deserialize, ErrorKind, Path, PathBuf, PullRequestIssueCommentsPageQuery,
	PullRequestReviewStatePageQuery, Report, eyre, github, merge_pull_request_issue_comment_page,
	merge_pull_request_review_state_page, next_pull_request_issue_comments_cursor,
	next_pull_request_review_threads_cursor, pull_request_review_state_from_page,
	query_pull_request_issue_comments_page, query_pull_request_review_state_page,
	resolve_configured_env_var,
};

pub(crate) type PullRequestReadbackResult =
	std::result::Result<PullRequestReviewState, PullRequestReadbackFailure>;

pub(crate) trait PullRequestReviewStateInspector {
	fn inspect_review_state(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestReviewState>;

	fn inspect_review_state_readback(&self, cwd: &Path, pr_url: &str) -> PullRequestReadbackResult {
		self.inspect_review_state(cwd, pr_url).map_err(PullRequestReadbackFailure::from)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PullRequestReadbackRootCause {
	MissingGithubCli,
	MissingGithubToken,
	GithubAuthFailed,
	GithubApiReadFailed,
	GithubResponseParseFailed,
	PullRequestShapeReadFailed,
	LineageValidationFailed,
	TrackerIssueReadbackFailed,
}
impl PullRequestReadbackRootCause {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::MissingGithubCli => "missing_github_cli",
			Self::MissingGithubToken => "missing_github_token",
			Self::GithubAuthFailed => "github_auth_failed",
			Self::GithubApiReadFailed => "github_api_read_failed",
			Self::GithubResponseParseFailed => "github_response_parse_failed",
			Self::PullRequestShapeReadFailed => "pull_request_shape_read_failed",
			Self::LineageValidationFailed => "lineage_validation_failed",
			Self::TrackerIssueReadbackFailed => "tracker_issue_readback_failed",
		}
	}
}

#[derive(Debug)]
pub(crate) struct PullRequestReadbackFailure {
	pub(crate) root_cause: PullRequestReadbackRootCause,
	pub(crate) error: Report,
}
impl PullRequestReadbackFailure {
	pub(crate) fn from_report(error: Report) -> Self {
		let root_cause = classify_pull_request_readback_report(&error);

		Self { root_cause, error }
	}

	pub(crate) fn into_report(self) -> Report {
		self.error
	}

	pub(crate) fn root_cause(&self) -> PullRequestReadbackRootCause {
		self.root_cause
	}
}

impl From<Report> for PullRequestReadbackFailure {
	fn from(error: Report) -> Self {
		Self::from_report(error)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestReviewState {
	pub(crate) url: String,
	pub(crate) state: String,
	pub(crate) is_draft: bool,
	pub(crate) review_decision: Option<String>,
	pub(crate) merge_commit_allowed: bool,
	pub(crate) pending_review_requests: usize,
	pub(crate) mergeable: String,
	pub(crate) merge_state_status: String,
	pub(crate) head_ref_name: String,
	pub(crate) head_ref_oid: String,
	pub(crate) merge_commit_oid: Option<String>,
	pub(crate) head_repository_name: Option<String>,
	pub(crate) head_repository_owner: Option<String>,
	pub(crate) status_check_rollup_state: Option<String>,
	pub(crate) unresolved_review_threads: usize,
	pub(crate) issue_description_external_review_thumbs_up_count: usize,
	pub(crate) issue_comments: Vec<PullRequestIssueCommentState>,
	pub(crate) reviews: Vec<PullRequestReviewSummaryState>,
}

pub(crate) struct GhPullRequestReviewStateInspector {
	pub(crate) github_token_env_var: Option<String>,
	pub(crate) github_command_path: Option<PathBuf>,
}
impl PullRequestReviewStateInspector for GhPullRequestReviewStateInspector {
	fn inspect_review_state(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestReviewState> {
		self.inspect_review_state_readback(cwd, pr_url)
			.map_err(PullRequestReadbackFailure::into_report)
	}

	fn inspect_review_state_readback(&self, cwd: &Path, pr_url: &str) -> PullRequestReadbackResult {
		let github_token = resolve_configured_env_var(
			"github.token_env_var",
			self.github_token_env_var.as_deref(),
		)?;
		let locator = github::parse_pull_request_url(pr_url)?;
		let mut review_threads_after: Option<String> = None;
		let mut review_state: Option<PullRequestReviewState> = None;
		let mut comments_after: Option<String> = None;

		loop {
			let repository =
				query_pull_request_review_state_page(PullRequestReviewStatePageQuery {
					cwd,
					owner: &locator.owner,
					repo: &locator.repo,
					number: locator.number,
					review_threads_after: review_threads_after.as_deref(),
					pr_url,
					github_token: github_token.as_str(),
					gh_command_path: self.github_command_path.as_deref(),
				})?;
			let pull_request = repository.pull_request.as_ref().ok_or_else(|| {
				eyre::eyre!(
					"GitHub GraphQL response for `{pr_url}` did not include a pull request."
				)
			})?;
			let next_cursor = match &mut review_state {
				Some(review_state) =>
					merge_pull_request_review_state_page(review_state, &repository, pull_request)?,
				None => {
					let next_cursor = next_pull_request_review_threads_cursor(pull_request)?;

					comments_after =
						next_pull_request_issue_comments_cursor(&pull_request.comments, pr_url)?;
					review_state =
						Some(pull_request_review_state_from_page(&repository, pull_request)?);

					next_cursor
				},
			};
			let Some(next_cursor) = next_cursor else {
				break;
			};

			review_threads_after = Some(next_cursor);
		}

		let mut review_state = review_state.ok_or_else(|| {
			eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
		})?;

		while let Some(cursor) = comments_after.take() {
			let pull_request =
				query_pull_request_issue_comments_page(PullRequestIssueCommentsPageQuery {
					cwd,
					owner: &locator.owner,
					repo: &locator.repo,
					number: locator.number,
					comments_after: &cursor,
					pr_url,
					github_token: github_token.as_str(),
					gh_command_path: self.github_command_path.as_deref(),
				})?;

			comments_after =
				merge_pull_request_issue_comment_page(&mut review_state, &pull_request)?;
		}

		Ok(review_state)
	}
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateResponse {
	pub(crate) data: PullRequestReviewStateData,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateData {
	pub(crate) repository: Option<PullRequestReviewStateRepository>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateRepository {
	#[serde(rename = "mergeCommitAllowed")]
	pub(crate) merge_commit_allowed: bool,
	#[serde(rename = "pullRequest")]
	pub(crate) pull_request: Option<PullRequestReviewStateNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsResponse {
	pub(crate) data: PullRequestIssueCommentsData,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsData {
	pub(crate) repository: Option<PullRequestIssueCommentsRepository>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsRepository {
	#[serde(rename = "pullRequest")]
	pub(crate) pull_request: Option<PullRequestIssueCommentsNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsNode {
	pub(crate) url: String,
	pub(crate) comments: PullRequestIssueCommentConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateNode {
	pub(crate) url: String,
	pub(crate) state: String,
	#[serde(rename = "isDraft")]
	pub(crate) is_draft: bool,
	#[serde(rename = "reviewDecision")]
	pub(crate) review_decision: Option<String>,
	#[serde(rename = "reviewRequests")]
	pub(crate) review_requests: PullRequestReviewRequestConnection,
	pub(crate) mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	pub(crate) merge_state_status: String,
	#[serde(rename = "headRefName")]
	pub(crate) head_ref_name: String,
	#[serde(rename = "headRefOid")]
	pub(crate) head_ref_oid: String,
	#[serde(rename = "mergeCommit")]
	pub(crate) merge_commit: Option<PullRequestMergeCommitNode>,
	#[serde(rename = "headRepository")]
	pub(crate) head_repository: Option<PullRequestRepository>,
	#[serde(rename = "headRepositoryOwner")]
	pub(crate) head_repository_owner: Option<PullRequestRepositoryOwner>,
	#[serde(rename = "reactionGroups")]
	pub(crate) reaction_groups: Vec<PullRequestReactionGroup>,
	pub(crate) comments: PullRequestIssueCommentConnection,
	pub(crate) reviews: PullRequestReviewConnection,
	#[serde(rename = "reviewThreads")]
	pub(crate) review_threads: PullRequestReviewThreadConnection,
	pub(crate) commits: PullRequestCommitConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestRepositoryOwner {
	pub(crate) login: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestMergeCommitNode {
	pub(crate) oid: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestRepository {
	pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	pub(crate) total_count: usize,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewThreadConnection {
	pub(crate) nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	pub(crate) page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	pub(crate) is_resolved: bool,
	#[serde(rename = "isOutdated")]
	pub(crate) is_outdated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestIssueCommentState {
	pub(crate) database_id: i64,
	pub(crate) author_login: Option<String>,
	pub(crate) body: String,
	pub(crate) created_at_unix_epoch: i64,
	pub(crate) external_review_eyes_reaction_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestReviewSummaryState {
	pub(crate) author_login: Option<String>,
	pub(crate) body: String,
	pub(crate) state: String,
	pub(crate) submitted_at_unix_epoch: i64,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentConnection {
	pub(crate) nodes: Vec<PullRequestIssueCommentNode>,
	#[serde(rename = "pageInfo")]
	pub(crate) page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentNode {
	#[serde(rename = "databaseId")]
	pub(crate) database_id: i64,
	pub(crate) body: String,
	#[serde(rename = "createdAt")]
	pub(crate) created_at: String,
	pub(crate) author: Option<PullRequestActor>,
	#[serde(rename = "reactionGroups")]
	pub(crate) reaction_groups: Vec<PullRequestReactionGroup>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewConnection {
	pub(crate) nodes: Vec<PullRequestReviewNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewNode {
	pub(crate) body: String,
	pub(crate) state: String,
	#[serde(rename = "submittedAt")]
	pub(crate) submitted_at: Option<String>,
	pub(crate) author: Option<PullRequestActor>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReactionGroup {
	pub(crate) content: String,
	pub(crate) users: PullRequestReactionUsersConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReactionUsersConnection {
	pub(crate) nodes: Vec<PullRequestActor>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestActor {
	pub(crate) login: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	pub(crate) has_next_page: bool,
	#[serde(rename = "endCursor")]
	pub(crate) end_cursor: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitConnection {
	pub(crate) nodes: Vec<PullRequestCommitNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitNode {
	pub(crate) commit: PullRequestCommitPayload,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	pub(crate) status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestStatusCheckRollup {
	pub(crate) state: String,
}

pub(crate) fn classify_pull_request_readback_report(
	error: &Report,
) -> PullRequestReadbackRootCause {
	if report_has_io_error_kind(error, ErrorKind::NotFound) {
		return PullRequestReadbackRootCause::MissingGithubCli;
	}
	if report_contains_any(
		error,
		&[
			"must be configured for this github-backed operation",
			"failed to read environment variable",
			"must not be blank",
		],
	) {
		return PullRequestReadbackRootCause::MissingGithubToken;
	}
	if report_chain_has_serde_json_error(error) {
		return PullRequestReadbackRootCause::GithubResponseParseFailed;
	}
	if report_contains_any(
		error,
		&[
			"pull request url",
			"did not include a repository",
			"did not include a pull request",
			"without an end cursor",
		],
	) {
		return PullRequestReadbackRootCause::PullRequestShapeReadFailed;
	}
	if report_contains_any(
		error,
		&[
			"bad credentials",
			"requires authentication",
			"authentication required",
			"not logged in",
			"gh auth login",
			"http 401",
			"http 403",
		],
	) {
		return PullRequestReadbackRootCause::GithubAuthFailed;
	}

	PullRequestReadbackRootCause::GithubApiReadFailed
}

pub(crate) fn report_has_io_error_kind(error: &Report, kind: ErrorKind) -> bool {
	error.chain().any(|cause| {
		cause.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() == kind)
	})
}

pub(crate) fn report_chain_has_serde_json_error(error: &Report) -> bool {
	error.chain().any(|cause| cause.downcast_ref::<serde_json::Error>().is_some())
}

pub(crate) fn report_contains_any(error: &Report, needles: &[&str]) -> bool {
	error.chain().any(|cause| {
		let message = cause.to_string().to_ascii_lowercase();

		needles.iter().any(|needle| message.contains(needle))
	})
}
