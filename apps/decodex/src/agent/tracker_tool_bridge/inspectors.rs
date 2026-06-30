use std::{env, path::Path, process::Command};

use serde::Deserialize;

use crate::{
	agent::tracker_tool_bridge::{
		LocalRepoDetails, LocalRepoInspector, PullRequestDetails, PullRequestInspector,
		ReviewHandoffContext,
	},
	github,
};

pub(super) struct GhPullRequestInspector;
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

pub(super) struct LocalGitRepoInspector;
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
pub(super) struct RepositoryIdentity {
	pub(super) name: String,
	pub(super) owner: String,
}

pub(super) fn review_blocking_status_lines(status: &str) -> Vec<String> {
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

pub(super) fn resolve_review_handoff_github_token(
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

pub(super) fn resolve_lane_default_branch(cwd: &Path) -> std::result::Result<String, String> {
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

pub(super) fn resolve_lane_default_branch_from_remote(
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

pub(super) fn resolve_lane_default_branch_from_local_cache(
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

pub(super) fn parse_remote_head_symref_output(stdout: &str) -> Option<String> {
	stdout.lines().find_map(|line| {
		let line = line.trim();

		line.strip_prefix("ref: refs/heads/")
			.and_then(|remainder| remainder.strip_suffix("\tHEAD"))
			.map(str::to_owned)
	})
}

pub(super) fn parse_github_repository_identity(
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
