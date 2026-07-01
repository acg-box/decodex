use std::collections::HashSet;

use crate::{
	orchestrator::{OperatorRunStatus, OperatorStatusSnapshot},
	prelude::{Result, eyre},
};

pub(super) fn select_interrupt_lane_run(
	snapshot: &OperatorStatusSnapshot,
	issue: &str,
	run_id: &str,
) -> Result<OperatorRunStatus> {
	let runs = matching_lane_runs(snapshot, issue, Some(run_id));

	if runs.is_empty() {
		eyre::bail!(
			"No local lane matched issue `{issue}` and run `{run_id}` in project `{}`.",
			snapshot.project_id
		);
	}

	Ok(runs[0].clone())
}

fn matching_lane_runs(
	snapshot: &OperatorStatusSnapshot,
	issue: &str,
	run_id: Option<&str>,
) -> Vec<OperatorRunStatus> {
	let mut seen_run_ids = HashSet::new();
	let mut runs = Vec::new();

	for run in snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()) {
		if !seen_run_ids.insert(run.run_id.clone()) {
			continue;
		}
		if !lane_issue_matches(run, issue) {
			continue;
		}
		if run_id.is_some_and(|expected| expected != run.run_id) {
			continue;
		}

		runs.push(run.clone());
	}

	runs
}

fn lane_issue_matches(run: &OperatorRunStatus, issue: &str) -> bool {
	let issue = issue.trim();

	run.issue_id == issue
		|| run.issue_identifier.as_deref() == Some(issue)
		|| run
			.issue_identifier
			.as_ref()
			.is_some_and(|identifier| identifier.eq_ignore_ascii_case(issue))
}
