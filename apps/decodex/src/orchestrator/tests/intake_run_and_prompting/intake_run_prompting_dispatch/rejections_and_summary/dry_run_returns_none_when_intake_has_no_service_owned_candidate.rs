use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn dry_run_returns_none_when_intake_has_no_service_owned_candidate() {
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], false);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run without queued issues should succeed");

		assert!(summary.is_none(), "empty intake should simply produce no dry-run selection");
	}
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = tests::sample_issue_with_project_slug_and_sort_fields(
			"issue-1",
			"PUB-101",
			"other-service",
			"Todo",
			&[],
			Some(3),
			"2026-03-13T04:16:17.133Z",
		);
		let tracker = FakeTracker::new(vec![issue]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run should succeed");

		assert!(summary.is_none(), "service-scoped queue labels should isolate intake");
	}
}
