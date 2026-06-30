use super::*;

#[test]
fn ghost_lane_live_status_overlay_tracker_backoff_stays_read_only() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let missing_tracker = GhostLaneTestTracker::missing();
	let error_tracker =
		GhostLaneTestTracker::refresh_error("Linear connector timed out while testing");

	context
		.state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let mut diagnostics = super::super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&missing_tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let error = super::super::apply_ghost_lane_live_status_blockers_with_tracker(
		&error_tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&mut diagnostics,
	)
	.expect_err("overlay tracker error should surface for recovery backoff wrapping");
	let message = super::super::remember_recovery_tracker_backoff_message(
		&context,
		&error,
		"ghost_lane_recovery",
	)
	.expect("timeout should become a recovery backoff message");

	assert!(message.contains("ghost_lane_recovery"));
	assert!(
		context
			.state_store
			.connector_backoff(context.config.service_id(), "linear")
			.expect("backoff should read")
			.is_none(),
		"read-only live-status overlay must not persist connector backoff"
	);
}

#[test]
fn ghost_lane_diagnose_live_status_overlay_blocks_active_thread_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = context.config.worktree_root().join("PUB-012");
	let mut diagnostics = vec![super::super::GhostLaneDiagnostic {
		project_id: String::from("pubfi"),
		issue_id: String::from("PUB-012"),
		issue_identifier: Some(String::from("PUBFI-012")),
		run_id: String::from("run-12"),
		attempt_number: 1,
		attempt_status: String::from("running"),
		classification: String::from(GHOST_LANE_CLASSIFICATION),
		reason: String::from("test"),
		run_lease: true,
		control_channel: String::from("missing"),
		evidence: Vec::new(),
		blockers: Vec::new(),
		next_action: String::from("test"),
	}];

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	context
		.state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	context
		.state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-12",
		1,
		Some("thread-12"),
		Some("turn-12"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");
	super::super::apply_ghost_lane_live_status_blockers_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&mut diagnostics,
	)
	.expect("status overlay should run");

	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("status:thread_active")));
	assert!(diagnostic.blockers.contains(&String::from("status:retained_worktree_present")));
}

#[test]
fn ghost_lane_cleanup_live_status_gate_rejects_active_thread_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = context.config.worktree_root().join("PUB-012");
	let diagnostic = super::super::GhostLaneDiagnostic {
		project_id: String::from("pubfi"),
		issue_id: String::from("PUB-012"),
		issue_identifier: Some(String::from("PUBFI-012")),
		run_id: String::from("run-12"),
		attempt_number: 1,
		attempt_status: String::from("running"),
		classification: String::from(GHOST_LANE_CLASSIFICATION),
		reason: String::from("test"),
		run_lease: true,
		control_channel: String::from("missing"),
		evidence: Vec::new(),
		blockers: Vec::new(),
		next_action: String::from("test"),
	};

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	context
		.state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	context
		.state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-12",
		1,
		Some("thread-12"),
		Some("turn-12"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");

	let error = super::super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("live status should reject cleanup");
	let message = format!("{error:#}");

	assert!(message.contains("live status reported blockers"));
	assert!(message.contains("thread_active"));
	assert!(message.contains("retained_worktree_present"));
}

#[test]
fn ghost_lane_cleanup_terminalizes_missing_issue_lease_and_records_private_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let mut diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUBFI-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
	assert!(diagnostic.recoverable());
	assert_eq!(diagnostic.issue_id, "PUB-012");
	assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
	assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
	assert!(diagnostic.evidence.contains(&String::from("worktree_missing")));
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
	assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
	assert!(diagnostic.evidence.contains(&String::from("review_lineage_missing")));

	super::super::apply_ghost_lane_cleanup(&store, &diagnostic).expect("cleanup should apply");

	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").is_empty(),
		"cleanup should clear the local run lease"
	);

	let runs = store.list_project_issue_runs("pubfi", "PUB-012").expect("issue runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].status(), GHOST_LANE_TERMINAL_STATUS);

	let events = store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("private events should load");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), GHOST_LANE_CLEANUP_EVENT);
	assert_eq!(
		events[0].payload()["schema"].as_str(),
		Some("decodex.ghost_lane_recovery_private_event/1")
	);
	assert_eq!(events[0].payload()["cleared_run_lease"].as_bool(), Some(true));
}

