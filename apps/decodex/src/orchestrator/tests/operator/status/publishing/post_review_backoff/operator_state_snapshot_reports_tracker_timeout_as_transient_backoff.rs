use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, Instant, OffsetDateTime, StateStore, TRACKER_TRANSIENT_TIMEOUT_WARNING,
	eyre, orchestrator, slice,
};

#[test]
fn operator_state_snapshot_reports_tracker_timeout_as_transient_backoff() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
	let now = Instant::now();
	let error = eyre::eyre!("Linear connector timed out during GraphQL request: deadline elapsed");
	let connector_backoff =
		orchestrator::tracker_connector_backoff(&error, now, "operator_snapshot_refresh")
			.expect("timeout should create transient backoff")
			.to_operator_status(config.service_id(), OffsetDateTime::now_utc().unix_timestamp());
	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[TRACKER_TRANSIENT_TIMEOUT_WARNING],
		slice::from_ref(&connector_backoff),
	)
	.expect("snapshot should build from local state");

	assert_eq!(
		snapshot.warnings,
		vec![
			String::from(orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING),
			String::from("external_observer_status_skipped"),
		]
	);
	assert_eq!(snapshot.connector_backoffs, vec![connector_backoff]);
	assert_eq!(snapshot.connector_backoffs[0].connector, "linear");
	assert_eq!(snapshot.connector_backoffs[0].sync_phase, "operator_snapshot_refresh");
	assert_eq!(snapshot.connector_backoffs[0].quota_class, "linear_graphql_timeout");
	assert_eq!(snapshot.connector_backoffs[0].reset_source, "local_transient_timeout");
	assert_eq!(
		snapshot.connector_backoffs[0].warning,
		orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING
	);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"timeout publish should not query queued labels during backoff"
	);
}
