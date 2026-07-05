use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, RecoveryRuntimeMutationPolicy, STALE_ACTIVE_RELEASE_EVENT,
		tests::{self, GhostLaneTestTracker, stale_active::release},
	},
	tracker,
};

#[test]
fn stale_active_release_removes_active_label_and_terminalizes_stale_run() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");

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
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
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

	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");
	let events = context
		.state_store
		.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
		.expect("private events should read");

	assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
	assert!(events.iter().any(|event| {
		event.event_type() == STALE_ACTIVE_RELEASE_EVENT
			&& event.payload()["schema"] == release::STALE_ACTIVE_RECOVERY_SCHEMA
			&& event.payload()["active_label_release"] == "pending_final_mutation"
			&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
	}));
}
