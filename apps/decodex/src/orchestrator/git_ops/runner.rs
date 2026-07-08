use std::path::Path;

use color_eyre::Report;

use crate::{
	orchestrator::git_ops::{
		self, RepoGateCommandOutcome, RepoGateFailure, RepoGateFailureDiagnostic,
		RepoGateFailureKind, command, diagnostic,
		rewrite::{self},
	},
	prelude::Result,
};

pub(crate) fn run_repo_gate_commands(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
) -> Result<()> {
	run_repo_gate_commands_with_rewrite_policy(canonicalize_commands, verify_commands, cwd, false)
		.map(|_| ())
}

pub(crate) fn run_repo_gate_commands_with_owned_rewrites(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
) -> Result<RepoGateCommandOutcome> {
	run_repo_gate_commands_with_rewrite_policy(canonicalize_commands, verify_commands, cwd, true)
}

pub(crate) fn run_repo_gate_commands_with_rewrite_policy(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
	allow_owned_rewrites: bool,
) -> Result<RepoGateCommandOutcome> {
	command::preflight_repo_gate_command_runtime(cwd)?;

	let baseline_tracked_diff = rewrite::read_repo_gate_tracked_diff_snapshot(cwd, "baseline")?;

	if let Err(error) = run_canonicalize_commands(canonicalize_commands, cwd) {
		return Err(rewrite::repo_gate_scope_envelope_failure_or_source(
			cwd,
			"canonicalize",
			&baseline_tracked_diff,
			error,
		));
	}
	if let Err(error) = run_verify_commands(verify_commands, cwd) {
		return Err(rewrite::repo_gate_scope_envelope_failure_or_source(
			cwd,
			"verify",
			&baseline_tracked_diff,
			error,
		));
	}

	rewrite::repo_gate_diff_rewrite_outcome(
		cwd,
		"verification",
		&baseline_tracked_diff,
		allow_owned_rewrites,
	)
}

pub(crate) fn run_canonicalize_commands(commands: &[String], cwd: &Path) -> Result<()> {
	for command in commands {
		let output = command::run_repo_gate_shell_command(command, cwd)?;

		if !output.status.success() {
			let output_text = git_ops::repo_gate_output_text(&output);
			let diagnostic = RepoGateFailureDiagnostic::from_output(
				"canonicalize",
				command,
				&output,
				&output_text,
			);

			return Err(Report::new(
				RepoGateFailure::new(
					diagnostic::repo_gate_failure_kind_for_output(
						RepoGateFailureKind::CanonicalizeCommandFailed,
						&output_text,
					),
					format!(
						"Repo canonicalize command `{}` failed in `{}`: {}",
						command,
						cwd.display(),
						output_text
					),
				)
				.with_diagnostic(diagnostic),
			));
		}
	}

	Ok(())
}

fn run_verify_commands(commands: &[String], cwd: &Path) -> Result<()> {
	for command in commands {
		let output = command::run_repo_gate_shell_command(command, cwd)?;

		if !output.status.success() {
			let output_text = git_ops::repo_gate_output_text(&output);
			let diagnostic =
				RepoGateFailureDiagnostic::from_output("verify", command, &output, &output_text);

			return Err(Report::new(
				RepoGateFailure::new(
					diagnostic::repo_gate_failure_kind_for_output(
						RepoGateFailureKind::VerifyCommandFailed,
						&output_text,
					),
					format!(
						"Repo verify command `{}` failed in `{}`: {}",
						command,
						cwd.display(),
						output_text
					),
				)
				.with_diagnostic(diagnostic),
			));
		}
	}

	Ok(())
}
