use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

#[test]
fn live_operator_status_snapshot_routes_retained_success_state_lane_out_of_queue() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-review",
		"PUB-106",
		"In Review",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	issue.blockers = vec![status::sample_blocker("issue-done", "PUB-105", "Done")];

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-106",
			&worktree_path.display().to_string(),
		)
		.expect("retained review worktree should record");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == "PUB-106")
		.expect("retained success-state worktree should be owned by post-review status");

	assert!(
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "PUB-106"),
		"post-review retained lanes must not also appear as queue intake blockers"
	);
	assert_eq!(lane.reason, "missing_review_handoff_record");
	assert_eq!(
		project.queued_candidate_count, 0,
		"post-review retained lanes must not inflate intake backlog"
	);
}
