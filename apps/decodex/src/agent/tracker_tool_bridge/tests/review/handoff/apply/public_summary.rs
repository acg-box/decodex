use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, PullRequestDetails, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::records,
};

#[test]
fn review_handoff_writeback_replaces_private_summary_with_public_fallback() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let pull_request = PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/151"),
	};
	let inspector =
		FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request.clone())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": pull_request.url,
			"summary": "Completed from /Users/example/repo with GITHUB_PAT_Y configured."
		}),
	);

	assert!(response.success);

	bridge.apply_review_handoff().expect("private-looking summaries should use fallback text");

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("review handoff comment should write");

	assert!(comment.contains("Implementation completed and the PR is ready for review."));
	assert!(!comment.contains("/Users/example/repo"));
	assert!(!comment.contains("GITHUB_PAT_Y"));

	let record = records::parse_linear_execution_event_record(comment)
		.expect("review handoff should write a valid Linear execution event");

	assert_eq!(
		record.summary.as_deref(),
		Some("Implementation completed and the PR is ready for review.")
	);
	assert_eq!(record.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/151"));
}
