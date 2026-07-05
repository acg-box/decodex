use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn candidate_selection_skips_todo_issue_with_nonterminal_blockers() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut blocked_high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:15:17.133Z",
	);

	blocked_high_priority.blockers =
		vec![tests::sample_blocker("issue-9", "PUB-109", "In Progress")];

	let unblocked_lower_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::new(vec![blocked_high_priority.clone(), unblocked_lower_priority.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![blocked_high_priority, unblocked_lower_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-103");
}
