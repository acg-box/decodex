use crate::{
	prelude::Result,
	recovery::evidence,
	state::{ProjectRunStatus, StateStore},
};

pub(super) fn inspect_ghost_lane_review_lineage(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	if state_store.issue_has_review_lifecycle_record(project_id, run.issue_id())? {
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if ghost_lane_run_has_review_policy_checkpoint(project_id, state_store, run)? {
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let mut records = state_store.list_linear_execution_events(project_id, run.issue_id())?;

	if let Some(issue_identifier) = issue_identifier
		.filter(|issue_identifier| !issue_identifier.eq_ignore_ascii_case(run.issue_id()))
	{
		records.extend(state_store.list_linear_execution_events(project_id, issue_identifier)?);
	}

	if records.iter().any(evidence::ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		evidence.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn ghost_lane_run_has_review_policy_checkpoint(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
) -> Result<bool> {
	for phase in ["handoff", "repair"] {
		if state_store
			.review_policy_checkpoint(
				project_id,
				run.issue_id(),
				run.run_id(),
				run.attempt_number(),
				phase,
			)?
			.is_some()
		{
			return Ok(true);
		}
	}

	Ok(false)
}
