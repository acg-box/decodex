#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum ReasonCode {
	ActiveOwnedWork,
	ContradictoryAuthority,
	ExternalSignalPending,
	HumanAttentionSignal,
	IncompleteAuthority,
	NoRunnableWork,
	PostReviewLifecycleMissing,
	ReadyToLand,
	RetainedLaneReusable,
	RetryBudgetAvailable,
	RetryBudgetExhausted,
	TerminalCleanupPending,
}

impl ReasonCode {
	pub(in crate::orchestrator) const fn as_str(self) -> &'static str {
		match self {
			Self::ActiveOwnedWork => "active_owned_work",
			Self::ContradictoryAuthority => "contradictory_authority",
			Self::ExternalSignalPending => "external_signal_pending",
			Self::HumanAttentionSignal => "human_attention_signal",
			Self::IncompleteAuthority => "incomplete_authority",
			Self::NoRunnableWork => "no_runnable_work",
			Self::PostReviewLifecycleMissing => "post_review_lifecycle_missing",
			Self::ReadyToLand => "ready_to_land",
			Self::RetainedLaneReusable => "retained_lane_reusable",
			Self::RetryBudgetAvailable => "retry_budget_available",
			Self::RetryBudgetExhausted => "retry_budget_exhausted",
			Self::TerminalCleanupPending => "terminal_cleanup_pending",
		}
	}

	pub(in crate::orchestrator) const fn public_summary(self) -> &'static str {
		match self {
			Self::ActiveOwnedWork => "owned work remains active",
			Self::ContradictoryAuthority => "lane authority signals conflict",
			Self::ExternalSignalPending => "waiting for an external review or check signal",
			Self::HumanAttentionSignal => "human attention was requested for this lane",
			Self::IncompleteAuthority => "lane authority is incomplete",
			Self::NoRunnableWork => "no runnable owned work is currently available",
			Self::PostReviewLifecycleMissing => "post-review lifecycle authority is missing",
			Self::ReadyToLand => "pull request satisfies landing prerequisites",
			Self::RetainedLaneReusable => "retained lane can be resumed",
			Self::RetryBudgetAvailable => "retry budget remains available",
			Self::RetryBudgetExhausted => "retry budget is exhausted",
			Self::TerminalCleanupPending => "terminal cleanup remains pending",
		}
	}
}
