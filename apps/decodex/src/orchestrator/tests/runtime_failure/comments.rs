use super::{
	AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure, AppServerPhaseGoalFailure,
	AppServerTransportFailure, AppServerTurnFailure, AppServerZeroEvidenceStartFailure,
	CodexAccountAuthFailure, Duration, FakeTracker, IssueDispatchMode, IssueRunPlan,
	LoopGuardrailReason, LoopGuardrailStopRequested, ManualAttentionRequested, PhaseGoalKind,
	RUN_LEASE_IDLE_TIMEOUT, RepoGateFailureKind, Report, RetainedPartialProgress, RetryComment,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition,
	RunFailureWritebackDisposition, StalledRunNeedsAttention, TEST_SERVICE_ID, WorktreeSpec,
	harness_outcome_payload_for_retryable_failure, orchestrator, sample_issue, temp_project_layout,
	tracker,
};

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
		(LoopGuardrailReason::ValidationRepeat, "validation_repeat", "repeated validation failure"),
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
		(LoopGuardrailReason::ReviewChurn, "review_churn", "repeated review findings"),
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
		("dependency_blocked", "dependency_program_stale", "Execution Program readiness"),
		("research_contract_required", "uncovered_direction", "research or decision contract"),
		("ownership_ambiguous", "ambiguous_retained_progress", "retained partial progress"),
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
		(ReviewPolicyStopReason::Blocked, "review_policy_blocked", "do not dispatch research"),
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
