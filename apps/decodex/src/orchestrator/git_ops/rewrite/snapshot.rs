use std::{
	collections::{BTreeMap, BTreeSet},
	path::Path,
	process::Command,
};

use color_eyre::Report;

use crate::{
	orchestrator::git_ops::{
		self, RepoGateFailure, RepoGateFailureKind, diagnostic,
		rewrite::model::RepoGateTrackedDiffSnapshot,
	},
	prelude::Result,
};

pub(crate) fn read_repo_gate_tracked_diff_snapshot(
	cwd: &Path,
	phase: &str,
) -> Result<RepoGateTrackedDiffSnapshot> {
	let full_diff = read_repo_gate_tracked_diff(cwd, phase)?;
	let changed_paths = read_repo_gate_tracked_diff_paths(cwd, phase)?;
	let mut path_diffs = BTreeMap::new();

	for path in changed_paths {
		let path_diff = read_repo_gate_tracked_diff_for_path(cwd, phase, &path)?;

		path_diffs.insert(path, path_diff);
	}

	Ok(RepoGateTrackedDiffSnapshot { full_diff, path_diffs })
}

pub(in crate::orchestrator::git_ops) fn repo_gate_rewritten_files(
	before: &RepoGateTrackedDiffSnapshot,
	after: &RepoGateTrackedDiffSnapshot,
) -> Vec<String> {
	let mut paths = before.path_diffs.keys().cloned().collect::<BTreeSet<_>>();

	paths.extend(after.path_diffs.keys().cloned());

	paths
		.into_iter()
		.filter(|path| before.path_diffs.get(path) != after.path_diffs.get(path))
		.collect()
}

fn read_repo_gate_tracked_diff_paths(cwd: &Path, phase: &str) -> Result<BTreeSet<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file path check after repo gate {phase} in `{}`: {error}",
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = git_ops::repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			diagnostic::repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file paths after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(diagnostic::repo_gate_git_output_lines(&output))
}

fn read_repo_gate_tracked_diff(cwd: &Path, phase: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--no-ext-diff", "--binary", "HEAD", "--"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file diff check after repo gate {phase} in `{}`: {error}",
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = git_ops::repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			diagnostic::repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file diff after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_repo_gate_tracked_diff_for_path(cwd: &Path, phase: &str, path: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--no-ext-diff", "--binary", "HEAD", "--"])
		.arg(path)
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file diff check for `{}` after repo gate {phase} in `{}`: {error}",
					path,
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = git_ops::repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			diagnostic::repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file diff for `{}` after repo gate {phase} in `{}`: {}",
				path,
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
