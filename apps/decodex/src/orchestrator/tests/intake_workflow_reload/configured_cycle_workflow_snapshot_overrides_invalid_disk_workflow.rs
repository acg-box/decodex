use std::fs;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, TargetIssueRunContext,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn configured_cycle_workflow_snapshot_overrides_invalid_disk_workflow() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let workflow_snapshot = workflow.to_markdown().expect("workflow markdown should render");

	fs::write(config.workflow_path(), "not valid workflow markdown")
		.expect("invalid workflow should be written");

	assert!(
		orchestrator::load_configured_cycle_workflow(&config, None).is_err(),
		"without an override the configured workflow load should fail"
	);

	let loaded = orchestrator::load_configured_cycle_workflow(&config, Some(&workflow_snapshot))
		.expect("configured workflow load should accept the supplied snapshot");
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &loaded,
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
	.expect("target issue dry run should succeed with the supplied snapshot");

	assert!(summary.is_some(), "the child path should still run off the cached snapshot");
}
