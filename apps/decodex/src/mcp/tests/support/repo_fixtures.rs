use std::{
	fs,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::test_support::TestEnvVarGuard;

pub(in crate::mcp::tests) fn test_repo() -> TempDir {
	let repo = TempDir::new().expect("temp repo should exist");

	write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
	write_file(repo.path().join("docs/index.md"), "# Docs\n");
	write_file(repo.path().join("docs/policy.md"), "# Policy\n");
	write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n\nSpec body.\n");
	write_file(repo.path().join("docs/decisions/mcp-gateway.md"), "# MCP\n");

	repo
}

pub(in crate::mcp::tests) fn isolated_mcp_runtime_home(repo: &TempDir) -> TestEnvVarGuard {
	let runtime_home = repo.path().join("operator-home");
	let runtime_home = runtime_home.to_string_lossy().into_owned();

	TestEnvVarGuard::set_many([
		("CODEX_HOME".to_owned(), runtime_home.clone()),
		("HOME".to_owned(), runtime_home),
	])
}

pub(in crate::mcp::tests) fn write_project_config(config_path: &Path, repo_root: &Path) {
	write_file(
		config_path.to_path_buf(),
		&format!(
			r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[paths]
repo_root = "{}"
"#,
			repo_root.display()
		),
	);
}

pub(in crate::mcp::tests) fn write_project_workflow(repo_root: &Path) {
	write_file(
		repo_root.join("WORKFLOW.md"),
		r#"
+++
version = 1
max_turns = 1

[tracker]
queued_state = "Todo"
in_progress_state = "In Progress"
success_state = "Done"
terminal_states = ["Done", "Canceled"]

[tools]
comment = "issue_comment"
transition = "issue_transition"
label = "issue_label"
progress_checkpoint = "issue_progress_checkpoint"
review_checkpoint = "issue_review_checkpoint"
review_handoff = "issue_review_handoff"
terminal_finalize = "issue_terminal_finalize"
+++
"#,
	);
}

pub(in crate::mcp::tests) fn write_decodex_project_config(config_path: &Path, repo_root: &Path) {
	write_file(
		config_path.to_path_buf(),
		&format!(
			r#"
service_id = "decodex"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[codex]
review = "standard"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
			repo_root.display()
		),
	);
}

pub(in crate::mcp::tests) fn write_decodex_workflow(repo_root: &Path) {
	write_file(
		repo_root.join("WORKFLOW.md"),
		r#"+++
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
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
"#,
	);
}

pub(in crate::mcp::tests) fn write_file(path: PathBuf, contents: &str) {
	let parent = path.parent().expect("test path should have parent");

	fs::create_dir_all(parent).expect("parent directory should exist");
	fs::write(path, contents).expect("test file should write");
}
