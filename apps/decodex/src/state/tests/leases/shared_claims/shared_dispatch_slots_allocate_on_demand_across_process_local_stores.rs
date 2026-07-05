use tempfile::TempDir;

use crate::state::StateStore;

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

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
