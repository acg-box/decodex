use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	workflow::WorkflowDocument,
};

#[test]
fn candidate_selection_skips_issue_claimed_by_another_process() {
	let workflow = WorkflowDocument::parse_markdown(&tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Claim-aware workflow policy.\n",
		1,
	))
	.expect("workflow should parse");
	let (_temp_dir, config, _default_workflow) = tests::temp_project_layout();
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let local_store = StateStore::open_in_memory().expect("local state store should open");
	let claimed_issue = tests::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-100",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let free_issue = tests::sample_issue_with_sort_fields(
		"issue-free",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:18.133Z",
	);

	remote_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("remote dispatch-slot root should configure");
	local_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("local dispatch-slot root should configure");

	assert!(
		remote_store
			.try_acquire_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
			.expect("remote issue claim should succeed")
	);

	let tracker = FakeTracker::new(vec![claimed_issue.clone(), free_issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![claimed_issue, free_issue.clone()],
		&workflow,
		&local_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed")
	.expect("the unclaimed issue should still be selected");

	assert_eq!(selected.id, free_issue.id);
}
