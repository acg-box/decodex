use std::{fs, time::Duration};

use color_eyre::Report;

use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerTransportFailure,
		AppServerTurnFailure, ReviewPolicyStopReason, ReviewPolicyStopRequested,
	},
	orchestrator::{
		self, AppServerZeroEvidenceStartFailure, IssueDispatchMode, IssueRunPlan,
		ManualAttentionRequested, PassiveRetainedAttentionRuntime, PhaseGoalKind,
		PrepareIssueRunContext, RUN_LEASE_IDLE_TIMEOUT, RepoGateFailure, RepoGateFailureKind,
		RetainedReviewRunIdentity, ReviewHandoffNeedsAttention, ServiceConfig,
		StalledRunNeedsAttention, TERMINAL_GUARD_MARKER_FILE, WorkflowDocument,
		tests::{
			FakeTracker, TEST_SERVICE_ID, recovery_terminal_support, {self},
		},
	},
	state::{self, StateStore},
	tracker::{self, records},
	worktree::{WorktreeManager, WorktreeSpec},
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

#[test]
fn terminal_failure_with_retained_tracked_changes_records_retained_partial_progress() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-101", ".worktrees/PUB-101", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained terminal patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
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
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::CommandSpawnFailed,
		String::from("Failed to spawn repo gate command `cargo make test`."),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("terminal failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention"))
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("Source failure class `repo_gate_command_spawn_failed`"))),
		"retained partial progress evidence should preserve the source failure class"
	);
}

#[test]
fn repo_gate_tracked_rewrites_left_records_retained_partial_progress_without_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-102");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);
	fs::write(worktree_path.join("README.md"), "repo gate left tracked rewrites\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-102"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-102-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::TrackedRewritesLeft,
		String::from("Repo gate verification left tracked-file rewrites."),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("tracked repo-gate rewrites should retain partial progress");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and will retry")),
		"tracked repo-gate rewrites should not continue automatic retry"
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("Source failure class `repo_gate_tracked_rewrites_left`"))),
		"retained progress evidence should preserve the source repo-gate failure class"
	);
}

#[test]
fn retryable_runtime_failure_with_retained_tracked_changes_retries_before_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-102");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained validation-ready runtime patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-102"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-102-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::msg(
		"app-server run ended after a validation-ready checkpoint without a terminal path",
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty generic runtime failure should remain retryable");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("retryable_execution_failure")
			&& comment.contains("decodex will retry automatically")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| !comment
				.contains("decodex retained partial progress and needs attention")),
		"retained work must not force manual attention while retry budget remains"
	);
	assert!(
		comments
			.iter()
			.all(|comment| records::parse_linear_execution_event_record(comment).is_none()),
		"retryable retained work should not write a terminal needs-attention ledger event"
	);
}

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

#[test]
fn app_server_failures_skip_retry_and_require_attention() {
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
			"skills",
			"skills/list returned no enabled skills.",
		)),
		"app_server_runtime_preflight_failed",
		"repair the local Codex runtime configuration",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))),
		"app_server_codex_home_preflight_failed",
		"inspect the local Decodex and Codex home sharing",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerHomePreflightFailure::initialize_mismatch(
			String::from("/tmp/per-account-codex-home"),
			String::from("/Users/test/.codex"),
		)),
		"app_server_codex_home_mismatch",
		"restart `decodex serve`",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerTransportFailure::new(String::from(
			"App-server stdout disconnected unexpectedly.",
		))),
		"app_server_transport_disconnected",
		"resolve the Codex app-server transport failure manually",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"turn/start",
			false,
		)),
		"app_server_transport_disconnected",
		"resolve the Codex app-server transport failure during `turn/start` manually",
	);
}

#[test]
fn app_server_preflight_timeouts_retry_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("preflight timeout should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_plugin_list_timeout")
			&& comment.contains("retry app-server preflight automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)
		.expect("retry schedule should be readable")
		.expect("retry schedule marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
}

#[test]
fn exhausted_app_server_preflight_timeout_retry_budget_requires_attention_with_timeout_class() {
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
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted preflight timeout should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_plugin_list_timeout")
			&& comment
				.contains("app_server_preflight_failed evidence for the `plugin/list` timeout")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}

#[test]
fn phase_goal_terminal_path_missing_retries_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
		PhaseGoalKind::HandoffEvidence,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("missing terminal path should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("phase_goal_terminal_path_missing")
			&& comment.contains("terminal-path recovery automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)
		.expect("retry schedule should be readable")
		.expect("retry schedule marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
}

#[test]
fn phase_goal_terminal_path_missing_with_retained_changes_retries_before_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-103");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-103", ".worktrees/PUB-103", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained handoff patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-103"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-103-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
		PhaseGoalKind::HandoffEvidence,
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty terminal-path failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("phase_goal_terminal_path_missing")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention during terminal-path retry"
	);
}

#[test]
fn retryable_app_server_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
		"app_server_transport_disconnected",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"transient model failure",
			None,
		)),
		"retryable_execution_failure",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
		)),
		"app_server_usage_limit_exceeded",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerZeroEvidenceStartFailure::new(
			String::from("PUB-104"),
			String::from("pub-104-attempt-1-123"),
		)),
		"app_server_zero_evidence_start_failed",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		7,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		8,
		Report::new(AppServerDynamicToolFailure::tool_for_test(
			Some(String::from("issue_comment")),
			"tool rejected",
		)),
		"app_server_dynamic_tool_failed",
	);
}

#[test]
fn retryable_orchestrator_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("cargo make check failed."),
		)),
		"repo_gate_verify_failed",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(StalledRunNeedsAttention {
			issue_identifier: String::from("PUB-103"),
			run_id: String::from("pub-103-attempt-1-123"),
			idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
		}),
		"stalled_run_detected",
	);
}

