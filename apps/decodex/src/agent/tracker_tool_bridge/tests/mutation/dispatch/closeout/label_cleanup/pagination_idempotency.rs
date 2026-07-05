use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::{self, TrackerLabel, TrackerState},
};

#[test]
fn closeout_clear_clears_active_label_when_issue_labels_paginate() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };
	completed_issue.labels_complete = false;

	completed_issue.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![completed_issue.clone()]])
		.with_label_lookup_issues(&active_label, vec![completed_issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![completed_issue.clone()]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should clear the active and queue labels incrementally when issue labels paginate");

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[vec![String::from("label-active")], vec![String::from("label-queued")],]
	);
}

#[test]
fn closeout_clear_treats_missing_lane_label_removal_as_idempotent() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-active"), name: active_label });
	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-queued"), name: queue_label });

	let tracker =
		FakeTracker::with_label_update_error("Linear GraphQL request failed: Label not on issue");

	tracker.refresh_snapshots.replace(vec![vec![completed_issue.clone()]]);

	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should ignore already-absent Linear lane labels");
}

#[test]
fn closeout_clear_skips_lane_labels_when_server_confirms_absent() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.retain(|label| label.name != active_label.as_str() && label.name != queue_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![completed_issue.clone()]]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should be idempotent after lane labels are already gone");

	assert!(tracker.label_removals.borrow().is_empty());
}
