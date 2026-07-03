use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{RUN_ACTIVITY_MARKER_FILE, ReviewHandoffMarker, StateStore},
	tracker::{
		self, TrackerComment, records,
		records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-1626",
			issue_identifier: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
		},
		"review_handoff",
		String::from("2026-06-28T00:00:00Z"),
		"review_handoff",
	);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	event.branch = Some(String::from("x/pubfi-pub-1626"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
	event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(String::from("Recorded review handoff lineage."));
	event.terminal_path = Some(String::from("review_handoff"));

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
	store.record_linear_execution_event(&event).expect("linear event should record");

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
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(
		diagnostic.next_action.contains("review-handoff diagnose PUB-1626 --json"),
		"PR lineage blockers should route to review-handoff recovery, got {:?}",
		diagnostic.next_action
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_tracker_comment_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "linear-issue-1626",
			issue_identifier: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
		},
		"review_handoff",
		String::from("2026-06-28T00:00:00Z"),
		"review_handoff",
	);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	event.branch = Some(String::from("x/pubfi-pub-1626"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
	event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(String::from("Recorded review handoff lineage."));
	event.terminal_path = Some(String::from("review_handoff"));

	let comment = TrackerComment {
		body: records::append_structured_comment_record(
			&records::render_linear_execution_event_comment_body(&event, None),
			&event,
		)
		.expect("structured comment should serialize"),
		created_at: String::from("2026-06-28T00:00:00Z"),
	};

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

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]).with_comments(vec![comment]);
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
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let marker = ReviewHandoffMarker::new(
		"run-1626",
		1,
		"x/pubfi-pub-1626",
		"https://github.com/hack-ink/decodex/pull/1626",
		"main",
		"x/pubfi-pub-1626",
		"2222222222222222222222222222222222222222",
	);
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
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
		.upsert_review_handoff_marker("pubfi", "PUB-1626", &marker)
		.expect("identifier-keyed review lifecycle should record");

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
	assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_unreadable_activity_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	fs::create_dir_all(worktree_path.join(RUN_ACTIVITY_MARKER_FILE))
		.expect("directory marker should create");

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
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
	assert!(
		diagnostic.evidence.iter().any(|entry| entry.starts_with("worktree_status_error:")),
		"diagnostic should include marker read error evidence: {:?}",
		diagnostic.evidence
	);
	assert!(!diagnostic.recoverable());
}
