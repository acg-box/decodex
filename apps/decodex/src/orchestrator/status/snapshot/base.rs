use time::OffsetDateTime;

use crate::{
	orchestrator::{
		runtime,
		status::{
			self, AccountActivityMode, OperatorCodexAccountControlStatus, OperatorProjectStatus,
			OperatorStatusSnapshot, ServiceConfig, StateStore,
		},
	},
	prelude::Result,
};

pub(crate) fn build_operator_status_snapshot(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Probe,
	)
}

pub(crate) fn build_operator_status_snapshot_with_account_mode(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
	account_activity_mode: AccountActivityMode,
) -> Result<OperatorStatusSnapshot> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (leased_runs, recent_runs) = state_store.list_project_runs(project.service_id(), limit)?;
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = status::operator_project_display_name(project);
	let recent_runs = recent_runs
		.into_iter()
		.map(|run| {
			status::operator_run_status(
				project,
				&loop_evidence,
				&project_display_name,
				run,
				now_unix_epoch,
			)
		})
		.collect::<Result<Vec<_>>>()?;
	let current_lanes = status::operator_current_lane_statuses(
		project,
		state_store,
		&loop_evidence,
		&project_display_name,
		leased_runs,
		&recent_runs,
		now_unix_epoch,
	)?;
	let history_lanes = status::operator_history_lanes(&current_lanes, &recent_runs);
	let (worktrees, mut warnings, warning_details) =
		status::operator_status_worktrees(project, state_store)?;
	let accounts =
		status::codex_account_activity_summaries(project, &mut warnings, account_activity_mode);
	let mut snapshot = OperatorStatusSnapshot {
		project_id: project.service_id().to_owned(),
		run_limit: limit,
		status_source: None,
		snapshot_age_seconds: None,
		warnings,
		warning_details,
		connector_backoffs: Vec::new(),
		projects: vec![OperatorProjectStatus {
			project_id: project.service_id().to_owned(),
			config_path: String::new(),
			repo_root: project.repo_root().display().to_string(),
			enabled: true,
			github_cli_authority: status::operator_github_cli_authority(project),
			current_lane_count: current_lanes.len(),
			running_lane_count: current_lanes.len(),
			queued_candidate_count: 0,
			post_review_lane_count: 0,
			retained_worktree_count: 0,
			waiting_lane_count: 0,
			attention_count: 0,
			cleanup_blocked_count: 0,
			cleanup_pending_count: 0,
			connector_state: String::from("ok"),
			last_activity_at: None,
			warning_count: 0,
		}],
		account_control: global_codex_account_control_status(),
		accounts,
		current_lanes,
		recent_runs,
		history_lanes,
		execution_programs: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees,
		post_review_lanes: Vec::new(),
	};

	status::refresh_worktree_ownership(&mut snapshot, None);
	status::refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}

pub(crate) fn global_codex_account_control_status() -> OperatorCodexAccountControlStatus {
	let account_selector = runtime::global_fixed_account_selector().ok().flatten();
	let mode = if account_selector.is_some() { "fixed" } else { "balanced" };

	OperatorCodexAccountControlStatus { mode: String::from(mode), account_selector }
}
