mod events;
mod records;

#[cfg(test)] pub(crate) use self::events::latest_phase_goal_recovery_candidate;
pub(crate) use self::events::{
	issue_has_blocking_lane_decision_evidence, latest_open_issue_phase_goal_before_attempt,
	phase_goal_kind_from_str,
};

use color_eyre::Report;

use crate::{
	agent::{PhaseGoalKind, PhaseGoalTransition},
	orchestrator::{
		self, IssueRunPlan, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT, Result, RunSummary,
		ServiceConfig, StateStore, WorkflowDocument,
		execution_phase_goal::{
			RepoGatePhaseGoalController, recovery::records::PhaseGoalRecoveryRecord,
		},
	},
};

pub(crate) struct PhaseGoalRecoveryContinuation {
	pub(crate) source_phase: PhaseGoalKind,
	pub(crate) next_phase: PhaseGoalKind,
}

pub(crate) fn maybe_continue_after_phase_goal_recovery(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<RunSummary>> {
	let source_error_message = records::phase_goal_recovery_source_error_message(error);
	let Some(recovery) = recover_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		records::phase_goal_recovery_source_error_class(error),
		Some(source_error_message.as_str()),
	)?
	else {
		return Ok(None);
	};
	let mut summary = orchestrator::run_summary_from_issue_run(project.service_id(), issue_run);

	summary.continuation_pending = true;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		source_phase = recovery.source_phase.as_str(),
		next_phase = recovery.next_phase.as_str(),
		error = %error,
		"Recovered phase goal after app-server failure; scheduling continuation."
	);

	Ok(Some(summary))
}

pub(crate) fn recover_phase_goal_continuation(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_error_class: &str,
	source_error_message: Option<&str>,
) -> Result<Option<PhaseGoalRecoveryContinuation>> {
	if !orchestrator::worktree_has_tracked_changes(&issue_run.worktree.path) {
		return Ok(None);
	}

	let Some(source_phase) =
		events::latest_phase_goal_recovery_candidate(project, state_store, issue_run)?
	else {
		return Ok(None);
	};
	let controller = RepoGatePhaseGoalController { project, workflow, state_store, issue_run };
	let transition = controller.validate_phase_goal_output(source_phase)?;
	let next_phase = match transition {
		PhaseGoalTransition::Continue(next_goal)
		| PhaseGoalTransition::ScheduleContinuation(next_goal) => next_goal.phase,
		PhaseGoalTransition::CompleteRun => return Ok(None),
	};
	let prior_recovery_count = records::matching_phase_goal_recovery_count(
		project,
		state_store,
		issue_run,
		source_phase,
		source_error_class,
	)?;
	let recovery_record = PhaseGoalRecoveryRecord {
		project,
		state_store,
		issue_run,
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
	};

	if prior_recovery_count >= PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT {
		records::record_phase_goal_recovery_blocked(recovery_record, prior_recovery_count)?;

		return Ok(None);
	}

	records::record_phase_goal_recovery_continuation(recovery_record)?;

	Ok(Some(PhaseGoalRecoveryContinuation { source_phase, next_phase }))
}
