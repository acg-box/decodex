use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
	thread,
	time::{Duration, Instant},
};

use tempfile::TempDir;

use crate::{
	test_support::hermetic_git_command, workflow::WorkflowDocument, worktree::WorktreeManager,
};

fn workspace_hooks(workspace_hooks_frontmatter: &str) -> crate::workflow::WorkflowWorkspaceHooks {
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
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

{workspace_hooks_frontmatter}

[context]
read_first = []
+++
			"#,
	);

	WorkflowDocument::parse_markdown(&markdown)
		.expect("workflow should parse")
		.frontmatter()
		.execution()
		.workspace_hooks()
		.clone()
}

fn test_git_command() -> Command {
	hermetic_git_command()
}

fn run_git(repo_root: &Path, args: &[&str]) {
	let output = test_git_command()
		.args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
		.arg("-C")
		.arg(repo_root)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {:?} failed in {}: {}",
		args,
		repo_root.display(),
		String::from_utf8_lossy(&output.stderr)
	);
}

fn git_stdout(repo_root: &Path, args: &[&str]) -> String {
	let output = test_git_command()
		.arg("-C")
		.arg(repo_root)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {:?} failed in {}: {}",
		args,
		repo_root.display(),
		String::from_utf8_lossy(&output.stderr)
	);

	String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn init_repo() -> (TempDir, PathBuf) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("repo");
	let default_origin = repo_root.parent().unwrap().join("source-origin.git");

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git(
		default_origin.parent().unwrap(),
		&["init", "--bare", default_origin.to_str().unwrap()],
	);
	run_git(&repo_root, &["init", "--initial-branch", "main"]);
	run_git(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git(&repo_root, &["config", "commit.gpgsign", "false"]);
	run_git(&repo_root, &["config", "tag.gpgsign", "false"]);
	run_git(&repo_root, &["remote", "add", "origin", default_origin.to_str().unwrap()]);

	fs::write(repo_root.join("README.md"), "hello\n").expect("seed file should write");

	run_git(&repo_root, &["add", "README.md"]);
	run_git(&repo_root, &["commit", "-m", "seed"]);

	(temp_dir, repo_root)
}

mod cleanup;

mod lifecycle;

#[test]
fn plans_worktree_paths_and_identity_scoped_branch_names() {
	let (_temp_dir, repo_root) = init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let default_spec = manager.plan_for_issue("PUB-101");

	assert_eq!(default_spec.branch_name, "x/pubfi-pub-101");
	assert_eq!(default_spec.path, worktree_root.join("PUB-101"));
	assert!(!default_spec.reused_existing);

	run_git(&repo_root, &["config", "codex.github-identity", "y"]);

	let routed_spec = manager.plan_for_issue("PUB-101");

	assert_eq!(routed_spec.branch_name, "y/pubfi-pub-101");
}
