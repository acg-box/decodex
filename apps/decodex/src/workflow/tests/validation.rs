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

#[test]
fn rejects_unknown_workflow_fields() {
	for (case_name, edit, field) in [
		(
			"nested tracker field",
			Edit::Replace(
				"needs_attention_label = \"decodex:needs-attention\"",
				"needs_attention_label = \"decodex:needs-attention\"\nunexpected_tracker_key = \"pubfi\"",
			),
			"unexpected_tracker_key",
		),
		(
			"execution field",
			Edit::Replace(
				"verify_commands = []",
				"verify_commands = []\nunexpected_execution_field = [\"cargo make test\"]",
			),
			"unexpected_execution_field",
		),
		(
			"top-level table",
			Edit::Replace(
				"[context]\nread_first = []",
				"[context]\nread_first = []\n\n[unexpected]\nenabled = true",
			),
			"unexpected",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| edit.apply(markdown));
		let error = result.expect_err(case_name);

		assert!(error.to_string().contains(&format!("unknown field `{field}`")));
	}
}

#[test]
fn rejects_missing_frontmatter() {
	let result = WorkflowDocument::parse_markdown("Read the repo policy first.");

	assert!(result.is_err());
}

#[test]
fn rejects_missing_or_empty_required_workflow_contract() {
	for (case_name, edit, expected) in [
		(
			"missing agent block",
			Edit::Remove(
				r#"[agent]
transport = "stdio://"

"#,
			),
			"agent",
		),
		("missing max_attempts", Edit::Remove("max_attempts = 3\n"), "max_attempts"),
		(
			"empty terminal states",
			Edit::Replace(
				r#"terminal_states = ["Done", "Canceled", "Duplicate"]"#,
				"terminal_states = []",
			),
			"`tracker.terminal_states` must not be empty",
		),
		(
			"blank agent transport",
			Edit::Replace(r#"transport = "stdio://""#, r#"transport = """#),
			"`agent.transport` must not be empty",
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

#[test]
fn rejects_missing_required_workflow_sections_and_fields() {
	for (needle, expected) in [
		("gate_profiles = {}\n", "gate_profiles"),
		(
			r#"[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

"#,
			"workspace_hooks",
		),
		(
			r#"[context]
read_first = []
"#,
			"context",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(needle, "");
		});
		let error = result.expect_err("missing required workflow sections should fail");

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{expected}`: {error:?}"
		);
	}
}

#[test]
fn rejects_invalid_gate_command_entries() {
	for (case_name, edit, expected) in [
		(
			"blank canonicalize command",
			Edit::Replace("canonicalize_commands = []", "canonicalize_commands = [\"\"]"),
			"`execution.canonicalize_commands` entries",
		),
		(
			"untrimmed verify command",
			Edit::Replace("verify_commands = []", "verify_commands = [\"  cargo make test  \"]"),
			"`execution.verify_commands` entries",
		),
		(
			"blank profile canonicalize command",
			Edit::Replace(
				r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
				r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = [" "]
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#,
			),
			"`execution.gate_profiles.config_subset.canonicalize_commands` entries",
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

#[test]
fn rejects_invalid_context_read_first_entries() {
	for (case_name, replacement, expected) in [
		(
			"blank read_first entry",
			"read_first = [\"\"]",
			"`context.read_first` entries must not be empty",
		),
		(
			"parent traversal read_first path",
			"read_first = [\"../secret.md\"]",
			"must not contain `.`, `..`, root, or prefix components",
		),
		(
			"absolute read_first path",
			"read_first = [\"/tmp/secret.md\"]",
			"must be repository-relative paths",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace("read_first = []", replacement);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
