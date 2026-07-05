use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_diagnose_blocks_shared_claim_lock_file() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let owner_store = StateStore::open_in_memory().expect("owner store should open");
	let store = StateStore::open_in_memory().expect("reader store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	owner_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("owner store should configure dispatch root");

	assert!(
		owner_store
			.try_acquire_lease("pubfi", &issue.id, "run-live", "In Progress")
			.expect("owner should acquire shared claim")
	);

	store
		.observe_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("reader store should observe dispatch root");
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

	assert_eq!(diagnostic.classification, recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.active_shared_claim);
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");

	store
		.upsert_lease("pubfi", &issue.identifier, "run-identifier", "In Progress")
		.expect("identifier-keyed lease should record");
	store
		.record_run_attempt("run-identifier", &issue.identifier, 1, "running")
		.expect("identifier-keyed run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.identifier,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("identifier-keyed worktree mapping should record");

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

	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-identifier"));
	assert!(diagnostic.run_lease);
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(!diagnostic.recoverable());
}
