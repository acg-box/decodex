use crate::workflow::{WorkflowDocument, WorkflowGateMatchMode};

#[test]
fn parses_named_gate_profile() {
	let document = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
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
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
			"#,
	)
	.expect("workflow with gate profile should parse");
	let profile = document
		.frontmatter()
		.execution()
		.gate_profiles()
		.get("config_subset")
		.expect("config_subset profile should exist");

	assert_eq!(profile.match_mode(), WorkflowGateMatchMode::Only);
	assert_eq!(profile.paths(), ["config/**"]);
	assert_eq!(profile.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}
