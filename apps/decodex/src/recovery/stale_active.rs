//! Stale-active recovery command orchestration.

use super::{
	GHOST_LANE_TERMINAL_STATUS, RecoveryRuntimeMutationPolicy, StaleActiveDiagnoseRequest,
	StaleActiveRecoveryReport, StaleActiveReleaseRequest, active_recovery_tracker_backoff_message,
	apply_stale_active_release, diagnose_stale_active_issues, load_recovery_context_for_dry_run,
	load_recovery_context_read_only, preflight_stale_active_worktree_cleanup,
	remember_recovery_tracker_backoff_message, render_stale_active_recovery_report,
};
use crate::prelude::{Result, eyre};
use std::path::Path;

/// Run a read-only tracker-present stale active ownership diagnostic.
pub(crate) fn run_stale_active_diagnose(
	config_path: Option<&Path>,
	request: &StaleActiveDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context_read_only(config_path)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let diagnostics = match diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		request.issue.as_deref(),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "stale_active_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let report = StaleActiveRecoveryReport {
		project_id: context.config.service_id().to_owned(),
		diagnostics,
	};

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_stale_active_recovery_report(&report));
	}

	Ok(())
}

/// Release a tracker-present stale active ownership label after fail-closed checks.
pub(crate) fn run_stale_active_release(
	config_path: Option<&Path>,
	request: &StaleActiveReleaseRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		Some(&request.issue),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "stale_active_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let diagnostic = diagnostics
		.pop()
		.ok_or_else(|| eyre::eyre!("No stale active issue matched `{}`.", request.issue))?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because safety inspection reported blockers: {}",
			request.issue,
			diagnostic.blockers.join(", ")
		);
	}

	preflight_stale_active_worktree_cleanup(&context.state_store, &diagnostic)?;

	if request.dry_run {
		println!(
			"dry run: stale active release validated for project={} issue={} run_id={} attempt={} classification={}",
			diagnostic.project_id,
			diagnostic.issue_identifier,
			diagnostic.latest_run_id.as_deref().unwrap_or("none"),
			diagnostic
				.latest_attempt_number
				.map(|attempt| attempt.to_string())
				.unwrap_or_else(|| String::from("none")),
			diagnostic.classification
		);

		return Ok(());
	}

	apply_stale_active_release(&context, &diagnostic)?;

	println!(
		"stale active release ok: project={} issue={} active_label_released=yes queue_label_preserved={} terminal_status={}",
		diagnostic.project_id,
		diagnostic.issue_identifier,
		diagnostic.queue_label_present,
		GHOST_LANE_TERMINAL_STATUS
	);

	Ok(())
}
