use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, PassiveRetainedAttentionRuntime,
		RetainedReviewRunIdentity, ReviewHandoffNeedsAttention,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self},
	worktree::{WorktreeManager, WorktreeSpec},
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

#[test]
fn duplicate_remote_terminal_failure_event_does_not_reapply_tracker_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Review", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let first_state_store = StateStore::open_in_memory().expect("state store should open");
	let second_state_store = StateStore::open_in_memory().expect("state store should open");
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
	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&first_state_store,
		&issue_run,
		&error,
	)
	.expect("first terminal failure writeback should apply");
	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&second_state_store,
		&issue_run,
		&error,
	)
	.expect("remote duplicate terminal failure writeback should no-op");

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
		second_state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("remote duplicate should be learned into local execution events")
			.len(),
		1
	);
}

#[test]
fn duplicate_passive_retained_review_attention_event_does_not_reapply_tracker_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Review", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let run_identity = RetainedReviewRunIdentity {
		run_id: String::from("pub-101-attempt-8-123"),
		attempt_number: 8,
	};

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let worktree_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("worktree mapping query should succeed")
		.expect("worktree mapping should exist");
	let runtime = PassiveRetainedAttentionRuntime {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
	};

	orchestrator::apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		&issue,
		&worktree_mapping,
		&run_identity,
		"missing_review_handoff_record",
	)
	.expect("first passive retained attention writeback should apply");
	orchestrator::apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		&issue,
		&worktree_mapping,
		&run_identity,
		"missing_review_handoff_record",
	)
	.expect("duplicate passive retained attention writeback should no-op");

	assert_eq!(tracker.state_updates.borrow().len(), 1);
	assert_eq!(tracker.label_additions.borrow().len(), 1);
	assert_eq!(tracker.label_removals.borrow().len(), 1);
	assert_eq!(
		tracker
			.comments
			.borrow()
			.iter()
			.filter(|comment| comment.contains("missing_review_handoff_record"))
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

#[test]
fn rebound_handoff_marker_suppresses_stale_missing_handoff_attention_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Review", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_identity = RetainedReviewRunIdentity {
		run_id: String::from("pub-101-attempt-8-123"),
		attempt_number: 8,
	};

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			&issue.id,
			&tests::sample_review_handoff_marker(
				&worktree.branch_name,
				"https://github.com/hack-ink/decodex/pull/101",
				&head_oid,
			),
		)
		.expect("rebound handoff marker should record");

	let worktree_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("worktree mapping query should succeed")
		.expect("worktree mapping should exist");
	let runtime = PassiveRetainedAttentionRuntime {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
	};

	orchestrator::apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		&issue,
		&worktree_mapping,
		&run_identity,
		"missing_review_handoff_record",
	)
	.expect("stale passive retained attention should no-op after rebind");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
