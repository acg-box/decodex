use std::path::Path;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RunSummary, TargetIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting},
	},
	state::StateStore,
	tracker,
};

#[test]
fn dry_run_selects_one_issue_and_plans_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![tests::sample_issue("Todo", &[])]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("run once should succeed")
		.expect("one issue should be selected");

	assert_eq!(
		summary,
		RunSummary {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-1"),
			issue_identifier: String::from("PUB-101"),
			issue_state: String::from("In Progress"),
			initial_issue_state: String::from("Todo"),
			retry_project_slug: String::new(),
			dispatch_mode: IssueDispatchMode::Normal,
			branch_name: String::from("x/pubfi-pub-101"),
			worktree_path: Path::new(&config.worktree_root().join("PUB-101")).to_path_buf(),
			attempt_number: 1,
			run_id: summary.run_id.clone(),
			continuation_pending: false,
			program_dispatch: None,
		}
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn targeted_identifier_dispatch_accepts_status_ready_queued_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == issue.identifier)
		.expect("queued issue should appear in status");

	assert_eq!(candidate.classification, "ready");
	assert_eq!(candidate.reason, "eligible_for_dispatch");

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted identifier run should succeed")
		.expect("status-ready queued issue should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}

#[test]
fn targeted_inferred_dispatch_keeps_retry_for_active_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted active identifier run should succeed")
		.expect("active target should fall back to retry dispatch");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Retry);
}

#[test]
fn targeted_inferred_dispatch_reenters_continuation_pending_queued_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[queue_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt("pub-101-attempt-3-123", &issue.id, 3, "continuation_pending")
		.expect("continuation attempt should record");
	state_store
		.update_run_thread("pub-101-attempt-3-123", "thread-123")
		.expect("thread id should record");

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted continuation run should succeed")
		.expect("continuation-pending issue should dispatch");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Retry);
	assert_eq!(summary.run_id, "pub-101-attempt-3-123");
	assert_eq!(summary.attempt_number, 3);
}

#[test]
fn targeted_inferred_dispatch_reenters_interrupted_validation_repair_continuation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[queue_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "pub-101-attempt-3-123";

	state_store
		.record_run_attempt(run_id, &issue.id, 3, "interrupted")
		.expect("interrupted attempt should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"progress_checkpoint",
			serde_json::json!({"phase": "implementing"}),
		)
		.expect("progress event should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_transition",
			serde_json::json!({"phase": "implement_to_validation_ready", "signal": "validation_fail"}),
		)
		.expect("validation failure event should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_set",
			serde_json::json!({"phase": "repair_validation_failures"}),
		)
		.expect("repair goal event should record");

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted continuation run should succeed")
		.expect("interrupted validation-repair continuation should dispatch");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Retry);
	assert_eq!(summary.run_id, run_id);
	assert_eq!(summary.attempt_number, 3);
}
