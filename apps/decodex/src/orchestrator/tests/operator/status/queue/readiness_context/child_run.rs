use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, ProtocolActivityEventSummary, ProtocolActivityMarker,
	ProtocolActivitySummary, StateStore, fs, orchestrator, state,
};

#[test]
fn live_operator_status_snapshot_surfaces_failed_child_run_after_archive_race() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-109",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![ProtocolActivityEventSummary {
			event_type: String::from("thread/archive/discarded"),
			category: String::from("thread"),
			detail: Some(String::from("discarded")),
		}],
	};

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-109", ".worktrees/PUB-109", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained child patch\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt("run-archive-race", &issue.id, 4, "failed")
		.expect("failed run attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-109",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_event("run-archive-race", 963, "thread/archive/discarded", "{}")
		.expect("archive discard should record");
	state_store
		.append_event(
			"run-archive-race",
			963,
			"item/commandExecution/outputDelta",
			r#"{"delta":"late output"}"#,
		)
		.expect("late output should be discarded without corrupting status");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-archive-race",
			attempt_number: 4,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "thread/archive/discarded",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol marker should write");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-109")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.issue_identifier.as_deref() == Some("PUB-109"))
		.expect("retained worktree should remain visible with queue ownership");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.run_id.as_deref(), Some("run-archive-race"));
	assert_eq!(attention.attempt_number, Some(4));
	assert_eq!(attention.attempt_status.as_deref(), Some("failed"));
	assert_eq!(attention.last_event_type.as_deref(), Some("thread/archive/discarded"));
	assert_eq!(attention.event_count, 2);
	assert_eq!(attention.worktree_path.as_deref(), Some(".worktrees/PUB-109"));
	assert!(attention.worktree_has_tracked_changes);
	assert!(attention.summary.contains("Child implementation attempt failed"));
	assert!(attention.summary.contains("parent journal or closeout handling"));
	assert_eq!(worktree.ownership, "orphaned_live_thread");
	assert_eq!(worktree.branch_name, "x/pubfi-pub-109");
	assert_eq!(worktree.worktree_path, ".worktrees/PUB-109");
}
