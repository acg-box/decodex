use std::collections::BTreeSet;

use crate::workflow::WorkflowDocument;

#[test]
fn falls_back_to_full_gate_for_mixed_docs_and_runtime_changes() {
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
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

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
			"#,
	)
	.expect("workflow should parse");
	let changed_files = ["config/base.toml", "src/orchestrator/git_ops.rs"]
		.into_iter()
		.map(str::to_owned)
		.collect::<BTreeSet<_>>();
	let selection =
		document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.canonicalize_commands(), ["cargo make fmt"]);
	assert_eq!(selection.verify_commands(), ["cargo make test"]);
}
