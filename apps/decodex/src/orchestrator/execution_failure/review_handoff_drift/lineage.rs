use crate::orchestrator::execution_failure::{
	self, CodexAccountAuthFailure, Command, LoopGuardrailStopRequested, ManualAttentionRequested,
	Path, Report, RetainedReviewNeedsAttention, ReviewHandoffNeedsAttention,
	ReviewPolicyStopRequested, review_handoff_drift::types::ReviewHandoffFailureDriftLineage,
};

pub(super) fn review_handoff_failure_drift_can_handle(error: &Report) -> bool {
	!execution_failure::run_failure_requires_terminal_attention(error)
		&& error.downcast_ref::<ManualAttentionRequested>().is_none()
		&& error.downcast_ref::<LoopGuardrailStopRequested>().is_none()
		&& error.downcast_ref::<ReviewHandoffNeedsAttention>().is_none()
		&& error.downcast_ref::<RetainedReviewNeedsAttention>().is_none()
		&& error.downcast_ref::<ReviewPolicyStopRequested>().is_none()
		&& error.downcast_ref::<CodexAccountAuthFailure>().is_none()
}

pub(super) fn review_handoff_failure_drift_source_error_class(error: &Report) -> &'static str {
	execution_failure::retained_progress_source_error_class(error)
		.unwrap_or("retryable_execution_failure")
}

pub(super) fn review_handoff_failure_drift_lineage(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffFailureDriftLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffFailureDriftLineage::Exact;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffFailureDriftLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffFailureDriftLineage::Descends,
		Some(1) => ReviewHandoffFailureDriftLineage::Diverged,
		_ => ReviewHandoffFailureDriftLineage::Unknown,
	}
}
