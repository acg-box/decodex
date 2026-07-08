use std::{fs, time::Duration};

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition, RunLeaseReconciliation,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::{self, StateStore},
	tracker::records,
	worktree::WorktreeManager,
};

#[test]
fn routes_to_attention_after_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = support::reconciliation_sample_service_owned_issue("In Progress");
	let run_id = "run-stalled-exhausted";
	let worktree_path = config.worktree_root().join("PUB-101");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_budget_attempt_count(&worktree_path, "older-run", 2, 3)
		.expect("retry budget marker should write");

	state_store
		.record_run_attempt(run_id, &issue.id, 3, "running")
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

	let action = RunLeaseReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: RunLeaseDisposition::Stalled {
			idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
		},
		workflow: workflow.clone(),
	};

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("reconciliation should succeed");

	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("stalled_run_detected")
			&& comment.contains("needs attention")
			&& comment.contains("clear label `decodex:needs-attention`")
	}));

	let ledger_event = tracker
		.comments
		.borrow()
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("exhausted stalled run should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "terminal_failure");
	assert_eq!(ledger_event.error_class.as_deref(), Some("stalled_run_detected"));
}
