use tempfile::TempDir;

use crate::recovery::{
	GHOST_LANE_BLOCKED_CLASSIFICATION, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
	RecoveryRuntimeMutationPolicy,
	tests::{self, GhostLaneTestTracker},
};

#[test]
fn ghost_lane_diagnostic_allows_mcp_test_fixture_control_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	tests::seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

	let diagnostics = super::diagnose_ghost_lanes_read_only(
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
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	tests::seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());
	tests::append_mcp_test_fixture_ghost_lane_cleanup_audit(&context.state_store);

	let diagnostics = super::diagnose_ghost_lanes_read_only(
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
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();

	tests::seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

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

	let diagnostics = super::diagnose_ghost_lanes_read_only(
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