#[test]
fn dirty_retryable_runtime_failures_keep_automatic_recovery_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
		"app_server_transport_disconnected",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerDynamicToolFailure::tool_for_test(
			Some(String::from("issue_comment")),
			"tool rejected",
		)),
		"app_server_dynamic_tool_failed",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		7,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("cargo make check failed."),
		)),
		"repo_gate_verify_failed",
	);
}

#[test]
fn startup_transport_failures_retry_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected unexpectedly."),
		"thread/start",
		true,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("startup transport failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_transport_disconnected")
			&& comment.contains("thread/start")
			&& comment.contains("restart the app-server and retry automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);
}

#[test]
fn exhausted_startup_transport_retry_budget_requires_attention_with_transport_class() {
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
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected unexpectedly."),
		"thread/start",
		true,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted startup transport failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_transport_disconnected")
			&& comment.contains("failure during `thread/start` manually")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}

#[test]
fn usage_limit_turn_failures_retry_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"You've hit your usage limit.",
		Some(String::from("usageLimitExceeded")),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("usage limit failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("reselect or refresh the Codex account")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);
}

#[test]
fn usage_limit_turn_failures_with_retained_tracked_changes_retry_before_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-103");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-103", ".worktrees/PUB-103", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained usage-limit patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-103"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-103-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"You've hit your usage limit.",
		Some(String::from("usageLimitExceeded")),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty usage-limit failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("reselect or refresh the Codex account")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention while usage-limit retry remains"
	);
}

#[test]
fn exhausted_usage_limit_retry_budget_requires_attention_with_usage_class() {
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
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"You've hit your usage limit.",
		Some(String::from("usageLimitExceeded")),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted usage limit failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("inspect Codex account usage")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}

#[test]
fn dirty_runtime_failures_record_retained_progress_instead_of_terminal_failure() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-101", ".worktrees/PUB-101", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained runtime recovery work\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 1,
	};
	let error = Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
		"model",
		"configured model was not present in model/list.",
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty runtime failure should retain partial progress");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("app_server_runtime_preflight_failed")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention"))
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained runtime failure should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("app_server_runtime_preflight_failed"))),
		"retained progress evidence should preserve the source runtime error class"
	);
}

#[test]
fn explicit_manual_attention_keeps_manual_terminal_path_with_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-101", ".worktrees/PUB-101", "main"],
	);
	fs::write(worktree_path.join("README.md"), "manual attention work\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
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

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("manual attention should keep its terminal path");

	let ledger_event = tracker
		.comments
		.borrow()
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("manual attention should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("human_attention_required"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("manual_attention"));
	assert_eq!(ledger_event.summary.as_deref(), Some("Decodex run failed and needs attention."));
}

#[test]
fn prepare_issue_run_clears_terminal_guard_marker_when_new_attempt_starts() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(vec![], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("worktree should exist before retry guard clearing");
	let marker_path = worktree.path.join(TERMINAL_GUARD_MARKER_FILE);

	fs::write(&marker_path, "stale terminal guard\n").expect("terminal guard marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("startable issue should produce a run plan");

	assert_eq!(issue_run.worktree.path, worktree.path);
	assert!(
		!marker_path.exists(),
		"starting a new attempt should clear stale terminal-guard markers"
	);
}

#[test]
fn retryable_failures_ignore_prior_continuation_attempts_in_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
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
		attempt_number: 4,
		run_id: String::from("pub-101-attempt-4-123"),
		retry_budget_base: 0,
	};

	state_store
		.record_run_attempt("pub-101-attempt-1-123", &issue.id, 1, "succeeded")
		.expect("first continuation attempt should record");
	state_store
		.record_run_attempt("pub-101-attempt-2-123", &issue.id, 2, "succeeded")
		.expect("second continuation attempt should record");
	state_store
		.record_run_attempt("pub-101-attempt-3-123", &issue.id, 3, "succeeded")
		.expect("third continuation attempt should record");
	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("current failed attempt should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("command failed"),
	)
	.expect("retryable failure handling should succeed");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("retryable_execution_failure")
			&& comment.contains("- run_sequence_attempt: `4` (not retry-budget count)")
			&& comment.contains("- retry_budget_attempt: `1` / `3`")
	}));
	assert!(!tracker.comments.borrow().iter().any(|comment| {
		comment.contains("needs attention") || comment.contains("retry_budget_exhausted")
	}));
}

#[test]
fn manual_attention_failure_overrides_succeeded_run_status() {
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("run attempt should record");
	state_store.update_run_status("run-1", "failed").expect("failed outcome should persist");

	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"failed"
	);
}

fn assert_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-{case_number}");
	let issue_identifier = format!("PUB-10{case_number}");
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: format!("x/pubfi-{}", issue_identifier.to_lowercase()),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join(&issue.identifier),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: format!("pub-10{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(comments.iter().all(|comment| {
		!comment.contains("decodex run failed and needs attention")
			&& !comment.contains("decodex retained partial progress and needs attention")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}

fn assert_dirty_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-dirty-{case_number}");
	let issue_identifier = format!("PUB-30{case_number}");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let branch_name = format!("x/pubfi-{}", issue_identifier.to_lowercase());
	let worktree_rel_path = format!(".worktrees/{issue_identifier}");
	let worktree_path = config.worktree_root().join(&issue_identifier);

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", &branch_name, &worktree_rel_path, "main"],
	);
	fs::write(
		worktree_path.join("README.md"),
		format!("dirty retryable recovery case {case_number}\n"),
	)
	.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name,
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: format!("pub-30{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("dirty retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(
		comments.iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention for `{expected_error_class}` while retry budget remains"
	);
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
