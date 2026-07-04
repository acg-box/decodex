use crate::orchestrator::run_cycle::{IssueDispatchMode, TargetIssueRunContext};

pub(crate) fn target_issue_run_context_with_dispatch_mode<'a, T>(
	context: &TargetIssueRunContext<'a, T>,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode,
		preferred_run_identity: context.preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	}
}
