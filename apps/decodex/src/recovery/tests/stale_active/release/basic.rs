use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, RecoveryRuntimeMutationPolicy, STALE_ACTIVE_RELEASE_EVENT,
		tests::{
			self, FinalNeedsAttentionTracker, GhostLaneTestTracker,
			stale_active::{self},
		},
	},
	state::RUN_CONTROL_CHANNEL_DIR,
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
	let mut diagnostics = super::diagnose_stale_active_issues(
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

	super::apply_stale_active_release_with_tracker(
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
			&& event.payload()["schema"] == super::STALE_ACTIVE_RECOVERY_SCHEMA
			&& event.payload()["active_label_release"] == "pending_final_mutation"
			&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
	}));
}

#[test]
fn stale_active_release_allows_final_reentry_when_control_channel_was_never_published() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");

	tests::init_git_repo(context.config.repo_root());
	tests::run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
	tests::commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
	tests::run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	tests::run_git(
		context.config.repo_root(),
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	tests::run_git(
		context.config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-1626",
			worktree_path.to_str().expect("worktree path should be utf-8"),
			"main",
		],
	);
	stale_active::seed_dead_orphan_runtime_telemetry_without_control_channel(
		&context.state_store,
		&issue,
		&worktree_path,
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::diagnose_stale_active_issues(
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

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));

	super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("stale active release should treat missing control channel as inactive reentry");

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_terminal_guards_terminal_looking_run_before_final_safety_check() {
	for status in ["failed", "interrupted"] {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = tests::sample_recovery_context(
			&temp_dir,
			RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let worktree_path = context.config.worktree_root().join("PUB-1626");
		let mut issue =
			tests::sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");

		tests::init_git_repo(context.config.repo_root());
		tests::run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
		tests::commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
		tests::run_git(
			context.config.repo_root(),
			&["update-ref", "refs/remotes/origin/main", "HEAD"],
		);
		tests::run_git(
			context.config.repo_root(),
			&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
		);
		tests::run_git(
			context.config.repo_root(),
			&[
				"worktree",
				"add",
				"-b",
				"x/pubfi-pub-1626",
				worktree_path.to_str().expect("worktree path should be utf-8"),
				"main",
			],
		);
		stale_active::seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);

		context
			.state_store
			.update_run_status("run-1626", status)
			.expect("run should carry terminal-looking app-server status");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
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

		assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
		assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("terminal-looking stale-active run should release after terminal guard");

		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}
}

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
	let mut diagnostics = super::diagnose_stale_active_issues(
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

	super::apply_stale_active_release_with_tracker(
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

#[test]
fn stale_active_release_keeps_active_label_gate_when_tracker_label_removal_fails() {
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

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()])
		.remove_error("Linear label removal failed");
	let mut diagnostics = super::diagnose_stale_active_issues(
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
	let error = super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("tracker removal failure should abort release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");
	let events = context
		.state_store
		.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
		.expect("private events should read");
	let mapping =
		context.state_store.worktree_for_issue(&issue.id).expect("worktree mapping should read");

	assert!(error.to_string().contains("Linear label removal failed"));
	assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
	assert!(events.iter().any(|event| event.event_type() == STALE_ACTIVE_RELEASE_EVENT));
	assert!(mapping.is_none());
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_revalidates_needs_attention_before_final_label_removal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let needs_attention_label =
		context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

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

	let tracker = FinalNeedsAttentionTracker::new(issue, needs_attention_label);
	let mut diagnostics = super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("initial stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	let error = super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late needs-attention should block active-label release");
	let message = error.to_string();

	assert!(message.contains("safety inspection changed before apply"));
	assert!(message.contains("needs_attention_label_present"));
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"active label should not be removed after late needs-attention appears"
	);
}
