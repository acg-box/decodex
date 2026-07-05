use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker, stale_active::release},
	},
	state::RUN_CONTROL_CHANNEL_DIR,
	tracker,
};

#[test]
fn stale_active_release_removes_run_control_marker_only_directory() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let control_dir = worktree_path.join(RUN_CONTROL_CHANNEL_DIR);
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");

	fs::create_dir_all(&control_dir).expect("run-control marker directory should create");
	fs::write(control_dir.join("run-1626-1.channel"), "channel\n")
		.expect("run-control marker should write");

	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = release::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	release::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("stale active release should apply");

	assert!(!worktree_path.exists(), "marker-only directory should be removed");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}
