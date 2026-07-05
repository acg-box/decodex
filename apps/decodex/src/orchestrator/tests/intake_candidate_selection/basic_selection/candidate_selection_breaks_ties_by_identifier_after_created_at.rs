use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn candidate_selection_breaks_ties_by_identifier_after_created_at() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let later_identifier = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let earlier_identifier = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![later_identifier.clone(), earlier_identifier.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![later_identifier, earlier_identifier],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-101");
}
