use std::{path::Path, process::Command};

use crate::agent::tracker_tool_bridge::{
	LocalRepoDetails, LocalRepoInspector,
	inspectors::{command, repository},
};

pub(in crate::agent::tracker_tool_bridge) struct LocalGitRepoInspector;
impl LocalRepoInspector for LocalGitRepoInspector {
	fn inspect_local_repo(&self, cwd: &Path) -> std::result::Result<LocalRepoDetails, String> {
		let head_oid = command::run_command_for_stdout(
			"git",
			&["rev-parse", "HEAD"],
			cwd,
			"inspect lane HEAD",
		)?;
		let head_tree_oid = command::run_command_for_stdout(
			"git",
			&["rev-parse", "HEAD^{tree}"],
			cwd,
			"inspect lane HEAD tree",
		)?;
		let worktree_status = command::run_command_for_stdout_allow_empty(
			"git",
			&["status", "--porcelain=v1", "--untracked-files=all"],
			cwd,
			"inspect review-blocking worktree status",
		)?;
		let default_branch = resolve_lane_default_branch(cwd)?;
		let origin_url = command::run_command_for_stdout(
			"git",
			&["config", "--get", "remote.origin.url"],
			cwd,
			"inspect lane origin repository",
		)?;
		let repository = repository::parse_github_repository_identity(origin_url.trim())?;

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

pub(in crate::agent::tracker_tool_bridge) fn review_blocking_status_lines(
	status: &str,
) -> Vec<String> {
	status
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.filter(|line| !is_ignorable_runtime_status_line(line))
		.map(ToOwned::to_owned)
		.collect()
}

pub(in crate::agent::tracker_tool_bridge) fn resolve_lane_default_branch(
	cwd: &Path,
) -> std::result::Result<String, String> {
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

	Ok(repository::parse_remote_head_symref_output(
		String::from_utf8_lossy(&remote_probe.stdout).as_ref(),
	))
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

fn is_ignorable_runtime_status_line(line: &str) -> bool {
	let Some(path) = line.strip_prefix("?? ") else {
		return false;
	};

	path == ".decodex-run-activity"
		|| path.starts_with(".decodex-run-activity/")
		|| path == ".decodex-run-control"
		|| path.starts_with(".decodex-run-control/")
}
