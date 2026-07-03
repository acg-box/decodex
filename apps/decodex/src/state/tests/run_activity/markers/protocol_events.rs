use std::{fs, path::Path};

use tempfile::TempDir;
use time::OffsetDateTime;

use crate::state::{
	self, ProtocolActivityMarker, ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE, Result,
};

#[test]
fn run_protocol_non_work_events_do_not_refresh_progress_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - 3_600;

	fs::write(
		temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_progress}\nlast_protocol_activity_unix_epoch={stale_progress}\nlast_progress_unix_epoch={stale_progress}\n"
		),
	)
	.expect("initial marker should write");

	let account_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("account/rateLimits/updated"),
			category: String::from("rate_limit"),
			detail: Some(String::from("pro")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		1,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol > stale_progress)
	);

	let first_protocol_activity = marker
		.last_protocol_activity_unix_epoch()
		.expect("account protocol activity should update protocol time");

	write_test_protocol_activity_marker(
		temp_dir.path(),
		2,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("second account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol >= first_protocol_activity)
	);

	let goal_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("thread/goal/updated"),
			category: String::from("protocol"),
			detail: Some(String::from("active")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		3,
		"thread/goal/updated",
		Some(&goal_activity),
	)
	.expect("goal status protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));

	write_test_protocol_activity_marker(temp_dir.path(), 4, "item/fileChange/patchUpdated", None)
		.expect("work protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert!(
		marker
			.last_progress_unix_epoch()
			.is_some_and(|last_progress| last_progress > stale_progress)
	);
}

fn write_test_protocol_activity_marker(
	worktree_path: &Path,
	event_count: i64,
	last_event_type: &str,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Result<()> {
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count,
			last_event_type,
			child_agent_activity: None,
			protocol_activity,
		},
	)
}
