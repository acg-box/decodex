use crate::{
	orchestrator::{
		self, IssueDispatchMode,
		tests::{
			FakeTracker, TEST_SERVICE_ID, recovery_terminal_support, {self},
		},
	},
	state::StateStore,
	test_support,
	tracker::{self, TrackerState, records},
	worktree::WorktreeManager,
};

#[test]
fn closeout_dispatch_completes_merged_lane_without_agent_turn() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = recovery_terminal_support::issue_with_completed_state(tests::sample_issue(
		"In Review",
		&[active_label.as_str()],
	));
	let mut completed_issue = issue.clone();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![
			vec![issue.clone()],
			vec![issue.clone()],
			vec![completed_issue.clone()],
			vec![completed_issue.clone()],
		],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/701";
	let _path_guard = recovery_terminal_support::install_fake_closeout_gh_responses(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	recovery_terminal_support::initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	recovery_terminal_support::route_origin_github_url_to_local_bare_repo(
		config.repo_root(),
		&remote_root,
	);

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
	);

	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"pub-701-attempt-3-closeout",
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "starting")
		.expect("run attempt should record");

	let summary =
		orchestrator::execute_issue_run(&tracker, &config, &workflow, &state_store, issue_run)
			.expect("deterministic closeout should complete");

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		[(issue.id.clone(), String::from("state-done"))]
	);
	assert_eq!(tracker.comments.borrow().len(), 2);
	assert!(tracker.comments.borrow()[0].contains("decodex closeout completed"));

	let event_types = tracker
		.comments
		.borrow()
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.map(|record| record.event_type)
		.collect::<Vec<_>>();

	assert_eq!(event_types, vec![String::from("closeout"), String::from("cleanup_complete")]);
	let lifecycle_record = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, &worktree.branch_name)
		.expect("lifecycle authority lookup should succeed")
		.expect("deterministic closeout should preserve lifecycle authority");
	assert_eq!(lifecycle_record.next_state(), "closed");
	assert_eq!(lifecycle_record.transition(), "closeout_completed");
	assert_eq!(lifecycle_record.cleanup_state(), "completed");
	assert!(!worktree.path.exists(), "deterministic closeout should remove the retained worktree");
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"deterministic closeout should clear retained worktree state"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"deterministic closeout should not leave an run lease"
	);
	assert_eq!(
		tracker.label_removals.borrow().len(),
		2,
		"deterministic closeout should clear active and queue lane labels"
	);
}
