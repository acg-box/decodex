use crate::workflow::WorkflowDocument;

pub(crate) fn workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(workflow_markdown()).expect("workflow should parse")
}

pub(super) fn workflow_markdown() -> &'static str {
	r#"+++
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
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60
[context]
read_first = []
+++
"#
}
