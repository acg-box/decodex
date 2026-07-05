use tempfile::TempDir;

use crate::recovery::{
	self, GHOST_LANE_TERMINAL_STATUS, RecoveryRuntimeMutationPolicy,
	tests::{self, GhostLaneTestTracker},
};

#[test]
fn review_handoff_diagnose_targeted_terminal_identifier_worktree_before_tracker_lookup() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::identifier_error(
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

	context
		.state_store
		.record_run_attempt("run-01", stale_issue_id, 1, GHOST_LANE_TERMINAL_STATUS)
		.expect("terminal attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			stale_issue_id,
			"x/pubfi-pub-001",
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let diagnostic = recovery::diagnose_issue_with_tracker(&context, &tracker, stale_issue_id)
		.expect("targeted retained review diagnostic should classify local residue");

	assert_eq!(
		diagnostic.classification,
		crate::recovery::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
	);
	assert_eq!(diagnostic.issue_id, stale_issue_id);
	assert_eq!(diagnostic.issue_state, "local_terminal_residue");
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"targeted terminal local residue must not be sent to tracker refresh"
	);
}
