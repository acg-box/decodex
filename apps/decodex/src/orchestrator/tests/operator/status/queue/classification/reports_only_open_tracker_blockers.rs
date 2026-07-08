use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

#[test]
fn reports_only_open_tracker_blockers() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-107",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	issue.blockers = vec![
		status::sample_blocker("issue-done", "PUB-106", "Done"),
		status::sample_blocker("issue-open", "PUB-105", "Todo"),
	];

	let tracker = FakeTracker::new(vec![issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}
