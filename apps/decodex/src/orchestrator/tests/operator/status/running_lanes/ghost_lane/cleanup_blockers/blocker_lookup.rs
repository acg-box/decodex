use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, orchestrator,
};

#[test]
fn ghost_lane_cleanup_status_blockers_treat_invalid_local_issue_id_as_missing_issue() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("invalid local issue id should be classified as a missing tracker issue");

	assert!(blockers.is_empty(), "missing issue with no live evidence should allow cleanup");
}

#[test]
fn preserves_live_blockers_after_invalid_issue_lookup() {
	let (temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let channel_path = temp_dir.path().join("missing-control-channel.json");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("invalid local issue id should still run local safety checks");

	assert!(blockers.contains(&String::from("control_channel_present")));
	assert!(blockers.contains(&String::from("control_channel_file_missing")));
}

#[test]
fn does_not_hide_validation_error_for_server_issue() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let issue_id = "00000000-0000-0000-0000-000000000012";

	state_store
		.record_run_attempt("run-12", issue_id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", issue_id, "run-12", "In Progress")
		.expect("lease should record");

	let error = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		issue_id,
		"run-12",
	)
	.expect_err("server issue id validation errors must remain tracker failures");

	assert!(error.to_string().contains("Argument Validation Error"));
}

#[test]
fn ghost_lane_cleanup_status_blockers_reject_existing_tracker_issue() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = running_lanes::sample_issue("In Progress", &[]);

	issue.id = String::from("PUB-012");
	issue.identifier = String::from("PUB-012");

	let tracker = FakeTracker::new(vec![issue]);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.contains(&String::from("tracker_issue_present")));
	assert!(blockers.contains(&String::from("issue_state:In Progress")));
}
