use std::slice;

use crate::{
	orchestrator::{
		self, GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING, OperatorRunStatus,
		OperatorStatusSnapshot, status_ghost_lane_cleanup::conditions,
	},
	prelude::Result,
	tracker::{self, IssueTracker, TrackerIssue},
};

pub(crate) fn mark_operator_run_tracker_issue_missing(
	snapshot: &mut OperatorStatusSnapshot,
	run_id: &str,
	issue_id: &str,
	selector: &str,
) {
	for run in snapshot.current_lanes.iter_mut().chain(snapshot.recent_runs.iter_mut()) {
		if run.run_id == run_id || run.issue_id == issue_id {
			if run.issue_identifier.is_none() {
				run.issue_identifier = Some(selector.to_owned());
			}

			conditions::append_lane_control_condition(
				run,
				GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING,
			);
		}
	}
}

pub(crate) fn ghost_lane_tracker_issue<T>(
	tracker: &T,
	run: &OperatorRunStatus,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	if !run.issue_id.trim().is_empty() && !run.issue_id.eq_ignore_ascii_case("unknown") {
		match tracker.refresh_issues(slice::from_ref(&run.issue_id)) {
			Ok(issues) => {
				if let Some(issue) = issues.into_iter().next() {
					return Ok(Some(issue));
				}
			},
			Err(error)
				if tracker::issue_lookup_missing_error_for_candidate(&error, &run.issue_id) => {},
			Err(error) => return Err(error),
		}
	}

	let Some(selector) = orchestrator::operator_run_tracker_issue_identifier_selector(run) else {
		return Ok(None);
	};

	match tracker.get_issue_by_identifier(&selector) {
		Ok(issue) => Ok(issue),
		Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
			Ok(None)
		},
		Err(error) => Err(error),
	}
}
