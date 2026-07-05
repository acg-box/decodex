use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, RetainedReviewRepairPushFailed, RunFailureWritebackDisposition, TestEnvVarGuard,
		orchestrator, process,
	},
};

#[test]
fn retained_review_repair_push_failures_are_structured_terminal_attention() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_REPAIR_PUSH_TOKEN_ENV_{}", process::id());
	let config =
		runtime_failure::service_config_with_github_token_env_var(&base_config, &missing_env_var);
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);
	let error = orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect_err("missing GitHub token should produce a typed push failure");
	let push_failure = error
		.downcast_ref::<RetainedReviewRepairPushFailed>()
		.expect("missing push authority should preserve typed error");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(push_failure.error_class(), "retained_review_repair_push_auth_failed");
	assert_eq!(error_class, "retained_review_repair_push_auth_failed");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(next_action.contains(&issue_run.worktree.branch_name));
	assert_eq!(
		orchestrator::run_failure_writeback_disposition(&error),
		RunFailureWritebackDisposition::TerminalAttention
	);
}
