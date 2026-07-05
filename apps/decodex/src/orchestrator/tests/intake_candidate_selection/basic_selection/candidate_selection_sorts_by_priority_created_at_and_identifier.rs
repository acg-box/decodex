use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn candidate_selection_sorts_by_priority_created_at_and_identifier() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:18:17.133Z",
	);
	let oldest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:15:17.133Z",
	);
	let newest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-4",
		"PUB-104",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:19:17.133Z",
	);
	let no_priority = tests::sample_issue_with_sort_fields(
		"issue-5",
		"PUB-105",
		"Todo",
		&[],
		None,
		"2026-03-13T04:14:17.133Z",
	);
	let tracker = FakeTracker::new(vec![
		no_priority.clone(),
		newest_same_priority.clone(),
		oldest_same_priority.clone(),
		high_priority.clone(),
	]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![no_priority, newest_same_priority, oldest_same_priority, high_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-102");
}
