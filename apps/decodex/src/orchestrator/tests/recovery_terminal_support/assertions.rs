use std::fs;

use color_eyre::Report;

use crate::orchestrator::IssueRunPlan;
#[rustfmt::skip]
use crate::orchestrator::tests::{self, FakePullRequestReviewStateInspector, FakeTracker};
#[rustfmt::skip]
use crate::orchestrator::{self, IssueDispatchMode};
#[rustfmt::skip]
use crate::state::StateStore;
#[rustfmt::skip]
use crate::worktree::WorktreeSpec;
use crate::orchestrator::tests::recovery_terminal_support::CloseoutIdentityFixture;

pub(in crate::orchestrator::tests) fn assert_closeout_lane_ready(
	fixture: &CloseoutIdentityFixture,
) {
	let mut merged_review_state = tests::sample_pull_request_review_state(
		&fixture.pr_url,
		&fixture.worktree.branch_name,
		&fixture.head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
	)
	.expect("post-review lane status should build");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy(
			&fixture.tracker,
			&fixture.issue,
			&fixture.config,
			&fixture.workflow,
			&fixture.state_store,
		)
		.expect("closeout policy should evaluate"),
		"closeout dispatch policy should accept the merged retained lane: {:?}",
		orchestrator::closeout_dispatch_block_reason(
			&fixture.tracker,
			&fixture.issue,
			&fixture.config,
			&fixture.workflow,
			&fixture.state_store,
		)
		.expect("closeout block reason should evaluate")
	);
}

pub(in crate::orchestrator::tests) fn assert_app_server_failure_requires_attention(
	error: Report,
	error_class: &str,
	next_action_fragment: &str,
) {
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

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("app-server failure handling should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains(error_class)
			&& comment.contains(next_action_fragment)
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
