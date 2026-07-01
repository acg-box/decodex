#[allow(clippy::wildcard_imports)] use super::*;

const INTERNAL_RETAINED_DRAIN_MAX_PASSES: usize = 2;

mod complete;
mod prepare;
mod project;
mod target;

use complete::complete_issue_run;
#[cfg(test)]
pub(crate) use complete::{
	drain_non_github_review_retained_tail_with_inspector, run_retained_closeout_for_handoff_summary,
};
pub(crate) use prepare::prepare_issue_run;
pub(crate) use project::{plan_project_issue_run_with_exclusions, run_project_once};
pub(crate) use target::{
	closeout_lane_active_claim_blocks_dispatch, run_target_issue_once,
	run_target_issue_once_with_inferred_dispatch,
};
#[cfg(test)]
pub(crate) use target::{
	select_target_status_visible_program_candidate, target_issue_active_claim_blocks_dispatch,
};

pub(crate) fn run_configured_cycle(request: RunCycleRequest<'_>) -> Result<Option<RunSummary>> {
	let config = ServiceConfig::from_path(request.config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;

	if let Some(issue_id) = request.preferred_issue_id {
		let target_context = TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: request.state_store,
			issue_id,
			preferred_issue_state: request.preferred_issue_state,
			preferred_initial_issue_state: request.preferred_initial_issue_state,
			dry_run: request.dry_run,
			lease_preacquired: request.preferred_lease_acquired,
			preferred_issue_claim_fd: request.preferred_issue_claim_fd,
			preferred_dispatch_slot_fd: request.preferred_dispatch_slot_fd,
			preferred_dispatch_slot_index: request.preferred_dispatch_slot_index,
			dispatch_mode: request.preferred_dispatch_mode.unwrap_or(IssueDispatchMode::Normal),
			preferred_run_identity: request.preferred_run_identity,
			preferred_retry_budget_base: request.preferred_retry_budget_base,
		};

		return match request.preferred_dispatch_mode {
			Some(_) => run_target_issue_once(target_context),
			None => run_target_issue_once_with_inferred_dispatch(target_context),
		};
	}

	run_project_once(&tracker, &config, &workflow, request.state_store, request.dry_run)
}

pub(crate) fn load_configured_cycle_workflow(
	config: &ServiceConfig,
	preferred_workflow_snapshot: Option<&str>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();

	match preferred_workflow_snapshot {
		Some(snapshot) => WorkflowDocument::parse_markdown(snapshot),
		None => WorkflowDocument::from_path(&workflow_path),
	}
}
