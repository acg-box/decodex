use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy, STALE_ACTIVE_CLASSIFICATION,
		tests::{
			self, GhostLaneTestTracker,
			stale_active::{self},
		},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_diagnose_allows_app_server_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("In Progress", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	stale_active::append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"implement_to_validation_ready",
		"app_server_dynamic_tool_protocol_failure",
	);

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

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
}

#[test]
fn stale_active_diagnose_blocks_repo_gate_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("In Progress", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	stale_active::append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"implement_to_validation_ready",
		"repo_gate_verify_failed",
	);

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
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_repair_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("In Progress", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	stale_active::append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"repair_accepted_review_findings",
		"app_server_dynamic_tool_protocol_failure",
	);

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
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}
