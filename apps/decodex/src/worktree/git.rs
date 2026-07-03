use std::{
	ffi::OsStr,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::prelude::{Result, eyre};

pub(super) fn configured_branch_owner(repo_root: &Path) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--get", "codex.github-identity"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	(!value.is_empty()).then_some(value)
}

pub(super) fn worktree_is_registered(repo_root: &Path, expected_path: &Path) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["worktree", "list", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to list linked worktrees in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	for line in String::from_utf8_lossy(&output.stdout).lines() {
		let Some(path) = line.strip_prefix("worktree ") else {
			continue;
		};
		let candidate = fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

		if candidate == expected_path {
			return Ok(true);
		}
	}

	Ok(false)
}

pub(super) fn resolve_source_repo_git_common_dir(repo_root: &Path) -> Result<PathBuf> {
	Ok(fs::canonicalize(PathBuf::from(git_stdout(
		repo_root,
		["rev-parse", "--path-format=absolute", "--git-common-dir"],
		"resolve source repository git common dir",
	)?))?)
}

pub(super) fn git_stdout<I, S>(repo_root: &Path, args: I, action: &str) -> Result<String>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn run_git<I, S>(repo_root: &Path, args: I, action: &str) -> Result<()>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(())
}

pub(super) fn normalize_origin_remote_for_worktrees(repo_root: &Path) -> Result<()> {
	let Some(origin_url) = try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	else {
		return Ok(());
	};

	if !is_relative_filesystem_remote(origin_url.as_str()) {
		return Ok(());
	}

	let absolute_origin = fs::canonicalize(repo_root.join(&origin_url))?;
	let absolute_origin = absolute_origin.to_str().ok_or_else(|| {
		eyre::eyre!(
			"Resolved absolute origin path `{}` is not valid UTF-8.",
			absolute_origin.display()
		)
	})?;

	run_git(
		repo_root,
		["remote", "set-url", "origin", absolute_origin],
		"normalize the source repository origin remote for linked worktrees",
	)
}

pub(super) fn is_relative_filesystem_remote(remote_url: &str) -> bool {
	if remote_url.starts_with("./") || remote_url.starts_with("../") {
		return true;
	}
	if remote_url == "~" || remote_url.starts_with("~/") {
		return false;
	}

	!remote_url.contains("://") && !remote_url.contains(':') && !Path::new(remote_url).is_absolute()
}

pub(super) fn fetch_remote_branch_if_present(repo_root: &Path, branch_name: &str) -> Result<bool> {
	if try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	.is_none()
	{
		return Ok(false);
	}

	let remote_ref = format!("refs/heads/{branch_name}");
	let mut branch_check = Command::new("git");

	configure_noninteractive_git(&mut branch_check);

	let branch_check = branch_check
		.arg("-C")
		.arg(repo_root)
		.args(["ls-remote", "--exit-code", "--heads", "origin", remote_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(2) {
			return Ok(false);
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect remote worktree branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");
	let mut fetch = Command::new("git");

	configure_noninteractive_git(&mut fetch);

	let output = fetch
		.arg("-C")
		.arg(repo_root)
		.args([
			"fetch",
			"--quiet",
			"--no-tags",
			"origin",
			&format!("refs/heads/{branch_name}:{remote_tracking_ref}"),
		])
		.output()?;

	if output.status.success() {
		return Ok(true);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to fetch remote worktree branch `{branch_name}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

pub(super) fn sanitize_branch_component(value: &str) -> String {
	value
		.chars()
		.map(|ch| match ch {
			'A'..='Z' => ch.to_ascii_lowercase(),
			'a'..='z' | '0'..='9' => ch,
			'-' | '_' => '-',
			_ => '-',
		})
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}

fn try_git_stdout<I, S>(repo_root: &Path, args: I, action: &str) -> Result<Option<String>>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if output.status.success() {
		return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	if stderr.contains("No such remote") {
		return Ok(None);
	}

	eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
}

fn configure_noninteractive_git(command: &mut Command) -> &mut Command {
	command.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never")
}
