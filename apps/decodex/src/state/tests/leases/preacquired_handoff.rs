use std::os::fd::{AsRawFd, IntoRawFd};

use tempfile::TempDir;

use crate::state::{PreacquiredLeaseGuards, StateStore, tests};

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

#[cfg(unix)]
#[test]
fn adopted_dispatch_slot_handoff_does_not_block_other_issues_after_parent_releases_local_guard() {
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
		.release_dispatch_slot("PUB-101")
		.expect("parent should release its local guard after handoff");

	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("child-held guard should not block another issue")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

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
