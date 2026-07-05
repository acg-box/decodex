use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, ActiveWorkflowOverride, ChildRunRef,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	workflow::WorkflowDocument,
};

#[test]
fn active_child_reconciliation_keeps_spawn_time_workflow_until_exit() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let active_workflow = WorkflowDocument::parse_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Spawn-time workflow policy.\n", 1)
			.replace("max_attempts = 3", "max_attempts = 5"),
	)
	.expect("workflow should parse");
	let current_workflow = WorkflowDocument::parse_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Current workflow policy.\n", 1)
			.replace("startable_states = [\"Todo\"]", "startable_states = [\"Backlog\"]"),
	)
	.expect("workflow should parse");
	let child_issue = tests::sample_issue("Todo", &[]);
	let stale_issue = tests::sample_issue_with_sort_fields(
		"issue-stale",
		"PUB-202",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![child_issue.clone(), stale_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-child", &child_issue.id, 1, "running")
		.expect("child run attempt should record");
	state_store
		.upsert_lease("pubfi", &child_issue.id, "run-child", "In Progress")
		.expect("child lease should record");
	state_store
		.record_run_attempt("run-stale", &stale_issue.id, 1, "running")
		.expect("stale run attempt should record");
	state_store
		.upsert_lease("pubfi", &stale_issue.id, "run-stale", "In Progress")
		.expect("stale lease should record");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&current_workflow,
		&state_store,
		Some(ActiveWorkflowOverride {
			child: ChildRunRef {
				issue_id: &child_issue.id,
				run_id: "run-child",
				attempt_number: 1,
			},
			workflow: &active_workflow,
		}),
		OffsetDateTime::now_utc().unix_timestamp() + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.iter().all(|action| action.issue.id != child_issue.id),
		"the current child should keep its spawn-time workflow snapshot"
	);
	assert!(actions.iter().any(|action| {
		action.issue.id == stale_issue.id
			&& matches!(action.disposition, orchestrator::RunLeaseDisposition::NotDispatchable)
	}));
}
