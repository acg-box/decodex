use std::{fs, time::Duration};

use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition, RunLeaseReconciliation,
		tests::{self, FakeTracker},
	},
	state::{self, StateStore},
	tracker::records,
	worktree::WorktreeManager,
};

#[test]
fn stalled_run_reconciliation_schedules_retry_before_attention_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = tests::sample_issue("In Progress", &[]);
	let run_id = "run-stalled";
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

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("stalled_run_detected")
			&& comment.contains("retry the stalled lane automatically")
	}));
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.all(|comment| !comment.contains("clear label `decodex:needs-attention`"))
	);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.all(|comment| records::parse_linear_execution_event_record(comment).is_none())
	);

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry marker should load")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert!(
		marker.retry_ready_at_unix_epoch().is_some_and(
			|retry_ready_at| retry_ready_at > OffsetDateTime::now_utc().unix_timestamp()
		)
	);
}
