use std::collections::BTreeMap;

use orchestrator::{
	AgentGitCredentialEnvironment, AgentGitCredentialsUnavailable, RepoGateFailureKind,
};
use orchestrator::AppServerZeroEvidenceStartFailure;
use orchestrator::LoopGuardrailReason;
use orchestrator::LoopGuardrailStopRequested;
use orchestrator::RunFailureWritebackDisposition;
use orchestrator::StalledRunNeedsAttention;

use crate::agent::CodexAccountAuthFailure;

fn git_config_value(
	repo_root: &Path,
	key: &str,
	credentials: Option<&AgentGitCredentialEnvironment>,
) -> Option<String> {
	let mut probe = Command::new("git");

	probe.arg("-C").arg(repo_root).args(["config", "--get", key]);

	if let Some(credentials) = credentials {
		credentials.process_env().apply_to(&mut probe).expect("agent env should apply");
	}

	let output = probe.output().expect("git config probe should run");

	output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn injected_git_config_keys(credentials: &AgentGitCredentialEnvironment) -> Vec<String> {
	let mut probe = Command::new("git");

	credentials.process_env().apply_to(&mut probe).expect("agent env should apply");

	probe
		.get_envs()
		.filter_map(|(key, value)| {
			Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
		})
		.filter(|(key, _)| key.starts_with("GIT_CONFIG_KEY_"))
		.map(|(_, value)| value)
		.collect()
}

fn loop_guardrail_issue_run(
	config: &ServiceConfig,
	issue: &TrackerIssue,
	attempt_number: i64,
) -> IssueRunPlan {
	IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number,
		run_id: format!("pub-101-attempt-{attempt_number}-123"),
		retry_budget_base: 0,
	}
}

fn harness_outcome_payload_for_retryable_failure(error: Report) -> Value {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(Vec::new());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("retryable failure writeback should succeed");

	state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list")
		.into_iter()
		.find(|event| event.event_type() == "harness_outcome")
		.expect("harness outcome should record")
		.payload()
		.clone()
}

#[test]
fn terminal_failure_comments_surface_actionable_error_classes() {
	for (error_class, next_action, expected_snippets) in [
		(
			"human_attention_required",
			"inspect the issue comment and worktree, resolve the blocker manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["inspect the issue comment and worktree", "resolve the blocker manually"][..],
		),
		(
			"review_handoff_writeback_failed",
			"inspect the tracker state, PR, and worktree, repair the incomplete review handoff manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["repair the incomplete review handoff manually"][..],
		),
		(
			"stalled_run_detected",
			"inspect the worktree and app-server activity for the stalled lane, resolve the blocker manually, `decodex:needs-attention` could not be applied because it does not exist on the team; the issue remains in `In Progress` to block automatic retries, so move it back to a startable state manually if another automated run is desired",
			&["does not exist on the team", "remains in `In Progress`"][..],
		),
	] {
		let comment = orchestrator::format_terminal_failure_comment(
			"pub-101-attempt-1-123",
			1,
			String::from(".worktrees/PUB-101"),
			"x/pubfi-pub-101",
			None,
			error_class,
			next_action,
		);

		assert!(comment.contains(&format!("- error_class: `{error_class}`")));
		assert!(comment.contains("Sensitive runtime details were withheld"));

		for expected_snippet in expected_snippets {
			assert!(comment.contains(expected_snippet), "{error_class} missing {expected_snippet}");
		}
	}
}

