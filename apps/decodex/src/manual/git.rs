use std::{
	fs,
	path::{Path, PathBuf},
	process::{Command, Stdio},
};

use crate::{
	prelude::{Result, eyre},
	state,
};

pub(super) fn paths_match_for_manual_commit_guard(left: &Path, right: &Path) -> bool {
	let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
	let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());

	left == right
}

pub(super) fn current_worktree_root(cwd: &Path) -> Result<PathBuf> {
	let root = run_git_capture(cwd, &["rev-parse", "--show-toplevel"])?;

	Ok(PathBuf::from(root))
}

pub(super) fn current_branch_name(cwd: &Path) -> Result<String> {
	let branch = run_git_capture(cwd, &["branch", "--show-current"])?;

	if branch.is_empty() {
		eyre::bail!("Current Git checkout is detached; switch back to a lane branch first.");
	}

	Ok(branch)
}

pub(super) fn current_branch_name_if_attached(cwd: &Path) -> Result<Option<String>> {
	let branch = run_git_capture(cwd, &["branch", "--show-current"])?;

	Ok((!branch.is_empty()).then_some(branch))
}

pub(super) fn current_head_oid(cwd: &Path) -> Result<String> {
	run_git_capture(cwd, &["rev-parse", "HEAD"])
}

pub(super) fn ensure_clean_worktree(cwd: &Path) -> Result<()> {
	let status = run_git_capture(cwd, &["status", "--porcelain"])?;

	if status.lines().any(is_landing_blocking_status_line) {
		eyre::bail!("Worktree has uncommitted changes. Commit or stash them before landing.");
	}

	Ok(())
}

pub(super) fn is_landing_blocking_status_line(line: &str) -> bool {
	let line = line.trim_end();

	!line.is_empty() && !state::is_untracked_decodex_runtime_artifact_status_line(line)
}

pub(super) fn run_git_capture(cwd: &Path, args: &[&str]) -> Result<String> {
	let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		eyre::bail!("`git {}` failed in `{}`: {detail}", args.join(" "), cwd.display());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn run_git_checked_with_stdio(cwd: &Path, args: &[&str]) -> Result<()> {
	let status = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(args)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()?;

	if status.success() {
		return Ok(());
	}

	eyre::bail!("`git {}` failed in `{}`.", args.join(" "), cwd.display());
}
