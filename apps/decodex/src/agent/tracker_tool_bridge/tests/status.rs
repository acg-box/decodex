use crate::agent::tracker_tool_bridge;

#[test]
fn review_blocking_status_keeps_source_changes_and_ignores_runtime_markers() {
	assert_eq!(
		tracker_tool_bridge::review_blocking_status_lines(
			" M apps/decodex/src/agent/tracker_tool_bridge.rs\n\
			 ?? apps/decodex/src/agent/new_file.rs\n\
			 ?? .decodex-run-activity\n\
			 ?? .decodex-run-control/run-1.channel\n"
		),
		vec![
			String::from("M apps/decodex/src/agent/tracker_tool_bridge.rs"),
			String::from("?? apps/decodex/src/agent/new_file.rs"),
		]
	);
}
