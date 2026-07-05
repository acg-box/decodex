use crate::orchestrator::tests::operator::status::running_lanes::{self, StateStore, orchestrator};

#[test]
fn operator_status_snapshot_excludes_completed_lingering_lease_from_current_lanes() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let completed_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"XY-379",
		"Done",
		&[],
		Some(3),
		"2026-04-29T17:00:33.133Z",
	);
	let active_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-2",
		"XY-378",
		"In Progress",
		&[],
		Some(3),
		"2026-04-29T17:01:33.133Z",
	);
	let completed_run_id = "xy-379-attempt-1-1777482033";
	let current_lane_run_id = "xy-378-attempt-1-1777482000";

	state_store
		.record_run_attempt(completed_run_id, &completed_issue.id, 1, "running")
		.expect("completed run should record");
	state_store
		.upsert_lease("pubfi", &completed_issue.id, completed_run_id, "In Progress")
		.expect("stale run lease should remain in runtime db");
	state_store
		.append_event(completed_run_id, 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("terminal protocol evidence should record");
	state_store
		.update_run_status(completed_run_id, "succeeded")
		.expect("terminal status should update");
	state_store
		.record_run_attempt(current_lane_run_id, &active_issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, current_lane_run_id, "In Progress")
		.expect("run lease should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let completed_run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == completed_run_id)
		.expect("completed stale-lease run should remain in history");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, current_lane_run_id);
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(completed_run.phase, "completed");
	assert!(
		completed_run.run_lease,
		"regression setup should keep the stale lease visible in history"
	);
}