#[test]
fn failure_writeback_disposition_marks_retryable_recovery_classes() {
	for (case_name, error, expected_disposition) in [
		(
			"startup transport",
			Report::new(AppServerTransportFailure::with_phase(
				String::from("App-server stdout disconnected before thread start."),
				"thread/start",
				true,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"generic turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"transient model failure",
				None,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"usage limit turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"You've hit your usage limit.",
				Some(String::from("usageLimitExceeded")),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"repo gate lock contention",
			Report::new(orchestrator::RepoGateFailure::new(
				RepoGateFailureKind::GitLockContention,
				String::from("fatal: Unable to create '.git/index.lock': File exists."),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"zero-evidence app-server start failure",
			Report::new(AppServerZeroEvidenceStartFailure::new(
				String::from("PUB-101"),
				String::from("pub-101-attempt-1-123"),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"app-server capability preflight timeout",
			Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
				"plugin/list",
				String::from("Timed out while waiting for app-server output."),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"phase goal terminal path missing",
			Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
				PhaseGoalKind::HandoffEvidence,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"dynamic tool protocol failure",
			Report::new(AppServerDynamicToolFailure::protocol_for_test(
				Some(String::from("issue_comment")),
				"dynamic tool declaration was missing input schema",
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"stalled current lane",
			Report::new(StalledRunNeedsAttention {
				issue_identifier: String::from("PUB-101"),
				run_id: String::from("pub-101-attempt-1-123"),
				idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
			}),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
	] {
		assert_eq!(
			orchestrator::run_failure_writeback_disposition(&error),
			expected_disposition,
			"{case_name}"
		);
	}
}

#[test]
fn failure_writeback_disposition_marks_terminal_attention_classes() {
	for (case_name, error, expected_disposition) in [
		(
			"turn transport",
			Report::new(AppServerTransportFailure::with_phase(
				String::from("App-server stdout disconnected during turn start."),
				"turn/start",
				false,
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"operator attention turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"operator attention required",
				Some(String::from("operatorAttentionRequired")),
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"app-server capability preflight blocker",
			Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
				"model",
				"configured model was not present in model/list.",
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"unsupported phase goal API",
			Report::new(AppServerPhaseGoalFailure::unsupported_for_test("thread/goal/set")),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"repo gate spawn failure",
			Report::new(orchestrator::RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				String::from("Failed to spawn repo gate command `cargo make test`: missing tool"),
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"codex account auth failure",
			Report::new(CodexAccountAuthFailure::new(
				Some(String::from("...123456")),
				Some(String::from("bad@example.com")),
				"Codex account `bad@example.com` token refresh failed with HTTP 401 Unauthorized.",
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
	] {
		assert_eq!(
			orchestrator::run_failure_writeback_disposition(&error),
			expected_disposition,
			"{case_name}"
		);
	}
}

#[test]
fn loop_guardrail_terminal_failure_details_normalize_stop_classes() {
	let recovery_gate = "clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired";

	for (reason, error_class, expected_snippet) in [
		(
			LoopGuardrailReason::ValidationRepeat,
			"validation_repeat",
			"repeated validation failure",
		),
		(
			LoopGuardrailReason::NoEffectiveDiff,
			"no_effective_diff",
			"do not continue automatic repair",
		),
		(
			LoopGuardrailReason::RemainingDeltaUnchanged,
			"remaining_delta_unchanged",
			"unchanged remaining delta",
		),
		(
			LoopGuardrailReason::ReviewChurn,
			"review_churn",
			"repeated review findings",
		),
		(
			LoopGuardrailReason::DependencyProgramStale,
			"dependency_program_stale",
			"Execution Program readiness",
		),
		(
			LoopGuardrailReason::UncoveredDirection,
			"uncovered_direction",
			"research or decision contract",
		),
		(
			LoopGuardrailReason::AmbiguousRetainedProgress,
			"ambiguous_retained_progress",
			"retained partial progress",
		),
	] {
		let error = Report::new(LoopGuardrailStopRequested {
			issue_identifier: String::from("PUB-101"),
			run_id: String::from("pub-101-attempt-3-123"),
			reason,
			consecutive_count: 3,
			fingerprint: String::from("fingerprint"),
			source_error_class: Some(String::from("repo_gate_verify_failed")),
			architecture_recovery_reason_code: None,
		});
		let (actual_error_class, next_action) =
			orchestrator::terminal_failure_comment_details(false, &error, recovery_gate);

		assert_eq!(actual_error_class, error_class);
		assert!(next_action.contains(expected_snippet), "{error_class} missing expected action");
		assert!(next_action.contains("clear label `decodex:needs-attention`"));
	}
}

#[test]
fn manual_attention_loop_error_classes_preserve_runtime_attribution() {
	let recovery_gate = "clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired";

	for (source_error_class, expected_error_class, expected_snippet) in [
		("review_policy_exhausted", "review_churn", "repeated review findings"),
		(
			"dependency_blocked",
			"dependency_program_stale",
			"Execution Program readiness",
		),
		(
			"research_contract_required",
			"uncovered_direction",
			"research or decision contract",
		),
		(
			"ownership_ambiguous",
			"ambiguous_retained_progress",
			"retained partial progress",
		),
	] {
		let error = Report::new(ManualAttentionRequested {
			issue_identifier: String::from("PUB-101"),
			label: String::from("decodex:needs-attention"),
			run_id: String::from("pub-101-attempt-1-123"),
			error_class: Some(String::from(source_error_class)),
		});
		let (actual_error_class, next_action) =
			orchestrator::terminal_failure_comment_details(true, &error, recovery_gate);

		assert_eq!(actual_error_class, expected_error_class);
		assert!(next_action.contains(expected_snippet), "{expected_error_class} action missing");
	}

	let generic = Report::new(ManualAttentionRequested {
		issue_identifier: String::from("PUB-101"),
		label: String::from("decodex:needs-attention"),
		run_id: String::from("pub-101-attempt-1-123"),
		error_class: Some(String::from("operator_requested_stop")),
	});
	let (actual_error_class, next_action) =
		orchestrator::terminal_failure_comment_details(true, &generic, recovery_gate);

	assert_eq!(actual_error_class, "human_attention_required");
	assert!(next_action.contains("inspect the issue comment and worktree"));
}

#[test]
fn terminal_failure_comments_surface_review_handoff_pr_url_when_available() {
	let pr_url = "https://github.com/hack-ink/decodex/pull/101";
	let comment = orchestrator::format_terminal_failure_comment(
		"pub-101-attempt-1-123",
		1,
		String::from(".worktrees/PUB-101"),
		"x/pubfi-pub-101",
		Some(pr_url),
		"review_handoff_writeback_failed",
		"repair the incomplete review handoff manually",
	);

	assert!(comment.contains(&format!("- pr_url: `{pr_url}`")));
	assert!(comment.contains("- error_class: `review_handoff_writeback_failed`"));
}

#[test]
fn review_policy_terminal_failure_details_include_research_boundaries() {
	for (reason, error_class, expected_snippet) in [
		(
			ReviewPolicyStopReason::Exhausted,
			"review_policy_exhausted",
			"bounded convergence research follow-up",
		),
		(
			ReviewPolicyStopReason::ArchitectureReviewRequired,
			"architecture_review_required",
			"bounded architecture research follow-up",
		),
		(
			ReviewPolicyStopReason::Blocked,
			"review_policy_blocked",
			"do not dispatch research",
		),
	] {
		let error = Report::new(ReviewPolicyStopRequested {
			head_sha: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			issue_identifier: String::from("PUB-101"),
			fingerprint: (reason == ReviewPolicyStopReason::Exhausted)
				.then(|| String::from("review_finding:test")),
			nonclean_rounds: Some(3),
			reason,
			run_id: String::from("pub-101-attempt-1-123"),
		});
		let (actual_error_class, next_action) = orchestrator::terminal_failure_comment_details(
			false,
			&error,
			"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		);

		assert_eq!(actual_error_class, error_class);
		assert!(next_action.contains(expected_snippet), "{error_class} missing research boundary");
		assert!(next_action.contains("clear label `decodex:needs-attention`"));
	}
}

#[test]
fn preserve_manual_attention_request_wraps_finalize_miss() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = orchestrator::preserve_manual_attention_request(
		Ok(RunCompletionDisposition::ManualAttention),
		&issue_run,
		&workflow,
		Report::msg("run completed without issue_terminal_finalize"),
	);

	assert!(error.downcast_ref::<orchestrator::ManualAttentionRequested>().is_some());
	assert!(error.to_string().contains("run completed without issue_terminal_finalize"));
}

#[test]
fn retained_partial_progress_uses_actionable_terminal_failure_comment() {
	let error = Report::new(RetainedPartialProgress {
		issue_identifier: String::from("PUB-101"),
		run_id: String::from("pub-101-attempt-3-123"),
		worktree_path: String::from(".worktrees/PUB-101"),
		source_error_class: None,
	});
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "partial_progress_retained");
	assert!(next_action.contains("inspect retained worktree `.worktrees/PUB-101`"));
	assert!(next_action.contains("finish validation and PR handoff or reset the patch manually"));
	assert!(next_action.contains("clear label `decodex:needs-attention`"));

	let comment = orchestrator::format_terminal_failure_comment(
		"pub-101-attempt-3-123",
		3,
		String::from(".worktrees/PUB-101"),
		"x/pubfi-pub-101",
		None,
		error_class,
		&next_action,
	);

	assert!(comment.contains("decodex retained partial progress and needs attention"));
	assert!(comment.contains("- recorded_at: `"));
	assert!(!comment.contains("decodex run failed and needs attention"));
	assert!(!comment.contains("- failed_at: `"));
	assert!(comment.contains("full recovery context"));
}

#[test]
fn ensure_automation_activity_label_noops_when_active_ownership_is_confirmed() {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue = sample_issue("In Progress", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&active_label, vec![issue.clone()]);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true).expect(
		"server-confirmed active ownership should not fail when the first label page is truncated",
	);

	assert!(
		tracker.label_updates.borrow().is_empty()
			&& tracker.label_additions.borrow().is_empty()
			&& tracker.label_removals.borrow().is_empty(),
		"server-confirmed active ownership should not trigger a label mutation"
	);

	let mut issue = sample_issue("In Progress", &[active_label.as_str()]);

	issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()]);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true)
		.expect("existing active ownership should not require a paginated team-label lookup");

	assert!(
		tracker.label_updates.borrow().is_empty()
			&& tracker.label_additions.borrow().is_empty()
			&& tracker.label_removals.borrow().is_empty(),
		"no-op active-label checks should not trigger a label mutation"
	);
}

#[test]
fn ensure_automation_activity_label_uses_incremental_team_label_lookup_for_mutation() {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue = sample_issue("In Progress", &[]);

	issue.labels_complete = false;

	issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()]).with_team_label_lookup_id(
		&issue.team.id,
		&active_label,
		"label-active",
	);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true)
		.expect("active-label mutation should resolve the team label id server-side");

	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-active")])],
	);
	assert!(tracker.label_updates.borrow().is_empty());
}

#[test]
fn review_policy_terminal_failure_comments_use_runtime_owned_error_classes() {
	for (error_class, next_action) in [
		(
			"review_policy_exhausted",
			"inspect the repeated review findings and current worktree, decide the next repair or redesign manually, prepare a bounded convergence research follow-up only after the current head, review phase, non-clean round count, and validated findings are structured and machine-checkable, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
		(
			"architecture_review_required",
			"inspect the current findings and worktree, perform the required architecture review manually, prepare a bounded architecture research follow-up only after the current head, review phase, stop class, and architecture concern are structured and machine-checkable, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
		(
			"review_policy_blocked",
			"inspect the blocking condition and worktree, resolve the blocker manually, do not dispatch research unless the blocker is reclassified as a structured architecture or convergence stop, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
	] {
		let comment = orchestrator::format_terminal_failure_comment(
			"pub-101-attempt-1-123",
			1,
			String::from(".worktrees/PUB-101"),
			"x/pubfi-pub-101",
			None,
			error_class,
			next_action,
		);

		assert!(comment.contains(&format!("- error_class: `{error_class}`")));
		assert!(comment.contains("Sensitive runtime details were withheld"));
	}
}

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
	let payload =
		harness_outcome_payload_for_retryable_failure(Report::msg("transient runtime failure"));

	assert_eq!(payload["validation"]["result"], "not_recorded");
	assert_eq!(payload["validation"]["failure_count"], 0);
	assert_eq!(payload["validation"]["failure_classes"], serde_json::json!([]));
}

#[test]
fn retryable_repo_gate_writeback_marks_harness_outcome_validation_failed() {
	let payload = harness_outcome_payload_for_retryable_failure(Report::new(
		orchestrator::RepoGateFailure::new(
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
	let error = Report::new(orchestrator::RepoGateFailure::new(
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
fn repo_gate_lock_contention_runtime_retry_writes_specific_retry_schedule_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path.clone(),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::GitLockContention,
		String::from(
			"Failed to inspect tracked-file cleanliness after repo gate verification in `/tmp/repo`: fatal: Unable to create '.git/index.lock': File exists.",
		),
	));

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	orchestrator::write_retry_schedule_marker_for_runtime_retry(&error, &workflow, &issue_run, 1)
		.expect("lock contention should write a specific retry marker");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("git_lock_contention"));
	assert!(
		marker.retry_ready_at_unix_epoch().is_some_and(
			|retry_ready_at| retry_ready_at > OffsetDateTime::now_utc().unix_timestamp()
		)
	);
}

#[test]
fn app_server_preflight_timeout_runtime_retry_writes_failure_retry_schedule_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path.clone(),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	orchestrator::write_retry_schedule_marker_for_runtime_retry(&error, &workflow, &issue_run, 1)
		.expect("preflight timeout should write a failure retry marker");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert!(
		marker.retry_ready_at_unix_epoch().is_some_and(
			|retry_ready_at| retry_ready_at > OffsetDateTime::now_utc().unix_timestamp()
		)
	);
}

#[test]
fn retry_budget_current_failure_does_not_double_count_handed_off_base() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-456"),
		retry_budget_base: 1,
	};

	state_store
		.record_run_attempt("pub-101-attempt-1-123", &issue.id, 1, "failed")
		.expect("previous failed attempt should record");
	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("current failed attempt should record");

	assert_eq!(
		orchestrator::retry_budget_attempts_for_current_failure(&state_store, &issue_run)
			.expect("retry budget should compute"),
		2,
		"the daemon handoff base already includes the previous persisted failed attempt"
	);
}

#[test]
fn repo_gate_terminal_failures_preserve_specific_error_class_after_retry_exhaustion() {
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command `cargo make test` failed in `/tmp/repo`: test failed"),
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "repo_gate_verify_failed");
	assert!(next_action.contains("repair the repo verification failure manually"));
}

#[test]
fn repo_gate_lock_contention_terminal_failures_preserve_specific_error_class_after_retry_exhaustion()
 {
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::GitLockContention,
		String::from(
			"Failed to inspect tracked-file cleanliness after repo gate verification in `/tmp/repo`: fatal: Unable to create '.git/index.lock': File exists.",
		),
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "repo_gate_git_lock_contention");
	assert!(next_action.contains("active or stale `.git/index.lock` holder"));
}

#[test]
fn loop_guardrail_stops_repeated_validation_failures_after_three_observations() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let error = || {
		Report::new(orchestrator::RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error(),
			)
			.expect("guardrail observation should persist")
			.is_none(),
			"guardrail should allow repair attempt {attempt_number}"
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error(),
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "loop_guardrail_checkpoint");
	assert_eq!(events[0].payload()["reason"], "validation_repeat");
}

#[test]
fn loop_guardrail_starts_architecture_recovery_when_boundary_is_within_authority() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let error = || {
		Report::new(orchestrator::RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error(),
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = error();
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let recovery = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(recovery) => recovery,
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(_) => {
			panic!("repo-gate validation repeat should recover autonomously")
		},
	};

	assert_eq!(recovery.attempt_number, 1);
	assert!(recovery.detail.contains("materially different implementation strategy"));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
			.expect("checkpoint read should succeed")
			.is_none(),
		"started recovery should clear the stopped guardrail reason"
	);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["disposition"] == "within_authority"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "architecture_recovery_started"
			&& event.payload()["authority_boundary_check"]["disposition"] == "within_authority"
			&& event.payload()["retained_worktree"]["tracked_status"].is_string()
			&& event.payload()["validation_failures"]["source_error_class"]
				== "repo_gate_verify_failed"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_started"
			&& event.payload()["next_strategy"] == "materially_different_architecture_recovery"
	}));
}

