use crate::{
	orchestrator::tests::operator::{
		status,
		status::{
			FakeTracker, OffsetDateTime, StateStore, TRACKER_RATE_LIMIT_WARNING, orchestrator,
		},
	},
	state::ConnectorBackoffInput,
};

#[test]
fn live_operator_status_snapshot_honors_persisted_tracker_backoff_without_linear_reads() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 60;

	state_store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: config.service_id(),
			connector: "linear",
			sync_phase: "run_cycle",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch,
			reset_source: "local_default",
			warning: TRACKER_RATE_LIMIT_WARNING,
		})
		.expect("connector backoff should persist");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should use local backoff state");

	assert!(snapshot.warnings.contains(&String::from(orchestrator::TRACKER_RATE_LIMIT_WARNING)));
	assert!(snapshot.warnings.contains(&String::from("external_observer_status_skipped")));
	assert_eq!(snapshot.connector_backoffs.len(), 1);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"persisted backoff should skip queued-label reads"
	);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"persisted backoff should skip execution-ledger reads"
	);
}
