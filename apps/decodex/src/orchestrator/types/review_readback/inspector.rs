use crate::orchestrator::types::{
	self, Path, PathBuf, PullRequestIssueCommentsPageQuery, PullRequestReadbackFailure,
	PullRequestReviewState, PullRequestReviewStatePageQuery, ServiceConfig, eyre, github,
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

pub(crate) struct GhPullRequestReviewStateInspector {
	pub(crate) github_token_env_var: Option<String>,
	pub(crate) github_command_path: Option<PathBuf>,
	pub(crate) required_status_contexts: Vec<String>,
	pub(crate) allowed_status_creators: Vec<String>,
}
impl GhPullRequestReviewStateInspector {
	pub(crate) fn for_project(project: &ServiceConfig) -> Self {
		Self {
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			github_command_path: project.github().command_path().map(Path::to_path_buf),
			required_status_contexts: project.github().landing_required_status_contexts().to_vec(),
			allowed_status_creators: project.github().landing_required_status_creators().to_vec(),
		}
	}

	#[cfg(test)]
	pub(crate) fn legacy(github_token_env_var: Option<String>) -> Self {
		Self {
			github_token_env_var,
			github_command_path: None,
			required_status_contexts: Vec::new(),
			allowed_status_creators: Vec::new(),
		}
	}
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
		let github_token = types::resolve_configured_env_var(
			"github.token_env_var",
			self.github_token_env_var.as_deref(),
		)?;
		let locator = github::parse_pull_request_url(pr_url)?;
		let mut review_threads_after: Option<String> = None;
		let mut review_state: Option<PullRequestReviewState> = None;
		let mut comments_after: Option<String> = None;

		loop {
			let repository =
				types::query_pull_request_review_state_page(PullRequestReviewStatePageQuery {
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
				Some(review_state) => types::merge_pull_request_review_state_page(
					review_state,
					&repository,
					pull_request,
				)?,
				None => {
					let next_cursor = types::next_pull_request_review_threads_cursor(pull_request)?;

					comments_after = types::next_pull_request_issue_comments_cursor(
						&pull_request.comments,
						pr_url,
					)?;
					review_state = Some(types::pull_request_review_state_from_page(
						&repository,
						pull_request,
					)?);

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
				types::query_pull_request_issue_comments_page(PullRequestIssueCommentsPageQuery {
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
				types::merge_pull_request_issue_comment_page(&mut review_state, &pull_request)?;
		}
		review_state.required_status_contexts = github::inspect_required_commit_status_contexts(
			cwd,
			&locator.owner,
			&locator.repo,
			&review_state.head_ref_oid,
			review_state.base_ref_oid.as_deref(),
			&self.required_status_contexts,
			&self.allowed_status_creators,
			github_token.as_str(),
			self.github_command_path.as_deref(),
		)?;

		Ok(review_state)
	}
}
