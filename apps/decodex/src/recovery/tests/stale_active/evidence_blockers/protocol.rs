use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{self, ProtocolActivityMarker, StateStore},
	tracker::{self},
};

#[test]
fn stale_active_diagnose_blocks_private_progress_from_older_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-old", &issue.id, 1, "running")
		.expect("old run attempt should record");
	store
		.record_run_attempt("run-new", &issue.id, 2, "running")
		.expect("new run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-old",
			1,
			"source_progress",
			serde_json::json!({"phase": "implementation"}),
		)
		.expect("private progress should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-new"));
	assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("private_progress_evidence_ref:run-old:1:source_progress")),
		"private progress blockers should identify the retained evidence row"
	);
	assert!(
		diagnostic.next_action.contains("Preserve retained progress")
			&& diagnostic
				.next_action
				.contains("decodex evidence PUB-1626 --run-id run-old --attempt 1 --json")
			&& !diagnostic
				.next_action
				.contains("decodex evidence PUB-1626 --run-id run-new --attempt 2 --json")
			&& diagnostic.next_action.contains("private_progress_evidence_ref")
			&& diagnostic.next_action.contains("manual attention"),
		"private progress blockers should route to exact private evidence inspection, got {:?}",
		diagnostic.next_action
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_protocol_event_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_event("run-1626", 1, "turn/item", r#"{"kind":"progress"}"#)
		.expect("protocol event should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
	assert!(
		diagnostic.next_action.contains("runtime ownership still appears live or unsettled")
			&& diagnostic.next_action.contains("decodex lane inspect PUB-1626"),
		"runtime evidence blockers should route to lane-control inspection, got {:?}",
		diagnostic.next_action
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_marker_protocol_activity_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 1,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_marker_present")));
	assert!(
		diagnostic.blockers.contains(&String::from("activity_marker_protocol_activity_present"))
	);
	assert!(!diagnostic.recoverable());
}
