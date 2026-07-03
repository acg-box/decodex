use std::{fs, process};

use time::OffsetDateTime;

use crate::{
	agent::{MODEL_EXECUTION_IDLE_TIMEOUT, RUN_LEASE_IDLE_TIMEOUT},
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker, TEST_SERVICE_ID, recovery_reconciliation::support},
	},
	state::{self, RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_REPO_GATE, StateStore},
	tracker,
};

#[test]
fn stalled_idle_duration_ignores_future_last_activity() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::reconciliation_sample_service_owned_issue("In Progress");
	let run_id = "run-future-activity";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");

	assert_eq!(
		orchestrator::stalled_idle_duration(
			&state_store,
			&state_store
				.run_attempt(run_id)
				.expect("run lookup should succeed")
				.expect("run attempt should exist"),
			None,
			last_activity - 1
		)
		.expect("idle duration should evaluate"),
		None
	);
}

#[test]
fn run_lease_reconciliation_uses_worktree_activity_marker_from_child_process() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-shared-activity";
	let worktree_path = config.worktree_root().join("PUB-101");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

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
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);

	fs::write(
		&marker_path,
		format!(
			"run_id={run_id}\nattempt_number=1\nlast_activity_unix_epoch={}\n",
			last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64
		),
	)
	.expect("activity marker should write");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"fresh child activity marker should prevent daemon stall reconciliation"
	);
}

#[test]
fn run_lease_reconciliation_allows_running_model_execution_until_model_timeout() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-model-execution-idle";
	let worktree_path = config.worktree_root().join("PUB-101-model");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

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
			"x/pubfi-pub-101-model",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");
	let protocol_activity = r#"{"turn_status":"running","waiting_reason":"model_execution","rate_limit_status":null,"recent_events":[]}"#;

	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={run_id}\nattempt_number=1\nlast_activity_unix_epoch={last_activity}\nlast_protocol_activity_unix_epoch={last_activity}\nlast_progress_unix_epoch={last_activity}\nprotocol_activity={protocol_activity}\n"
		),
	)
	.expect("activity marker should write");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"running model execution should not stall on the generic active idle timeout"
	);

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::Stalled{ idle_for }
					if idle_for >= MODEL_EXECUTION_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn run_lease_reconciliation_defers_live_repo_gate_even_with_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-live-repo-gate";
	let worktree_path = config.worktree_root().join("PUB-101-repo-gate");

	tests::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-101-repo-gate",
			".worktrees/PUB-101-repo-gate",
			"main",
		],
	);
	fs::write(worktree_path.join("README.md"), "repo gate is still running\n")
		.expect("tracked worktree file should change");

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
			"x/pubfi-pub-101-repo-gate",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_operation_marker_for_process(
		&worktree_path,
		run_id,
		1,
		process::id(),
		RUN_OPERATION_REPO_GATE,
	)
	.expect("live repo-gate marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker should load")
		.expect("marker should exist");
	let last_activity = marker.last_activity_unix_epoch().expect("marker should record activity");
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"live repo gate operation should retain scheduler ownership instead of attention"
	);

	state::write_run_operation_marker_for_process(
		&worktree_path,
		run_id,
		1,
		u32::MAX,
		RUN_OPERATION_REPO_GATE,
	)
	.expect("stopped repo-gate marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker should reload")
		.expect("marker should exist");
	let last_activity = marker.last_activity_unix_epoch().expect("marker should record activity");
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::StalledRetainedPartialProgress{ idle_for }
					if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn exited_child_reconciliation_defers_dirty_worktree_with_retry_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-failed-with-retry-marker";
	let worktree_path = config.worktree_root().join("PUB-101-retry-marker");

	tests::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-101-retry-marker",
			".worktrees/PUB-101-retry-marker",
			"main",
		],
	);
	fs::write(worktree_path.join("README.md"), "retry still owns this patch\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101-retry-marker",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_retry_schedule(&worktree_path, run_id, 1, "failure", 1)
		.expect("retry schedule marker should write");

	state_store
		.append_event(run_id, 1, "turn/completed", "{\"status\":\"completed\"}")
		.expect("protocol event should record");

	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child reconciliation should evaluate");

	assert!(
		actions.is_empty(),
		"retry marker should keep failed dirty worktree under retry scheduler ownership"
	);
}
