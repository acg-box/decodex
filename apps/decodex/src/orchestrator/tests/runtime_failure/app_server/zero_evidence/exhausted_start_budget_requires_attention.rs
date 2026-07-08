use crate::orchestrator::{
	tests,
	tests::runtime_failure::{
		AppServerZeroEvidenceStartFailure, FakeTracker, IssueDispatchMode, IssueRunPlan, Report,
		StateStore, WorktreeSpec, fs, orchestrator,
	},
};

#[test]
fn exhausted_start_budget_requires_attention() {
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
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let error = Report::new(AppServerZeroEvidenceStartFailure::new(
		issue.identifier.clone(),
		issue_run.run_id.clone(),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted zero-evidence failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_zero_evidence_start_failed")
			&& comment.contains("verify `decodex probe stdio://`")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry")),
		"exhausted zero-evidence failure should not keep retrying"
	);
}
