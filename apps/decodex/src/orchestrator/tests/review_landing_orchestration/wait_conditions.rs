use std::fs;

use crate::{
	orchestrator::{
		self, ReviewOrchestrationMarker, StateStore,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
		},
	},
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn reconcile_post_review_orchestration_tolerates_already_merged_merge_race() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response_with_merge_exit_code(&temp_dir, &head_oid, 1);
	let config = tests::service_config_with_github_token_env_var_and_command_path(
		&config,
		"PATH",
		&gh_command_path,
	);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_orchestration_marker(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should accept an already-merged PR race");

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "waiting_for_merge");
	assert!(marker.auto_merge_enabled_at_unix_epoch().is_some());
	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			head_oid,
			String::from("--subject"),
			String::from(landed_merge_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
			String::from("pr"),
			String::from("view"),
			String::from(pr_url),
			String::from("--json"),
			String::from("state,headRefOid,mergeCommit"),
		]
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"already-merged race handling should persist orchestration in StateStore, not Linear comments",
	);
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_waits_for_green_checks_before_requesting_external_review() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"main",
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("PENDING"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_waits_when_pr_readback_degrades() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"main",
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Err(color_eyre::eyre::eyre!(
			"gh api failed"
		))]),
	)
	.expect("post-review orchestration should tolerate degraded PR readback");

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_waits_when_worktree_head_read_fails() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let branch_ref_path =
		config.repo_root().join(".git").join("refs").join("heads").join(&worktree.branch_name);
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);
	fs::remove_file(&branch_ref_path).expect("branch ref should remove");
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should tolerate local worktree readback failure");

	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}

#[test]
fn reconcile_post_review_orchestration_waits_when_worktree_branch_read_fails() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let missing_worktree_path = temp_dir.path().join("missing-retained-worktree");
	let branch_name = "x/pubfi-pub-101";
	let head_oid = tests::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	fs::create_dir_all(&missing_worktree_path).expect("broken worktree path should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			branch_name,
			&missing_worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(branch_name, pr_url, &head_oid),
	);
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should tolerate local branch readback failure");

	assert!(
		tracker.comments.borrow().is_empty(),
		"unexpected tracker comments: {:#?}",
		tracker.comments.borrow()
	);
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
