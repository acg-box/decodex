#[test]
fn repo_gate_rejects_dirty_tracked_files_left_by_canonicalize_commands() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let repo_root = config.repo_root();

	commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'after\\n' > tracked.txt")],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect_err("tracked autofix rewrites should fail the repo gate");
	let tracked_contents = fs::read_to_string(repo_root.join("tracked.txt"))
		.expect("tracked file should remain readable");
	let tracked_status = git_output(repo_root, &["status", "--porcelain", "--untracked-files=no"]);
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("verification"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_tracked_rewrites_left");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::ContinueRepair
	);
	assert_eq!(tracked_contents, "after\n");
	assert!(tracked_status.contains("tracked.txt"));
}

#[test]
fn repo_gate_cleanliness_check_spawn_failures_require_human_attention() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_cleanliness_check_with_git(
		OsStr::new("/definitely-missing-git-for-tests"),
		repo_root,
	)
	.expect_err("missing git binary should preserve repo gate classification");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("tracked-file cleanliness check"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
}

#[test]
fn repo_gate_classifies_git_index_lock_contention_as_retryable_runtime_failure() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_commands(
		&[String::from(
			"printf \"%s\\n\" \"fatal: Unable to create '.git/index.lock': File exists.\" >&2; exit 1",
		)],
		&[],
		repo_root,
	)
	.expect_err("git index.lock contention should fail the repo gate");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_git_lock_contention");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::RetryAfterBackoff
	);
}

#[test]
fn repo_gate_selects_matching_profile_for_scoped_lane_changes() {
	let (temp_dir, config, workflow) =
		temp_project_layout_with_workflow_markdown(&profile_scoped_workflow_markdown("pubfi"));
	let repo_root = config.repo_root();
	let remote_root = temp_dir.path().join("origin.git");

	add_origin_remote(repo_root, &remote_root);
	checkout_new_branch(repo_root, "config-subset");
	commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), Some("config_subset"));
	assert!(selection.canonicalize_commands().is_empty());
	assert_eq!(selection.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}

#[test]
fn repo_gate_falls_back_to_full_gate_when_changed_file_classification_is_unavailable() {
	let (_temp_dir, config, workflow) =
		temp_project_layout_with_workflow_markdown(&profile_scoped_workflow_markdown("pubfi"));
	let repo_root = config.repo_root();

	checkout_new_branch(repo_root, "config-subset");
	commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.canonicalize_commands(), ["cargo make fmt", "cargo make lint"]);
	assert_eq!(selection.verify_commands(), ["cargo make check"]);
}

#[test]
fn repo_gate_shell_falls_back_to_non_login_posix_sh_for_missing_absolute_shell() {
	let (shell, shell_flag) = orchestrator::repo_gate_shell_from_env(Some(OsString::from(
		"/definitely-missing-shell-for-tests",
	)));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_uses_non_login_mode_when_shell_is_bin_sh() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/sh")));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_keeps_login_mode_for_other_configured_shells() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/bash")));

	assert_eq!(Path::new(&shell), Path::new("/bin/bash"));
	assert_eq!(shell_flag, "-lc");
}
