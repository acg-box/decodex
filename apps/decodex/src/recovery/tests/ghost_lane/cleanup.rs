use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT, GHOST_LANE_TERMINAL_STATUS,
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::StateStore,
};

#[test]
fn ghost_lane_cleanup_terminalizes_missing_issue_lease_and_records_private_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let mut diagnostics =
		super::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUBFI-012"))
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

	super::apply_ghost_lane_cleanup(&store, &diagnostic).expect("cleanup should apply");

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
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	context
		.state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let diagnostics = super::diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-012"),
	)
	.expect("ghost lane diagnostic should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
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
