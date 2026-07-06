use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING, OperatorRunStatus,
		status_ghost_lane_cleanup::{conditions, projection, tracker_issue},
		status_run_projection,
	},
	prelude::Result,
	state::StateStore,
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

pub(crate) fn ghost_lane_cleanup_status_blockers<T>(
	tracker: &T,
	project: &ServiceConfig,
	_workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> Result<Vec<String>>
where
	T: IssueTracker,
{
	let Some(mut run) = ghost_lane_cleanup_status_run(project, state_store, issue_id, run_id)?
	else {
		return Ok(vec![String::from("status_current_lane_missing")]);
	};

	if let Some(issue) = tracker_issue::ghost_lane_tracker_issue(tracker, &run)? {
		return Ok(vec![
			String::from("tracker_issue_present"),
			format!("issue_state:{}", issue.state.name),
		]);
	}

	conditions::append_lane_control_condition(&mut run, GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING);
	projection::apply_missing_issue_ghost_lane_status_projection(project, state_store, &mut run)?;

	if conditions::missing_issue_ghost_lane_status_allows_cleanup(&run)
		|| conditions::missing_issue_ghost_lane_status_is_cleanup_complete(&run)
	{
		return Ok(Vec::new());
	}

	let mut blockers = vec![
		format!("ownership_state:{}", run.ownership_state),
		format!("policy_state:{}", run.policy_state),
		format!("lane_control_next_action:{}", run.lane_control_next_action),
	];

	blockers.extend(run.lane_control_conditions.iter().cloned());
	blockers.sort();
	blockers.dedup();

	Ok(blockers)
}

fn ghost_lane_cleanup_status_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> Result<Option<OperatorRunStatus>> {
	let (leased_runs, _) = state_store.list_project_runs_read_only(project.service_id(), 0)?;
	let Some(run) =
		leased_runs.into_iter().find(|run| run.issue_id() == issue_id && run.run_id() == run_id)
	else {
		return Ok(None);
	};
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = orchestrator::operator_project_display_name(project);
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	Ok(Some(status_run_projection::operator_run_status(
		project,
		&loop_evidence,
		&project_display_name,
		run,
		now_unix_epoch,
	)?))
}
