use tempfile::TempDir;

use crate::state::StateStore;

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

#[test]
fn manages_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
		.expect("lease should be inserted");

	let lease = store
		.lease_for_issue("PUB-101")
		.expect("lease read should succeed")
		.expect("lease should exist");

	assert_eq!(lease.issue_id(), "PUB-101");
	assert_eq!(lease.run_id(), "run-1");
	assert_eq!(lease.project_id(), "pubfi");
	assert_eq!(lease.issue_state(), LEASE_IN_PROGRESS_STATE);

	store.clear_lease("PUB-101").expect("lease should be deleted");

	assert!(store.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_none());
}

#[test]
fn tracks_issue_specific_leases_without_project_limit() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("first lease acquisition should succeed")
	);
	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("second lease acquisition should succeed for another issue")
	);
	assert!(
		!store
			.try_acquire_lease("pubfi", "PUB-101", "run-3", LEASE_IN_PROGRESS_STATE)
			.expect("duplicate issue acquisition should be rejected")
	);
	assert!(
		store
			.try_acquire_lease("other", "PUB-201", "run-4", LEASE_IN_PROGRESS_STATE)
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
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
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
