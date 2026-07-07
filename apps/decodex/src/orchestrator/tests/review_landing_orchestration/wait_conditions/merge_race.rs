use std::fs;

use crate::orchestrator::{
	self, StateStore,
	tests::{
		self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
	},
};
use crate::state::ReviewPolicyCheckpointInput;

#[test]
fn reconcile_post_review_orchestration_tolerates_already_merged_merge_race() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/2","change":"Land current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
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

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_transition_fixture(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "run-1:runtime-review:repair:ready",
			attempt_number: 1,
			phase: "repair",
			review_level: "strict",
			status: "clean",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("runtime review checkpoint should persist");

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

	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

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
			String::from("pr"),
			String::from("view"),
			String::from(pr_url),
			String::from("--json"),
			String::from("state,headRefOid,mergeCommit"),
		]
	);
	let lifecycle = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, "main")
		.expect("lifecycle record should read")
		.expect("landing authority should record");
	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.merge_commit(), Some("cafebabe"));
	assert!(
		tracker.comments.borrow().is_empty(),
		"already-merged race handling should persist orchestration in StateStore, not Linear comments",
	);
	assert!(tracker.label_additions.borrow().is_empty());
}
