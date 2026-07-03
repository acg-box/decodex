use std::collections::HashSet;

use time::OffsetDateTime;

use crate::{
	orchestrator::status::{self, OperatorRunStatus, ProjectRunStatus, ServiceConfig, StateStore},
	prelude::Result,
};

pub(crate) fn build_lane_inspect_operator_runs(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &str,
	run_id: Option<&str>,
	limit: usize,
) -> Result<Vec<OperatorRunStatus>> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (current_lanes, recent_runs) =
		state_store.list_project_runs(project.service_id(), limit)?;
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = status::operator_project_display_name(project);
	let mut seen_run_ids = HashSet::new();
	let mut runs = Vec::new();

	for run in current_lanes.into_iter().chain(recent_runs) {
		if !seen_run_ids.insert(run.run_id().to_owned()) {
			continue;
		}
		if !project_run_status_issue_matches(&run, issue) {
			continue;
		}
		if run_id.is_some_and(|expected| expected != run.run_id()) {
			continue;
		}

		let mut run = status::operator_run_status(
			project,
			&loop_evidence,
			&project_display_name,
			run,
			now_unix_epoch,
		)?;

		apply_terminal_ledger_projection_to_lane_inspect_run(project, state_store, &mut run)?;

		runs.push(run);
	}

	Ok(runs)
}

pub(crate) fn apply_terminal_ledger_projection_to_lane_inspect_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &mut OperatorRunStatus,
) -> Result<()> {
	let records = state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

	if records.is_empty() {
		return Ok(());
	}

	let records = status::local_history_ledger_records(records);
	let outcome = status::operator_history_ledger_outcome(&records);

	if status::history_ledger_outcome_is_terminal(&outcome)
		&& !status::current_lane_has_authoritative_live_owner(run)
	{
		status::apply_terminal_history_ledger_outcome_to_run(run, &outcome);
	}

	Ok(())
}

pub(crate) fn project_run_status_issue_matches(run: &ProjectRunStatus, issue: &str) -> bool {
	let issue = issue.trim();
	let worktree_path = run.worktree_path().map(|path| path.display().to_string());
	let issue_identifier = status::operator_run_issue_identifier_from_fields(
		run.run_id(),
		run.branch_name(),
		worktree_path.as_deref(),
	);

	run.issue_id() == issue
		|| issue_identifier.as_deref() == Some(issue)
		|| issue_identifier
			.as_ref()
			.is_some_and(|identifier| identifier.eq_ignore_ascii_case(issue))
}
