use std::{ffi::OsStr, fs};

use crate::{
	orchestrator::{
		self, PhaseGoalKind, RepoGateFailure, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
};

#[test]
fn repo_gate_rejects_dirty_tracked_files_left_by_canonicalize_commands() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'after\\n' > tracked.txt")],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect_err("tracked autofix rewrites should fail the repo gate");
	let tracked_contents = fs::read_to_string(repo_root.join("tracked.txt"))
		.expect("tracked file should remain readable");
	let tracked_status =
		tests::git_output(repo_root, &["status", "--porcelain", "--untracked-files=no"]);
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("verification"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_tracked_rewrites_left");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert_eq!(tracked_contents, "after\n");
	assert!(tracked_status.contains("tracked.txt"));
}

#[test]
fn repo_gate_stops_canonicalize_failure_after_scope_envelope_widening() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(repo_root, "outside.txt", "before\n", "add outside file");
	fs::write(repo_root.join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'rewritten\\n' > outside.txt; exit 1")],
		&[],
		repo_root,
	)
	.expect_err("canonicalize widening should stop before ordinary repair retry");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(
		error.to_string().contains("outside.txt"),
		"error should name the out-of-scope rewrite"
	);
}

#[test]
fn repo_gate_stops_verify_failure_after_scope_envelope_widening() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(repo_root, "outside.txt", "before\n", "add outside file");
	fs::write(repo_root.join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_repo_gate_commands(
		&[],
		&[String::from("printf 'rewritten\\n' > outside.txt; exit 1")],
		repo_root,
	)
	.expect_err("verify widening should stop before ordinary repair retry");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(
		error.to_string().contains("outside.txt"),
		"error should name the out-of-scope rewrite"
	);
}

#[test]
fn completion_repo_gate_records_lane_decision_for_scope_envelope_violation() {
	let failing_verify = "printf 'rewritten\\n' > outside.txt; exit 1";
	let workflow_markdown =
		tests::sample_workflow_markdown("pubfi", &[], "Completion gate policy.\n", 1).replace(
			"verify_commands = []",
			&format!(
				"verify_commands = [{}]",
				serde_json::to_string(failing_verify).expect("command should serialize")
			),
		);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = support::phase_goal_repo_gate_issue_run(&config, &issue);

	tests::commit_worktree_change(config.repo_root(), "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(
		config.repo_root(),
		"outside.txt",
		"before\n",
		"add outside file",
	);
	fs::write(config.repo_root().join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_completion_repo_gate(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		PhaseGoalKind::HandoffEvidence,
	)
	.expect_err("completion repo-gate scope violation should stop");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private lane decision events should load");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");

	let decision = repo_gate_failure
		.tracked_rewrite_decision()
		.expect("scope envelope violation should retain rewrite decision");
	let decision_json = decision.to_json();

	assert_eq!(decision_json["sourceErrorClass"], "repo_gate_verify_failed");
	assert_eq!(decision_json["sourceRepoGateFailure"]["stage"], "verify");
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "needs_attention"
			&& event.payload()["repo_gate_disposition"] == "needs_human_attention"
			&& event.payload()["scope_envelope_violation"] == true
	}));
}

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

#[test]
fn repo_gate_cleanliness_check_spawn_failures_require_human_attention() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
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
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
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
	let (temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();
	let remote_root = temp_dir.path().join("origin.git");

	tests::add_origin_remote(repo_root, &remote_root);
	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
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
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();

	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.canonicalize_commands(), ["cargo make fmt", "cargo make lint-fix"]);
	assert_eq!(selection.verify_commands(), ["cargo make check"]);
}