#[test]
fn loop_guardrail_requires_human_when_boundary_evidence_is_missing() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("no effective diff should stop");
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let terminal_stop = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(_) => {
			panic!("missing authority evidence must not start recovery")
		},
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(stop) => stop,
	};

	assert_eq!(terminal_stop.terminal_error_class(), "contract_boundary_required");

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["disposition"] == "insufficient_evidence"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "contract_boundary_required"
			&& event.payload()["authority_boundary_check"]["disposition"] == "insufficient_evidence"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_terminal"
			&& event.payload()["reason_code"] == "contract_boundary_required"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "authority_decision_request"
			&& event.payload()["reason"] == "contract_boundary_required"
	}));
}

#[test]
fn loop_guardrail_stops_unchanged_remaining_delta_when_validation_text_changes() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::new(orchestrator::RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			format!("Repo verify command failed with assertion variant {attempt_number}"),
		));

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command failed with assertion variant 3"),
	));
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third unchanged delta should evaluate")
	.expect("unchanged remaining delta should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::RemainingDeltaUnchanged);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let validation_checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(
		validation_checkpoint.consecutive_count(),
		1,
		"changing validation text should keep validation_repeat below threshold"
	);

	let delta_checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "remaining_delta_unchanged")
		.expect("remaining-delta checkpoint should read")
		.expect("remaining-delta checkpoint should exist");

	assert_eq!(delta_checkpoint.consecutive_count(), 3);
}

