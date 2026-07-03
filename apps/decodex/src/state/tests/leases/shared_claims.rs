use std::{
	fs::File,
	io::{Result, Write as _},
	path::Path,
};

use tempfile::TempDir;

use crate::state::StateStore;

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

struct TestFile;
impl TestFile {
	fn write(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> Result<()> {
		let mut file = File::create(path)?;

		file.write_all(body.as_ref())
	}
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("second store should acquire another shared slot")
	);
	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", LEASE_IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected across processes")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-3", LEASE_IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected while the first lease is active")
	);

	store_one.clear_lease("PUB-101").expect("shared issue claim should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("remote issue claim should succeed")
	);

	let leases = observer_store
		.list_active_shared_leases("pubfi")
		.expect("shared claim listing should succeed");

	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[0].run_id(), "run-1");
	assert_eq!(leases[0].issue_state(), LEASE_IN_PROGRESS_STATE);
}
