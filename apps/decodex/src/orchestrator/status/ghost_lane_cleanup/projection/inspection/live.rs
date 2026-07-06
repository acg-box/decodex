use crate::orchestrator::{self, OperatorRunStatus, status_ghost_lane_evidence};

pub(in crate::orchestrator::status::ghost_lane_cleanup::projection::inspection) fn inspect_status_ghost_lane_live_evidence(
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let mut live_blockers = Vec::new();

	if run.process_alive == Some(true) {
		live_blockers.push(String::from("process_alive"));
	}
	if matches!(run.thread_status.as_deref(), Some("active")) || !run.thread_active_flags.is_empty()
	{
		live_blockers.push(String::from("thread_active"));
	}
	if orchestrator::operator_run_has_recent_app_server_execution(run) {
		live_blockers.push(String::from("protocol_recent"));
	}
	if run.event_count > 0 || run.last_event_type.is_some() || run.last_event_at.is_some() {
		live_blockers.push(String::from("protocol_event_evidence_present"));
	}
	if run.child_agent_activity.is_some() {
		live_blockers.push(String::from("child_agent_activity_present"));
	}
	if run.protocol_activity.is_some() {
		live_blockers.push(String::from("protocol_activity_present"));
	}
	if run.thread_id.is_some() || run.turn_id.is_some() {
		live_blockers.push(String::from("thread_reference_present"));
	}
	if live_blockers.is_empty() {
		conditions.push(String::from("no_live_execution_evidence"));

		return;
	}
	if mcp_test_fixture
		&& live_blockers.iter().all(|blocker| {
			status_ghost_lane_evidence::mcp_test_fixture_allowed_live_blocker(blocker)
		}) {
		conditions.push(String::from("mcp_test_fixture_protocol_or_thread_evidence_present"));

		return;
	}

	blockers.extend(live_blockers);
}
