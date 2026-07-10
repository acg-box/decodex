use std::cell::RefCell;

use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakeTracker, TEST_SERVICE_ID, sample_local_repo},
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
