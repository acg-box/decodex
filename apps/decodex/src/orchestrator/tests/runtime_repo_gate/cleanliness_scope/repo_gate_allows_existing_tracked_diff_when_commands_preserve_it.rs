use std::fs;

use crate::orchestrator::{self, tests};

#[test]
fn repo_gate_allows_existing_tracked_diff_when_commands_preserve_it() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");
	fs::write(repo_root.join("tracked.txt"), "after\n")
		.expect("tracked implementation diff should write");
	orchestrator::run_repo_gate_commands(
		&[],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect("repo gate should allow an existing implementation diff");
}
