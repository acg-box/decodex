use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

#[test]
fn operator_state_snapshot_publish_skips_external_observers_after_tick_failure() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&["control_plane_tick_failed"],
		&[],
	)
	.expect("snapshot should build from local state");

	assert_eq!(
		snapshot.warnings,
		vec![
			String::from("control_plane_tick_failed"),
			String::from("external_observer_status_skipped"),
		]
	);
	assert_eq!(snapshot.projects[0].warning_count, 2);
	assert_eq!(snapshot.projects[0].connector_state, "degraded");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"degraded publish should not query queued labels"
	);
}
