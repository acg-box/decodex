use crate::{
	config::ServiceConfig, orchestrator::OperatorRunStatus, prelude::Result, state::StateStore,
	tracker::records::LinearExecutionEventRecord,
};

pub(super) fn inspect_status_ghost_lane_review_lineage(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	if state_store.issue_has_review_lifecycle_record(project.service_id(), &run.issue_id)? {
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if status_run_has_review_policy_checkpoint(project, state_store, run)? {
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let mut records =
		state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

	if let Some(issue_identifier) = run
		.issue_identifier
		.as_deref()
		.filter(|identifier| !identifier.eq_ignore_ascii_case(&run.issue_id))
	{
		records.extend(
			state_store.list_linear_execution_events(project.service_id(), issue_identifier)?,
		);
	}

	if records.iter().any(operator_linear_execution_event_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		conditions.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn status_run_has_review_policy_checkpoint(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> Result<bool> {
	for phase in ["handoff", "repair"] {
		if state_store
			.review_policy_checkpoint(
				project.service_id(),
				&run.issue_id,
				&run.run_id,
				run.attempt_number,
				phase,
			)?
			.is_some()
		{
			return Ok(true);
		}
	}

	Ok(false)
}

fn operator_linear_execution_event_has_pr_or_review_lineage(
	record: &LinearExecutionEventRecord,
) -> bool {
	record.pr_url.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_head_sha.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_base_ref.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| matches!(
			record.event_type.as_str(),
			"review_handoff"
				| "review_handoff_rebind"
				| "review_handoff_adopt"
				| "review_repair"
				| "landed" | "closeout"
				| "cleanup_complete"
		) || record.terminal_path.as_deref() == Some("review_handoff")
}
