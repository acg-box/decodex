use crate::workflow::{
	WorkflowDocument,
	tests::{shared, shared::Edit},
};

#[test]
fn parses_explicit_completed_state() {
	let document = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Released", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Released"
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
	.expect("workflow document should parse");

	assert_eq!(document.frontmatter().tracker().completed_state(), "Released");
	assert_eq!(document.frontmatter().tracker().resolved_completed_state(), "Released");
}

#[test]
fn rejects_invalid_completed_state_contract() {
	for (case_name, edit, expected) in [
		(
			"missing completed_state",
			Edit::Remove("completed_state = \"Done\"\n"),
			"completed_state",
		),
		(
			"completed_state outside terminal_states",
			Edit::Replace(
				r#"terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done""#,
				r#"terminal_states = ["Released", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done""#,
			),
			"`tracker.completed_state` must be one of `tracker.terminal_states`",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| edit.apply(markdown));
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
