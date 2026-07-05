use std::os::fd::IntoRawFd;

use tempfile::TempDir;

use crate::state::{PreacquiredLeaseGuards, StateStore};

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

#[cfg(unix)]
#[test]
fn adopted_issue_claim_blocks_same_issue_after_parent_clears_local_guard() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("parent should acquire the shared issue claim")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			LEASE_IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.clear_lease("PUB-101")
		.expect("parent should drop its local lease without unlocking the child handoff");

	assert!(
		issue_claim_path.exists(),
		"parent-side handoff cleanup must not remove the child-held issue-claim anchor"
	);
	assert!(
		dispatch_slot_path.exists(),
		"parent-side handoff cleanup must not remove the child-held dispatch-slot anchor"
	);
	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("same issue should stay claimed while the child still holds the handoff fd")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");

	assert!(
		!issue_claim_path.exists(),
		"child terminal cleanup should remove the inherited issue-claim anchor"
	);
	assert!(
		!dispatch_slot_path.exists(),
		"child terminal cleanup should remove the inherited dispatch-slot anchor"
	);
}
