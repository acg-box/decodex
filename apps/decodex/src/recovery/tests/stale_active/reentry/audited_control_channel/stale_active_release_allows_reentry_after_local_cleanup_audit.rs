use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, RUN_CONTROL_CHANNEL_STATUS_FAILED,
		RecoveryRuntimeMutationPolicy, STALE_ACTIVE_CLASSIFICATION,
		tests::{
			self, GhostLaneTestTracker,
			stale_active::{self},
		},
	},
	tracker,
};

#[test]
fn stale_active_release_allows_reentry_after_local_cleanup_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let mut issue =
		tests::sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");

	stale_active::seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);

	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	context
		.state_store
		.retire_run_control_channel_for_attempt("run-1626", 1, RUN_CONTROL_CHANNEL_STATUS_FAILED)
		.expect("control channel should retire");

	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");

	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");

	stale_active::append_stale_active_release_audit(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = stale_active::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));

	stale_active::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("reentry release should remove active label");

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
}
