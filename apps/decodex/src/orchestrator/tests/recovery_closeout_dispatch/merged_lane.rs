use std::path::Path;

use crate::{
	orchestrator::{
		self, IssueDispatchMode,
		tests::{
			FakeTracker, TEST_SERVICE_ID, recovery_terminal_support, {self},
		},
	},
	state::{ReviewLifecycleTransitionInput, StateStore},
	test_support,
	tracker::{self, TrackerState, records},
	worktree::WorktreeManager,
};

struct StaleReviewWaitSync<'a> {
	service_id: &'a str,
	issue_id: &'a str,
	branch_name: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	pr_url: &'a str,
	head_oid: &'a str,
}

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
	let run_id = issue_run.run_id.clone();
	let attempt_number = issue_run.attempt_number;

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

	assert_stale_review_wait_sync_preserves_closed_lifecycle(
		&state_store,
		StaleReviewWaitSync {
			service_id: config.service_id(),
			issue_id: &issue.id,
			branch_name: &worktree.branch_name,
			run_id: &run_id,
			attempt_number,
			pr_url,
			head_oid: &head_oid,
		},
	);
	assert_closeout_cleared_runtime_state(&state_store, &tracker, &issue.id, &worktree.path);
}

fn assert_closeout_cleared_runtime_state(
	state_store: &StateStore,
	tracker: &FakeTracker,
	issue_id: &str,
	worktree_path: &Path,
) {
	assert!(!worktree_path.exists(), "deterministic closeout should remove the retained worktree");
	assert!(
		state_store.worktree_for_issue(issue_id).expect("worktree lookup should succeed").is_none(),
		"deterministic closeout should clear retained worktree state"
	);
	assert!(
		state_store.lease_for_issue(issue_id).expect("lease lookup should succeed").is_none(),
		"deterministic closeout should not leave an run lease"
	);
	assert_eq!(
		tracker.label_removals.borrow().len(),
		2,
		"deterministic closeout should clear active and queue lane labels"
	);
}

fn assert_stale_review_wait_sync_preserves_closed_lifecycle(
	state_store: &StateStore,
	input: StaleReviewWaitSync<'_>,
) {
	let event_count_before_stale_sync = state_store
		.list_private_execution_events_for_issue(input.service_id, input.issue_id)
		.expect("private events should list")
		.len();

	state_store
		.record_review_lifecycle_transition(
			input.service_id,
			input.issue_id,
			ReviewLifecycleTransitionInput {
				run_id: input.run_id,
				attempt_number: input.attempt_number,
				branch_name: input.branch_name,
				pr_url: input.pr_url,
				head_sha: input.head_oid,
				phase: "request_pending",
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 1,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
			},
		)
		.expect("stale review-wait sync after closeout should be ignored");

	let preserved_lifecycle_record = state_store
		.review_lifecycle_record(input.service_id, input.issue_id, input.branch_name)
		.expect("lifecycle authority lookup should succeed")
		.expect("deterministic closeout should preserve lifecycle authority");
	let event_count_after_stale_sync = state_store
		.list_private_execution_events_for_issue(input.service_id, input.issue_id)
		.expect("private events should list")
		.len();

	assert_eq!(preserved_lifecycle_record.next_state(), "closed");
	assert_eq!(preserved_lifecycle_record.transition(), "closeout_completed");
	assert_eq!(preserved_lifecycle_record.next_action(), "no_action");
	assert_eq!(preserved_lifecycle_record.phase(), "closed");
	assert_eq!(event_count_after_stale_sync, event_count_before_stale_sync);
}
