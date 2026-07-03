use crate::orchestrator::PostReviewLaneDecision;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneKernelInput<'a> {
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) lifecycle_present: bool,
	pub(crate) proposed_decision: PostReviewLaneDecision,
	pub(crate) reason: &'a str,
	pub(crate) retry_budget_exhausted: bool,
}
