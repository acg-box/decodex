//! Ghost-lane recovery command orchestration.

use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	recovery::{
		self, GHOST_LANE_TERMINAL_STATUS, GhostLaneCleanupRequest, GhostLaneDiagnoseRequest,
		GhostLaneRecoveryReport,
	},
};

/// Run a read-only missing-issue ghost-lane diagnostic.
pub(crate) fn run_ghost_lane_diagnose(
	config_path: Option<&Path>,
	request: &GhostLaneDiagnoseRequest,
) -> Result<()> {
	let context = recovery::load_recovery_context_read_only(config_path)?;

	if let Some(message) = recovery::active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match recovery::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		request.issue.as_deref(),
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) = recovery::remember_recovery_tracker_backoff_message(
				&context,
				&error,
				"ghost_lane_recovery",
			) {
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};

	if let Err(error) = recovery::apply_ghost_lane_live_status_blockers(&context, &mut diagnostics)
	{
		if let Some(message) = recovery::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		) {
			println!("{message}");

			return Ok(());
		}

		return Err(error);
	}

	let report =
		GhostLaneRecoveryReport { project_id: context.config.service_id().to_owned(), diagnostics };

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", recovery::render_ghost_lane_recovery_report(&report));
	}

	Ok(())
}

/// Terminalize a proven missing-issue ghost lane and clear its local run lease.
pub(crate) fn run_ghost_lane_cleanup(
	config_path: Option<&Path>,
	request: &GhostLaneCleanupRequest,
) -> Result<()> {
	let context = recovery::load_recovery_context_for_dry_run(config_path, request.dry_run)?;

	if let Some(message) = recovery::active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match if context.runtime_mutation_policy.allows_runtime_writes() {
		recovery::diagnose_ghost_lanes(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&context.tracker,
			Some(&request.issue),
		)
	} else {
		recovery::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&context.tracker,
			Some(&request.issue),
		)
	} {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) = recovery::remember_recovery_tracker_backoff_message(
				&context,
				&error,
				"ghost_lane_recovery",
			) {
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let diagnostic = diagnostics
		.pop()
		.ok_or_else(|| eyre::eyre!("No leased lane matched `{}`.", request.issue))?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover ghost-lane cleanup` refused `{}` because safety inspection reported blockers: {}",
			request.issue,
			diagnostic.blockers.join(", ")
		);
	}

	if let Err(error) =
		recovery::ensure_ghost_lane_live_status_allows_cleanup(&context, &diagnostic)
	{
		if let Some(message) = recovery::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		) {
			println!("{message}");

			return Ok(());
		}

		return Err(error);
	}

	if request.dry_run {
		println!(
			"dry run: ghost lane cleanup validated for project={} issue={} run_id={} attempt={} classification={}",
			diagnostic.project_id,
			recovery::render_ghost_lane_issue(&diagnostic),
			diagnostic.run_id,
			diagnostic.attempt_number,
			diagnostic.classification
		);

		return Ok(());
	}

	recovery::apply_ghost_lane_cleanup(&context.state_store, &diagnostic)?;

	println!(
		"ghost lane cleanup ok: project={} issue={} run_id={} attempt={} status={} lease_cleared=yes",
		diagnostic.project_id,
		recovery::render_ghost_lane_issue(&diagnostic),
		diagnostic.run_id,
		diagnostic.attempt_number,
		GHOST_LANE_TERMINAL_STATUS
	);

	Ok(())
}
