use time::OffsetDateTime;

use crate::orchestrator::tests::operator::status::running_lanes::{
	self, MODEL_EXECUTION_IDLE_TIMEOUT, ProtocolActivityMarker, ProtocolActivitySummary,
	RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_AGENT_RUN, StateStore, fs, orchestrator, state,
};

#[test]
fn operator_status_snapshot_diagnoses_protocol_only_model_execution() {
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

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/status/changed"),
				category: String::from("thread"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/goal/updated"),
				category: String::from("protocol"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
		],
		..ProtocolActivitySummary::default()
	};

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "account/rateLimits/updated",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol-only marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("model_execution"));
	assert_eq!(run.progress_diagnostic.as_deref(), Some("protocol_only_activity"));
	assert_eq!(run.execution_liveness, "process_alive");
	assert!(run.suspected_stall);
	assert_ne!(run.last_progress_at, run.last_protocol_activity_at);
	assert!(rendered.contains("progress_diagnostic: protocol_only_activity"));
}
