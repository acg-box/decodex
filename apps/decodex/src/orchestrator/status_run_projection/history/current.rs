use std::collections::BTreeSet;

use crate::{
	orchestrator::{
		OperatorRunStatus, ProjectLoopEvidenceSnapshot, ServiceConfig, StateStore,
		status_run_projection,
	},
	prelude::Result,
};

pub(crate) fn hydrate_current_lane_lifecycle_metrics(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lanes: &mut [OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> Result<()> {
	for current_lane in current_lanes {
		let attempts = current_lane_lifecycle_attempts(
			project,
			state_store,
			loop_evidence,
			project_display_name,
			current_lane,
			recent_runs,
			now_unix_epoch,
		)?;

		current_lane.lifecycle_metrics =
			status_run_projection::operator_lane_lifecycle_metrics(&attempts);
	}

	Ok(())
}

pub(crate) fn current_lane_lifecycle_attempts(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lane: &OperatorRunStatus,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> Result<Vec<OperatorRunStatus>> {
	let issue_runs =
		state_store.list_project_issue_runs(project.service_id(), &current_lane.issue_id)?;
	let mut attempts = issue_runs
		.into_iter()
		.map(|run| {
			status_run_projection::operator_run_status(
				project,
				loop_evidence,
				project_display_name,
				run,
				now_unix_epoch,
			)
		})
		.collect::<Result<Vec<_>>>()?;

	if attempts.is_empty() {
		let group_key = status_run_projection::operator_run_group_key(current_lane);

		attempts.extend(
			recent_runs
				.iter()
				.filter(|run| status_run_projection::operator_run_group_key(run) == group_key)
				.cloned(),
		);
	}

	let current_lane_snapshot = operator_run_current_lane_snapshot_attempt(current_lane);

	if let Some(attempt) = attempts.iter_mut().find(|run| run.run_id == current_lane.run_id) {
		*attempt = current_lane_snapshot;
	} else {
		attempts.push(current_lane_snapshot);
	}

	Ok(attempts)
}

pub(crate) fn operator_run_current_lane_snapshot_attempt(
	run: &OperatorRunStatus,
) -> OperatorRunStatus {
	let mut snapshot = run.clone();
	let mut evidence = BTreeSet::<String>::new();

	evidence.insert(String::from("current_lane_snapshot"));
	evidence.extend(snapshot.lifecycle_evidence.iter().cloned());

	snapshot.lifecycle_source = String::from("current_snapshot");
	snapshot.lifecycle_evidence = evidence.into_iter().collect();

	snapshot
}
