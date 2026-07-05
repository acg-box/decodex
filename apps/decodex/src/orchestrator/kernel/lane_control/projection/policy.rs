use crate::orchestrator::kernel::state::PolicyState;

pub(in crate::orchestrator::kernel::lane_control::projection) fn policy_requires_attention(
	policy: PolicyState,
) -> bool {
	matches!(
		policy,
		PolicyState::ReviewChurnExceeded
			| PolicyState::ContinuationRecoveryChurnExceeded
			| PolicyState::AuthorityBoundaryRequired
			| PolicyState::HumanAttentionRequired
	)
}
