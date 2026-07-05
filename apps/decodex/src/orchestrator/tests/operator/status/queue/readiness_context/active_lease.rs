use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

#[test]
fn live_operator_status_snapshot_reports_ready_when_another_issue_has_active_lease() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let waiting_issue = status::sample_issue_with_sort_fields(
		"issue-waiting",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![waiting_issue]);

	state_store
		.upsert_lease(config.service_id(), "issue-running", "run-active", "In Progress")
		.expect("run lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("queued issue should exist");

	assert_eq!(candidate.issue_identifier, "PUB-101");
	assert_eq!(candidate.classification, "ready");
	assert_eq!(candidate.reason, "eligible_for_dispatch");
	assert_eq!(candidate.attention, None);
}
