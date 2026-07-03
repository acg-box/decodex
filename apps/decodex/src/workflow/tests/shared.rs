use crate::{prelude::Result, workflow::WorkflowDocument};

pub(super) enum Edit<'a> {
	Remove(&'a str),
	Replace(&'a str, &'a str),
}
impl Edit<'_> {
	pub(super) fn apply(&self, markdown: &mut String) {
		match self {
			Self::Remove(needle) => *markdown = markdown.replace(needle, ""),
			Self::Replace(needle, replacement) => {
				*markdown = markdown.replace(needle, replacement);
			},
		}
	}
}

pub(super) fn parse_valid_workflow_with(
	rewrite: impl FnOnce(&mut String),
) -> Result<WorkflowDocument> {
	let mut markdown = valid_workflow_markdown();

	rewrite(&mut markdown);

	WorkflowDocument::parse_markdown(&markdown)
}

pub(super) fn valid_workflow_markdown() -> String {
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
		"#
	.to_string()
}
