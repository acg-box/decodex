use crate::{
	config::ServiceConfig,
	orchestrator::{OperatorRunStatus, status_ghost_lane_evidence},
	prelude::Result,
	state::StateStore,
};

pub(in crate::orchestrator::status::ghost_lane_cleanup::projection::inspection) fn inspect_status_ghost_lane_private_evidence(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
	)?;

	if events.is_empty() {
		conditions.push(String::from("private_evidence_missing"));
	} else if mcp_test_fixture {
		conditions.push(String::from("mcp_test_fixture_private_control_evidence_present"));

		if events.iter().any(status_ghost_lane_evidence::private_event_is_cleanup_audit) {
			conditions.push(String::from("ghost_lane_cleanup_audit_present"));
		}
	} else if status_ghost_lane_evidence::private_events_are_cleanup_audit_evidence(&events) {
		conditions.push(String::from("ghost_lane_cleanup_audit_present"));
	} else {
		blockers.push(String::from("private_evidence_present"));
	}

	Ok(())
}
