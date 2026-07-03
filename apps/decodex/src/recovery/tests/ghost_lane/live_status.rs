use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION, GhostLaneDiagnostic,
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{self},
};

#[test]
fn ghost_lane_live_status_overlay_tracker_backoff_stays_read_only() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
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

	let mut diagnostics = super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&missing_tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let error = super::apply_ghost_lane_live_status_blockers_with_tracker(
		&error_tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&mut diagnostics,
	)
	.expect_err("overlay tracker error should surface for recovery backoff wrapping");
	let message =
		super::remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
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
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = context.config.worktree_root().join("PUB-012");
	let mut diagnostics = vec![super::GhostLaneDiagnostic {
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
	super::apply_ghost_lane_live_status_blockers_with_tracker(
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
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = context.config.worktree_root().join("PUB-012");
	let diagnostic = GhostLaneDiagnostic {
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

	let error = super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
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
