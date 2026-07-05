use tempfile::TempDir;

use crate::state::StateStore;

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

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
