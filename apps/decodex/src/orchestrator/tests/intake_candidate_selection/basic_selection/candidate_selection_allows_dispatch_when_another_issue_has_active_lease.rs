use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn candidate_selection_allows_dispatch_when_another_issue_has_active_lease() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_lease("pubfi", "issue-active", "run-1", "In Progress")
		.expect("lease should record");

	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_some(),
		"another active lease must not impose a project-level dispatch cap"
	);
}
