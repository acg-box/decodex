use crate::workflow::WorkflowDocument;

#[test]
fn workflow_document_markdown_round_trips() {
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
max_attempts = 5
max_turns = 6
max_retry_backoff_ms = 120000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = ["docs/index.md", "README.md"]
+++

Read the repo policy first.
Then validate the lane.
			"#,
	)
	.expect("workflow document should parse");
	let reparsed = WorkflowDocument::parse_markdown(
		&document.to_markdown().expect("workflow markdown should render"),
	)
	.expect("rendered workflow should parse");

	assert_eq!(reparsed, document);
}
