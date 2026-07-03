use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		IssueDispatchMode, IssueRunPlan, LoopGuardrailReason, LoopGuardrailStopRequested,
		ManualAttentionRequested, Report, ReviewPolicyStopReason, ReviewPolicyStopRequested,
		RunCompletionDisposition, WorktreeSpec, orchestrator,
	},
};

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
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
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
