use std::fs;

use tempfile::NamedTempFile;

use crate::workflow::WorkflowDocument;

#[test]
fn loads_workflow_document_from_path() {
	let file = NamedTempFile::new().expect("temp file should exist");

	fs::write(
		file.path(),
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

Read the repo policy first.
			"#,
	)
	.expect("workflow document should be written");

	let document =
		WorkflowDocument::from_path(file.path()).expect("workflow should load from path");

	assert_eq!(document.frontmatter().tracker().completed_state(), "Done");
}
