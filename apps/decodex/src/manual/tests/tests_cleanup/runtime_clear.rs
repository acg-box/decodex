use crate::{
	manual::{self, tests},
	state::{ReviewLifecycleHandoffFixture, StateStore},
};

#[test]
fn manual_closeout_runtime_clear_removes_lane_state() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let other_issue = tests::sample_issue("issue-2", "XY-226", true, &["decodex:active:pubfi"]);
	let handoff = ReviewLifecycleHandoffFixture::new(
		"run-1-failed",
		1,
		"y/decodex-xy-225",
		"https://github.com/hack-ink/decodex/pull/67",
		"main",
		"y/decodex-xy-225",
		"deadbeef",
	);

	state_store
		.upsert_lease("decodex", &issue.id, "run-1", "In Progress")
		.expect("issue lease should persist");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("issue running attempt should persist");
	state_store
		.record_run_attempt("run-1-starting", &issue.id, 2, "starting")
		.expect("issue starting attempt should persist");
	state_store
		.record_run_attempt("run-1-failed", &issue.id, 3, "failed")
		.expect("issue terminal attempt should persist");
	state_store
		.upsert_worktree("decodex", &issue.id, "y/decodex-xy-225", "/tmp/worktrees/xy-225")
		.expect("issue worktree should persist");
	state_store
		.upsert_review_lifecycle_handoff_fixture("decodex", &issue.id, &handoff)
		.expect("issue handoff should persist");
	state_store
		.upsert_lease("decodex", &other_issue.id, "run-2", "In Progress")
		.expect("other issue lease should persist");
	state_store
		.record_run_attempt("run-2", &other_issue.id, 1, "running")
		.expect("other issue running attempt should persist");

	manual::clear_manual_closeout_runtime_state(&state_store, &issue.id, handoff.run_id())
		.expect("manual closeout runtime state should clear");

	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.all(|lease| lease.issue_id() != issue.id)
	);
	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.any(|lease| lease.issue_id() == other_issue.id)
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none()
	);
	assert!(
		state_store
			.review_lifecycle_handoff_fixture("decodex", &issue.id, "y/decodex-xy-225")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-starting")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-failed")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-2")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"running"
	);
}
