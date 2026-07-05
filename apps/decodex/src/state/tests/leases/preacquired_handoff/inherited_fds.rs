use std::os::fd::{AsRawFd, IntoRawFd};

use tempfile::TempDir;

use crate::state::{PreacquiredLeaseGuards, StateStore, tests};

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

#[cfg(unix)]
#[test]
fn adopted_preacquired_lease_restores_close_on_exec_on_inherited_fds() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("child store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let issue_claim_fd = child_issue_claim.as_raw_fd();
	let dispatch_slot_fd = child_guard.as_raw_fd();

	assert!(
		!tests::fd_has_close_on_exec(issue_claim_fd),
		"handoff issue-claim fd should clear close-on-exec before exec"
	);
	assert!(
		!tests::fd_has_close_on_exec(dispatch_slot_fd),
		"handoff dispatch-slot fd should clear close-on-exec before exec"
	);

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

	assert!(
		tests::fd_has_close_on_exec(issue_claim_fd),
		"adopted issue-claim fd must restore close-on-exec before spawning grandchildren"
	);
	assert!(
		tests::fd_has_close_on_exec(dispatch_slot_fd),
		"adopted dispatch-slot fd must restore close-on-exec before spawning grandchildren"
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
	parent_store.clear_lease("PUB-101").expect("parent lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_child_clear_releases_lock_when_descendant_keeps_inherited_fds_open() {
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
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let _descendant_issue_claim =
		child_issue_claim.try_clone().expect("descendant should inherit the issue-claim fd");
	let _descendant_guard =
		child_guard.try_clone().expect("descendant should inherit the dispatch-slot fd");

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
	parent_store.clear_lease("PUB-101").expect("parent should drop its local handoff guard");
	child_store.clear_lease("PUB-101").expect("child lease should clear");

	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("descendant-held fds must not keep the cleared lease claimed"),
		"clearing an adopted child lease must release the shared claim and slot even if a descendant still holds inherited fds"
	);
}
