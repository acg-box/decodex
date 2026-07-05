use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy, STALE_ACTIVE_CLASSIFICATION,
		tests::{
			self, GhostLaneTestTracker,
			stale_active::{self},
		},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_diagnose_allows_dead_orphan_thread_runtime_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("In Progress", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
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

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("process_not_alive")));
	assert!(diagnostic.evidence.contains(&String::from("stale_active_control_channel_present")));
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
	);
}

#[test]
fn stale_active_diagnose_allows_dead_process_leased_claim_cleanup() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

	store
		.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
		.expect("dead run lease should remain recorded");

	stale_active::append_dead_process_interrupt_control_telemetry(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
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

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.run_lease);
	assert!(diagnostic.active_shared_claim);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("stale_run_lease_present")));
	assert!(diagnostic.evidence.contains(&String::from("stale_active_shared_claim_present")));
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
	);
}

#[test]
fn stale_active_diagnose_blocks_dead_marker_for_different_leased_run() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

	store
		.record_run_attempt("run-latest", &issue.id, 2, "running")
		.expect("latest run should record");
	store
		.upsert_lease("pubfi", &issue.id, "run-latest", "In Progress")
		.expect("dead run lease should remain recorded");

	stale_active::append_dead_process_interrupt_control_telemetry(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
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

	assert_eq!(diagnostic.classification, crate::recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(
		!diagnostic.evidence.contains(&String::from("stale_run_lease_present")),
		"mismatched marker must not authorize lease cleanup"
	);
	assert!(!diagnostic.recoverable());
}
