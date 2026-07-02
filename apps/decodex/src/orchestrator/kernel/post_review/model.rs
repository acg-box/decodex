use crate::orchestrator::PostReviewLaneDecision;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct PostReviewLaneKernelInput<'a> {
	pub(in crate::orchestrator) issue_id: &'a str,
	pub(in crate::orchestrator) run_id: Option<&'a str>,
	pub(in crate::orchestrator) lifecycle_present: bool,
	pub(in crate::orchestrator) proposed_decision: PostReviewLaneDecision,
	pub(in crate::orchestrator) reason: &'a str,
	pub(in crate::orchestrator) retry_budget_exhausted: bool,
}
