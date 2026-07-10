use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use tempfile::TempDir;

use crate::mcp_stdio::support::TestProject;

pub(crate) fn test_repo() -> TempDir {
	let repo = TempDir::new().expect("temp repo should exist");

	write_file(repo.path().join("Cargo.toml"), "[workspace]\n");

	repo
}

pub(crate) fn test_project() -> TestProject {
	let home = TempDir::new().expect("temp home should exist");
	let project = TempDir::new().expect("temp project should exist");
	let repo_path = project.path().join("repo");
	let project_config_dir = project.path().join("project");
	let config_path = project_config_dir.join("project.toml");

	fs::create_dir_all(repo_path.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(&project_config_dir).expect("project config dir should exist");

	write_file(repo_path.join("README.md"), "test repo\n");
	write_file(
		project_config_dir.join("WORKFLOW.md"),
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
	);
	write_file(
		config_path.clone(),
		&format!(
			r#"service_id = "decodex"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "HOME"

[codex]
review = "standard"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
			repo_path.display()
		),
	);
	git_status_success(&repo_path, &["init", "-b", "main"]);
	git_status_success(&repo_path, &["config", "user.name", "Decodex Tests"]);
	git_status_success(&repo_path, &["config", "user.email", "decodex-tests@example.com"]);
	git_status_success(&repo_path, &["config", "commit.gpgsign", "false"]);
	git_status_success(&repo_path, &["add", "."]);
	git_status_success(&repo_path, &["commit", "-m", "bootstrap repo"]);

	TestProject {
		home_path: home.path().to_path_buf(),
		repo_path,
		config_path,
		_home: home,
		_project: project,
	}
}

fn git_status_success(cwd: &Path, args: &[&str]) {
	let output =
		hermetic_git_command().arg("-C").arg(cwd).args(args).output().expect("git should run");

	assert!(
		output.status.success(),
		"git {:?} failed: {}",
		args,
		String::from_utf8_lossy(&output.stderr)
	);
}

fn hermetic_git_command() -> Command {
	let mut command = Command::new("git");

	command
		.env("GIT_CONFIG_GLOBAL", "/dev/null")
		.env("GIT_CONFIG_SYSTEM", "/dev/null")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never")
		.args([
			"-c",
			"core.hooksPath=/dev/null",
			"-c",
			"commit.gpgsign=false",
			"-c",
			"tag.gpgsign=false",
			"-c",
			"init.defaultBranch=main",
		]);

	command
}

fn write_file(path: PathBuf, contents: &str) {
	let parent = path.parent().expect("test path should have parent");

	fs::create_dir_all(parent).expect("parent directory should exist");
	fs::write(path, contents).expect("test file should write");
}
