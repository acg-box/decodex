use std::time::Duration;

use crate::orchestrator::{
	self, IssueDispatchMode, StateStore, TERMINAL_GUARDED_RUN_STATUS, TargetIssueRunContext,
	tests::{self, FakeTracker, retry_scheduling::support},
};

#[test]
fn retry_delay_distinguishes_continuation_and_capped_failure_backoff() {
	let (_, _, workflow) = tests::temp_project_layout();

	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Continuation, 1, &workflow,),
		Duration::from_millis(1_000)
	);
	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Failure, 1, &workflow),
		Duration::from_millis(10_000)
	);
	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Failure, 10, &workflow),
		Duration::from_millis(300_000)
	);
}

#[test]
fn retry_run_dry_run_enforces_active_ownership() {
	for (case_name, issue, expected_dispatch) in [
		("active issue", support::sample_service_owned_issue("In Progress"), true),
		("unowned issue", tests::sample_issue("In Progress", &[]), false),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![vec![issue.clone()], vec![issue.clone()]],
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.id,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("retry run should succeed");

		assert_eq!(summary.is_some(), expected_dispatch, "{case_name}");
	}
}

#[test]
fn targeted_run_dry_run_accepts_startable_issue_with_normal_dispatch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
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
	.expect("targeted run should succeed");

	assert!(summary.is_some(), "normal targeted dispatch should accept startable issues");
}

#[test]
fn retry_run_dry_run_rejects_terminal_guarded_issue_without_attention_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue =
		support::sample_service_owned_issue_without_needs_attention_team_label("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal guard attempt should record");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Retry,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("retry run should succeed");

	assert!(
		summary.is_none(),
		"retry should reject issues that remain in progress only as a terminal guard"
	);
}
