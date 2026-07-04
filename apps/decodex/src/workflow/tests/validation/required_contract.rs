use crate::workflow::{
	WorkflowDocument,
	tests::{shared, shared::Edit},
};

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
