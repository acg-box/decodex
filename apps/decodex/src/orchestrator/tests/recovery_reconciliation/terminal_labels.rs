use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, RunLeaseDisposition, RunLeaseReconciliation,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_lease_reconciliation_ignores_startable_preclaim_states() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-startable",
		"PUB-204",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt("run-startable", &issue.id, 1, "running")
		.expect("run attempt should record");
	tests::seed_test_lane_claim(&state_store, config.service_id(), &issue.id, "run-startable");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-startable", "In Progress")
		.expect("lease should record");

	let now = OffsetDateTime::now_utc().unix_timestamp() + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("run lease inspection should succeed");

	assert!(actions.is_empty(), "startable pre-claim states should not be interrupted");
}

#[test]
fn run_lease_reconciliation_clears_terminal_lane_labels() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-terminal";
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	tests::seed_test_lane_claim(&state_store, config.service_id(), &issue.id, run_id);
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
		disposition: RunLeaseDisposition::Terminal,
		workflow: workflow.clone(),
	};

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("terminal reconciliation should succeed");

	assert!(
		state_store
			.claim_for_lane(config.service_id(), &issue.id)
			.expect("claim lookup")
			.is_none()
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn run_lease_reconciliation_keeps_nonterminal_not_dispatchable_worktrees() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = tests::sample_issue("Todo", &[]);
	let run_id = "run-not-dispatchable";
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	tests::seed_test_lane_claim(&state_store, config.service_id(), &issue.id, run_id);
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
		disposition: RunLeaseDisposition::NotDispatchable,
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

	assert!(
		state_store
			.claim_for_lane(config.service_id(), &issue.id)
			.expect("claim lookup")
			.is_none()
	);
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
		"interrupted"
	);
}
