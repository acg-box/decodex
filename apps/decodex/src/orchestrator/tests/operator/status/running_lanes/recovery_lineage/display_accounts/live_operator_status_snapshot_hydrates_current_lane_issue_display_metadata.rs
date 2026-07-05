use std::fs;

use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, orchestrator,
};

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_issue_display_metadata() {
	let (temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "xy-392-attempt-1-1777551056";
	let channel_path = temp_dir.path().join("control.channel");
	let mut issue = running_lanes::sample_issue_with_sort_fields(
		"issue-active",
		"XY-392",
		"In Progress",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);

	issue.title = String::from("Hydrate issue display metadata on run rows");

	running_lanes::git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("run lease should record");
	state_store.update_run_thread(run_id, "thread-1").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-1").expect("turn should record");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	state_store
		.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
		.expect("control channel should publish");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let current_lane = snapshot.current_lanes.first().expect("current lane should exist");
	let recent_run = snapshot.recent_runs.first().expect("recent run should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(current_lane.project_id, config.service_id());
	assert_eq!(current_lane.project_display_name, "hack-ink/pubfi-mono-v2");
	assert_eq!(current_lane.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(current_lane.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(current_lane.author.as_deref(), Some("Yvette"));

	let expected_private_evidence_command = format!(
		"decodex evidence --config {} XY-392 --run-id {run_id} --attempt 1 --json",
		config.config_path().display()
	);

	assert_eq!(current_lane.private_evidence.read_command, expected_private_evidence_command);
	assert_eq!(recent_run.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(recent_run.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(recent_run.author.as_deref(), Some("Yvette"));
	assert_eq!(snapshot_json["current_lanes"][0]["project_id"], "pubfi");
	assert_eq!(snapshot_json["current_lanes"][0]["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(snapshot_json["current_lanes"][0]["issue_identifier"], "XY-392");
	assert_eq!(
		snapshot_json["current_lanes"][0]["title"],
		"Hydrate issue display metadata on run rows"
	);
	assert_eq!(snapshot_json["current_lanes"][0]["author"], "Yvette");
	assert_eq!(
		snapshot_json["current_lanes"][0]["private_evidence"]["read_command"],
		expected_private_evidence_command
	);
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["status"], "active");
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["thread_id"], "thread-1");
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["turn_id"], "turn-1");
}
