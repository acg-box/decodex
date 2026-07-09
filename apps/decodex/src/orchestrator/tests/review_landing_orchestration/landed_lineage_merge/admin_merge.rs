use std::fs;

use crate::{
	orchestrator::{
		self, StateStore,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
		},
	},
	state::ReviewPolicyCheckpointInput,
};

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_after_external_pass() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_github_token_env_var_and_command_path(
		&config,
		"PATH",
		&gh_command_path,
	);
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
		.expect("strict runtime review checkpoint should persist");

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
	.expect("post-review orchestration should succeed");

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
			head_oid.clone(),
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

	let lifecycle = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, "main")
		.expect("lifecycle record should read")
		.expect("landing authority should record");

	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.transition(), "landed");
	assert_eq!(lifecycle.merge_commit(), Some("cafebabe"));
}

#[test]
fn records_lifecycle_attention_when_admin_merge_unavailable() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"blocked retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "blocked.txt", "ready\n", merge_subject);

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
		.expect("strict runtime review checkpoint should persist");

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

	review_state.merge_commit_allowed = false;

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should request attention");

	let lifecycle = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, "main")
		.expect("lifecycle record should read")
		.expect("manual attention authority should record");

	assert_eq!(lifecycle.next_state(), "manual_attention_required");
	assert_eq!(lifecycle.transition(), "manual_attention_required");
	assert_eq!(lifecycle.next_action(), "request_manual_attention");
}
