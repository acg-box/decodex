use crate::orchestrator::tests::{
	self,
	runtime_failure::{FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator, tracker},
};

#[test]
fn reconciliation_clears_stale_leases_and_terminal_worktrees() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker =
		FakeTracker::new(vec![issue.clone()]).with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should succeed");

	assert!(summary.is_none());
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert!(
		state_store.worktree_for_issue(&issue.id).expect("worktree lookup should work").is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn reconciliation_runs_without_project_validation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker = FakeTracker::with_refresh_snapshots_and_project(
		vec![issue.clone()],
		vec![vec![issue.clone()]],
		false,
	)
	.with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should still succeed without any project validation");

	assert!(summary.is_none(), "reconciliation-only startup should not dispatch a new lane here");
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}
