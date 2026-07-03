use std::cell::RefCell;

use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
			sample_local_repo,
		},
	},
	tracker::{
		privacy_classifier::{
			PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
		},
		records,
	},
};

struct FakeProjectionClassifier {
	verdict: PublicProjectionPrivacyClassification,
	seen_text: RefCell<Vec<String>>,
}
impl FakeProjectionClassifier {
	fn new(verdict: PublicProjectionPrivacyClassification) -> Self {
		Self { verdict, seen_text: RefCell::new(Vec::new()) }
	}
}

impl PublicProjectionPrivacyClassifier for FakeProjectionClassifier {
	fn classify_public_projection_text(
		&self,
		field_name: &str,
		text: &str,
	) -> PublicProjectionPrivacyClassification {
		self.seen_text.borrow_mut().push(format!("{field_name}:{text}"));

		self.verdict.clone()
	}
}

#[test]
fn progress_checkpoint_unavailable_classifier_preserves_private_event() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let classifier =
		FakeProjectionClassifier::new(PublicProjectionPrivacyClassification::Unavailable {
			reason: String::from("fake unavailable classifier"),
		});
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_classifier_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&classifier,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "verifying",
				"docs_impact": "none",
			"focus": "Private verification focus stays local.",
			"next_action": "Continue verification.",
			"blockers": [],
			"evidence": ["Private verification evidence stays local."],
			"head_sha": sample_local_repo().head_oid
		}),
	);

	assert!(response.success);

	let record = records::parse_linear_execution_event_record(&tracker.comments.borrow()[0])
		.expect("progress checkpoint should be a Linear execution event");

	assert_eq!(
		record.summary.as_deref(),
		Some(records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY)
	);

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(
		private_events[0].payload()["focus"],
		serde_json::json!("Private verification focus stays local.")
	);
	assert_eq!(
		private_events[0].payload()["evidence"],
		serde_json::json!(["Private verification evidence stays local."])
	);
}

#[test]
fn blocked_progress_checkpoint_requires_concrete_blocker() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "blocked",
				"docs_impact": "none",
			"focus": "Unblock closeout.",
			"next_action": "Wait for a blocker to be clarified.",
			"blockers": [],
			"evidence": []
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("requires at least one blocker")))
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn progress_checkpoint_rejects_stale_head_sha() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
				"docs_impact": "none",
			"focus": "Keep execution state tied to the current lane head.",
			"next_action": "Reject stale checkpoint writes.",
			"blockers": [],
			"evidence": [],
			"head_sha": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("does not match the current lane HEAD")))
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn progress_checkpoint_normalizes_matching_short_head_sha_to_full_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "closeout",
				"docs_impact": "none",
			"focus": "Finish retained closeout bookkeeping.",
			"next_action": "Record the closeout checkpoint with the live lane head.",
			"blockers": [],
			"evidence": [],
			"head_sha": &tests::sample_local_repo().head_oid[..7]
		}),
	);

	assert!(response.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let record = records::parse_linear_execution_event_record(&tracker.comments.borrow()[0])
		.expect("progress checkpoint should be a Linear execution event");

	assert!(record.commit_sha.is_none());

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(
		private_events[0].payload()["head_sha"],
		serde_json::json!(tests::sample_local_repo().head_oid)
	);
}
