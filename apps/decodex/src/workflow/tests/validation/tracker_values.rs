use crate::workflow::WorkflowDocument;

#[test]
fn rejects_blank_required_tracker_policy_values() {
	for (
		field,
		in_progress_state,
		success_state,
		failure_state,
		opt_out_label,
		needs_attention_label,
	) in [
		(
			"in_progress_state",
			"\"\"",
			"\"In Review\"",
			"\"Todo\"",
			"\"decodex:manual-only\"",
			"\"decodex:needs-attention\"",
		),
		(
			"success_state",
			"\"In Progress\"",
			"\"\"",
			"\"Todo\"",
			"\"decodex:manual-only\"",
			"\"decodex:needs-attention\"",
		),
		(
			"failure_state",
			"\"In Progress\"",
			"\"In Review\"",
			"\"\"",
			"\"decodex:manual-only\"",
			"\"decodex:needs-attention\"",
		),
		(
			"opt_out_label",
			"\"In Progress\"",
			"\"In Review\"",
			"\"Todo\"",
			"\"\"",
			"\"decodex:needs-attention\"",
		),
		(
			"needs_attention_label",
			"\"In Progress\"",
			"\"In Review\"",
			"\"Todo\"",
			"\"decodex:manual-only\"",
			"\"\"",
		),
	] {
		let result = WorkflowDocument::parse_markdown(&format!(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = {in_progress_state}
success_state = {success_state}
completed_state = "Done"
failure_state = {failure_state}
opt_out_label = {opt_out_label}
needs_attention_label = {needs_attention_label}

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
				"#,
		));

		assert!(
			result
				.expect_err("blank required tracker value should fail")
				.to_string()
				.contains(&format!("`tracker.{field}` must not be empty"))
		);
	}
}

#[test]
fn rejects_blank_required_policy_entries() {
	for (field, startable_states, terminal_states) in [
		("startable_states", "[\"\"]", "[\"Done\", \"Canceled\", \"Duplicate\"]"),
		("terminal_states", "[\"Todo\"]", "[\"Done\", \"\"]"),
	] {
		let result = WorkflowDocument::parse_markdown(&format!(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = {startable_states}
terminal_states = {terminal_states}
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
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
				"#,
		));

		assert!(
			result
				.expect_err("blank required tracker entry should fail")
				.to_string()
				.contains(&format!("`tracker.{field} entries` must not be empty"))
		);
	}
}
