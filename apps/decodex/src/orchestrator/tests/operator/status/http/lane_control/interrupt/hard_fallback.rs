use crate::orchestrator::tests::operator::status::http::{
	self, Command, ProjectRegistration, StateStore, fs, lane_control::support, orchestrator, state,
};

#[cfg(unix)]
#[test]
fn operator_lane_interrupt_api_force_hard_fallbacks_after_run_lease_missing() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut fixture = support::run_lease_missing_control_fixture(&config, &state_store);
	let project_id = config.service_id().to_owned();
	let issue_identifier = fixture.issue.identifier.clone();
	let run_id = "pub-101-attempt-1";
	let body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","force":true}}"#
	);
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		body
	);
	let steer_body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","expectedTurnId":"turn-1","message":"preserve partial work and report current state"}}"#
	);
	let steer_request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_STEER_ENDPOINT_PATH,
		steer_body.len(),
		steer_body
	);
	let steer_response = String::from_utf8(orchestrator::build_operator_lane_steer_http_response(
		&state_store,
		steer_request.as_bytes(),
	))
	.expect("lane steer response should be utf-8");
	let steer_data = support::operator_json_response_body(&steer_response, "lane steer");
	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let data = support::operator_json_response_body(&response, "lane interrupt");

	support::reap_run_lease_missing_child(&mut fixture);

	assert!(steer_response.starts_with("HTTP/1.1 409 Conflict\r\n"), "{steer_response}");
	assert_eq!(steer_data["outcome"], "rejected");
	assert_eq!(steer_data["reason"], "run_lease_missing");
	assert_eq!(steer_data["failureClass"], "run_control_action_failed");
	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "rejected");
	assert_eq!(data["softInterrupt"]["errorClass"], "run_lease_missing");
	assert_eq!(data["hardInterrupt"]["classification"], "hard_interrupt_fallback");
	assert_eq!(data["hardInterrupt"]["status"], "sent");
	assert_eq!(
		data["hardInterrupt"]["processId"].as_u64(),
		Some(u64::from(fixture.child_process_id))
	);
	assert_eq!(data["hardInterrupt"]["processAliveAfter"], false);

	support::assert_run_lease_missing_control_audit(&config, &state_store, &fixture, run_id);
}

#[cfg(unix)]
#[test]
fn operator_lane_interrupt_api_force_hard_fallbacks_terminal_live_process_without_soft_owner() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let project_id = config.service_id().to_owned();
	let issue_identifier = issue.identifier.clone();
	let run_id = "pub-101-attempt-1";
	let body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","force":true}}"#
	);
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		body
	);
	let child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("lane child process should start");
	let child_process_id = child.id();
	let mut child = child;

	fs::create_dir_all(&worktree_path).expect("worktree should exist");
	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, child_process_id)
		.expect("activity marker should write");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("run attempt should record");
	state_store.update_run_thread(run_id, "thread-1").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-1").expect("turn should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let data = support::operator_json_response_body(&response, "lane interrupt");

	if orchestrator::process_is_alive(child_process_id) {
		child.kill().expect("lane child process should be killable after failed fallback");
	}

	child.wait().expect("lane child process should reap");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "unavailable");
	assert_eq!(data["softInterrupt"]["errorClass"], "lane_not_active");
	assert_eq!(data["hardInterrupt"]["classification"], "hard_interrupt_fallback");
	assert_eq!(data["hardInterrupt"]["status"], "sent");
	assert_eq!(data["hardInterrupt"]["processId"].as_u64(), Some(u64::from(child_process_id)));
	assert_eq!(data["hardInterrupt"]["processAliveAfter"], false);
}
