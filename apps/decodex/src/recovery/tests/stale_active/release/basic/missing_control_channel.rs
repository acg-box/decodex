use tempfile::TempDir;

use crate::{
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, RecoveryRuntimeMutationPolicy,
		tests::{
			self, GhostLaneTestTracker,
			stale_active::{self, release},
		},
	},
	tracker,
};

#[test]
fn allows_final_reentry_without_control_channel() {
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

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));

	release::apply_stale_active_release_with_tracker(
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
fn guards_terminal_looking_run_before_final_check() {
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

		assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
		assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));

		release::apply_stale_active_release_with_tracker(
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
fn releases_clean_retained_worktree_when_active_label_is_already_absent() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[]);

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

	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "interrupted")
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

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("active_label_already_absent_cleanup")));

	release::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("clean retained worktree should release without an active-label mutation");

	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
	assert!(!worktree_path.exists(), "retained worktree should be removed");
	assert!(tracker.label_removals.borrow().is_empty());
}
