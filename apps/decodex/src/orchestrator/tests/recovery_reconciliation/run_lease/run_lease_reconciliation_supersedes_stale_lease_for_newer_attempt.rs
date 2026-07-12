use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_lease_reconciliation_supersedes_stale_lease_for_newer_attempt() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-superseded-lease",
		"PUB-207",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let stale_run_id = "run-superseded-lease-1";
	let newer_run_id = "run-superseded-lease-2";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	state_store
		.record_lane_run_attempt(config.service_id(), stale_run_id, &issue.id, 1, "running")
		.expect("stale run should record");
	state_store
		.record_lane_run_attempt(config.service_id(), newer_run_id, &issue.id, 2, "succeeded")
		.expect("newer run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, stale_run_id, "In Progress")
		.expect("stale lease should record");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("run lease inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		&actions[0].disposition,
		RunLeaseDisposition::Superseded {
			newer_run_id: observed_run_id,
			newer_attempt_number: 2,
		} if observed_run_id == newer_run_id
	));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("superseded reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert_eq!(
		state_store
			.run_attempt(stale_run_id)
			.expect("run attempt lookup should succeed")
			.expect("stale run should exist")
			.status(),
		"interrupted"
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"superseded stale lease must not write needs-attention comments"
	);
}
