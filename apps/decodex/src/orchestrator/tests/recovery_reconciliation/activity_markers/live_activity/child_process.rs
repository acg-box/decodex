use std::fs;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::{RUN_ACTIVITY_MARKER_FILE, StateStore},
};

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
