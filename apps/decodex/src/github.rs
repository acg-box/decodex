use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
	process::{Command, Output},
	thread,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	git_credentials,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

const PULL_REQUEST_LANDING_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      baseRefName
      mergeable
      mergeStateStatus
      headRefName
      headRefOid
      reviewRequests(first: 1) {
        totalCount
      }
      reviewThreads(first: 100, after: $reviewThreadsAfter) {
        nodes {
          isResolved
          isOutdated
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
  }
}
"#;
const GH_BINARY: &str = "gh";
const GH_FALLBACK_PATHS: &[&str] =
	&["/run/current-system/sw/bin/gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GhCommandDiscoveryTier {
	Configured,
	Path,
	UserBin,
	KnownFallback,
	Missing,
}
impl GhCommandDiscoveryTier {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Configured => "configured",
			Self::Path => "path",
			Self::UserBin => "user-bin",
			Self::KnownFallback => "known-fallback",
			Self::Missing => "missing",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GhCommandResolution {
	command_path: PathBuf,
	resolved_path: Option<PathBuf>,
	configured_path: Option<PathBuf>,
	discovery_tier: GhCommandDiscoveryTier,
}
impl GhCommandResolution {
	pub(crate) fn command_path(&self) -> &Path {
		&self.command_path
	}

	pub(crate) fn resolved_path(&self) -> Option<&Path> {
		self.resolved_path.as_deref()
	}

	pub(crate) fn configured_path(&self) -> Option<&Path> {
		self.configured_path.as_deref()
	}

	pub(crate) const fn discovery_tier(&self) -> GhCommandDiscoveryTier {
		self.discovery_tier
	}

