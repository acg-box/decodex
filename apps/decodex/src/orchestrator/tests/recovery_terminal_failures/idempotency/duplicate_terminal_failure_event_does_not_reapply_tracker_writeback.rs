use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, ReviewHandoffNeedsAttention,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self},
	worktree::WorktreeSpec,
};

#[test]
fn duplicate_terminal_failure_event_does_not_reapply_tracker_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Review", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
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
	let error = Report::new(ReviewHandoffNeedsAttention {
		issue_identifier: issue.identifier.clone(),
		pr_url: String::from("https://github.com/helixbox/pubfi-mono-v2/pull/307"),
		run_id: issue_run.run_id.clone(),
	});

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("first terminal failure writeback should apply");
	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("duplicate terminal failure writeback should no-op");

	assert_eq!(tracker.state_updates.borrow().len(), 1);
	assert_eq!(tracker.label_additions.borrow().len(), 1);
	assert_eq!(tracker.label_removals.borrow().len(), 1);
	assert_eq!(
		tracker
			.comments
			.borrow()
			.iter()
			.filter(|comment| comment.contains("review_handoff_writeback_failed"))
			.count(),
		1
	);
	assert_eq!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.len(),
		1
	);
}
