use std::path::Path;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RunSummary, TargetIssueRunContext,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
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
