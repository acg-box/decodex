use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ChildAgentActivitySummary, EffectiveRuntimeMarker, FakeTracker, OffsetDateTime,
	OperatorRunStatus, OperatorStatusSnapshot, ProtocolActivityMarker, ProtocolActivitySummary,
	RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT, RecoveredRuntimeState, ReviewCheckpointSeed,
	ReviewPolicyCheckpointInput, ServiceConfig, StateStore, TEST_SERVICE_ID, fs, orchestrator,
	process, state, tracker,
};
pub(super) fn assert_terminal_pending_status_projection(snapshot: &OperatorStatusSnapshot) {
	let project = snapshot.projects.first().expect("project summary should exist");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal-pending run should remain inspectable in recent runs");

	assert!(
		snapshot.current_lanes.is_empty(),
		"terminal-finalized runs must not keep presenting as active execution"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(run.status, "review_handoff_pending");
	assert_eq!(run.attempt_status, "running");
	assert_eq!(run.phase, "terminal_pending");
	assert_eq!(run.wait_reason.as_deref(), Some("review_handoff_writeback"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert!(!run.suspected_stall);
	assert_eq!(run.last_event_type.as_deref(), Some("skills/changed"));
	assert_eq!(
		run.loop_status.as_ref().map(|status| status.summary.as_str()),
		Some("terminal lifecycle: review_handoff_pending")
	);
}

pub(super) fn assert_terminal_pending_lane_inspect(state_store: &StateStore) {
	let response = String::from_utf8(orchestrator::build_operator_lane_inspect_http_response(
		state_store,
		format!(
			"GET {}?projectId=pubfi&issue=PUB-101&runId=run-1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
			orchestrator::OPERATOR_LANE_INSPECT_ENDPOINT_PATH
		)
		.as_bytes(),
	))
	.expect("lane inspect response should be utf-8");
	let body = operator_status_response_body(&response, "lane inspect");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(body.contains(r#""matchedRunCount":1"#));
	assert!(body.contains(r#""status":"review_handoff_pending""#));
	assert!(body.contains(r#""phase":"terminal_pending""#));
	assert!(body.contains(r#""waitReason":"review_handoff_writeback""#));
	assert!(body.contains(r#""currentOperation":"review_writeback""#));
	assert!(body.contains(r#""runLease":false"#));
	assert!(body.contains(r#""executionLiveness":"not_running""#));
	assert!(body.contains(r#""softInterruptAvailable":false"#));
	assert!(body.contains(r#""hardInterruptAvailable":false"#));
}

pub(super) fn assert_terminal_pending_interrupt_rejects_force(state_store: &StateStore) {
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":"run-1","force":true}"#;
	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		state_store,
		format!(
			"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
			orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
			body.len(),
			String::from_utf8_lossy(body)
		)
		.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let body = operator_status_response_body(&response, "lane interrupt");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(body.contains(r#""classification":"soft_interrupt_unavailable""#));
	assert!(body.contains(r#""errorClass":"lane_not_active""#));
	assert!(body.contains(r#""hardInterrupt":null"#));
}

fn operator_status_response_body<'a>(response: &'a str, context: &str) -> &'a str {
	response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.unwrap_or_else(|| running_lanes::panic!("{context} response should include body"))
}

#[test]
fn operator_status_snapshot_promotes_starting_after_app_server_activity() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "model/response",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.queue_lease_state, "held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.effective_model.as_deref(), Some("gpt-5.4"));
	assert!(rendered.contains("status: running"));
	assert!(rendered.contains("attempt_status: starting"));
}

#[test]
fn operator_status_snapshot_counts_stale_starting_run_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nlast_progress_unix_epoch={stale_activity}\n"
		),
	)
	.expect("stale processless marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, None);
	assert!(run.protocol_idle_for_seconds.is_some_and(|idle| {
		u64::try_from(idle).is_ok_and(|idle| idle >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
	}));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_shadows_stale_attempt_when_newer_leased_attempt_exists() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let current_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(current_run_id, &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, current_run_id, "In Progress")
		.expect("current run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, current_run_id);
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 1);
	assert_eq!(project.attention_count, 0);
	assert!(rendered.contains("Current lanes: 1"));
	assert!(rendered.contains("Running lanes: 1"));
	assert!(!rendered.contains(&format!("- run_id: {stale_run_id}")));
	assert!(
		rendered.contains(&format!("lifecycle_evidence: run={stale_run_id}")),
		"shadowed attempts should remain available only in lifecycle evidence"
	);
}

#[test]
fn operator_status_snapshot_shadows_stale_attempt_when_newer_attempt_has_released_lease() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let newer_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 3, "succeeded")
		.expect("newer run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == newer_run_id));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 0);
}

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

#[test]
fn operator_status_snapshot_rolls_current_child_bucket_elapsed_time_into_bucket() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let started_at = OffsetDateTime::now_utc().unix_timestamp() - 90;

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 1,
			last_event_type: "item/tool/call",
			child_agent_activity: Some(&ChildAgentActivitySummary {
				buckets: vec![state::ChildAgentActivityBucket {
					name: String::from("Tracker"),
					event_count: 1,
					tool_call_count: 1,
					..state::ChildAgentActivityBucket::default()
				}],
				current_bucket: Some(String::from("Tracker")),
				current_detail: Some(String::from("issue_progress_checkpoint")),
				current_started_unix_epoch: Some(started_at),
				current_elapsed_seconds: Some(0),
				event_count: 1,
				tool_call_count: 1,
				..ChildAgentActivitySummary::default()
			}),
			protocol_activity: None,
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let activity = run.child_agent_activity.as_ref().expect("activity should render");
	let protocol_activity =
		run.protocol_activity.as_ref().expect("protocol fallback should render");
	let tracker_bucket =
		activity.buckets.iter().find(|bucket| bucket.name == "Tracker").expect("tracker bucket");

	assert_eq!(run.wait_reason.as_deref(), Some("tool_execution"));
	assert_eq!(protocol_activity.waiting_reason.as_deref(), Some("tool_execution"));
	assert_eq!(
		snapshot.projects[0].waiting_lane_count, 0,
		"normal active tool execution is running work, not project-level waiting"
	);
	assert_eq!(run.lifecycle_metrics.attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 1);
	assert_eq!(run.lifecycle_metrics.phases.len(), 1);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert!(activity.current_elapsed_seconds.is_some_and(|elapsed| elapsed >= 90));
	assert!(
		tracker_bucket.wall_seconds >= 90,
		"current tool-call elapsed time should contribute to tracker bucket wall time"
	);
}

#[test]
fn operator_status_current_lane_lifecycle_reconstructs_all_issue_attempts() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let worktree_path = config.worktree_root().join("PUB-101");
	let development_activity = ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 600,
			event_count: 2,
			tool_call_count: 1,
			input_tokens: 100,
			output_tokens: 30,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds: 600,
		event_count: 2,
		tool_call_count: 1,
		input_tokens_cumulative: 100,
		output_tokens_cumulative: 30,
		..ChildAgentActivitySummary::default()
	};
	let review_activity = ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 300,
			event_count: 3,
			tool_call_count: 2,
			input_tokens: 200,
			output_tokens: 40,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds: 300,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_cumulative: 200,
		output_tokens_cumulative: 40,
		..ChildAgentActivitySummary::default()
	};

	state_store
		.record_run_attempt("run-development", &issue.id, 1, "failed")
		.expect("development attempt should record");
	state_store
		.record_run_activity_summary("run-development", 1, Some(&development_activity), None)
		.expect("development activity should record");
	state_store
		.record_run_attempt("run-review", &issue.id, 2, "running")
		.expect("review attempt should record");
	state_store
		.record_run_activity_summary("run-review", 2, Some(&review_activity), None)
		.expect("review activity should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-review", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			run_id: "run-review",
			attempt_number: 2,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: r#"{
				"review_cost_control": {
					"review_class": "compact_current_head_review",
					"risk_class": "low",
					"compact_eligible": true,
					"fallback_reason": null
				},
				"finding_route_summary": {
					"route_counts": [{"route": "risk_note", "count": 1}],
					"next_action": "Carry the routed risk note into follow-up planning."
				},
				"finding_policy": {
					"active_fingerprints": [],
					"stop_fingerprint": null
				}
			}"#,
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(run.lifecycle_metrics.attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.missing_attempt_count, 0);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 3);
	assert_eq!(run.lifecycle_metrics.input_tokens_cumulative, 300);
	assert_eq!(run.lifecycle_metrics.output_tokens_cumulative, 70);
	assert_eq!(run.lifecycle_metrics.phases.len(), 2);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert_eq!(run.lifecycle_metrics.phases[0].attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[0].wall_seconds, 600);
	assert_eq!(run.lifecycle_metrics.phases[1].phase, "review");
	assert_eq!(run.lifecycle_metrics.phases[1].attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[1].wall_seconds, 300);

	assert_compact_review_checkpoint_status(run);
}

#[test]
fn operator_status_supersedes_stale_repair_findings_after_clean_handoff_checkpoint() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let run_id = "run-review";
	let repair_head = "1111111111111111111111111111111111111111";
	let clean_head = "2222222222222222222222222222222222222222";

	state_store
		.record_run_attempt(run_id, &issue.id, 2, "running")
		.expect("review attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, run_id, "In Progress")
		.expect("lease should record");

	let stale_repair_next_action = seed_stale_repair_and_clean_handoff_checkpoints(
		&state_store,
		&config,
		&issue.id,
		run_id,
		repair_head,
		clean_head,
	);
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let review = loop_status.review.as_ref().expect("review status should render");
	let checkpoint = review.checkpoint.as_ref().expect("review checkpoint should render");

	assert_eq!(review.phase, "handoff");
	assert_eq!(review.status, "clean");
	assert_eq!(checkpoint.head_sha, clean_head);
	assert!(checkpoint.active_fingerprints.is_empty());
	assert_eq!(run.policy_state, "allowed");
	assert_eq!(
		run.lane_control_next_action,
		"Push or update the PR and record review handoff for the clean current lane head."
	);
	assert_ne!(loop_status.next_action.as_deref(), Some(stale_repair_next_action));
}

fn seed_stale_repair_and_clean_handoff_checkpoints(
	state_store: &StateStore,
	config: &ServiceConfig,
	issue_id: &str,
	run_id: &str,
	repair_head: &str,
	clean_head: &str,
) -> &'static str {
	let stale_repair_next_action = "Repair the stale review finding.";
	let repair_details_json = r#"{
		"finding_route_summary": {
			"route_counts": [{"route": "current_blocker", "count": 1}],
			"next_action": "Repair the stale review finding."
		},
		"finding_policy": {
			"active_fingerprints": ["stale-finding"],
			"stop_fingerprint": null
		}
	}"#;
	let clean_details_json = r#"{
		"review_cost_control": {
			"review_class": "full_current_head_review",
			"risk_class": "localized",
			"compact_eligible": false,
			"fallback_reason": "repair_review"
		},
		"finding_route_summary": {
			"route_counts": [],
			"next_action": null
		},
		"finding_policy": {
			"active_fingerprints": [],
			"stop_fingerprint": null
		}
	}"#;

	seed_review_policy_checkpoint_with_event(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "repair",
			status: "findings",
			head_sha: repair_head,
			nonclean_rounds: 1,
			details_json: repair_details_json,
		},
	);
	seed_review_policy_checkpoint_with_event(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "handoff",
			status: "clean",
			head_sha: clean_head,
			nonclean_rounds: 0,
			details_json: clean_details_json,
		},
	);
	seed_review_policy_checkpoint(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "repair",
			status: "findings",
			head_sha: repair_head,
			nonclean_rounds: 1,
			details_json: repair_details_json,
		},
	);

	stale_repair_next_action
}

fn seed_review_policy_checkpoint_with_event(
	state_store: &StateStore,
	config: &ServiceConfig,
	seed: ReviewCheckpointSeed<'_>,
) {
	seed_review_policy_checkpoint(state_store, config, seed);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			seed.issue_id,
			seed.run_id,
			2,
			"review_checkpoint",
			serde_json::json!({
				"phase": seed.phase,
				"status": seed.status,
				"head_sha": seed.head_sha,
				"nonclean_rounds": seed.nonclean_rounds
			}),
		)
		.expect("review checkpoint event should record");
}

fn seed_review_policy_checkpoint(
	state_store: &StateStore,
	config: &ServiceConfig,
	seed: ReviewCheckpointSeed<'_>,
) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: seed.issue_id,
			run_id: seed.run_id,
			attempt_number: 2,
			phase: seed.phase,
			review_level: config.codex().review_level().as_str(),
			status: seed.status,
			head_sha: seed.head_sha,
			nonclean_rounds: seed.nonclean_rounds,
			details_json: seed.details_json,
		})
		.expect("review policy checkpoint should record");
}

fn assert_compact_review_checkpoint_status(run: &OperatorRunStatus) {
	let review_checkpoint = run
		.loop_status
		.as_ref()
		.and_then(|loop_status| loop_status.review.as_ref())
		.and_then(|review| review.checkpoint.as_ref())
		.expect("review checkpoint should render in loop status");

	assert_eq!(review_checkpoint.route_counts[0].route, "risk_note");
	assert_eq!(review_checkpoint.route_counts[0].count, 1);
	assert_eq!(review_checkpoint.review_class.as_deref(), Some("compact_current_head_review"));
	assert_eq!(review_checkpoint.risk_class.as_deref(), Some("low"));
	assert_eq!(review_checkpoint.compact_eligible, Some(true));
	assert_eq!(review_checkpoint.fallback_reason, None);
	assert_eq!(
		review_checkpoint.route_next_action.as_deref(),
		Some("Carry the routed risk note into follow-up planning.")
	);
}

fn sample_lifecycle_activity(
	wall_seconds: i64,
	event_count: i64,
	tool_call_count: i64,
	input_tokens: i64,
	output_tokens: i64,
) -> ChildAgentActivitySummary {
	ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds,
			event_count,
			tool_call_count,
			input_tokens,
			output_tokens,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds,
		event_count,
		tool_call_count,
		input_tokens_cumulative: input_tokens,
		output_tokens_cumulative: output_tokens,
		..ChildAgentActivitySummary::default()
	}
}

#[test]
fn operator_status_current_lane_lifecycle_recovers_from_local_evidence_after_restart() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let worktree_path = config.worktree_root().join("PUB-101");
	let development_activity = sample_lifecycle_activity(480, 4, 2, 600, 120);
	let review_activity = sample_lifecycle_activity(240, 3, 1, 300, 90);

	state_store
		.upsert_lease("pubfi", &issue.id, "run-review", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_activity_summary("run-development", 1, Some(&development_activity), None)
		.expect("development activity should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-development",
			1,
			"issue_progress_checkpoint",
			serde_json::json!({ "source": "restart-recovery-test" }),
		)
		.expect("development private evidence should record");
	state_store
		.record_run_activity_summary("run-review", 2, Some(&review_activity), None)
		.expect("review activity should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			run_id: "run-review",
			attempt_number: 2,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-review",
			attempt_number: 2,
			thread_id: Some("thread-review"),
			turn_id: Some("turn-review"),
			event_count: 3,
			last_event_type: "model/response",
			child_agent_activity: Some(&review_activity),
			protocol_activity: None,
		},
	)
	.expect("worktree activity marker should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should recover");

	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(run.run_id, "run-review");
	assert_eq!(run.lifecycle_metrics.attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.recorded_attempt_count, 0);
	assert_eq!(run.lifecycle_metrics.recovered_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.current_snapshot_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 3);
	assert_eq!(run.lifecycle_metrics.input_tokens_cumulative, 900);
	assert_eq!(run.lifecycle_metrics.output_tokens_cumulative, 210);
	assert_eq!(run.lifecycle_metrics.phases.len(), 2);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert_eq!(run.lifecycle_metrics.phases[0].recovered_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[1].phase, "review");
	assert_eq!(run.lifecycle_metrics.phases[1].current_snapshot_attempt_count, 1);
	assert!(run.lifecycle_metrics.attempt_evidence.iter().any(|attempt| {
		attempt.run_id == "run-development"
			&& attempt.source == "recovered"
			&& attempt
				.evidence
				.iter()
				.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint")
	}));
	assert!(
		run.lifecycle_metrics.attempt_evidence.iter().any(|attempt| attempt.run_id == "run-review"
			&& attempt.source == "current_snapshot"
			&& attempt.evidence.iter().any(|evidence| evidence == "worktree_activity_marker"))
	);
}

#[test]
fn operator_status_snapshot_uses_structured_protocol_activity_summary() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("approval_or_user_input")),
		rate_limit_status: Some(String::from("primary")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("plan/update"),
				category: String::from("plan"),
				detail: Some(String::from("verify")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("item/tool/requestUserInput"),
				category: String::from("item"),
				detail: None,
			},
		],
	};

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "item/tool/requestUserInput",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("approval_or_user_input"));
	assert_eq!(run.protocol_activity.as_ref(), Some(&protocol_activity));
	assert_eq!(
		snapshot.projects[0].waiting_lane_count, 1,
		"approval or user-input waits should remain project-level waiting"
	);
	assert!(rendered.contains("protocol_activity: turn=running; waiting=approval_or_user_input; rate_limit=primary; recent=item/tool/requestUserInput, plan/update:verify"));
}

#[test]
fn operator_status_snapshot_prefers_newer_protocol_marker_over_stale_archive_event() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_event("run-1", 1, "thread/archive/discarded", "{}")
		.expect("archive event should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "item/tool/call",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.last_event_type.as_deref(), Some("item/tool/call"));
	assert_eq!(run.event_count, 2);
}

#[test]
fn operator_status_snapshot_sanitizes_private_protocol_activity_details() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("tool_execution")),
		rate_limit_status: None,
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker path=/srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker (/srv/decodex/runtime)")),
			},
		],
	};

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "configWarning",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let summary = run.protocol_activity.as_ref().expect("protocol summary should render");

	assert!(
		summary
			.recent_events
			.iter()
			.all(|event| event.detail.as_deref() == Some("redacted_sensitive_detail"))
	);
	assert!(rendered.contains("configWarning:redacted_sensitive_detail"));
	assert!(!rendered.contains("path=/srv"));
	assert!(!rendered.contains("(/srv"));
}

