use crate::{
	prelude::Result,
	recovery::evidence,
	state::{ProjectRunStatus, StateStore},
};

pub(super) fn inspect_ghost_lane_private_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	if events.is_empty() {
		evidence.push(String::from("private_evidence_missing"));
	} else if mcp_test_fixture {
		evidence.push(String::from("mcp_test_fixture_private_control_evidence_present"));

		if events.iter().any(evidence::ghost_lane_private_event_is_cleanup_audit) {
			evidence.push(String::from("ghost_lane_cleanup_audit_present"));
		}
	} else if evidence::ghost_lane_private_events_are_cleanup_audit_evidence(&events) {
		evidence.push(String::from("ghost_lane_cleanup_audit_present"));
	} else {
		blockers.push(String::from("private_evidence_present"));
	}

	Ok(())
}

pub(super) fn ghost_lane_mcp_test_fixture_control_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> Result<bool> {
	if !evidence::ghost_lane_has_mcp_test_fixture_identity(project_id, run, issue_identifier) {
		return Ok(false);
	}

	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	Ok(evidence::ghost_lane_events_are_mcp_test_recovery_evidence(&events))
}
