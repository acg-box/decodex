use std::collections::BTreeSet;

use crate::workflow::{WorkflowDocument, WorkflowGateMatchMode, tests::shared};

#[test]
fn parses_named_gate_profile() {
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
	.expect("workflow with gate profile should parse");
	let profile = document
		.frontmatter()
		.execution()
		.gate_profiles()
		.get("config_subset")
		.expect("config_subset profile should exist");

	assert_eq!(profile.match_mode(), WorkflowGateMatchMode::Only);
	assert_eq!(profile.paths(), ["config/**"]);
	assert_eq!(profile.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}

#[test]
fn selects_matching_gate_profile_when_all_changed_files_match_profile_paths() {
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
	let changed_files = ["config/base.toml", "config/service.toml"]
		.into_iter()
		.map(str::to_owned)
		.collect::<BTreeSet<_>>();
	let selection =
		document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

	assert_eq!(selection.profile_name(), Some("config_subset"));
	assert!(selection.canonicalize_commands().is_empty());
	assert_eq!(selection.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}

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

#[test]
fn falls_back_to_full_gate_for_ambiguous_profile_matches() {
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

[execution.gate_profiles.config_prod]
match_mode = "only"
paths = ["config/prod.toml"]
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
	let changed_files =
		["config/prod.toml"].into_iter().map(str::to_owned).collect::<BTreeSet<_>>();
	let selection =
		document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.verify_commands(), ["cargo make test"]);
}

#[test]
fn rejects_incomplete_gate_profiles() {
	for (case_name, paths, commands, expected) in [
		(
			"missing paths",
			"[]",
			r#"verify_commands = ["python3 -c 'print(\"ok\")'"]"#,
			"`execution.gate_profiles.config_subset.paths` must not be empty",
		),
		(
			"missing commands",
			r#"["config/**"]"#,
			"verify_commands = []",
			"`execution.gate_profiles.config_subset` must declare at least one canonicalize or verify command",
		),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(
				r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
				&format!(
					r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = {paths}
canonicalize_commands = []
{commands}

"#,
				),
			);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn rejects_gate_profile_paths_that_escape_repo() {
	for (path, expected) in [
		("../config/**", "must not contain `.`, `..`, root, or prefix components"),
		("/tmp/config/**", "must be repository-relative paths"),
	] {
		let result = shared::parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(
				r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
				&format!(
					r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["{path}"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#
				),
			);
		});
		let error = result.expect_err("escaping gate profile path should fail");

		assert!(error.to_string().contains(expected), "unexpected error for `{path}`: {error:?}");
	}
}
