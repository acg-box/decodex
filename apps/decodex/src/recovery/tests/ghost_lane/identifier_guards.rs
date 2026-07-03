use std::cell::RefCell;

use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION,
		tests::{self, GhostLaneTestTracker},
	},
	state::StateStore,
};

#[test]
fn ghost_lane_diagnostic_treats_invalid_local_issue_id_refresh_as_missing_issue() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let tracker = GhostLaneTestTracker::refresh_error(
		"Linear GraphQL request failed: Argument Validation Error",
	);

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");

	let diagnostics = super::diagnose_ghost_lanes_read_only(
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

	let diagnostics = super::diagnose_ghost_lanes_read_only(
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
	let mut issue = tests::sample_issue("In Progress");

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

	let diagnostics =
		super::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUBFI-012"))
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

	let error =
		super::diagnose_ghost_lanes("pubfi", temp_dir.path(), &store, &tracker, Some("PUBFI-012"))
			.expect_err("unrelated issue prefixes should not match by numeric suffix alone");

	assert!(format!("{error:#}").contains("No leased lane matched"), "unexpected error: {error:#}");
	assert!(
		store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
		"failed selector matches must preserve attention"
	);
}
