use std::fs;

use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn stalled_run_reconciliation_reports_retained_partial_progress_for_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-dirty";
	let worktree_path = config.worktree_root().join("PUB-102");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained partial work\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-102",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "turn/diff/updated", "{\"changes\":1}")
		.expect("stalled dirty issue protocol event should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("dirty stalled-run inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
			if idle_for >= RUN_LEASE_IDLE_TIMEOUT
	));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);

	support::assert_dirty_stalled_retained_progress_comments(&tracker.comments.borrow());
}