	pub(crate) const fn available(&self) -> bool {
		self.resolved_path.is_some()
	}
}

#[derive(Debug)]
pub(crate) struct PullRequestLocator {
	pub(crate) owner: String,
	pub(crate) repo: String,
	pub(crate) number: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepositoryContext {
	pub(crate) owner: String,
	pub(crate) name: String,
	pub(crate) default_branch: String,
	pub(crate) merge_commit_allowed: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IssueCommentCreateResponse {
	pub(crate) id: i64,
	#[serde(rename = "created_at")]
	pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestMergeViewResponse {
	pub(crate) state: String,
	#[serde(rename = "headRefOid")]
	pub(crate) head_ref_oid: Option<String>,
	#[serde(rename = "mergeCommit")]
	pub(crate) merge_commit: Option<PullRequestMergeCommit>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestMergeCommit {
	pub(crate) oid: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateResponse {
	data: PullRequestLandingStateData,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateData {
	repository: Option<PullRequestLandingStateRepository>,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateRepository {
	#[serde(rename = "pullRequest")]
	pull_request: Option<PullRequestLandingStateNode>,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateNode {
	url: String,
	state: String,
	#[serde(rename = "isDraft")]
	is_draft: bool,
	#[serde(rename = "reviewDecision")]
	review_decision: Option<String>,
	#[serde(rename = "baseRefName")]
	base_ref_name: String,
	#[serde(rename = "mergeable")]
	mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	merge_state_status: String,
	#[serde(rename = "headRefName")]
	head_ref_name: String,
	#[serde(rename = "headRefOid")]
	head_ref_oid: String,
	#[serde(rename = "reviewRequests")]
	review_requests: PullRequestReviewRequestConnection,
	#[serde(rename = "reviewThreads")]
	review_threads: PullRequestReviewThreadConnection,
	commits: PullRequestCommitConnection,
}

struct PullRequestLandingStatePageQuery<'a> {
	cwd: &'a Path,
	owner: &'a str,
	repo: &'a str,
	number: u64,
	review_threads_after: Option<&'a str>,
	pr_url: &'a str,
	github_token: &'a str,
	gh_command_path: Option<&'a Path>,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	total_count: usize,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewThreadConnection {
	nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	page_info: PullRequestPageInfo,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	is_resolved: bool,
	#[serde(rename = "isOutdated")]
	is_outdated: bool,
}

#[derive(Debug, Deserialize)]
struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	has_next_page: bool,
	#[serde(rename = "endCursor")]
	end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitConnection {
	nodes: Vec<PullRequestCommitNode>,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitNode {
	commit: PullRequestCommitPayload,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
struct PullRequestStatusCheckRollup {
	state: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewResponse {
	name: String,
	owner: RepositoryViewOwner,
	#[serde(rename = "defaultBranchRef")]
	default_branch_ref: RepositoryViewBranchRef,
	#[serde(rename = "mergeCommitAllowed")]
	merge_commit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewOwner {
	login: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewBranchRef {
	name: String,
}

#[derive(Debug, Deserialize)]
struct CommitViewResponse {
	commit: CommitViewCommit,
}

#[derive(Debug, Deserialize)]
struct CommitViewCommit {
	message: String,
}

pub(crate) fn configure_gh_command(command: &mut Command, github_token: &str) {
	git_credentials::clear_injected_git_config(command);

	command
		.env("GH_TOKEN", github_token)
		.env("GITHUB_TOKEN", github_token)
		.env("GH_PROMPT_DISABLED", "1")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never");
}

pub(crate) fn gh_command_with_config(configured_path: Option<&Path>) -> Command {
	Command::new(gh_command_resolution(configured_path).command_path())
}

pub(crate) fn gh_command_resolution(configured_path: Option<&Path>) -> GhCommandResolution {
	gh_command_resolution_from_env(configured_path, env::var_os("PATH"), env::var_os("HOME"))
}

pub(crate) fn parse_pull_request_url(pr_url: &str) -> Result<PullRequestLocator> {
	let normalized = pr_url.trim().trim_end_matches('/');
	let suffix = normalized.strip_prefix("https://github.com/").ok_or_else(|| {
		eyre::eyre!("Pull request URL `{pr_url}` must start with `https://github.com/`.")
	})?;
	let mut segments = suffix.split('/');
	let owner = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the owner."))?;
	let repo = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the repository."))?;
	let pull_segment = segments
		.next()
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the `pull` segment."))?;

	if pull_segment != "pull" {
		eyre::bail!(
			"Pull request URL `{pr_url}` must use `/pull/<number>`, not `/{pull_segment}`."
		);
	}

	let number = segments
		.next()
		.ok_or_else(|| {
			eyre::eyre!("Pull request URL `{pr_url}` is missing the pull request number.")
		})?
		.parse::<u64>()
		.map_err(|error| {
			eyre::eyre!("Pull request URL `{pr_url}` has an invalid number: {error}")
		})?;

	Ok(PullRequestLocator { owner: owner.to_owned(), repo: repo.to_owned(), number })
}

pub(crate) fn post_pull_request_issue_comment(
	cwd: &Path,
	pr_url: &str,
	body: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<(i64, i64)> {
	let locator = parse_pull_request_url(pr_url)?;
	let endpoint =
		format!("repos/{}/{}/issues/{}/comments", locator.owner, locator.repo, locator.number);
	let mut command = gh_command_with_config(gh_command_path);

	command.args(["api", endpoint.as_str(), "-f", &format!("body={body}")]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to post pull request comment on `{pr_url}`: {}", stderr.trim());
	}

	let response = serde_json::from_slice::<IssueCommentCreateResponse>(&output.stdout)?;
	let created_at_unix_epoch = OffsetDateTime::parse(&response.created_at, &Rfc3339)
		.map_err(|error| {
			eyre::eyre!(
				"Failed to parse GitHub comment timestamp `{}` for `{pr_url}`: {error}",
				response.created_at
			)
		})?
		.unix_timestamp();

	Ok((response.id, created_at_unix_epoch))
}

pub(crate) fn inspect_pull_request_landing_state(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestLandingState> {
	let locator = parse_pull_request_url(pr_url)?;
	let mut review_threads_after: Option<String> = None;
	let mut landing_state: Option<PullRequestLandingState> = None;

	loop {
		let pull_request =
			query_pull_request_landing_state_page(PullRequestLandingStatePageQuery {
				cwd,
				owner: &locator.owner,
				repo: &locator.repo,
				number: locator.number,
				review_threads_after: review_threads_after.as_deref(),
				pr_url,
				github_token,
				gh_command_path,
			})?;
		let next_cursor = match &mut landing_state {
			Some(landing_state) => {
				merge_pull_request_landing_state_page(landing_state, &pull_request)?
			},
			None => {
				let next_cursor = next_pull_request_review_threads_cursor(&pull_request, pr_url)?;

				landing_state = Some(pull_request_landing_state_from_page(&pull_request));

				next_cursor
			},
		};
		let Some(next_cursor) = next_cursor else {
			break;
		};

		review_threads_after = Some(next_cursor);
	}

	landing_state.ok_or_else(|| {
		eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
	})
}

pub(crate) fn inspect_repository_context(
	cwd: &Path,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<RepositoryContext> {
	let mut command = gh_command_with_config(gh_command_path);

	command.args(["repo", "view", "--json", "name,owner,defaultBranchRef,mergeCommitAllowed"]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to inspect current GitHub repository context: {}", stderr.trim());
	}

	let response = serde_json::from_slice::<RepositoryViewResponse>(&output.stdout)?;

	Ok(RepositoryContext {
		owner: response.owner.login,
		name: response.name,
		default_branch: response.default_branch_ref.name,
		merge_commit_allowed: response.merge_commit_allowed,
	})
}

pub(crate) fn pull_request_matches_repository(
	pr_url: &str,
	repository: &RepositoryContext,
) -> Result<bool> {
	let locator = parse_pull_request_url(pr_url)?;

	Ok(locator.owner.eq_ignore_ascii_case(&repository.owner)
		&& locator.repo.eq_ignore_ascii_case(&repository.name))
}

pub(crate) fn delete_pull_request_head_branch_if_present(
	cwd: &Path,
	pr_url: &str,
	branch_name: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	let locator = parse_pull_request_url(pr_url)?;

	delete_repository_branch_if_present(
		cwd,
		&locator.owner,
		&locator.repo,
		branch_name,
		github_token,
		gh_command_path,
	)
}

pub(crate) fn admin_merge_pull_request(
	cwd: &Path,
	pr_url: &str,
	reviewed_head_sha: &str,
	merge_subject: Option<&str>,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	let mut command = gh_command_with_config(gh_command_path);

	configure_admin_merge_command(&mut command, pr_url, reviewed_head_sha, merge_subject);

	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!("Failed to admin-merge `{pr_url}`.");
	}

	eyre::bail!("Failed to admin-merge `{pr_url}`: {detail}");
}

pub(crate) fn inspect_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let response = inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)?;

	if response.state != "MERGED" {
		eyre::bail!("Pull request `{pr_url}` did not reach `MERGED` state after landing.");
	}

	let Some(merge_commit) = response.merge_commit else {
		eyre::bail!("Pull request `{pr_url}` does not expose a merge commit after merge.");
	};

	Ok(merge_commit.oid)
}

pub(crate) fn wait_for_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match inspect_pull_request_merge_commit(cwd, pr_url, github_token, gh_command_path) {
			Ok(merge_commit) => return Ok(merge_commit),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if merge_commit_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn inspect_commit_subject(
	cwd: &Path,
	pr_url: &str,
	commit_oid: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let locator = parse_pull_request_url(pr_url)?;
	let mut command = gh_command_with_config(gh_command_path);

	command
		.args(["api", &format!("repos/{}/{}/commits/{}", locator.owner, locator.repo, commit_oid)]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect merge commit `{commit_oid}` for `{pr_url}`: {}",
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<CommitViewResponse>(&output.stdout)?;
	let subject = response
		.commit
		.message
		.lines()
		.next()
		.map(|line| line.trim_end_matches('\r'))
		.unwrap_or_default();

	if subject.is_empty() {
		eyre::bail!("Merge commit `{commit_oid}` for `{pr_url}` does not expose a subject line.");
	}

	Ok(subject.to_owned())
}

pub(crate) fn wait_for_commit_subject(
	cwd: &Path,
	pr_url: &str,
	commit_oid: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match inspect_commit_subject(cwd, pr_url, commit_oid, github_token, gh_command_path) {
			Ok(subject) => return Ok(subject),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if commit_subject_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn pull_request_is_merged_at_head(
	cwd: &Path,
	pr_url: &str,
	expected_head_sha: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<bool> {
	let response = inspect_pull_request_merge_readback(cwd, pr_url, github_token, gh_command_path)?;

	Ok(response.state == "MERGED" && response.head_ref_oid.as_deref() == Some(expected_head_sha))
}

pub(crate) fn inspect_pull_request_merge_readback(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestMergeViewResponse> {
	inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)
}

fn gh_command_resolution_from_env(
	configured_path: Option<&Path>,
	path_env: Option<OsString>,
	home: Option<OsString>,
) -> GhCommandResolution {
	if let Some(configured_path) = configured_path {
		let command_path = configured_path.to_path_buf();
		let resolved_path = command_path.is_file().then_some(command_path.clone());

		return GhCommandResolution {
			command_path,
			resolved_path,
			configured_path: Some(configured_path.to_path_buf()),
			discovery_tier: GhCommandDiscoveryTier::Configured,
		};
	}
	if let Some(path_env) = path_env {
		for path_entry in env::split_paths(&path_env) {
			let candidate = path_entry.join(GH_BINARY);

			if candidate.is_file() {
				return GhCommandResolution {
					command_path: candidate.clone(),
					resolved_path: Some(candidate),
					configured_path: None,
					discovery_tier: GhCommandDiscoveryTier::Path,
				};
			}
		}
	}
	if let Some(home) = home {
		let home = PathBuf::from(home);

		for relative_candidate in [[".local", "bin", GH_BINARY], [".cargo", "bin", GH_BINARY]] {
			let candidate = relative_candidate
				.iter()
				.fold(home.clone(), |path, component| path.join(*component));

			if candidate.is_file() {
				return GhCommandResolution {
					command_path: candidate.clone(),
					resolved_path: Some(candidate),
					configured_path: None,
					discovery_tier: GhCommandDiscoveryTier::UserBin,
				};
			}
		}
	}

	for candidate in GH_FALLBACK_PATHS {
		let candidate = PathBuf::from(candidate);

		if candidate.is_file() {
			return GhCommandResolution {
				command_path: candidate.clone(),
				resolved_path: Some(candidate),
				configured_path: None,
				discovery_tier: GhCommandDiscoveryTier::KnownFallback,
			};
		}
	}

	GhCommandResolution {
		command_path: PathBuf::from(GH_BINARY),
		resolved_path: None,
		configured_path: None,
		discovery_tier: GhCommandDiscoveryTier::Missing,
	}
}

fn delete_repository_branch_if_present(
	cwd: &Path,
	owner: &str,
	repo: &str,
	branch_name: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	if branch_name.trim().is_empty() {
		eyre::bail!("Refusing to delete an empty GitHub branch name.");
	}

	let endpoint =
		format!("repos/{owner}/{repo}/git/refs/heads/{}", github_api_ref_path(branch_name));
	let mut command = gh_command_with_config(gh_command_path);

	command.args(["api", "--method", "DELETE", "--silent", endpoint.as_str()]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() || gh_delete_ref_missing_branch(&output) {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	eyre::bail!(
		"Failed to delete retained remote branch `{branch_name}` from GitHub repository `{owner}/{repo}`: {detail}"
	);
}

fn inspect_pull_request_merge_response(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestMergeViewResponse> {
	let mut command = gh_command_with_config(gh_command_path);

	command.args(["pr", "view", pr_url, "--json", "state,headRefOid,mergeCommit"]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to inspect merge result for `{pr_url}`: {}", stderr.trim());
	}

	serde_json::from_slice::<PullRequestMergeViewResponse>(&output.stdout).map_err(Into::into)
}

fn configure_admin_merge_command(
	command: &mut Command,
	pr_url: &str,
	reviewed_head_sha: &str,
	merge_subject: Option<&str>,
) {
	command.args(["pr", "merge", "--admin", "--merge", "--match-head-commit", reviewed_head_sha]);

	if let Some(merge_subject) = merge_subject {
		command.args(["--subject", merge_subject]);
	}

	command.args(["--body", ""]);
	command.arg(pr_url);
}

fn gh_delete_ref_missing_branch(output: &Output) -> bool {
	let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
	let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
	let combined = format!("{stderr}\n{stdout}");

	combined.contains("reference does not exist")
		|| combined.contains("reference not found")
		|| (combined.contains("http 422") && combined.contains("reference"))
}

fn github_api_ref_path(ref_name: &str) -> String {
	ref_name.split('/').map(github_api_path_component).collect::<Vec<_>>().join("/")
}

fn github_api_path_component(component: &str) -> String {
	let mut encoded = String::new();

	for byte in component.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
			encoded.push(char::from(byte));
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}

	encoded
}

fn query_pull_request_landing_state_page(
	query: PullRequestLandingStatePageQuery<'_>,
) -> Result<PullRequestLandingStateNode> {
	let mut command = gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_LANDING_STATE_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);

	if let Some(review_threads_after) = query.review_threads_after {
		command.args(["-F", &format!("reviewThreadsAfter={review_threads_after}")]);
	}

	command.current_dir(query.cwd);

	configure_gh_command(&mut command, query.github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect pull request landing state `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestLandingStateResponse>(&output.stdout)?;
	let Some(repository) = response.data.repository else {
		eyre::bail!("GitHub GraphQL response for `{}` did not include a repository.", query.pr_url);
	};
	let Some(pull_request) = repository.pull_request else {
		eyre::bail!(
			"GitHub GraphQL response for `{}` did not include a pull request.",
			query.pr_url
		);
	};

	Ok(pull_request)
}

fn pull_request_landing_state_from_page(
	pull_request: &PullRequestLandingStateNode,
) -> PullRequestLandingState {
	PullRequestLandingState {
		url: pull_request.url.clone(),
		state: pull_request.state.clone(),
		is_draft: pull_request.is_draft,
		review_decision: pull_request.review_decision.clone(),
		base_ref_name: pull_request.base_ref_name.clone(),
		pending_review_requests: pull_request.review_requests.total_count,
		mergeable: pull_request.mergeable.clone(),
		merge_state_status: pull_request.merge_state_status.clone(),
		head_ref_name: pull_request.head_ref_name.clone(),
		head_ref_oid: pull_request.head_ref_oid.clone(),
		status_check_rollup_state: pull_request
			.commits
			.nodes
			.first()
			.and_then(|node| node.commit.status_check_rollup.as_ref())
			.map(|rollup| rollup.state.clone()),
		unresolved_review_threads: count_unresolved_review_threads(&pull_request.review_threads),
	}
}

fn merge_pull_request_landing_state_page(
	landing_state: &mut PullRequestLandingState,
	pull_request: &PullRequestLandingStateNode,
) -> Result<Option<String>> {
	let page_state = pull_request_landing_state_from_page(pull_request);

	if landing_state.url != page_state.url
		|| landing_state.state != page_state.state
		|| landing_state.is_draft != page_state.is_draft
		|| landing_state.review_decision != page_state.review_decision
		|| landing_state.base_ref_name != page_state.base_ref_name
		|| landing_state.pending_review_requests != page_state.pending_review_requests
		|| landing_state.mergeable != page_state.mergeable
		|| landing_state.merge_state_status != page_state.merge_state_status
		|| landing_state.head_ref_name != page_state.head_ref_name
		|| landing_state.head_ref_oid != page_state.head_ref_oid
		|| landing_state.status_check_rollup_state != page_state.status_check_rollup_state
	{
		eyre::bail!("Pull request landing state changed while paginating `{}`.", landing_state.url);
	}

	landing_state.unresolved_review_threads += page_state.unresolved_review_threads;

	next_pull_request_review_threads_cursor(pull_request, landing_state.url.as_str())
}

fn count_unresolved_review_threads(review_threads: &PullRequestReviewThreadConnection) -> usize {
	review_threads.nodes.iter().filter(|thread| !thread.is_resolved && !thread.is_outdated).count()
}

fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestLandingStateNode,
	pr_url: &str,
) -> Result<Option<String>> {
	if !pull_request.review_threads.page_info.has_next_page {
		return Ok(None);
	}

	pull_request
		.review_threads
		.page_info
		.end_cursor
		.clone()
		.map(Some)
		.ok_or_else(|| {
			eyre::eyre!(
				"GitHub GraphQL response for `{pr_url}` reported additional review thread pages without an end cursor."
			)
		})
}

fn merge_commit_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string();

	message.contains("did not reach `MERGED` state after landing")
		|| message.contains("does not expose a merge commit after merge")
}

fn commit_subject_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string().to_ascii_lowercase();

	message.contains("failed to inspect merge commit")
		&& (message.contains("not found") || message.contains("http 404"))
}

#[cfg(test)]
mod tests {
	use std::{
		ffi::{OsStr, OsString},
		fs,
	};

	use crate::prelude::eyre;

	#[test]
	fn parses_pull_request_url() {
		let locator = super::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/20")
			.expect("pull request URL should parse");

		assert_eq!(locator.owner, "hack-ink");
		assert_eq!(locator.repo, "decodex");
		assert_eq!(locator.number, 20);
	}

	#[test]
	fn rejects_non_pull_github_url() {
		let error = super::parse_pull_request_url("https://github.com/hack-ink/decodex/issues/20")
			.expect_err("issue URL should be rejected");

		assert!(error.to_string().contains("/pull/<number>"));
	}

	#[test]
	fn rejects_missing_number() {
		let error = super::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/")
			.expect_err("missing pull number should be rejected");

		assert!(error.to_string().contains("missing the pull request number"));
	}

	#[test]
	fn configure_gh_command_sets_explicit_token_when_present() {
		let mut command = std::process::Command::new("gh");

		super::configure_gh_command(&mut command, "ghp_example");

		let envs = command
			.get_envs()
			.filter_map(|(key, value)| Some((key.to_owned(), value?.to_owned())))
			.collect::<std::collections::HashMap<_, _>>();

		assert_eq!(envs.get(OsStr::new("GH_TOKEN")), Some(&OsStr::new("ghp_example").to_owned()));
		assert_eq!(
			envs.get(OsStr::new("GITHUB_TOKEN")),
			Some(&OsStr::new("ghp_example").to_owned())
		);
	}

	#[test]
	fn configure_gh_command_disables_prompt_for_explicit_token_auth() {
		let mut command = std::process::Command::new("gh");

		super::configure_gh_command(&mut command, "ghp_example");

		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GH_PROMPT_DISABLED")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("1")),
			"configure_gh_command should disable interactive gh prompts"
		);
		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GIT_TERMINAL_PROMPT")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("0")),
			"configure_gh_command should disable interactive git prompts"
		);
		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GCM_INTERACTIVE")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("never")),
			"configure_gh_command should disable credential-manager prompts"
		);
	}

	#[test]
	fn gh_command_resolution_prefers_path_candidate() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let gh_path = temp_dir.path().join("gh");

		fs::write(&gh_path, "").expect("fake gh should write");

		let resolution = super::gh_command_resolution_from_env(
			None,
			Some(OsString::from(temp_dir.path().as_os_str())),
			None,
		);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::Path);
	}

	#[test]
	fn gh_command_resolution_falls_back_to_home_local_bin() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let bin_dir = temp_dir.path().join(".local/bin");
		let gh_path = bin_dir.join("gh");

		fs::create_dir_all(&bin_dir).expect("fake home bin should exist");
		fs::write(&gh_path, "").expect("fake gh should write");

		let resolution = super::gh_command_resolution_from_env(
			None,
			Some(OsString::new()),
			Some(OsString::from(temp_dir.path().as_os_str())),
		);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::UserBin);
	}

	#[test]
	fn gh_command_resolution_uses_configured_path_as_authority() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let gh_path = temp_dir.path().join("configured-gh");

		fs::write(&gh_path, "").expect("fake configured gh should write");

		let resolution =
			super::gh_command_resolution_from_env(Some(&gh_path), Some(OsString::new()), None);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.configured_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::Configured);
	}

	#[test]
	fn gh_command_resolution_knows_nix_profile_fallback() {
		assert!(super::GH_FALLBACK_PATHS.contains(&"/run/current-system/sw/bin/gh"));
	}

	#[test]
	fn merge_commit_wait_retries_only_visibility_errors() {
		assert!(super::merge_commit_wait_error_is_retryable(&eyre::eyre!(
			"Pull request `https://github.com/hack-ink/decodex/pull/1` does not expose a merge commit after merge."
		)));
		assert!(!super::merge_commit_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge result for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401"
		)));
	}

	#[test]
	fn commit_subject_wait_retries_only_not_found_visibility_errors() {
		assert!(super::commit_subject_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 404 Not Found"
		)));
		assert!(!super::commit_subject_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401 Unauthorized"
		)));
	}

	#[test]
	fn repository_match_rejects_foreign_pull_request_url() {
		let repository = super::RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		};

		assert!(
			!super::pull_request_matches_repository(
				"https://github.com/other-org/other-repo/pull/9",
				&repository
			)
			.expect("foreign pull request URL should parse")
		);
	}

	#[test]
	fn repository_match_accepts_case_insensitive_pull_request_url() {
		let repository = super::RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		};

		assert!(
			super::pull_request_matches_repository(
				"https://github.com/Hack-Ink/Decodex/pull/9",
				&repository
			)
			.expect("same repository with different casing should parse")
		);
	}

	#[test]
	fn admin_merge_command_matches_reviewed_head_commit() {
		let mut command = std::process::Command::new("gh");

		super::configure_admin_merge_command(
			&mut command,
			"https://github.com/hack-ink/decodex/pull/50",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			None,
		);

		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

		assert_eq!(
			args,
			vec![
				String::from("pr"),
				String::from("merge"),
				String::from("--admin"),
				String::from("--merge"),
				String::from("--match-head-commit"),
				String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
				String::from("--body"),
				String::from(""),
				String::from("https://github.com/hack-ink/decodex/pull/50"),
			]
		);
	}

	#[test]
	fn admin_merge_command_includes_subject_when_provided() {
		let mut command = std::process::Command::new("gh");

		super::configure_admin_merge_command(
			&mut command,
			"https://github.com/hack-ink/decodex/pull/50",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			Some(r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#),
		);

		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

		assert_eq!(
			args,
			vec![
				String::from("pr"),
				String::from("merge"),
				String::from("--admin"),
				String::from("--merge"),
				String::from("--match-head-commit"),
				String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
				String::from("--subject"),
				String::from(
					r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#
				),
				String::from("--body"),
				String::from(""),
				String::from("https://github.com/hack-ink/decodex/pull/50"),
			]
		);
	}

	#[test]
	fn github_api_ref_path_preserves_ref_slashes_and_encodes_segments() {
		assert_eq!(super::github_api_ref_path("y/decodex XY-235"), "y/decodex%20XY-235");
	}

	#[test]
	fn missing_remote_ref_errors_are_idempotent_cleanup() {
		let output = std::process::Output {
			status: std::process::Command::new("sh")
				.args(["-c", "exit 1"])
				.status()
				.expect("status command should run"),
			stdout: Vec::new(),
			stderr: b"gh: Reference does not exist (HTTP 422)".to_vec(),
		};

		assert!(super::gh_delete_ref_missing_branch(&output));
	}

	#[test]
	fn generic_github_not_found_is_not_idempotent_cleanup() {
		let output = std::process::Output {
			status: std::process::Command::new("sh")
				.args(["-c", "exit 1"])
				.status()
				.expect("status command should run"),
			stdout: Vec::new(),
			stderr: b"gh: Not Found (HTTP 404)".to_vec(),
		};

		assert!(!super::gh_delete_ref_missing_branch(&output));
	}
}
