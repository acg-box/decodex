use std::fs;

use color_eyre::Report;

use crate::{
	agent::{ReviewPolicyStopReason, ReviewPolicyStopRequested},
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, {self},
		},
	},
	state::{self, StateStore},
	worktree::WorktreeSpec,
};

#[test]
fn review_policy_exhausted_failures_start_architecture_recovery_pre_pr() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(ReviewPolicyStopRequested {
		head_sha: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		issue_identifier: issue.identifier.clone(),
		fingerprint: Some(String::from("review_finding:test")),
		nonclean_rounds: Some(3),
		reason: ReviewPolicyStopReason::Exhausted,
		run_id: issue_run.run_id.clone(),
	});

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("review policy failure handling should succeed");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("architecture_recovery_started")
			&& comment.contains("materially different architecture recovery strategy")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| { comment.contains("retryable_execution_failure") })
	);

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)
		.expect("run marker should read")
		.expect("run marker should exist");

	assert_eq!(marker.retry_kind(), Some("architecture_recovery"));

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["loop_guardrail"]["reason"] == "review_churn"
			&& event.payload()["authority_boundary_check"]["disposition"] == "within_authority"
	}));
}

#[test]
fn review_policy_blocked_failures_skip_retry_and_require_attention_in_review() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(ReviewPolicyStopRequested {
		head_sha: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		issue_identifier: issue.identifier.clone(),
		fingerprint: None,
		nonclean_rounds: Some(1),
		reason: ReviewPolicyStopReason::Blocked,
		run_id: issue_run.run_id.clone(),
	});

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("review policy failure handling should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_policy_blocked")
			&& comment.contains("resolve the blocker manually")
			&& comment.contains("do not dispatch research")
			&& comment.contains("clear label `decodex:needs-attention`")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| { comment.contains("retryable_execution_failure") })
	);
}