#[test]
fn loop_guardrail_stops_no_effective_diff_for_retryable_errors_without_delta() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("no effective diff should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::NoEffectiveDiff);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class, None);

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
		.expect("no-diff checkpoint should read")
		.expect("no-diff checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);
}

#[test]
fn handle_failure_recovers_review_handoff_state_drift_before_no_effective_diff_terminalization() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let issue = sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/957";

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		pr_url,
		"main",
		&issue_run.worktree.branch_name,
		&head_oid,
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_handoff_marker(config.service_id(), &issue.id, &handoff)
		.expect("review handoff marker should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("review handoff drift should recover before no-diff terminalization");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-review")))
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().all(|comment| {
		!comment.contains("decodex run failed and needs attention")
			&& !comment.contains("no_effective_diff")
	}));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"handoff recovery owns the lifecycle and clears stale retry/no-diff guardrails"
	);

	let run_attempt = state_store
		.run_attempt(&issue_run.run_id)
		.expect("run attempt should read")
		.expect("run attempt should remain present");

	assert_eq!(run_attempt.status(), "succeeded");

	let orchestration = state_store
		.review_orchestration_marker(config.service_id(), &issue.id, &handoff)
		.expect("review orchestration should read")
		.expect("review orchestration should be rebound");

	assert_eq!(orchestration.phase(), "request_pending");
	assert_eq!(orchestration.head_sha(), head_oid);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_recovered"
			&& event.payload()["reason"] == "current_review_handoff_marker"
			&& event.payload()["target_issue_state"] == "In Review"
	}));
}

