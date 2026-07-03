use std::{
	fs::File,
	io::{Result, Write as _},
	os::fd::{AsRawFd, IntoRawFd},
	path::Path,
};

use tempfile::TempDir;

use crate::state::tests::{self, IN_PROGRESS_STATE};
use crate::state::{PreacquiredLeaseGuards, StateStore};

struct TestFile;
impl TestFile {
	fn write(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> Result<()> {
		let mut file = File::create(path)?;

		file.write_all(body.as_ref())
	}
}

#[test]
fn manages_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should be inserted");

	let lease = store
		.lease_for_issue("PUB-101")
		.expect("lease read should succeed")
		.expect("lease should exist");

	assert_eq!(lease.issue_id(), "PUB-101");
	assert_eq!(lease.run_id(), "run-1");
	assert_eq!(lease.project_id(), "pubfi");
	assert_eq!(lease.issue_state(), IN_PROGRESS_STATE);

	store.clear_lease("PUB-101").expect("lease should be deleted");

	assert!(store.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_none());
}

#[test]
fn tracks_issue_specific_leases_without_project_limit() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first lease acquisition should succeed")
	);
	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second lease acquisition should succeed for another issue")
	);
	assert!(
		!store
			.try_acquire_lease("pubfi", "PUB-101", "run-3", IN_PROGRESS_STATE)
			.expect("duplicate issue acquisition should be rejected")
	);
	assert!(
		store
			.try_acquire_lease("other", "PUB-201", "run-4", IN_PROGRESS_STATE)
			.expect("other project should still acquire its own slot")
	);
}

#[test]
fn cleared_shared_lease_removes_lock_anchor_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("store should configure dispatch slot root");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("shared lease acquisition should succeed")
	);
	assert!(issue_claim_path.exists(), "active issue claim should create a lock anchor");
	assert!(dispatch_slot_path.exists(), "active dispatch slot should create a lock anchor");

	store.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		!issue_claim_path.exists(),
		"clearing the shared lease should remove its issue-claim anchor"
	);
	assert!(
		!dispatch_slot_path.exists(),
		"clearing the shared lease should remove its dispatch-slot anchor"
	);
}

#[test]
fn read_only_shared_claim_check_does_not_remove_unlocked_claim_anchor() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("store should configure dispatch slot root");

	TestFile::write(&issue_claim_path, "stale claim anchor\n")
		.expect("stale claim anchor should write");

	assert!(
		!store
			.issue_has_active_shared_claim_read_only("pubfi", "PUB-101")
			.expect("read-only shared claim check should read")
	);
	assert!(
		issue_claim_path.exists(),
		"read-only shared claim check should not remove an unlocked claim anchor"
	);
	assert!(
		!store
			.issue_has_active_shared_claim("pubfi", "PUB-101")
			.expect("normal shared claim check should read")
	);
	assert!(
		!issue_claim_path.exists(),
		"normal shared claim check should clean an unlocked claim anchor"
	);
}

#[test]
fn observe_dispatch_slot_root_does_not_prune_unlocked_claim_anchor() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	TestFile::write(&issue_claim_path, "stale claim anchor\n")
		.expect("stale claim anchor should write");

	store
		.observe_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("store should observe dispatch slot root");

	assert!(
		issue_claim_path.exists(),
		"read-only dispatch-slot observation should not remove an unlocked claim anchor"
	);
	assert!(
		!store
			.issue_has_active_shared_claim_read_only("pubfi", "PUB-101")
			.expect("read-only shared claim check should read")
	);
	assert!(
		issue_claim_path.exists(),
		"read-only shared claim check should still leave the anchor after observation"
	);
}

#[test]
fn configure_dispatch_slot_root_prunes_unlocked_shared_lock_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-999.lock");
	let stale_dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	TestFile::write(
		&stale_issue_claim_path,
		"project_id=pubfi\nissue_id=PUB-999\nrun_id=run-stale\nissue_state=In Progress\n",
	)
	.expect("stale issue-claim anchor should write");
	TestFile::write(&stale_dispatch_slot_path, "")
		.expect("stale dispatch-slot anchor should write");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("configuration should prune unlocked shared lock anchors");

	assert!(
		!stale_issue_claim_path.exists(),
		"configuration should remove unlocked stale issue-claim anchors"
	);
	assert!(
		!stale_dispatch_slot_path.exists(),
		"configuration should remove unlocked stale dispatch-slot anchors"
	);
}

#[test]
fn shared_dispatch_slots_allocate_on_demand_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");
	let store_three = StateStore::open_in_memory().expect("third store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("first store should configure dispatch slots");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("second store should configure dispatch slots");
	store_three
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("third store should configure dispatch slots");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should acquire another shared slot")
	);
	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("third store should acquire another shared slot")
	);
}

#[test]
fn shared_issue_claim_blocks_duplicate_issue_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected across processes")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-3", IN_PROGRESS_STATE)
			.expect("another issue should still acquire an independent dispatch slot")
	);
}

#[test]
fn shared_issue_claim_reopens_same_issue_after_clear_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected while the first lease is active")
	);

	store_one.clear_lease("PUB-101").expect("shared issue claim should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue claim should reopen after the first lease clears")
	);
}

#[test]
fn shared_issue_claim_listing_reports_other_process_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_store = StateStore::open_in_memory().expect("remote store should open");
	let observer_store = StateStore::open_in_memory().expect("observer store should open");

	remote_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("remote store should configure dispatch slot root");
	observer_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("observer store should configure dispatch slot root");

	assert!(
		remote_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("remote issue claim should succeed")
	);

	let leases = observer_store
		.list_active_shared_leases("pubfi")
		.expect("shared claim listing should succeed");

	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[0].run_id(), "run-1");
	assert_eq!(leases[0].issue_state(), IN_PROGRESS_STATE);
}

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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
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
			IN_PROGRESS_STATE,
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
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
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
			IN_PROGRESS_STATE,
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
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
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
			IN_PROGRESS_STATE,
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
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue should stay claimed by the child handoff")
	);
	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
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
			IN_PROGRESS_STATE,
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
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
			IN_PROGRESS_STATE,
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
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("descendant-held fds must not keep the cleared lease claimed"),
		"clearing an adopted child lease must release the shared claim and slot even if a descendant still holds inherited fds"
	);
}
