use std::{ffi::OsStr, path::Path};

use color_eyre::Report;

use crate::{
	orchestrator::git_ops::{
		self, RepoGateFailure, RepoGateFailureKind, command,
		diagnostic::{self},
		rewrite::{
			model::{
				RepoGateCommandOutcome, RepoGateScopeEnvelope, RepoGateTrackedDiffSnapshot,
				RepoGateTrackedRewriteDecision,
			},
			snapshot,
		},
	},
	prelude::Result,
};

pub(crate) fn repo_gate_diff_rewrite_outcome(
	cwd: &Path,
	phase: &str,
	before: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Result<RepoGateCommandOutcome> {
	let after = snapshot::read_repo_gate_tracked_diff_snapshot(cwd, phase)?;
	let Some(decision) = repo_gate_tracked_rewrite_decision(before, &after, allow_owned_rewrites)
	else {
		return Ok(RepoGateCommandOutcome::clean());
	};

	if decision.decision == "continue_to_commit_capable_phase" {
		return Ok(RepoGateCommandOutcome::with_tracked_rewrite_decision(decision));
	}

	let output = command::run_repo_gate_cleanliness_check_with_git(OsStr::new("git"), cwd)?;

	if !output.status.success() {
		let output_text = git_ops::repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			diagnostic::repo_gate_failure_kind_for_output(
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

	let output_text = git_ops::repo_gate_output_text(&output);
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

pub(crate) fn repo_gate_scope_envelope_failure_or_source(
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
	let after = match snapshot::read_repo_gate_tracked_diff_snapshot(cwd, phase) {
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
	let rewritten_files = snapshot::repo_gate_rewritten_files(before, &after);
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

fn repo_gate_tracked_rewrite_decision(
	before: &RepoGateTrackedDiffSnapshot,
	after: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Option<RepoGateTrackedRewriteDecision> {
	if before.full_diff == after.full_diff {
		return None;
	}

	let rewritten_files = snapshot::repo_gate_rewritten_files(before, after);
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
