use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, ManualAttentionRequested,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

#[test]
fn terminal_failures_without_needs_attention_label_use_nonstartable_guard_state() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue =
		tests::sample_issue_without_needs_attention_team_label("Todo", &[active_label.as_str()]);

	for label in &mut issue.labels {
		label.id = issue
			.team
			.labels
			.iter()
			.find(|team_label| team_label.name == label.name)
			.map(|team_label| team_label.id.clone())
			.expect("issue label should resolve to a team label id");
	}

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
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
	let error = Report::new(ManualAttentionRequested {
		issue_identifier: issue.identifier.clone(),
		label: String::from("decodex:needs-attention"),
		run_id: issue_run.run_id.clone(),
		error_class: None,
	});

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("terminal failure handling should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-progress")))
	);
	assert_eq!(
		tracker.label_removals.borrow().last(),
		Some(&(issue.id.clone(), vec![String::from("label-active")])),
		"terminal failure should clear the active automation label even when needs-attention is unavailable"
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("does not exist on the team")
			&& comment.contains("remains in `In Progress`")
	}));
	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		orchestrator::TERMINAL_GUARDED_RUN_STATUS
	);
	assert!(
		issue_run.worktree.path.join(orchestrator::TERMINAL_GUARD_MARKER_FILE).exists(),
		"fallback guard should leave a durable worktree marker for restart recovery"
	);
}

#[test]
fn terminal_failures_apply_incremental_label_mutations_when_issue_labels_paginate() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("Todo", &[active_label.as_str()]);

	issue.labels_complete = false;

	for label in &mut issue.labels {
		label.id = issue
			.team
			.labels
			.iter()
			.find(|team_label| team_label.name == label.name)
			.map(|team_label| team_label.id.clone())
			.expect("issue label should resolve to a team label id");
	}

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
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
	let error = Report::new(ManualAttentionRequested {
		issue_identifier: issue.identifier.clone(),
		label: String::from("decodex:needs-attention"),
		run_id: issue_run.run_id.clone(),
		error_class: None,
	});

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");
	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect(
			"terminal failure should use incremental label mutations when issue labels paginate",
		);

	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"terminal failure should still leave a durable tracker comment"
	);

	let ledger_event = tracker
		.comments
		.borrow()
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("terminal failure should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("human_attention_required"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("manual_attention"));
}
