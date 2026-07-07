use crate::{
	prelude::Result,
	recovery::identifiers,
	state::ProjectRunStatus,
	tracker::{self, IssueTracker},
};

pub(super) fn inspect_ghost_lane_tracker_issue<T>(
	tracker: &T,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let refreshed = match tracker.refresh_issues(&[run.issue_id().to_owned()]) {
		Ok(refreshed) => refreshed,
		Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, run.issue_id()) => {
			Vec::new()
		},
		Err(error) => return Err(error),
	};

	if !refreshed.is_empty() {
		blockers.push(String::from("tracker_issue_present"));

		return Ok(());
	}

	for selector in
		identifiers::ghost_lane_tracker_issue_selectors(run, issue_identifier, requested_selector)
	{
		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(_)) => {
				blockers.push(String::from("tracker_issue_present"));

				return Ok(());
			},
			Ok(None) => {},
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
			},
			Err(error) => return Err(error),
		}
	}

	evidence.push(String::from("tracker_issue_missing"));

	Ok(())
}
