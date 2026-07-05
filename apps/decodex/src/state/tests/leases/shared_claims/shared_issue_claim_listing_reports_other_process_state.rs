use tempfile::TempDir;

use crate::state::StateStore;

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

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
