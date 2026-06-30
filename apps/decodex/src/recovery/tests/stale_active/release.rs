use super::*;

#[test]
fn stale_active_release_removes_active_label_and_terminalizes_stale_run() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::super::super::apply_stale_active_release_with_tracker(
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
			&& event.payload()["schema"] == super::super::super::STALE_ACTIVE_RECOVERY_SCHEMA
			&& event.payload()["active_label_release"] == "pending_final_mutation"
			&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
	}));
}

#[test]
fn stale_active_release_allows_final_reentry_when_control_channel_was_never_published() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(context.config.repo_root());
	run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
	commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
	run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	run_git(
		context.config.repo_root(),
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git(
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
	seed_dead_orphan_runtime_telemetry_without_control_channel(
		&context.state_store,
		&issue,
		&worktree_path,
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));

	super::super::super::apply_stale_active_release_with_tracker(
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
		let context = sample_recovery_context(
			&temp_dir,
			super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(context.config.repo_root());
		run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
		commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
		run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
		run_git(
			context.config.repo_root(),
			&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
		);
		run_git(
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
		seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);
		context
			.state_store
			.update_run_status("run-1626", status)
			.expect("run should carry terminal-looking app-server status");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::super::super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
		assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));

		super::super::super::apply_stale_active_release_with_tracker(
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
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let control_dir = worktree_path.join(state::RUN_CONTROL_CHANNEL_DIR);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::super::super::apply_stale_active_release_with_tracker(
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
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	let error = super::super::super::apply_stale_active_release_with_tracker(
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
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let needs_attention_label =
		context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("initial stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	let error = super::super::super::apply_stale_active_release_with_tracker(
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

#[test]
fn stale_active_release_preflight_rejects_worktree_progress_after_diagnosis() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	fs::write(worktree_path.join("late_progress.rs"), "fn late_progress() {}\n")
		.expect("late untracked progress should write");
	let error = super::super::super::preflight_stale_active_worktree_cleanup(
		&context.state_store,
		&diagnostic,
	)
	.expect_err("preflight should reject late retained progress");

	assert!(
		error.to_string().contains("retained worktree changes appeared before cleanup"),
		"unexpected preflight error: {error:?}"
	);
}

#[test]
fn stale_active_release_revalidates_late_default_worktree_progress_without_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let default_worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	init_git_repo(&default_worktree_path);
	fs::write(default_worktree_path.join("late_default_progress.rs"), "fn late() {}\n")
		.expect("late default progress should write");
	let error = super::super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late default worktree progress should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_release_revalidates_late_run_lease_before_mutation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
		.expect("late lease should record");

	let error = super::super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late run lease should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_release_revalidates_late_review_policy_before_mutation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

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
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: context.config.service_id(),
			issue_id: &issue.id,
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("late review checkpoint should record");

	let error = super::super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late review checkpoint should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply")
			|| error.to_string().contains("review authority appeared"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_final_label_guard_rejects_late_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
		.expect("late lease should record");

	let error = super::super::super::ensure_stale_active_run_claim_guard(
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
fn stale_active_diagnose_blocks_when_run_lease_is_present() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress").expect("lease should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(!diagnostic.recoverable());
}
