use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, PullRequestDetails,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn review_handoff_persists_runtime_state_without_local_marker_cache() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/150"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/150"),
		}),
	]);
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/150",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	bridge
		.apply_review_handoff()
		.expect("runtime state persistence should not depend on local marker files");

	assert_eq!(tracker.state_updates.borrow().as_slice(), ["state-review"]);
	assert_eq!(tracker.comments.borrow().len(), 1);
	assert_eq!(
		tests::persisted_review_lifecycle_handoff_fixture(
			&bridge,
			&issue,
			&tests::sample_review_context_in(temp_dir.path())
		)
		.pr_url(),
		"https://github.com/hack-ink/decodex/pull/150"
	);
}

fn review_handoff_pr_details(
	url: &str,
	head_ref_name: &str,
	head_ref_oid: &str,
	owner: &str,
	repository: &str,
	base_ref_name: &str,
	is_draft: bool,
) -> PullRequestDetails {
	PullRequestDetails {
		head_ref_name: String::from(head_ref_name),
		head_ref_oid: String::from(head_ref_oid),
		head_repository_name: String::from(repository),
		head_repository_owner: String::from(owner),
		is_draft,
		state: String::from("OPEN"),
		base_ref_name: String::from(base_ref_name),
		url: String::from(url),
	}
}

#[test]
fn rejects_invalid_pull_requests_for_review_handoff() {
	for (case_name, pull_request, expected_error) in [
		(
			"another branch",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/43",
				"x/decodex-pub-999",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"main",
				false,
			),
			None,
		),
		(
			"draft pull request",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/44",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"main",
				true,
			),
			None,
		),
		(
			"stale PR head",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/45",
				"x/decodex-pub-618",
				"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
				"hack-ink",
				"decodex",
				"main",
				false,
			),
			None,
		),
		(
			"another repository",
			review_handoff_pr_details(
				"https://github.com/someone-else/decodex-fork/pull/46",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"someone-else",
				"decodex-fork",
				"main",
				false,
			),
			None,
		),
		(
			"non-default target branch",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/47",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"release/1.x",
				false,
			),
			Some("retained review lanes must target the repository default branch `main`"),
		),
	] {
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pr_url = pull_request.url.clone();
		let inspector = FakePullRequestInspector::new(vec![Ok(pull_request)]);
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			tests::sample_review_context(),
			&inspector,
			&local_repo_inspector,
		);
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": pr_url,
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success, "{case_name}");
		assert!(tracker.comments.borrow().is_empty(), "{case_name}");
		assert!(tracker.state_updates.borrow().is_empty(), "{case_name}");

		if let Some(expected_error) = expected_error {
			assert!(
				matches!(
					response.content_items.as_slice(),
					[DynamicToolContentItem::InputText{ text }] if text.contains(expected_error)
				),
				"{case_name}"
			);
		}

		assert!(bridge.apply_review_handoff().is_err(), "{case_name}");
	}
}
