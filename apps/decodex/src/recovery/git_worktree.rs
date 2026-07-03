//! Git worktree inspection helpers for recovery validation.

use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{
	prelude::{Result, eyre},
	state,
};

pub(super) enum ReviewHandoffLineage {
	Descends,
	Diverged,
	Unknown,
}

pub(super) fn git_toplevel_path(cwd: &Path) -> Result<PathBuf> {
	let output =
		Command::new("git").arg("-C").arg(cwd).args(["rev-parse", "--show-toplevel"]).output()?;

	if output.status.success() {
		return Ok(PathBuf::from(trimmed_stdout(&output.stdout)?));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect current Git worktree root from `{}`: {}",
		cwd.display(),
		stderr.trim()
	)
}

pub(super) fn worktree_checkout_branch_name(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(trimmed_stdout(&output.stdout)?));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect retained worktree branch in `{}`: {}",
		worktree_path.display(),
		stderr.trim()
	)
}

pub(super) fn worktree_head_oid(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["rev-parse", "--verify", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(trimmed_stdout(&output.stdout)?));
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

pub(super) fn worktree_head_descends_from_review_handoff(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffLineage::Descends;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffLineage::Descends,
		Some(1) => ReviewHandoffLineage::Diverged,
		_ => ReviewHandoffLineage::Unknown,
	}
}

pub(super) fn worktree_is_clean(worktree_path: &Path) -> Result<bool> {
	Ok(worktree_blocking_status_lines(worktree_path)?.is_empty())
}

pub(super) fn worktree_blocking_status_lines(worktree_path: &Path) -> Result<Vec<String>> {
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

pub(super) fn repository_relative_path(repo_root: &Path, path: &Path) -> Option<String> {
	let canonical_repo_root = fs::canonicalize(repo_root).ok()?;
	let canonical_path = fs::canonicalize(path).ok()?;
	let relative = canonical_path.strip_prefix(canonical_repo_root).ok()?;

	Some(relative.to_string_lossy().to_string())
}

pub(super) fn worktree_has_tracked_changes_for_recovery(worktree_path: &Path) -> Result<bool> {
	if !worktree_path.try_exists()? {
		return Ok(false);
	}
	if !worktree_path.join(".git").try_exists()? {
		return Ok(!state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path)?);
	}

	Ok(!worktree_blocking_status_lines(worktree_path)?.is_empty())
}

pub(super) fn worktree_head_has_unmerged_commits_against_remote_default(
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
		let value = trimmed_stdout(&output.stdout)?;

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

fn trimmed_stdout(stdout: &[u8]) -> Result<String> {
	Ok(String::from_utf8(stdout.to_vec())?.trim().to_owned())
}
