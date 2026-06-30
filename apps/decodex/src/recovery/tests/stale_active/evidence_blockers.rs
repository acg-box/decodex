use super::*;

#[test]
fn stale_active_diagnose_blocks_private_progress_from_older_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-new"));
	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_protocol_event_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_marker_protocol_activity_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_marker_present")));
	assert!(
		diagnostic.blockers.contains(&String::from("activity_marker_protocol_activity_present"))
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_untracked_worktree_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(&worktree_path);
	fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
		.expect("untracked source should write");
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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_non_git_retained_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(&worktree_path).expect("retained path should create");
	fs::write(worktree_path.join("retained.txt"), "retained work\n")
		.expect("retained file should write");
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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "non_git_files_present");
	assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

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
		.record_run_activity_summary("run-1626", 1, Some(&activity), None)
		.expect("child activity should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_when_worktree_status_unknown() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	fs::create_dir_all(&worktree_path).expect("worktree path should create");
	fs::write(worktree_path.join(".git"), "gitdir: /does/not/exist\n")
		.expect("invalid gitdir should write");
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

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "tracked_changes_unknown");
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_needs_attention_label() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let needs_attention_label = String::from("decodex:needs-attention");
	let mut issue = sample_issue_with_labels("Todo", &[active_label, needs_attention_label]);

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

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("needs_attention_label_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_review_policy_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

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
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: &issue.id,
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_review_policy_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

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
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("identifier-keyed review checkpoint should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_tracker_comment_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let marker = ReviewHandoffMarker::new(
		"run-1626",
		1,
		"x/pubfi-pub-1626",
		"https://github.com/hack-ink/decodex/pull/1626",
		"main",
		"x/pubfi-pub-1626",
		"2222222222222222222222222222222222222222",
	);

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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_unreadable_activity_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(worktree_path.join(state::RUN_ACTIVITY_MARKER_FILE))
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
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
	assert!(
		diagnostic.evidence.iter().any(|entry| entry.starts_with("worktree_status_error:")),
		"diagnostic should include marker read error evidence: {:?}",
		diagnostic.evidence
	);
	assert!(!diagnostic.recoverable());
}
