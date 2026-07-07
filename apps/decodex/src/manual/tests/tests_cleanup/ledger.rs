use tempfile::TempDir;

use crate::{
	manual::{self, ManualLandLedgerContext, tests, tests::support::TestTracker},
	state::{ReviewLifecycleHandoffFixture, ReviewLifecycleRecord, StateStore},
	tracker::{
		TrackerState, privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier, records,
	},
};

#[test]
fn manual_land_issue_closeout_writes_success_ledger_after_existing_marker() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");
	let tracker = TestTracker::new();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = tests::sample_issue("issue-1", "PUB-1161", true, &[]);

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let handoff = ReviewLifecycleHandoffFixture::new(
		String::from("pub-1161-attempt-1"),
		1,
		String::from("xy/pub-1161"),
		String::from("https://github.com/helixbox/pubfi-mono-v2/pull/95"),
		String::from("main"),
		String::from("xy/pub-1161"),
		String::from("3cf2d24033527a774340c7d70c5ce437c90afe55"),
	);
	let lifecycle_record = ReviewLifecycleRecord::from_test_lifecycle_fixtures(&handoff, None);

	state_store
		.record_run_attempt(handoff.run_id(), &issue.id, handoff.attempt_number(), "failed")
		.expect("failed handoff attempt should record");

	let merge_commit = "81e90b530148a0be69afa5bd33ce6ab84d485a3a";
	let landed_change_record = r#"{"schema":"decodex/commit/2","change":"Land PUB-1161","authority":"PUB-1161","impact":"compatible"}"#;

	manual::write_manual_land_closeout_receipt(
		&checkout,
		"https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		"xy/pub-1161",
		landed_change_record,
	)
	.expect("existing closeout marker should write");

	let ledger = ManualLandLedgerContext {
		service_id: "pubfi",
		issue: &issue,
		state_store: &state_store,
		lifecycle_record: &lifecycle_record,
		pr_url: "https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		branch_name: "xy/pub-1161",
		worktree_path: ".worktrees/PUB-1161",
		completed_state: "Done",
		default_branch: "main",
		privacy_classifier: &ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::apply_closeout(&checkout, &tracker, "Done", &ledger, landed_change_record)
		.expect("manual closeout should write landed and closeout events");
	manual::write_manual_land_cleanup_complete_event(&tracker, &ledger)
		.expect("manual cleanup should write cleanup_complete event");

	let comments = tracker.comments.borrow();
	let records = comments
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.collect::<Vec<_>>();
	let event_types = records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[vec![String::from("issue-1"), String::from("state-done"),]]
	);
	assert_eq!(event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert!(
		comments.iter().all(|comment| !comment.starts_with("decodex land completed")),
		"matching legacy closeout marker should not replay the ordinary closeout comment"
	);
	assert!(comments.iter().all(|comment| {
		comment.contains("- run_sequence_attempt: `1` (not retry-budget count)")
			&& !comment.contains("- attempt:")
	}));
	assert!(records.iter().all(|record| record.run_id == "pub-1161-attempt-1"));
	assert!(records.iter().all(|record| record.attempt_number == 1));
	assert_eq!(records[0].pr_head_sha.as_deref(), Some(handoff.pr_head_oid()));
	assert_eq!(records[0].commit_sha.as_deref(), Some(merge_commit));
	assert_eq!(records[1].target_state.as_deref(), Some("Done"));
	assert_eq!(records[2].cleanup_status.as_deref(), Some("completed"));

	let cached_records = state_store
		.list_linear_execution_events("pubfi", "issue-1")
		.expect("local ledger cache should read");
	let cached_event_types =
		cached_records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(cached_event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert_eq!(
		state_store
			.run_attempt(handoff.run_id())
			.expect("run attempt lookup should succeed")
			.expect("handoff attempt should exist")
			.status(),
		"succeeded"
	);
}
