fn post_review_sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	sample_issue(state_name, &[active_label.as_str()])
}

fn remove_local_git_metadata_for_post_review_status(worktree_path: &Path) {
	let commands: &[&[&str]] = &[
		&["config", "--local", "--unset-all", "codex.github-identity"],
		&["config", "--local", "--unset-all", "codex.linear-workspace"],
		&["remote", "remove", "origin"],
	];

	for args in commands {
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(worktree_path)
			.args(*args)
			.status()
			.expect("git metadata cleanup should run");
	}
}
