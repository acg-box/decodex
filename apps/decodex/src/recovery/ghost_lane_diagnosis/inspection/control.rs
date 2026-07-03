use crate::{recovery::evidence, state::ProjectRunStatus};

pub(super) fn inspect_ghost_lane_control_channel(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	let Some(channel) = run.control_channel() else {
		evidence.push(String::from("control_channel_missing"));

		return String::from("missing");
	};

	if channel.channel_path().exists() {
		evidence.push(String::from("control_channel_file_present"));
		blockers.push(String::from("control_channel_present"));
	} else {
		evidence.push(String::from("control_channel_file_missing"));

		if mcp_test_fixture {
			evidence.push(String::from("mcp_test_fixture_control_channel_row_present"));
		} else {
			blockers.push(String::from("control_channel_present"));
		}
	}

	format!("{}:present", channel.status())
}

pub(super) fn inspect_ghost_lane_live_evidence(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let mut live_blockers = Vec::new();

	if run.event_count() > 0 || run.last_event_type().is_some() || run.last_event_at().is_some() {
		live_blockers.push(String::from("protocol_event_evidence_present"));
	}
	if run.child_agent_activity().is_some() {
		live_blockers.push(String::from("child_agent_activity_present"));
	}
	if run.protocol_activity().is_some() {
		live_blockers.push(String::from("protocol_activity_present"));
	}
	if run.thread_id().is_some() || run.turn_id().is_some() {
		live_blockers.push(String::from("thread_reference_present"));
	}
	if live_blockers.is_empty() {
		evidence.push(String::from("no_live_execution_evidence"));

		return;
	}
	if mcp_test_fixture
		&& live_blockers
			.iter()
			.all(|blocker| evidence::ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker))
	{
		evidence.push(String::from("mcp_test_fixture_protocol_or_thread_evidence_present"));

		return;
	}

	blockers.extend(live_blockers);
}
