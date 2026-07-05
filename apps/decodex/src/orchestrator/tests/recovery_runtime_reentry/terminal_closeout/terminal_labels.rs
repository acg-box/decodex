use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker,
};

#[test]
fn run_project_once_clears_terminal_queued_lane_labels_without_dispatch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("terminal queued cleanup should succeed");

	assert!(summary.is_none(), "terminal queued issues should not dispatch");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn run_project_once_dry_run_keeps_terminal_queued_lane_labels() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("terminal queued dry run should succeed");

	assert!(summary.is_none(), "terminal queued dry run should not dispatch");
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"dry run should not mutate terminal queued labels"
	);
}
