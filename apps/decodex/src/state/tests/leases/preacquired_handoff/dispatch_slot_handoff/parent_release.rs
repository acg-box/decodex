use std::os::fd::IntoRawFd;

use tempfile::TempDir;

use crate::state::{PreacquiredLeaseGuards, StateStore};

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

#[cfg(unix)]
#[test]
fn parent_can_release_handed_off_guards_without_dropping_runtime_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
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
		.release_handed_off_guards("PUB-101")
		.expect("parent should release process-local guards after handoff");

	assert!(
		parent_store
			.lease_for_issue("PUB-101")
			.expect("parent lease lookup should succeed")
			.is_some(),
		"parent must keep the runtime lease visible after dropping local fd guards"
	);
	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("same issue should stay claimed by the child handoff")
	);
	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("another issue should acquire an independent dispatch slot")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}
