use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{
			self, GhostLaneTestTracker,
			stale_active::{self},
		},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_final_label_guard_rejects_late_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	stale_active::seed_lane_claim(&context.state_store, &issue.id, "run-1626");

	let error = super::ensure_stale_active_run_claim_guard(
		&context.config,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("final guard should reject late lease");

	assert!(
		error.to_string().contains("appeared before active-label release"),
		"unexpected final guard error: {error:?}"
	);
}

#[test]
fn stale_active_release_clears_only_matching_dead_process_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

	stale_active::seed_lane_claim(&store, &issue.id, "run-1626");

	stale_active::append_dead_process_interrupt_control_telemetry(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
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

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);

	store
		.upsert_lease("pubfi", &issue.identifier, "run-other", "In Progress")
		.expect("unrelated issue-key lease should record");

	let cleared = super::clear_stale_active_dead_run_claims_before_release(&store, diagnostic)
		.expect("dead matching run lease cleanup should run");

	assert!(cleared);
	assert!(store.claim_for_lane("pubfi", &issue.id).expect("matching claim read").is_none());
	assert_eq!(
		store
			.lease_for_issue(&issue.identifier)
			.expect("nonmatching lease should read")
			.expect("nonmatching lease should remain")
			.run_id(),
		"run-other"
	);
}

#[test]
fn stale_active_diagnose_blocks_when_run_lease_is_present() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	stale_active::seed_lane_claim(&store, &issue.id, "run-1626");

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
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(!diagnostic.recoverable());
}