#[test]
fn handle_failure_requires_rebind_when_clean_handoff_checkpoint_has_no_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = git_output(config.repo_root(), &["rev-parse", "HEAD"]);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			phase: "handoff",
			status: "clean",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("clean handoff checkpoint should persist");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("missing handoff marker should require explicit rebind attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert_eq!(
		tracker.label_additions.borrow().last(),
		Some(&(issue.id.clone(), vec![String::from("label-needs-attention")]))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_handoff_state_drift")
			&& comment.contains("restore or rebind the post-review lifecycle")
	}));
	assert!(tracker.comments.borrow().iter().all(|comment| {
		!comment.contains("no_effective_diff")
	}));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"missing handoff marker must not be reclassified as no effective diff"
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_detected"
			&& event.payload()["reason"] == "missing_review_handoff_marker"
			&& event.payload()["checkpoint_status"] == "clean"
	}));
}

#[test]
fn handle_failure_requires_rebind_when_handoff_marker_head_ref_mismatches_without_checkpoint() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		"https://github.com/hack-ink/decodex/pull/957",
		"main",
		"x/pubfi-pub-101-stale",
		&head_oid,
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_handoff_marker(config.service_id(), &issue.id, &handoff)
		.expect("review handoff marker should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("mismatched handoff marker should require explicit rebind attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert_eq!(
		tracker.label_additions.borrow().last(),
		Some(&(issue.id.clone(), vec![String::from("label-needs-attention")]))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_handoff_state_drift")
			&& comment.contains("restore or rebind the post-review lifecycle")
	}));
	assert!(tracker.comments.borrow().iter().all(|comment| {
		!comment.contains("no_effective_diff")
	}));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"untrusted handoff marker must not fall through to no effective diff"
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_detected"
			&& event.payload()["reason"] == "review_handoff_marker_pr_head_ref_mismatch"
			&& event.payload()["checkpoint_status"].is_null()
	}));
}

#[test]
fn handle_failure_requires_rebind_when_handoff_marker_issue_state_is_unsupported() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Backlog", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		"https://github.com/hack-ink/decodex/pull/957",
		"main",
		&issue_run.worktree.branch_name,
		&head_oid,
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_handoff_marker(config.service_id(), &issue.id, &handoff)
		.expect("review handoff marker should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("unsupported issue state should require explicit handoff recovery");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_handoff_state_drift")
			&& comment.contains("restore or rebind the post-review lifecycle")
	}));
	assert!(tracker.comments.borrow().iter().all(|comment| {
		!comment.contains("no_effective_diff")
	}));

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_detected"
			&& event.payload()["reason"] == "review_handoff_marker_issue_state_unsupported"
	}));
}

#[test]
fn loop_guardrail_does_not_classify_dirty_retained_diff_as_no_effective_diff() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	fs::write(config.repo_root().join("README.md"), "retained validation-ready patch\n")
		.expect("tracked file should become dirty");

	for attempt_number in 1..=3 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("phase completed local validation without terminal handoff");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(
			stop.is_none(),
			"dirty retained progress should not be reported as no_effective_diff"
		);
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"no_effective_diff is reserved for retryable failures with no effective delta"
	);
}

#[test]
fn loop_guardrail_does_not_classify_untracked_retained_files_as_no_effective_diff() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	fs::write(config.repo_root().join("new-runbook.md"), "retained validation-ready file\n")
		.expect("untracked source file should write");

	for attempt_number in 1..=3 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("phase completed local validation without terminal handoff");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(
			stop.is_none(),
			"untracked retained source files should not be reported as no_effective_diff"
		);
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"no_effective_diff is reserved for retryable failures with no effective delta"
	);
}

#[test]
fn loop_guardrail_ignores_untracked_decodex_runtime_artifacts_for_no_effective_diff() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let control_dir = config.repo_root().join(".decodex-run-control");

	fs::write(config.repo_root().join(RUN_ACTIVITY_MARKER_FILE), "heartbeat\n")
		.expect("runtime activity marker should write");
	fs::create_dir_all(&control_dir).expect("runtime control directory should exist");
	fs::write(control_dir.join("command.json"), "{}\n")
		.expect("runtime control file should write");

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("runtime-only artifacts should still count as no effective diff");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::NoEffectiveDiff);
	assert_eq!(stop.consecutive_count, 3);
}

#[test]
fn app_server_terminal_failures_preserve_specific_error_classes() {
	let cases = [
		(
			Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
				"model",
				"configured model was not present in model/list.",
			)),
			"app_server_runtime_preflight_failed",
			"repair the local Codex runtime configuration",
		),
		(
			Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
				"plugin/list",
				String::from("Timed out while waiting for app-server output."),
			)),
			"app_server_plugin_list_timeout",
			"app_server_preflight_failed evidence for the `plugin/list` timeout",
		),
		(
			Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
				"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
			))),
			"app_server_codex_home_preflight_failed",
			"inspect the local Decodex and Codex home sharing",
		),
		(
			Report::new(AppServerHomePreflightFailure::initialize_mismatch(
				String::from("/tmp/per-account-codex-home"),
				String::from("/Users/test/.codex"),
			)),
			"app_server_codex_home_mismatch",
			"restart `decodex serve`",
		),
		(
			Report::new(AppServerTransportFailure::new(String::from(
				"App-server stdout disconnected unexpectedly.",
			))),
			"app_server_transport_disconnected",
			"inspect the local app-server stderr tail",
		),
		(
			Report::new(AppServerZeroEvidenceStartFailure::new(
				String::from("PUB-101"),
				String::from("pub-101-attempt-1-123"),
			)),
			"app_server_zero_evidence_start_failed",
			"verify `decodex probe stdio://`",
		),
		(
			Report::new(CodexAccountAuthFailure::new(
				Some(String::from("...123456")),
				Some(String::from("bad@example.com")),
				"Codex account `bad@example.com` token refresh failed with HTTP 401 Unauthorized.",
			)),
			"codex_account_auth_failed",
			"re-login or remove Decodex Codex account",
		),
		(
			Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
				PhaseGoalKind::HandoffEvidence,
			)),
			"phase_goal_terminal_path_missing",
			"finish validation/review/handoff",
		),
		(
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"You've hit your usage limit.",
				Some(String::from("usageLimitExceeded")),
			)),
			"app_server_usage_limit_exceeded",
			"inspect Codex account usage",
		),
	];

	for (error, expected_class, expected_action) in cases {
		let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
			false,
			&error,
			"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		);

		assert_eq!(error_class, expected_class);
		assert!(next_action.contains(expected_action));
		assert!(next_action.contains("clear label `decodex:needs-attention`"));
	}
}

