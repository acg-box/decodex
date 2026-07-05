use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, IssueDispatchMode,
		tests::{self},
	},
	workflow::WorkflowDocument,
};

#[test]
fn review_repair_continuation_prompt_uses_configured_success_state() {
	let workflow = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "Ready For QA"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 4
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

Custom workflow.
"#,
	)
	.expect("workflow should parse");
	let issue = tests::sample_issue("Ready For QA", &[]);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::ReviewRepair,
		Some("https://github.com/hack-ink/decodex/pull/77"),
		workflow.frontmatter().tracker().success_state(),
		ReviewLevel::Standard,
	);

	assert!(continuation_input.contains("Ready For QA"));
	assert!(!continuation_input.contains("do not move the issue out of `In Review`"));
}
