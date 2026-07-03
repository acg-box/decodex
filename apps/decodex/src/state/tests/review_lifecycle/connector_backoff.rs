use tempfile::TempDir;

use crate::state::{ConnectorBackoffInput, StateStore};

#[test]
fn connector_backoff_roundtrip_and_clear_from_runtime_store() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: "pubfi",
			connector: "linear",
			sync_phase: "post_review_lane_status",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch: 1_777_392_000,
			reset_source: "linear",
			warning: "tracker_rate_limited",
		})
		.expect("connector backoff should persist");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let backoff = reopened
		.connector_backoff("pubfi", "linear")
		.expect("connector backoff should read")
		.expect("connector backoff should exist");

	assert_eq!(backoff.project_id(), "pubfi");
	assert_eq!(backoff.connector(), "linear");
	assert_eq!(backoff.sync_phase(), "post_review_lane_status");
	assert_eq!(backoff.quota_class(), "linear_graphql_rate_limit");
	assert_eq!(backoff.reset_unix_epoch(), 1_777_392_000);
	assert_eq!(backoff.reset_source(), "linear");
	assert_eq!(backoff.warning(), "tracker_rate_limited");

	reopened.clear_connector_backoff("pubfi", "linear").expect("connector backoff should clear");

	let reopened = StateStore::open(&state_path).expect("state store should reopen again");

	assert!(
		reopened
			.connector_backoff("pubfi", "linear")
			.expect("connector backoff should read after clear")
			.is_none()
	);
}