#[test]
fn operator_status_snapshot_ignores_marker_from_newer_attempt_for_stored_run() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("stored run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-2", 2, process::id())
		.expect("newer attempt marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-2",
		2,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.phase, "failed");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.process_id, None);
	assert_eq!(run.process_alive, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
}

#[test]
fn operator_status_snapshot_keeps_all_current_lanes_when_recent_runs_are_limited() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	for (run_id, issue, branch_suffix) in
		[("run-1", &first_issue, "101"), ("run-2", &second_issue, "102")]
	{
		state_store
			.record_run_attempt(run_id, &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				&format!("x/pubfi-pub-{branch_suffix}"),
				&config.worktree_root().join(&issue.identifier).display().to_string(),
			)
			.expect("worktree should record");
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.current_lanes.len(), 2);
	assert!(snapshot.current_lanes.iter().all(|run| run.run_lease));
}

#[test]
fn operator_status_snapshot_keeps_terminal_run_after_lane_cleanup() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);

	state_store.record_run_attempt("run-done", &issue.id, 1, "running").expect("run should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-done", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store.update_run_status("run-done", "succeeded").expect("terminal status should update");
	state_store.clear_lease(&issue.id).expect("terminal cleanup should clear run lease");
	state_store.clear_worktree(&issue.id).expect("terminal cleanup should clear worktree");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.recent_runs[0].run_id, "run-done");
	assert_eq!(snapshot.recent_runs[0].phase, "completed");
	assert!(!snapshot.recent_runs[0].run_lease);
	assert_eq!(snapshot.recent_runs[0].branch_name, None);
	assert_eq!(snapshot.recent_runs[0].worktree_path, None);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].latest_run.run_id, "run-done");
	assert!(rendered.contains("Run ledger shown: 1 issue lanes from 1 history attempts"));
	assert!(rendered.contains("run_id: run-done"));
}

