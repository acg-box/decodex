use std::{fs, process};

use tempfile::TempDir;

use crate::state::{self, RUN_ACTIVITY_MARKER_FILE};

pub(crate) fn assert_run_activity_marker_round_trips_clearable_auxiliary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 12_345)
		.expect("retry schedule should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-1");
	assert_eq!(marker.attempt_number(), 1);

	if let Some(host_boot_id) = state::current_host_boot_id() {
		assert_eq!(marker.host_boot_id(), Some(host_boot_id.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("host_boot_id={host_boot_id}\n")),
			"activity markers should record the host boot identity for reboot-safe liveness"
		);
	}
	if let Some(process_start_identity) = state::current_process_start_identity() {
		assert_eq!(marker.process_start_identity(), Some(process_start_identity.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("process_start_identity={process_start_identity}\n")),
			"activity markers should record the process start identity for PID-reuse-safe liveness"
		);
	}

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert_eq!(marker.retry_ready_at_unix_epoch(), Some(12_345));

	state::clear_run_retry_schedule(temp_dir.path()).expect("retry schedule should clear");

	let retry_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(retry_cleared.retry_kind(), None);
	assert_eq!(retry_cleared.retry_ready_at_unix_epoch(), None);
}
