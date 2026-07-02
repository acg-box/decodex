use crate::orchestrator::{
	self, HashMap, HashSet, OperatorHistoryLaneStatus, OperatorRunStatus, status_run_projection,
};

pub(in crate::orchestrator) fn operator_history_lanes(
	current_lanes: &[OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
) -> Vec<OperatorHistoryLaneStatus> {
	let current_lane_run_ids =
		current_lanes.iter().map(|run| run.run_id.as_str()).collect::<HashSet<_>>();
	let current_lane_issue_ids =
		current_lanes.iter().map(|run| run.issue_id.as_str()).collect::<HashSet<_>>();
	let mut lane_indexes = HashMap::new();
	let mut lanes = Vec::new();

	for run in recent_runs {
		if current_lane_run_ids.contains(run.run_id.as_str())
			|| current_lane_issue_ids.contains(run.issue_id.as_str())
		{
			continue;
		}

		let group_key = status_run_projection::operator_run_group_key(run);

		if let Some(index) = lane_indexes.get(&group_key) {
			let lane: &mut OperatorHistoryLaneStatus = &mut lanes[*index];

			lane.attempt_count += 1;

			if run.attempt_number > lane.latest_run.attempt_number {
				lane.latest_run = run.clone();
			}

			hydrate_history_lane_from_run(lane, run);

			lane.attempts.push(run.clone());

			lane.lifecycle_metrics =
				status_run_projection::operator_lane_lifecycle_metrics(&lane.attempts);

			continue;
		}

		lane_indexes.insert(group_key, lanes.len());

		let attempts = vec![run.clone()];
		let lifecycle_metrics = status_run_projection::operator_lane_lifecycle_metrics(&attempts);

		lanes.push(OperatorHistoryLaneStatus {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			title: run.title.clone(),
			author: run.author.clone(),
			issue_state: None,
			active_label_present: None,
			needs_attention_label_present: None,
			issue_key: status_run_projection::operator_run_issue_key(run),
			attempt_count: 1,
			ledger_outcome: orchestrator::not_loaded_history_ledger_outcome(),
			lifecycle_metrics,
			latest_run: run.clone(),
			attempts,
		});
	}

	lanes
}

pub(in crate::orchestrator) fn hydrate_history_lane_from_run(
	lane: &mut OperatorHistoryLaneStatus,
	run: &OperatorRunStatus,
) {
	if lane.issue_identifier.is_none()
		&& let Some(issue_identifier) =
			run.issue_identifier.as_ref().filter(|value| !value.trim().is_empty())
	{
		lane.issue_identifier = Some(issue_identifier.clone());
		lane.issue_key = issue_identifier.clone();
	}
	if lane.title.is_none() {
		lane.title = run.title.clone();
	}
	if lane.author.is_none() {
		lane.author = run.author.clone();
	}
}
