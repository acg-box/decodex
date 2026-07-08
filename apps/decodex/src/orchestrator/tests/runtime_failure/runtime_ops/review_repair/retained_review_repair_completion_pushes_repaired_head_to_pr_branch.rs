use crate::{
	orchestrator::tests::{
		self,
		runtime_failure::{self, TestEnvVarGuard, orchestrator, process},
	},
	test_support,
};

#[test]
fn retained_review_repair_completion_pushes_repaired_head_to_pr_branch() {
	let (temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_REPAIR_PUSH_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&base_config, &env_var);
	let remote_root = temp_dir.path().join("origin.git");
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);

	runtime_failure::add_origin_remote(config.repo_root(), &remote_root);
	runtime_failure::checkout_new_branch(config.repo_root(), &issue_run.worktree.branch_name);

	let local_head = runtime_failure::commit_worktree_change(
		config.repo_root(),
		"repair.txt",
		"repair\n",
		r#"{"schema":"decodex/commit/2","change":"Retain review repair","authority":"XY-1115","impact":"compatible"}"#,
	);

	orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect("retained review-repair completion should push the repaired head");

	let output = test_support::hermetic_git_command()
		.arg("--git-dir")
		.arg(&remote_root)
		.args(["rev-parse", &format!("refs/heads/{}", issue_run.worktree.branch_name)])
		.output()
		.expect("remote head probe should run");

	assert!(
		output.status.success(),
		"remote retained repair branch should exist: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), local_head);
}
