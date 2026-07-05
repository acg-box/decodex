pub(in crate::orchestrator::tests) fn profile_scoped_workflow_markdown(
	project_slug: &str,
) -> String {
	let _ = project_slug;
	let markdown = r#"
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
canonicalize_commands = ["cargo make fmt", "cargo make lint-fix"]
verify_commands = ["cargo make check"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
"#;

	markdown.to_string()
}

pub(in crate::orchestrator::tests) fn sample_workflow_markdown(
	_project_slug: &str,
	read_first: &[&str],
	workflow_body: &str,
	max_turns: u32,
) -> String {
	let read_first =
		read_first.iter().map(|path| format!("\"{path}\"")).collect::<Vec<_>>().join(", ");
	let context = format!("[context]\nread_first = [{read_first}]");
	let markdown = format!(
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
max_turns = {max_turns}
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

{context}
+++

{workflow_body}"#
	);

	markdown
}
