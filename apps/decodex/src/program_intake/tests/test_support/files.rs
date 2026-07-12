use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::program_intake::tests::test_support::workflow;

pub(crate) fn test_config() -> crate::config::ServiceConfig {
	crate::config::ServiceConfig::parse_toml(
		r#"
service_id = "decodex"
[tracker]
api_key_env_var = "HOME"
team_id = "team-test"
[github]
token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
[codex]
review = "standard"
[paths]
repo_root = "."
worktree_root = ".worktrees"
"#,
	)
	.expect("config should parse")
}

pub(crate) fn write_project_files(project_dir: &Path) -> PathBuf {
	fs::write(project_dir.join("WORKFLOW.md"), workflow::workflow_markdown())
		.expect("workflow should write");
	fs::write(
		project_dir.join("project.toml"),
		r#"
service_id = "decodex"
[tracker]
api_key_env_var = "HOME"
team_id = "team-test"
[github]
token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
[codex]
review = "standard"
[paths]
repo_root = "."
worktree_root = ".worktrees"
"#,
	)
	.expect("project config should write");

	project_dir.join("project.toml")
}
