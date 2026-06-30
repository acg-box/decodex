use super::*;

#[test]
fn worktree_blocking_status_lines_ignores_untracked_decodex_runtime_artifacts() {
	let (temp_dir, _, _) = temp_git_worktree("x/pubfi-pub-718");
	let repo = temp_dir.path();

	fs::write(repo.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
		.expect("activity marker should write");

	let control_dir = repo.join(crate::state::RUN_CONTROL_CHANNEL_DIR);

	fs::create_dir_all(&control_dir).expect("run-control directory should create");
	fs::write(control_dir.join("run-1-1.channel"), "channel\n")
		.expect("run-control channel should write");

	let blocking = super::super::worktree_blocking_status_lines(repo)
		.expect("worktree status should be readable");

	assert!(blocking.is_empty(), "runtime artifacts should not block rebind: {blocking:?}");
}
