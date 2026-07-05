use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy, STALE_ACTIVE_CLASSIFICATION,
		tests::{self, GhostLaneTestTracker},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_diagnose_classifies_tracker_present_active_without_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store.update_run_thread("run-1626", "thread-stale").expect("thread should record");
	store.update_run_turn("run-1626", "turn-stale").expect("turn should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = recovery::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert_eq!(
		diagnostic.reason,
		"tracker_issue_has_stale_active_label_without_live_or_retained_progress"
	);
	assert!(diagnostic.active_label_present);
	assert!(diagnostic.queue_label_present);
	assert!(!diagnostic.run_lease);
	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-1626"));
	assert!(diagnostic.blockers.is_empty(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("tracker_issue_present")));
	assert!(diagnostic.evidence.contains(&String::from("run_lease_missing")));
	assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
	assert!(diagnostic.evidence.contains(&String::from("stale_thread_reference_present")));
	assert!(diagnostic.next_action.contains("recover stale-active release PUB-1626 --dry-run"));
}
