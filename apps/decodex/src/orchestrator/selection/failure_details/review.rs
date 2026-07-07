use crate::orchestrator::ReviewPolicyStopReason;

pub(crate) fn review_policy_stop_terminal_next_action(
	reason: ReviewPolicyStopReason,
	recovery_gate: &str,
) -> String {
	match reason {
		ReviewPolicyStopReason::Exhausted => format!(
			"inspect the repeated review findings and current worktree, decide the next repair or redesign manually, prepare a bounded convergence research follow-up only after the current head, review phase, non-clean round count, and validated findings are structured and machine-checkable, {recovery_gate}"
		),
		ReviewPolicyStopReason::ArchitectureReviewRequired => format!(
			"inspect the current findings and worktree, perform the required architecture review manually, prepare a bounded architecture research follow-up only after the current head, review phase, stop class, and architecture concern are structured and machine-checkable, {recovery_gate}"
		),
		ReviewPolicyStopReason::Blocked => format!(
			"inspect the blocking condition and worktree, resolve the blocker manually, do not dispatch research unless the blocker is reclassified as a structured architecture or convergence stop, {recovery_gate}"
		),
	}
}

pub(crate) fn retained_review_needs_attention_error_class(reason: &str) -> &'static str {
	match reason {
		"external_review_admin_merge_failed" => "external_review_admin_merge_failed",
		"external_review_admin_merge_unavailable" => "external_review_admin_merge_unavailable",
		"external_review_merge_visibility_timeout" => "external_review_merge_visibility_timeout",
		"external_review_pass_signal_missing" => "external_review_pass_signal_missing",
		"external_review_request_ci_red_manual_attention" => {
			"external_review_request_ci_red_manual_attention"
		},
		"non_github_review_admin_merge_failed" => "non_github_review_admin_merge_failed",
		"non_github_review_admin_merge_unavailable" => "non_github_review_admin_merge_unavailable",
		"non_github_review_merge_visibility_timeout" => {
			"non_github_review_merge_visibility_timeout"
		},
		"pull_request_is_draft" => "pull_request_is_draft",
		"pull_request_merge_commit_lineage_check_failed" => {
			"pull_request_merge_commit_lineage_check_failed"
		},
		"pull_request_not_open" => "pull_request_not_open",
		"retained_admin_merge_subject_unavailable" => "retained_admin_merge_subject_unavailable",
		"review_lifecycle_authority_branch_mismatch" => {
			"review_lifecycle_authority_branch_mismatch"
		},
		"review_lifecycle_authority_head_mismatch" => "review_lifecycle_authority_head_mismatch",
		"review_lifecycle_authority_pr_mismatch" => "review_lifecycle_authority_pr_mismatch",
		"runtime_standard_review_blocked" => "runtime_standard_review_blocked",
		"runtime_standard_review_checkpoint_producer_failed" => {
			"runtime_standard_review_checkpoint_producer_failed"
		},
		"runtime_standard_review_needs_architecture_review" => {
			"runtime_standard_review_needs_architecture_review"
		},
		"runtime_standard_review_unknown_checkpoint_status" => {
			"runtime_standard_review_unknown_checkpoint_status"
		},
		"worktree_head_missing" => "worktree_head_missing",
		_ => "retained_review_needs_attention",
	}
}