#[test]
fn ghost_lane_cleanup_dry_run_validation_keeps_runtime_state_untouched() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	context
		.state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
	.expect("dry-run validation should allow cleanup");

	let runs = context
		.state_store
		.list_project_issue_runs("pubfi", "PUB-012")
		.expect("issue runs should load");
	let events = context
		.state_store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("private events should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].status(), "running");
	assert!(
		!context.state_store.list_leased_runs("pubfi").expect("leased runs should load").is_empty(),
		"dry-run validation must not clear the run lease"
	);
	assert!(events.is_empty(), "dry-run validation must not write cleanup audit events");
}

#[test]
fn ghost_lane_diagnostic_allows_mcp_test_fixture_control_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("mcp-test fixture ghost lane should diagnose");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
	assert!(diagnostic.recoverable());
	assert!(diagnostic.blockers.is_empty());
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
	);
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("mcp_test_fixture_protocol_or_thread_evidence_present"))
	);
}

#[test]
fn ghost_lane_diagnostic_allows_prior_mcp_test_fixture_cleanup_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());
	append_mcp_test_fixture_ghost_lane_cleanup_audit(&context.state_store);

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUBFI-012"),
	)
	.expect("prior cleanup audit should not block an idempotent diagnosis");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
	assert!(diagnostic.recoverable());
	assert!(diagnostic.blockers.is_empty());
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_mcp_fixture_has_mixed_private_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

	context
		.state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"progress_checkpoint",
			serde_json::json!({"source": "runtime", "phase": "implementing"}),
		)
		.expect("real private evidence should record");

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
}

#[test]
fn ghost_lane_diagnostic_treats_invalid_local_issue_id_refresh_as_missing_issue() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::refresh_error(
		"Linear GraphQL request failed: Argument Validation Error",
	);

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("missing local issue id should not abort ghost-lane diagnosis");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
	assert!(diagnostic.recoverable());
	assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
}

#[test]
fn ghost_lane_diagnostic_treats_missing_identifier_lookup_as_missing_issue() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::identifier_error(
		"Linear GraphQL request failed: Entity not found: Issue",
	);

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes_read_only(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("missing issue identifier should not abort ghost-lane diagnosis");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
	assert!(diagnostic.recoverable());
	assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_requested_issue_identifier_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue("In Progress");

	issue.id = String::from("linear-pubfi-012");
	issue.identifier = String::from("PUBFI-012");

	let tracker = GhostLaneTestTracker {
		issues: vec![issue],
		refresh_error: None,
		identifier_error: None,
		remove_error: None,
		comments: Vec::new(),
		refresh_queries: RefCell::new(Vec::new()),
		label_removals: RefCell::new(Vec::new()),
		state_updates: RefCell::new(Vec::new()),
	};

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUBFI-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
	assert!(diagnostic.blockers.contains(&String::from("tracker_issue_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_rejects_unrelated_requested_identifier() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();

	store.record_run_attempt("run-12", "ABC-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "ABC-012", "run-12", "In Progress").expect("lease should record");

	let error = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUBFI-012"),
	)
	.expect_err("unrelated issue prefixes should not match by numeric suffix alone");

	assert!(format!("{error:#}").contains("No leased lane matched"), "unexpected error: {error:#}");
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"failed selector matches must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_requested_worktree_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = temp_dir.path().join("PUBFI-012");

	fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUBFI-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_control_channel_row_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let channel_path = temp_dir.path().join("missing-control-channel.json");

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.evidence.contains(&String::from("control_channel_file_missing")));
	assert!(diagnostic.blockers.contains(&String::from("control_channel_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_private_evidence_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"diagnostic",
			serde_json::json!({"schema": "test.private/1"}),
		)
		.expect("private evidence should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_review_lifecycle_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let marker = ReviewHandoffMarker::new(
		"run-12",
		1,
		"x/pubfi-pub-012",
		"https://github.com/hack-ink/decodex/pull/12",
		"main",
		"x/pubfi-pub-012",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-012", &marker)
		.expect("review lifecycle should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_review_checkpoint_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_pr_lineage_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-012",
			issue_identifier: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-06-18T00:00:00Z"),
		"closeout",
	);

	event.branch = Some(String::from("x/pubfi-pub-012"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/12"));
	event.pr_head_sha = Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d7"));
	event.summary = Some(String::from("Recorded retained closeout."));

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store.record_linear_execution_event(&event).expect("linear event should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_retained_worktree_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = temp_dir.path().join("PUB-012");

	fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}

#[test]
fn ghost_lane_diagnostic_fails_closed_when_activity_summary_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store
		.record_run_activity_summary("run-12", 1, Some(&activity), None)
		.expect("activity summary should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::super::diagnose_ghost_lanes(
		"pubfi",
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"fail-closed diagnostics must preserve attention"
	);
}
