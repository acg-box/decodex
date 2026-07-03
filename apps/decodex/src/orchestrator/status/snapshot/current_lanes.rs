use std::collections::{HashMap, HashSet};

use crate::{
	orchestrator::status::{
		self, OperatorRunStatus, ProjectLoopEvidenceSnapshot, ProjectRunStatus, ServiceConfig,
		StateStore,
	},
	prelude::Result,
};

pub(crate) fn operator_current_lane_statuses(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	leased_runs: Vec<ProjectRunStatus>,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> Result<Vec<OperatorRunStatus>> {
	let mut current_lanes = leased_runs
		.into_iter()
		.map(|run| {
			status::operator_run_status(
				project,
				loop_evidence,
				project_display_name,
				run,
				now_unix_epoch,
			)
		})
		.collect::<Result<Vec<_>>>()?
		.into_iter()
		.filter(status::operator_run_counts_as_current_lane)
		.collect::<Vec<_>>();
	let latest_attempt_by_issue_key =
		operator_latest_attempt_by_issue_key(current_lanes.iter().chain(recent_runs.iter()));

	current_lanes.retain(|run| {
		!operator_run_is_superseded_by_newer_attempt(run, &latest_attempt_by_issue_key)
	});

	let mut current_lane_run_ids =
		current_lanes.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();

	for run in recent_runs {
		if current_lane_run_ids.contains(&run.run_id)
			|| operator_run_is_superseded_by_newer_attempt(run, &latest_attempt_by_issue_key)
			|| !status::operator_run_has_live_execution(run)
		{
			continue;
		}

		current_lane_run_ids.insert(run.run_id.clone());
		current_lanes.push(run.clone());
	}

	status::hydrate_current_lane_lifecycle_metrics(
		project,
		state_store,
		loop_evidence,
		project_display_name,
		&mut current_lanes,
		recent_runs,
		now_unix_epoch,
	)?;

	Ok(current_lanes)
}

pub(crate) fn operator_latest_attempt_by_issue_key<'a>(
	runs: impl Iterator<Item = &'a OperatorRunStatus>,
) -> HashMap<String, i64> {
	let mut latest_attempt_by_issue_key = HashMap::new();

	for run in runs {
		let issue_key = status::operator_run_group_key(run);
		let latest_attempt =
			latest_attempt_by_issue_key.entry(issue_key).or_insert(run.attempt_number);

		*latest_attempt = (*latest_attempt).max(run.attempt_number);
	}

	latest_attempt_by_issue_key
}

pub(crate) fn operator_run_is_superseded_by_newer_attempt(
	run: &OperatorRunStatus,
	latest_attempt_by_issue_key: &HashMap<String, i64>,
) -> bool {
	latest_attempt_by_issue_key
		.get(&status::operator_run_group_key(run))
		.is_some_and(|latest_attempt| run.attempt_number < *latest_attempt)
}
