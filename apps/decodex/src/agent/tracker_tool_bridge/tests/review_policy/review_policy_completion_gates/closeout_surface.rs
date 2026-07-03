use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, ISSUE_TRANSITION_TOOL_NAME, TempDir, TrackerState, TrackerToolBridge,
	WorkflowDocument,
};

#[test]
fn closeout_tool_surface_includes_issue_transition_for_completed_state() {
	let mut issue = tests::sample_review_issue();

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Use the tracker tools.
"#,
	)
	.expect("workflow should parse");
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let bridge = TrackerToolBridge::with_run_context(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let transition_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "Done" }),
	);
	let invalid_transition_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Progress" }),
	);

	assert!(tool_names.contains(&String::from(ISSUE_TRANSITION_TOOL_NAME)));
	assert!(transition_response.success);
	assert!(!invalid_transition_response.success);
	assert_eq!(tracker.state_updates.borrow().as_slice(), [String::from("state-done")]);
}