#[test]
fn app_server_preflight_terminal_action_surfaces_first_scan_error() {
	let mut details = BTreeMap::new();

	details.insert(
		String::from("first_error_path"),
		String::from("/tmp/plugins/build-web-data-visualization/skills/chart/SKILL.md"),
	);
	details.insert(
		String::from("first_error"),
		String::from("name: exceeds maximum length of 64 characters"),
	);

	let error = Report::new(AppServerCapabilityPreflightFailure::blocked_for_test_with_details(
		"skills",
		"skills/list returned no enabled skills.",
		details,
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "app_server_runtime_preflight_failed");
	assert!(next_action.contains("first_error_path=/tmp/plugins/build-web-data-visualization"));
	assert!(next_action.contains("first_error=name: exceeds maximum length of 64 characters"));
}

#[test]
fn zero_evidence_app_server_start_failure_is_promoted_records_private_evidence_and_retries() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let _env_guard = TestEnvVarGuard::set(
		"DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN",
		"synthetic-secret-token",
	);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	let error = orchestrator::promote_zero_evidence_app_server_start_failure(
		&config,
		&state_store,
		&issue_run,
		Report::msg("synthetic startup failure: synthetic-secret-token"),
	);

	assert!(
		error.downcast_ref::<orchestrator::AppServerZeroEvidenceStartFailure>().is_some(),
		"generic no-evidence startup errors should become typed app-server start failures"
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "app_server_zero_evidence_start_failure");
	assert_eq!(
		events[0].payload()["error_class"],
		"app_server_zero_evidence_start_failed"
	);
	assert_eq!(events[0].payload()["protocol_event_count"], 0);
	assert_eq!(events[0].payload()["thread_recorded"], false);
	assert_eq!(
		events[0].payload()["source_error_summary"],
		"synthetic startup failure: <redacted env:DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN>"
	);
	assert_eq!(
		events[0].payload()["source_error_chain"][0],
		"synthetic startup failure: <redacted env:DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN>"
	);
	assert!(
		!events[0].payload().to_string().contains("synthetic-secret-token"),
		"private diagnostic payload must redact known secret env values"
	);

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("retryable zero-evidence failure handling should succeed");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_zero_evidence_start_failed")
			&& comment.contains("restart the app-server and retry automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"zero-evidence startup failure should not request operator attention before retry budget exhaustion"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("retryable_execution_failure")),
		"zero-evidence startup failure must preserve its typed retry class"
	);
}

#[test]
fn exhausted_zero_evidence_start_retry_budget_requires_attention_with_typed_class() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let error = Report::new(AppServerZeroEvidenceStartFailure::new(
		issue.identifier.clone(),
		issue_run.run_id.clone(),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted zero-evidence failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_zero_evidence_start_failed")
			&& comment.contains("verify `decodex probe stdio://`")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry")),
		"exhausted zero-evidence failure should not keep retrying"
	);
}

#[test]
fn retryable_startup_transport_failure_does_not_promote_to_zero_evidence_attention() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	let error = orchestrator::promote_zero_evidence_app_server_start_failure(
		&config,
		&state_store,
		&issue_run,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
	);

	assert!(
		error.downcast_ref::<AppServerTransportFailure>().is_some(),
		"startup transport failures should stay retryable instead of becoming zero-evidence terminal attention"
	);
	assert!(
		error
			.downcast_ref::<orchestrator::AppServerZeroEvidenceStartFailure>()
			.is_none()
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list");

	assert!(events.is_empty());
}

#[test]
fn retryable_turn_failure_does_not_promote_to_zero_evidence_attention() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	let error = orchestrator::promote_zero_evidence_app_server_start_failure(
		&config,
		&state_store,
		&issue_run,
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
		)),
	);

	assert!(
		error.downcast_ref::<AppServerTurnFailure>().is_some(),
		"structured turn failures should stay retryable instead of becoming zero-evidence terminal attention"
	);
	assert!(
		error
			.downcast_ref::<orchestrator::AppServerZeroEvidenceStartFailure>()
			.is_none()
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list");

	assert!(events.is_empty());
}

#[test]
fn repo_gate_runtime_failures_require_manual_attention_without_retry_budget_wait() {
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::CommandSpawnFailed,
		String::from(
			"Failed to spawn repo gate command `cargo make fmt` in `/tmp/repo` via `/bin/sh` `-c`: missing tool",
		),
	));
	let repo_gate_failure = error
		.downcast_ref::<orchestrator::RepoGateFailure>()
		.expect("repo gate failure should downcast");

	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
}

#[test]
fn operation_marker_write_failures_do_not_abort_completion_flow() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let occupied_path = temp_dir.path().join("occupied");

	fs::write(&occupied_path, "not a directory").expect("blocking file should write");
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_REPO_GATE,
	);
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	);

	assert!(occupied_path.is_file());
	assert!(!occupied_path.join(RUN_ACTIVITY_MARKER_FILE).exists());
}

