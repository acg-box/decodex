use std::{path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	recovery::git_worktree::command,
	state,
};

pub(in crate::recovery) fn worktree_head_oid(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["rev-parse", "--verify", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(command::trimmed_stdout(&output.stdout)?));
	}
	if output.status.code() == Some(128) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect retained worktree HEAD in `{}`: {}",
		worktree_path.display(),
		stderr.trim()
	)
}

pub(in crate::recovery) fn worktree_is_clean(worktree_path: &Path) -> Result<bool> {
	Ok(worktree_blocking_status_lines(worktree_path)?.is_empty())
}

pub(in crate::recovery) fn worktree_blocking_status_lines(
	worktree_path: &Path,
) -> Result<Vec<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect retained worktree cleanliness in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let status = String::from_utf8(output.stdout)?;

	Ok(status
		.lines()
		.filter(|line| !line.trim_end().is_empty())
		.filter(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
		.map(ToOwned::to_owned)
		.collect())
}

pub(in crate::recovery) fn worktree_has_tracked_changes_for_recovery(
	worktree_path: &Path,
) -> Result<bool> {
	if !worktree_path.try_exists()? {
		return Ok(false);
	}
	if !worktree_path.join(".git").try_exists()? {
		return Ok(!state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path)?);
	}

	Ok(!worktree_blocking_status_lines(worktree_path)?.is_empty())
}

pub(in crate::recovery) fn worktree_head_has_unmerged_commits_against_remote_default(
	worktree_path: &Path,
) -> Result<Option<bool>> {
	let Some(default_ref) = worktree_remote_default_ref(worktree_path)? else {
		return Ok(None);
	};
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", "HEAD", default_ref.as_str()])
		.output()?;

	match output.status.code() {
		Some(0) => Ok(Some(false)),
		Some(1) => Ok(Some(true)),
		status => {
			let stderr = String::from_utf8_lossy(&output.stderr);

			eyre::bail!(
				"Failed to compare retained worktree HEAD in `{}` against `{default_ref}`: status={:?} {}",
				worktree_path.display(),
				status,
				stderr.trim()
			)
		},
	}
}

fn worktree_remote_default_ref(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
		.output()?;

	if output.status.success() {
		let value = command::trimmed_stdout(&output.stdout)?;

		if !value.is_empty() {
			return Ok(Some(value));
		}
	}

	for candidate in ["origin/main", "main"] {
		let revision = format!("{candidate}^{{commit}}");
		let output = Command::new("git")
			.arg("-C")
			.arg(worktree_path)
			.args(["rev-parse", "--verify", "--quiet", revision.as_str()])
			.output()?;

		if output.status.success() {
			return Ok(Some(candidate.to_owned()));
		}
	}

	Ok(None)
}
