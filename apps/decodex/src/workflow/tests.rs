use std::{collections::BTreeSet, fs};

use Edit::{Remove, Replace};
use tempfile::NamedTempFile;

use crate::{
	prelude::Result,
	workflow::{TrackerProvider, WorkflowDocument, WorkflowGateMatchMode},
};

enum Edit<'a> {
	Remove(&'a str),
	Replace(&'a str, &'a str),
}
impl Edit<'_> {
	fn apply(&self, markdown: &mut String) {
		match self {
			Self::Remove(needle) => *markdown = markdown.replace(needle, ""),
			Self::Replace(needle, replacement) => {
				*markdown = markdown.replace(needle, replacement);
			},
		}
	}
}

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
		let result = parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace(needle, replacement);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

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
		let result = parse_valid_workflow_with(|markdown| {
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
		let result = parse_valid_workflow_with(|markdown| {
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

#[test]
fn loads_workflow_document_from_path() {
	let file = NamedTempFile::new().expect("temp file should exist");

	fs::write(
		file.path(),
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
			"#,
	)
	.expect("workflow document should be written");

	let document =
		WorkflowDocument::from_path(file.path()).expect("workflow should load from path");

	assert_eq!(document.frontmatter().tracker().completed_state(), "Done");
}

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
		("missing completed_state", Remove("completed_state = \"Done\"\n"), "completed_state"),
		(
			"completed_state outside terminal_states",
			Replace(
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
		let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
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
			Replace(
				"needs_attention_label = \"decodex:needs-attention\"",
				"needs_attention_label = \"decodex:needs-attention\"\nunexpected_tracker_key = \"pubfi\"",
			),
			"unexpected_tracker_key",
		),
		(
			"execution field",
			Replace(
				"verify_commands = []",
				"verify_commands = []\nunexpected_execution_field = [\"cargo make test\"]",
			),
			"unexpected_execution_field",
		),
		(
			"top-level table",
			Replace(
				"[context]\nread_first = []",
				"[context]\nread_first = []\n\n[unexpected]\nenabled = true",
			),
			"unexpected",
		),
	] {
		let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
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
			Remove(
				r#"[agent]
transport = "stdio://"

"#,
			),
			"agent",
		),
		("missing max_attempts", Remove("max_attempts = 3\n"), "max_attempts"),
		(
			"empty terminal states",
			Replace(
				r#"terminal_states = ["Done", "Canceled", "Duplicate"]"#,
				"terminal_states = []",
			),
			"`tracker.terminal_states` must not be empty",
		),
		(
			"blank agent transport",
			Replace(r#"transport = "stdio://""#, r#"transport = """#),
			"`agent.transport` must not be empty",
		),
	] {
		let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
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
		let result = parse_valid_workflow_with(|markdown| {
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
			Replace("canonicalize_commands = []", "canonicalize_commands = [\"\"]"),
			"`execution.canonicalize_commands` entries",
		),
		(
			"untrimmed verify command",
			Replace("verify_commands = []", "verify_commands = [\"  cargo make test  \"]"),
			"`execution.verify_commands` entries",
		),
		(
			"blank profile canonicalize command",
			Replace(
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
		let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
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
		let result = parse_valid_workflow_with(|markdown| {
			*markdown = markdown.replace("read_first = []", replacement);
		});
		let error = result.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn workflow_document_markdown_round_trips() {
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
max_attempts = 5
max_turns = 6
max_retry_backoff_ms = 120000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = ["docs/index.md", "README.md"]
+++

Read the repo policy first.
Then validate the lane.
			"#,
	)
	.expect("workflow document should parse");
	let reparsed = WorkflowDocument::parse_markdown(
		&document.to_markdown().expect("workflow markdown should render"),
	)
	.expect("rendered workflow should parse");

	assert_eq!(reparsed, document);
}

fn parse_valid_workflow_with(rewrite: impl FnOnce(&mut String)) -> Result<WorkflowDocument> {
	let mut markdown = valid_workflow_markdown();

	rewrite(&mut markdown);

	WorkflowDocument::parse_markdown(&markdown)
}

fn valid_workflow_markdown() -> String {
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
