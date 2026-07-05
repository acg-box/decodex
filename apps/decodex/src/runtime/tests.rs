mod account_pool_path_lives_under_decodex_home;
mod agent_evidence_path_lives_under_decodex_home;
mod global_fixed_account_selector_round_trips_global_config;
mod project_config_refresh_preserves_disabled_state;
mod project_config_registration_requires_explicit_repo_root;
mod registered_config_path_for_cwd_matches_repo_and_worktree_roots;
mod registered_config_path_for_project_id_uses_service_id;
mod runtime_paths_live_under_codex_decodex_home;

use std::{fs, path::Path};

use crate::test_support::TestEnvVarGuard;

fn set_test_home(path: &Path) -> TestEnvVarGuard {
	TestEnvVarGuard::set("HOME", path.to_str().expect("test home should be UTF-8"))
}

fn write_config_body(config_path: &Path, repo_root: &Path) {
	fs::write(
		config_path,
		format!(
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
	)
	.expect("config should write");
}

fn write_workflow(config_dir: &Path) {
	fs::write(
		config_dir.join("WORKFLOW.md"),
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

Follow the project policy.
"#,
	)
	.expect("workflow should write");
}

fn write_config_without_repo_root(config_path: &Path) {
	fs::write(
		config_path,
		r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"
"#,
	)
	.expect("config should write");
}