#[test]
fn status_hydration_does_not_fabricate_run_leases_for_recovered_candidates() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	orchestrator::hydrate_status_snapshot_state(
		&config,
		&state_store,
		RecoveredRuntimeState { recoverable_issues: vec![issue.clone()] },
	)
	.expect("status hydration should succeed");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert!(
		snapshot.current_lanes.is_empty(),
		"recovered retry candidates should not appear as run leased runs"
	);
	assert!(
		snapshot.recent_runs.is_empty(),
		"status hydration should not persist synthetic recovered runs"
	);
}

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_thread_and_event_metadata_from_marker() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = running_lanes::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");
	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	orchestrator::hydrate_status_snapshot_state(&config, &state_store, recovered_state)
		.expect("status hydration should succeed");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.current_lanes[0].turn_id.as_deref(), Some("turn-1"));
	assert_eq!(snapshot.current_lanes[0].thread_status.as_deref(), Some("active"));
	assert_eq!(
		snapshot.current_lanes[0].thread_active_flags,
		vec![String::from("waitingOnApproval")]
	);
	assert!(snapshot.current_lanes[0].interactive_requested);
	assert_eq!(snapshot.current_lanes[0].event_count, 2);
	assert_eq!(snapshot.current_lanes[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.current_lanes[0].effective_model.as_deref(), Some("gpt-5.4"));
	assert_eq!(snapshot.current_lanes[0].effective_model_provider.as_deref(), Some("openai"));
	assert_eq!(snapshot.current_lanes[0].effective_approval_policy.as_deref(), Some("never"));
	assert_eq!(snapshot.current_lanes[0].effective_sandbox_mode.as_deref(), Some("workspaceWrite"));
	assert!(snapshot.current_lanes[0].last_event_at.is_some());
}
