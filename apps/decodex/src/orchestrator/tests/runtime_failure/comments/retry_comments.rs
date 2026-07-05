use crate::orchestrator::{
	RepoGateFailure,
	tests::runtime_failure::{
		self, AppServerCapabilityPreflightFailure, AppServerTurnFailure, Duration,
		RUN_LEASE_IDLE_TIMEOUT, RepoGateFailureKind, Report, RetryComment,
		StalledRunNeedsAttention, orchestrator,
	},
};

#[test]
fn retry_failure_comments_withhold_raw_error_text() {
	let comment = orchestrator::format_retry_comment(RetryComment {
		run_id: "pub-101-attempt-1-123",
		attempt_number: 1,
		retry_budget_attempt_number: 1,
		max_attempts: 3,
		worktree_path: String::from(".worktrees/PUB-101"),
		branch_name: "x/pubfi-pub-101",
		error_class: "retryable_execution_failure",
		next_action: "decodex will retry automatically",
	});

	assert!(comment.contains("- error_class: `retryable_execution_failure`"));
	assert!(comment.contains("Sensitive runtime details were withheld"));
	assert!(!comment.contains("error:"));
}

#[test]
fn retryable_failure_writeback_does_not_mark_non_validation_harness_outcome_failed() {
	let payload = runtime_failure::harness_outcome_payload_for_retryable_failure(Report::msg(
		"transient runtime failure",
	));

	assert_eq!(payload["validation"]["result"], "not_recorded");
	assert_eq!(payload["validation"]["failure_count"], 0);
	assert_eq!(payload["validation"]["failure_classes"], serde_json::json!([]));
}

#[test]
fn retryable_repo_gate_writeback_marks_harness_outcome_validation_failed() {
	let payload = runtime_failure::harness_outcome_payload_for_retryable_failure(Report::new(
		RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("verify command failed"),
		),
	));

	assert_eq!(payload["validation"]["result"], "failed");
	assert!(payload["validation"]["failure_count"].as_i64().is_some_and(|count| count >= 1));
	assert!(
		payload["validation"]["failure_classes"]
			.as_array()
			.expect("failure classes should be an array")
			.iter()
			.any(|class| class == "repo_gate_verify_failed")
	);
}

#[test]
fn repo_gate_retry_comments_preserve_continued_repair_error_class() {
	let comment = orchestrator::format_retry_comment(RetryComment {
		run_id: "pub-101-attempt-1-123",
		attempt_number: 1,
		retry_budget_attempt_number: 1,
		max_attempts: 3,
		worktree_path: String::from(".worktrees/PUB-101"),
		branch_name: "x/pubfi-pub-101",
		error_class: "repo_gate_verify_failed",
		next_action: "additional agent repair is required before repo verification can pass; decodex will retry automatically",
	});

	assert!(comment.contains("- error_class: `repo_gate_verify_failed`"));
	assert!(
		comment.contains("additional agent repair is required before repo verification can pass")
	);
}

#[test]
fn repo_gate_lock_contention_retry_comments_preserve_specific_error_class() {
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::GitLockContention,
		String::from(
			"Failed to inspect tracked-file cleanliness after repo gate verification in `/tmp/repo`: fatal: Unable to create '.git/index.lock': File exists.",
		),
	));
	let (error_class, next_action) = orchestrator::retry_comment_details(&error);

	assert_eq!(error_class, "repo_gate_git_lock_contention");
	assert!(next_action.contains("`.git/index.lock`"));
	assert!(next_action.contains("retry automatically"));
}

#[test]
fn stalled_run_retry_comments_preserve_specific_error_class() {
	let error = Report::new(StalledRunNeedsAttention {
		issue_identifier: String::from("PUB-101"),
		run_id: String::from("pub-101-attempt-1-123"),
		idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
	});
	let (error_class, next_action) = orchestrator::retry_comment_details(&error);

	assert_eq!(error_class, "stalled_run_detected");
	assert!(next_action.contains("retry the stalled lane automatically"));
	assert!(next_action.contains("retry budget exhausts"));
}

#[test]
fn app_server_preflight_timeout_retry_comments_preserve_specific_error_class() {
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));
	let (error_class, next_action) = orchestrator::retry_comment_details(&error);

	assert_eq!(error_class, "app_server_plugin_list_timeout");
	assert!(next_action.contains("retry app-server preflight automatically"));
	assert!(next_action.contains("`plugin/list` timeout"));
	assert!(next_action.contains("retry budget exhausts"));
}

#[test]
fn app_server_turn_retry_comments_preserve_specific_error_class() {
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"transient model failure",
		None,
	));
	let (error_class, next_action) = orchestrator::retry_comment_details(&error);

	assert_eq!(error_class, "app_server_turn_failed");
	assert_eq!(next_action, "decodex will retry automatically");
}
