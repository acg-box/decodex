use crate::workflow::{TrackerProvider, WorkflowDocument, tests::shared};

#[test]
fn parses_workflow_document() {
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
max_turns = 4
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Start with the repo's documented routing entrypoint when one exists.
Use `cargo make`.
			"#,
	)
	.expect("workflow document should parse");

	assert_eq!(document.frontmatter().version(), 1);
	assert_eq!(document.frontmatter().tracker().provider(), TrackerProvider::Linear);
	assert_eq!(document.frontmatter().tracker().completed_state(), "Done");
	assert_eq!(document.frontmatter().execution().max_attempts(), 3);
	assert_eq!(document.frontmatter().execution().max_turns(), 4);
	assert_eq!(document.frontmatter().execution().max_retry_backoff_ms(), 300_000);
	assert_eq!(document.frontmatter().execution().canonicalize_commands(), ["cargo make fmt"]);
	assert_eq!(document.frontmatter().execution().verify_commands(), ["cargo make test"]);
	assert_eq!(
		document.body(),
		"Start with the repo's documented routing entrypoint when one exists.\nUse `cargo make`."
	);
}

#[test]
fn parses_workspace_hooks() {
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
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = ["./scripts/bootstrap-worktree.sh"]
before_remove_commands = ["./scripts/cleanup-worktree.sh"]
timeout_seconds = 45

[context]
read_first = []
+++
			"#,
	)
	.expect("workflow with workspace hooks should parse");
	let hooks = document.frontmatter().execution().workspace_hooks();

	assert_eq!(hooks.after_create_commands(), ["./scripts/bootstrap-worktree.sh"]);
	assert_eq!(hooks.before_remove_commands(), ["./scripts/cleanup-worktree.sh"]);
	assert_eq!(hooks.timeout_seconds(), 45);
}

#[test]
fn rejects_invalid_workspace_hook_config() {
	for (case_name, needle, replacement, expected) in [
		(
			"zero timeout",
			"timeout_seconds = 60",
			"timeout_seconds = 0",
			"`execution.workspace_hooks.timeout_seconds` must be greater than zero",
		),
		(
			"surrounding whitespace",
			"after_create_commands = []",
			r#"after_create_commands = ["  ./scripts/bootstrap-worktree.sh  "]"#,
			"`execution.workspace_hooks.after_create_commands` entries must not include surrounding whitespace",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(needle, replacement);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
