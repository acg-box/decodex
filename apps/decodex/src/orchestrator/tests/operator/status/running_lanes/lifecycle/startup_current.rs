use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ChildAgentActivitySummary, EffectiveRuntimeMarker, OffsetDateTime,
	ProtocolActivityMarker, RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT, StateStore, fs,
	orchestrator, state,
};
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
