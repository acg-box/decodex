use std::{
	collections::{BTreeMap, BTreeSet},
	ffi::OsStr,
	path::Path,
	process::Command,
};

use color_eyre::Report;
use serde_json::{Value, json};

use super::{
	RepoGateFailure, RepoGateFailureDiagnostic, RepoGateFailureKind,
	command::run_repo_gate_cleanliness_check_with_git,
	diagnostic::{repo_gate_failure_kind_for_output, repo_gate_git_output_lines},
	repo_gate_output_text,
};
use crate::prelude::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepoGateTrackedRewriteDecision {
	files: Vec<String>,
	owned: bool,
	decision: &'static str,
	reason: &'static str,
	source_error_class: Option<&'static str>,
	source_diagnostic: Option<RepoGateFailureDiagnostic>,
}
impl RepoGateTrackedRewriteDecision {
	fn continue_to_commit_capable_phase(files: Vec<String>) -> Self {
		Self {
			files,
			owned: true,
			decision: "continue_to_commit_capable_phase",
			reason: "all rewritten files were already present in the pre-gate implementation diff and the repo gate passed",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	fn require_attention(files: Vec<String>, owned: bool, reason: &'static str) -> Self {
		Self {
			files,
			owned,
			decision: "repo_gate_tracked_rewrites_left",
			reason,
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	fn scope_envelope_violation(
		files: Vec<String>,
		source_error_class: Option<&'static str>,
		source_diagnostic: Option<RepoGateFailureDiagnostic>,
	) -> Self {
		Self {
			files,
			owned: false,
			decision: "scope_envelope_violation",
			reason: "one or more repo-gate rewrites were not present in the pre-gate lane diff",
			source_error_class,
			source_diagnostic,
		}
	}

	pub(crate) fn files_display(&self) -> String {
		if self.files.is_empty() {
			String::from("(no tracked files reported)")
		} else {
			self.files.join(", ")
		}
	}

	pub(crate) fn to_json(&self) -> Value {
		json!({
			"files": &self.files,
			"owned": self.owned,
			"decision": self.decision,
			"reason": self.reason,
			"sourceErrorClass": self.source_error_class,
			"sourceRepoGateFailure": self.source_diagnostic.as_ref().map(RepoGateFailureDiagnostic::to_json),
		})
	}

	pub(crate) fn is_scope_envelope_violation(&self) -> bool {
		self.decision == "scope_envelope_violation"
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RepoGateCommandOutcome {
	tracked_rewrite_decision: Option<RepoGateTrackedRewriteDecision>,
}
impl RepoGateCommandOutcome {
	fn clean() -> Self {
		Self::default()
	}

	fn with_tracked_rewrite_decision(decision: RepoGateTrackedRewriteDecision) -> Self {
		Self { tracked_rewrite_decision: Some(decision) }
	}

	pub(crate) fn tracked_rewrite_decision(&self) -> Option<&RepoGateTrackedRewriteDecision> {
		self.tracked_rewrite_decision.as_ref()
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RepoGateTrackedDiffSnapshot {
	full_diff: String,
	path_diffs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepoGateScopeEnvelope {
	authorized_paths: BTreeSet<String>,
}
impl RepoGateScopeEnvelope {
	fn from_pre_gate_diff(snapshot: &RepoGateTrackedDiffSnapshot) -> Self {
		Self { authorized_paths: snapshot.path_diffs.keys().cloned().collect() }
	}

	fn violation_files(&self, rewritten_files: impl IntoIterator<Item = String>) -> Vec<String> {
		rewritten_files.into_iter().filter(|path| !self.authorized_paths.contains(path)).collect()
	}
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
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
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

	Ok(repo_gate_git_output_lines(&output))
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
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
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
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
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

pub(super) fn read_repo_gate_tracked_diff_snapshot(
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

fn repo_gate_rewritten_files(
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

fn repo_gate_tracked_rewrite_decision(
	before: &RepoGateTrackedDiffSnapshot,
	after: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Option<RepoGateTrackedRewriteDecision> {
	if before.full_diff == after.full_diff {
		return None;
	}

	let rewritten_files = repo_gate_rewritten_files(before, after);
	let scope_envelope = RepoGateScopeEnvelope::from_pre_gate_diff(before);
	let owned = !rewritten_files.is_empty()
		&& scope_envelope.violation_files(rewritten_files.iter().cloned()).is_empty();

	if allow_owned_rewrites && owned {
		return Some(RepoGateTrackedRewriteDecision::continue_to_commit_capable_phase(
			rewritten_files,
		));
	}

	let reason = if owned {
		"all rewritten files were pre-gate implementation paths, but this lifecycle boundary requires a clean committed worktree"
	} else {
		"one or more rewritten files were not present in the pre-gate implementation diff"
	};

	Some(RepoGateTrackedRewriteDecision::require_attention(rewritten_files, owned, reason))
}

pub(super) fn repo_gate_diff_rewrite_outcome(
	cwd: &Path,
	phase: &str,
	before: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Result<RepoGateCommandOutcome> {
	let after = read_repo_gate_tracked_diff_snapshot(cwd, phase)?;
	let Some(decision) = repo_gate_tracked_rewrite_decision(before, &after, allow_owned_rewrites)
	else {
		return Ok(RepoGateCommandOutcome::clean());
	};

	if decision.decision == "continue_to_commit_capable_phase" {
		return Ok(RepoGateCommandOutcome::with_tracked_rewrite_decision(decision));
	}

	let output = run_repo_gate_cleanliness_check_with_git(OsStr::new("git"), cwd)?;

	if !output.status.success() {
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file cleanliness after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	let output_text = repo_gate_output_text(&output);
	let dirty_entries = output_text.trim();

	Err(Report::new(RepoGateFailure::new(
		RepoGateFailureKind::TrackedRewritesLeft,
		format!(
			"Repo gate {phase} rewrote tracked files in `{}`; owned={}; decision={}; reason={}; files={}; commit or revert these changes before continuing:\n{}",
			cwd.display(),
			decision.owned,
			decision.decision,
			decision.reason,
			decision.files_display(),
			dirty_entries
		),
	).with_tracked_rewrite_decision(decision)))
}

pub(super) fn repo_gate_scope_envelope_failure_or_source(
	cwd: &Path,
	phase: &str,
	before: &RepoGateTrackedDiffSnapshot,
	source_error: Report,
) -> Report {
	let source_error_text = source_error.to_string();
	let source_repo_gate_failure = source_error.downcast_ref::<RepoGateFailure>();
	let source_error_class = source_repo_gate_failure.map(RepoGateFailure::error_class);
	let source_diagnostic =
		source_repo_gate_failure.and_then(|failure| failure.diagnostic().cloned());
	let after = match read_repo_gate_tracked_diff_snapshot(cwd, phase) {
		Ok(after) => after,
		Err(error) => {
			tracing::warn!(
				repo_root = %cwd.display(),
				phase,
				error = %error,
				"Could not inspect repo-gate write-set after command failure."
			);

			return source_error;
		},
	};
	let rewritten_files = repo_gate_rewritten_files(before, &after);
	let scope_envelope = RepoGateScopeEnvelope::from_pre_gate_diff(before);
	let scope_violation_files = scope_envelope.violation_files(rewritten_files);

	if scope_violation_files.is_empty() {
		return source_error;
	}

	let decision = RepoGateTrackedRewriteDecision::scope_envelope_violation(
		scope_violation_files,
		source_error_class,
		source_diagnostic,
	);

	Report::new(
		RepoGateFailure::new(
			RepoGateFailureKind::ScopeEnvelopeViolation,
			format!(
				"Repo gate {phase} failed after writing files outside the lane scope envelope in `{}`; files={}; source failure: {}",
				cwd.display(),
				decision.files_display(),
				source_error_text
			),
		)
		.with_tracked_rewrite_decision(decision),
	)
}
