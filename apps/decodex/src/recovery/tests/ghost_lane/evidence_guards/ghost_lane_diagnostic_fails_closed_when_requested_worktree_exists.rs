use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{self, GHOST_LANE_BLOCKED_CLASSIFICATION, tests::GhostLaneTestTracker},
	state::{ReviewHandoffMarker, StateStore},
};

#[test]
fn ghost_lane_diagnostic_fails_closed_when_requested_worktree_exists() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::missing();
	let worktree_path = temp_dir.path().join("PUBFI-012");

	fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = recovery::diagnose_ghost_lanes(
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

	let diagnostics =
		recovery::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUB-012"))
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

	let diagnostics =
		recovery::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUB-012"))
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

	let diagnostics =
		recovery::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUB-012"))
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
