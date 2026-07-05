use std::path::Path;

use crate::orchestrator::OperatorRunStatus;

pub(in crate::orchestrator::status_ghost_lane_cleanup::projection::inspection) fn inspect_status_ghost_lane_control_channel(
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let Some(control_capability) = run.control_capability.as_ref() else {
		conditions.push(String::from("control_channel_missing"));

		return;
	};

	if Path::new(&control_capability.channel_path).exists() {
		conditions.push(String::from("control_channel_file_present"));
		blockers.push(String::from("control_channel_present"));
	} else {
		conditions.push(String::from("control_channel_file_missing"));

		if mcp_test_fixture {
			conditions.push(String::from("mcp_test_fixture_control_channel_row_present"));
		} else {
			blockers.push(String::from("control_channel_present"));
		}
	}
}