#[test]
fn validate_review_handoff_runtime_requires_gh_and_github_token_authority() {
	let (_temp_dir, config, _workflow) = temp_project_layout();

	{
		let _env_lock = TestEnvVarGuard::lock();
		let missing_env_var = format!("DECODEX_TEST_MISSING_GITHUB_TOKEN_ENV_{}", process::id());
		let config_missing_github =
			service_config_with_github_token_env_var(&config, &missing_env_var);

		assert!(orchestrator::validate_review_handoff_runtime(&config, true).is_ok());
		assert!(orchestrator::validate_review_handoff_runtime(&config, false).is_ok());
		assert!(orchestrator::validate_daemon_runtime().is_ok());
		assert!(orchestrator::validate_command_available("git", None, "test preflight").is_ok());

		let error = orchestrator::validate_review_handoff_runtime(&config_missing_github, false)
			.expect_err("missing github token env-var should fail live preflight");

		assert!(error.to_string().contains("github.token_env_var"));
	}

	let env_var = format!("DECODEX_TEST_BLANK_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "");
	let config_blank_github = service_config_with_github_token_env_var(&config, &env_var);
	let error = orchestrator::validate_review_handoff_runtime(&config_blank_github, false)
		.expect_err("blank github token authority should fail live preflight");

	assert!(error.to_string().contains("must not be blank"));

	let error = orchestrator::validate_command_available(
		"__decodex_missing_command__",
		None,
		"PR-backed review handoff",
	)
	.expect_err("missing command should fail preflight");

	assert!(
		error.to_string().contains("Required command `__decodex_missing_command__` is unavailable")
	);
}

#[test]
fn agent_git_credentials_use_runtime_env_without_persisting_the_token() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = service_config_with_github_token_env_var(&config, &env_var);
	let askpass_path =
		orchestrator::agent_git_askpass_path(config.worktree_root(), "run/with spaces");
	let credentials =
		orchestrator::prepare_agent_git_credentials(&config, "run/with spaces", config.repo_root())
			.expect("agent Git credentials should prepare");
	let script = fs::read_to_string(&askpass_path).expect("askpass script should exist");

	assert!(askpass_path.exists());
	assert!(script.contains("GH_TOKEN"));
	assert!(!script.contains("secret-token-value"));

	let inherited_signing_key = git_config_value(config.repo_root(), "user.signingkey", None);
	let agent_signing_key =
		git_config_value(config.repo_root(), "user.signingkey", Some(&credentials));

	assert_eq!(
		agent_signing_key, inherited_signing_key,
		"agent git environment should preserve inherited signing keys when the repo has no local key"
	);
	assert_eq!(
		git_config_value(config.repo_root(), "commit.gpgsign", Some(&credentials)).as_deref(),
		Some("false")
	);

	let inherited_git_config_keys = injected_git_config_keys(&credentials);

	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "commit.gpgsign"),
		"agent git environment should not disable inherited commit signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "tag.gpgsign"),
		"agent git environment should not disable inherited tag signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "user.signingkey"),
		"agent git environment should not mask inherited signing keys"
	);

	#[cfg(unix)]
	{
		assert_eq!(
			std::os::unix::fs::PermissionsExt::mode(
				&fs::metadata(&askpass_path).expect("askpass metadata should load").permissions(),
			) & 0o777,
			0o700
		);

		let github_username = Command::new(&askpass_path)
			.arg("Username for 'https://github.com/hack-ink/decodex.git'")
			.env("GH_TOKEN", "secret-token-value")
			.output()
			.expect("askpass helper should execute");

		assert!(github_username.status.success());
		assert_eq!(String::from_utf8_lossy(&github_username.stdout).trim(), "x-access-token");

		let github_password = Command::new(&askpass_path)
			.arg("Password for 'https://x-access-token@github.com/hack-ink/decodex.git'")
			.env("GH_TOKEN", "secret-token-value")
			.output()
			.expect("askpass helper should execute");

		assert!(github_password.status.success());
		assert_eq!(String::from_utf8_lossy(&github_password.stdout).trim(), "secret-token-value");

		let foreign_password = Command::new(&askpass_path)
			.arg("Password for 'https://example.com/repo.git'")
			.env("GH_TOKEN", "secret-token-value")
			.output()
			.expect("askpass helper should execute");

		assert!(
			!foreign_password.status.success(),
			"askpass helper should reject non-GitHub prompts"
		);
		assert!(
			!String::from_utf8_lossy(&foreign_password.stdout).contains("secret-token-value"),
			"askpass helper should not leak the GitHub token to non-GitHub prompts"
		);

		let lookalike_password = Command::new(&askpass_path)
			.arg("Password for 'https://x-access-token@github.com.evil/repo.git'")
			.env("GH_TOKEN", "secret-token-value")
			.output()
			.expect("askpass helper should execute");

		assert!(
			!lookalike_password.status.success(),
			"askpass helper should reject GitHub lookalike hosts"
		);
		assert!(
			!String::from_utf8_lossy(&lookalike_password.stdout).contains("secret-token-value"),
			"askpass helper should not leak the GitHub token to lookalike hosts"
		);
	}

	drop(credentials);

	assert!(
		!askpass_path.exists(),
		"runtime askpass helper should be removed after the run environment drops"
	);
}

#[test]
fn agent_git_credentials_pin_repo_local_signing_key_when_configured() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_SIGNING_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = service_config_with_github_token_env_var(&config, &env_var);

	git_status_success(config.repo_root(), &["config", "user.signingkey", "route-y-signing-key"]);

	let credentials = orchestrator::prepare_agent_git_credentials(
		&config,
		"run-with-signing",
		config.repo_root(),
	)
	.expect("agent Git credentials should prepare");
	let mut signing_key_probe = Command::new("git");

	signing_key_probe.arg("-C").arg(config.repo_root()).args([
		"config",
		"--get",
		"user.signingkey",
	]);
	credentials.process_env().apply_to(&mut signing_key_probe).expect("agent env should apply");

	let output = signing_key_probe.output().expect("git signing key probe should run");

	assert!(output.status.success());
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "route-y-signing-key");
}

