use std::fs;

use crate::{
	agent::{MODEL_EXECUTION_IDLE_TIMEOUT, RUN_LEASE_IDLE_TIMEOUT},
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::{RUN_ACTIVITY_MARKER_FILE, StateStore},
};

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
