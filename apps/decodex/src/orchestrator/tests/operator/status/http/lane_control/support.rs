use crate::orchestrator::tests::operator::status::http::{
	self, Command, ProjectRegistration, RUN_CONTROL_CHANNEL_DIR,
	RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE, RunLeaseMissingControlFixture, ServiceConfig,
	StateStore, Value, fs, orchestrator, state,
};
#[cfg(unix)]
pub(super) fn run_lease_missing_control_fixture(
	config: &ServiceConfig,
	state_store: &StateStore,
) -> RunLeaseMissingControlFixture {
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let channel_path =
		worktree_path.join(RUN_CONTROL_CHANNEL_DIR).join("pub-101-attempt-1.channel");
	let child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("lane child process should start");
	let child_process_id = child.id();

	assert!(
		orchestrator::process_is_alive(child_process_id),
		"lane child process should be live before control request"
	);

	fs::create_dir_all(channel_path.parent().expect("channel path should have parent"))
		.expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("control channel should write");
	state::write_run_activity_marker_for_process(
		&worktree_path,
		"pub-101-attempt-1",
		1,
		child_process_id,
	)
	.expect("activity marker should write");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("pub-101-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("pub-101-attempt-1", "thread-1").expect("thread should record");
	state_store.update_run_turn("pub-101-attempt-1", "turn-1").expect("turn should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "pub-101-attempt-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"pub-101-attempt-1",
			1,
			&channel_path,
			RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		)
		.expect("control channel should publish");
	state_store.clear_lease(&issue.id).expect("lease should clear");

	RunLeaseMissingControlFixture { issue, channel_path, child, child_process_id }
}

#[cfg(unix)]
pub(super) fn operator_json_response_body(response: &str, context: &str) -> Value {
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.unwrap_or_else(|| http::panic!("{context} response should include body"));

	serde_json::from_str(body).unwrap_or_else(|_| http::panic!("{context} response should be json"))
}

#[cfg(unix)]
pub(super) fn reap_run_lease_missing_child(fixture: &mut RunLeaseMissingControlFixture) {
	if orchestrator::process_is_alive(fixture.child_process_id) {
		fixture.child.kill().expect("lane child process should be killable after failed fallback");
	}

	fixture.child.wait().expect("lane child process should reap");
}

#[cfg(unix)]
pub(super) fn assert_run_lease_missing_control_audit(
	config: &ServiceConfig,
	state_store: &StateStore,
	fixture: &RunLeaseMissingControlFixture,
	run_id: &str,
) {
	let events = state_store
		.list_private_execution_events(config.service_id(), &fixture.issue.id, run_id, 1)
		.expect("private control audit should read");
	let missing_lease_steer_event = events
		.iter()
		.find(|event| {
			event.event_type() == "control_action"
				&& event.payload()["action"] == "steer"
				&& event.payload()["reason"] == "run_lease_missing"
		})
		.expect("run lease steer rejection should be audited");
	let missing_lease_interrupt_event = events
		.iter()
		.find(|event| {
			event.event_type() == "control_action"
				&& event.payload()["action"] == "interrupt"
				&& event.payload()["reason"] == "run_lease_missing"
		})
		.expect("run lease interrupt rejection should be audited");
	let expected_channel_path = fixture.channel_path.display().to_string();

	assert_eq!(
		missing_lease_steer_event.payload()["context"]["process_alive"].as_bool(),
		Some(true)
	);
	assert_eq!(
		missing_lease_steer_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
	assert_eq!(missing_lease_interrupt_event.payload()["lane"]["run_lease"].as_bool(), Some(false));
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["process_alive"].as_bool(),
		Some(true)
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["execution_liveness"].as_str(),
		Some("process_alive")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["ownership_state"].as_str(),
		Some("orphaned_live_thread")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["liveness_state"].as_str(),
		Some("process_alive")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["lane_control_next_action"].as_str(),
		Some("inspect_or_interrupt_orphaned_live_thread")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["lane_control_conditions"][0].as_str(),
		Some("run_lease_missing")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["control_capability"]["channel_path"]
			.as_str(),
		Some(expected_channel_path.as_str())
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
	assert!(events.iter().any(|event| {
		event.event_type() == "control_action"
			&& event.payload()["outcome"] == "fallback"
			&& event.payload()["reason"] == "hard_interrupt_fallback"
	}));
}