#[test]
fn missing_agent_git_credentials_stop_without_retry() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let config = service_config_with_github_token_env_var(&config, &missing_env_var);
	let error = match orchestrator::prepare_agent_git_credentials(
		&config,
		"run-missing-token",
		config.repo_root(),
	) {
		Ok(_) => panic!("missing github token should fail before app-server launch"),
		Err(error) => error,
	};
	let credentials_error = error
		.downcast_ref::<AgentGitCredentialsUnavailable>()
		.expect("credential preflight failure should be typed");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(credentials_error.token_env_var, missing_env_var);
	assert_eq!(error_class, "github_credentials_unavailable");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(!next_action.contains(&missing_env_var));
}

#[test]
fn live_run_without_candidate_does_not_require_github_token_authority() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], true);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("empty backlog should not require github token authority");

	assert!(summary.is_none());
}

#[test]
fn prepare_issue_run_with_candidate_does_not_require_github_token_authority_before_agent_execution()
{
	let (_temp_dir, config, workflow) = temp_project_layout();
	let listed_issue = sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		listed_issue.clone(),
	)
	.expect("candidate dispatch should prepare without github token authority")
	.expect("candidate issue should plan a run");

	assert_eq!(issue_run.issue.id, listed_issue.id);
	assert_eq!(issue_run.issue_state, "In Progress");
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_some()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert_eq!(
		state_store
			.latest_run_attempt_for_issue(&listed_issue.id)
			.expect("run attempt lookup should work")
			.expect("starting attempt should record")
			.status(),
		"starting"
	);
}

#[test]
fn execute_issue_run_clears_lease_when_active_label_setup_fails() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let mut listed_issue = sample_issue("Todo", &[]);
	let mut refreshed_issue = listed_issue.clone();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let worktree_path = config.worktree_root().join(&listed_issue.identifier);

	listed_issue.team.labels.retain(|label| label.name != active_label);
	refreshed_issue.team.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![refreshed_issue]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: listed_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: listed_issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: listed_issue.identifier.clone(),
			path: worktree_path.clone(),
			reused_existing: false,
		},
		retry_project_slug: listed_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(
			&issue_run.run_id,
			&listed_issue.id,
			issue_run.attempt_number,
			"starting",
		)
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &listed_issue.id, &issue_run.run_id, "In Progress")
		.expect("lease should record");

	let error = orchestrator::execute_issue_run(
		&tracker,
		&config,
		&workflow,
		&state_store,
		issue_run.clone(),
	)
	.expect_err("active-label setup failure should abort execution");

	assert!(error.to_string().contains("required label"));
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none(),
		"active-label setup failures should still release the lease"
	);
	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"failed",
		"active-label setup failures should mark the run failed before returning"
	);
}

#[test]
fn reconciliation_clears_stale_leases_and_terminal_worktrees() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker =
		FakeTracker::new(vec![issue.clone()]).with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should succeed");

	assert!(summary.is_none());
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert!(
		state_store.worktree_for_issue(&issue.id).expect("worktree lookup should work").is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn reconciliation_runs_without_project_validation() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = sample_issue("Done", &[active_label.as_str()]);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let tracker = FakeTracker::with_refresh_snapshots_and_project(
		vec![issue.clone()],
		vec![vec![issue.clone()]],
		false,
	)
	.with_label_lookup_issues(&queue_label, vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should still succeed without any project validation");

	assert!(summary.is_none(), "reconciliation-only startup should not dispatch a new lane here");
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should work").is_none());
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"terminated"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn exited_child_cleanup_updates_status_and_retry_budget_by_interrupt_flag() {
	for (case_name, mark_interrupted, expected_status, expected_retry_budget) in [
		("clean exit", false, "running", 0),
		("interrupted exit", true, "interrupted", 1),
	] {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("In Progress", &[]);

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef {
				issue_id: &issue.id,
				run_id: "run-1",
				attempt_number: 1,
			},
			mark_interrupted,
		)
		.expect(case_name);

		assert!(
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			expected_status,
			"{case_name}"
		);
		assert_eq!(
			state_store
				.retry_budget_attempt_count(&issue.id)
				.expect("retry budget count should succeed"),
			expected_retry_budget,
			"{case_name}"
		);
	}
}

#[test]
fn exited_child_cleanup_handles_worktree_mapping_ownership() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("Done", &[]);
		let removed_worktree_path = temp_dir.path().join("removed-lane");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store.update_run_status("run-1", "succeeded").expect("run status should update");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&removed_worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef {
				issue_id: &issue.id,
				run_id: "run-1",
				attempt_number: 1,
			},
			false,
		)
		.expect("removed worktree cleanup should succeed");

		assert!(
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
		);
		assert!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			"succeeded"
		);
	}
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("In Review", &[]);
		let existing_worktree_path = temp_dir.path().join("retained-lane");

		fs::create_dir_all(&existing_worktree_path).expect("worktree path should exist");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&existing_worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef {
				issue_id: &issue.id,
				run_id: "run-1",
				attempt_number: 1,
			},
			false,
		)
		.expect("existing worktree cleanup should succeed");

		assert_eq!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.expect("worktree mapping should remain")
				.worktree_path(),
			existing_worktree_path.as_path()
		);
	}
}

#[test]
fn exited_child_cleanup_requires_exact_run_id() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		true,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.run_attempt("other-run")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"running"
	);
}

#[test]
fn exited_child_cleanup_keeps_other_run_lease_and_worktree_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let removed_worktree_path = temp_dir.path().join("removed-lane");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.record_run_attempt("other-run", &issue.id, 2, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&removed_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
		false,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.expect("worktree mapping should remain")
			.worktree_path(),
		removed_worktree_path.as_path()
	);
}
