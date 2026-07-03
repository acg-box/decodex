//! Phase-goal protocol state, failures, and app-server runtime helpers.

use std::{
	error::Error,
	fmt::{self, Display, Formatter},
};

use color_eyre::Report;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use super::{
	AppServerClient, AppServerRunRequest, RunRecorder, ThreadGoal, ThreadGoalClearParams,
	ThreadGoalGetParams, ThreadGoalSetParams, ThreadGoalStatus,
};

pub(crate) trait PhaseGoalController {
	fn initial_phase_goal(&self) -> crate::prelude::Result<Option<PhaseGoalSpec>>;
	fn phase_goal_completed(
		&self,
		phase: PhaseGoalKind,
	) -> crate::prelude::Result<PhaseGoalTransition>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseGoalKind {
	ImplementToValidationReady,
	RepairValidationFailures,
	RepairAcceptedReviewFindings,
	ReviewRepairEvidence,
	HandoffEvidence,
}
impl PhaseGoalKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ImplementToValidationReady => "implement_to_validation_ready",
			Self::RepairValidationFailures => "repair_validation_failures",
			Self::RepairAcceptedReviewFindings => "repair_accepted_review_findings",
			Self::ReviewRepairEvidence => "review_repair_evidence",
			Self::HandoffEvidence => "handoff_evidence",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhaseGoalTransition {
	Continue(PhaseGoalSpec),
	CompleteRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerPhaseGoalFailureKind {
	Unsupported { method: &'static str },
	MissingTerminalPath { phase: PhaseGoalKind },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhaseGoalSpec {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) objective: String,
	pub(crate) token_budget: Option<i64>,
}
impl PhaseGoalSpec {
	pub(crate) fn new(
		phase: PhaseGoalKind,
		objective: impl Into<String>,
		token_budget: Option<i64>,
	) -> Self {
		Self { phase, objective: objective.into(), token_budget }
	}
}

#[derive(Debug)]
pub(crate) struct AppServerPhaseGoalFailure {
	kind: AppServerPhaseGoalFailureKind,
}
impl AppServerPhaseGoalFailure {
	pub(super) fn unsupported(method: &'static str) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::Unsupported { method } }
	}

	#[cfg(test)]
	pub(crate) fn unsupported_for_test(method: &'static str) -> Self {
		Self::unsupported(method)
	}

	pub(super) fn missing_terminal_path(phase: PhaseGoalKind) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } }
	}

	#[cfg(test)]
	pub(crate) fn missing_terminal_path_for_test(phase: PhaseGoalKind) -> Self {
		Self::missing_terminal_path(phase)
	}

	pub(crate) fn is_terminal_path_missing(&self) -> bool {
		matches!(self.kind, AppServerPhaseGoalFailureKind::MissingTerminalPath { .. })
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { .. } => {
				"app_server_phase_goal_unsupported"
			},
			AppServerPhaseGoalFailureKind::MissingTerminalPath { .. } => {
				"phase_goal_terminal_path_missing"
			},
		}
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"decodex will retry `{}` terminal-path recovery automatically; the next attempt must run the required review, handoff, closeout, or manual-attention terminal tool instead of treating phase-goal completion as issue completion",
				phase.as_str()
			),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`, confirm with `decodex probe stdio://`, restart `decodex serve`, {recovery_gate}"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"inspect the retained lane after phase goal `{}` completed without a terminal Decodex path, finish validation/review/handoff or route manual attention, {recovery_gate}",
				phase.as_str()
			),
		}
	}
}

impl Display for AppServerPhaseGoalFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => {
				write!(
					formatter,
					"Unsupported Codex app-server: required phase-goal method `{method}` is unavailable."
				)
			},
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => write!(
				formatter,
				"Phase goal `{}` completed without a Decodex terminal completion path.",
				phase.as_str()
			),
		}
	}
}

impl Error for AppServerPhaseGoalFailure {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PhaseGoalRunStatus {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) status: String,
}

pub(super) struct PhaseGoalRuntime<'a> {
	pub(super) controller: &'a dyn PhaseGoalController,
	pub(super) active_goal: PhaseGoalSpec,
}

pub(super) fn initialize_phase_goal_runtime<'a>(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &'a AppServerRunRequest<'_>,
	thread_id: &str,
) -> crate::prelude::Result<Option<PhaseGoalRuntime<'a>>> {
	let Some(controller) = request.phase_goal_controller else {
		return Ok(None);
	};
	let Some(active_goal) = controller.initial_phase_goal()? else {
		return Ok(None);
	};

	match set_thread_phase_goal(client, recorder, thread_id, &active_goal) {
		Ok(()) => Ok(Some(PhaseGoalRuntime { controller, active_goal })),
		Err(error) if app_server_method_not_found(&error) => {
			Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/set"))
				.wrap_err(error))
		},
		Err(error) => Err(error),
	}
}

pub(super) fn set_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	goal: &PhaseGoalSpec,
) -> crate::prelude::Result<()> {
	let response = client.set_thread_goal(ThreadGoalSetParams {
		thread_id: thread_id.to_owned(),
		objective: Some(goal.objective.clone()),
		status: Some(ThreadGoalStatus::Active),
		token_budget: goal.token_budget,
	})?;
	let payload = serde_json::json!({
		"phase": goal.phase.as_str(),
		"status": response.goal.status.as_str(),
		"threadId": response.goal.thread_id,
		"tokenBudget": response.goal.token_budget,
		"tokensUsed": response.goal.tokens_used,
		"timeUsedSeconds": response.goal.time_used_seconds,
	});

	recorder.record("thread/goal/set", &payload.to_string())?;

	record_phase_goal_private_event(recorder, "phase_goal_set", goal.phase, &payload)
}

pub(super) fn get_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	runtime: &PhaseGoalRuntime<'_>,
) -> crate::prelude::Result<ThreadGoal> {
	let response =
		client.get_thread_goal(ThreadGoalGetParams { thread_id: thread_id.to_owned() })?;
	let goal = response.goal.ok_or_else(|| {
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path(runtime.active_goal.phase))
			.wrap_err("Codex app-server returned no active phase goal for a goal-controlled lane.")
	})?;
	let payload = serde_json::json!({
		"phase": runtime.active_goal.phase.as_str(),
		"status": goal.status.as_str(),
		"threadId": goal.thread_id,
		"tokenBudget": goal.token_budget,
		"tokensUsed": goal.tokens_used,
		"timeUsedSeconds": goal.time_used_seconds,
	});

	recorder.record("thread/goal/get", &payload.to_string())?;

	record_phase_goal_private_event(
		recorder,
		"phase_goal_status",
		runtime.active_goal.phase,
		&payload,
	)?;

	Ok(goal)
}

pub(super) fn clear_thread_phase_goal_best_effort(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
) {
	match client.clear_thread_goal(ThreadGoalClearParams { thread_id: thread_id.to_owned() }) {
		Ok(response) => {
			let payload = serde_json::json!({ "cleared": response.cleared, "threadId": thread_id });

			if let Err(error) = recorder.record("thread/goal/clear", &payload.to_string()) {
				tracing::warn!(?error, "Failed to record app-server goal clear response.");
			}
		},
		Err(error) => {
			tracing::warn!(?error, "Failed to clear app-server phase goal after terminal path.")
		},
	}
}

pub(super) fn record_phase_goal_completed(
	recorder: &mut RunRecorder<'_>,
	phase: PhaseGoalKind,
	goal: &ThreadGoal,
) -> crate::prelude::Result<()> {
	let payload = serde_json::json!({
		"schema": "decodex.phase_goal_signal/1",
		"phase": phase.as_str(),
		"signal": "goal_complete",
		"threadId": goal.thread_id,
		"status": goal.status.as_str(),
		"tokenBudget": goal.token_budget,
		"tokensUsed": goal.tokens_used,
		"timeUsedSeconds": goal.time_used_seconds,
	});

	record_phase_goal_private_event(recorder, "phase_goal_completed", phase, &payload)
}

fn record_phase_goal_private_event(
	recorder: &mut RunRecorder<'_>,
	event_type: &str,
	phase: PhaseGoalKind,
	payload: &Value,
) -> crate::prelude::Result<()> {
	recorder.state_store.append_private_execution_event(
		recorder.project_id(),
		recorder.issue_id(),
		recorder.run_id,
		recorder.attempt_number,
		event_type,
		serde_json::json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": phase.as_str(),
			"payload": payload,
		}),
	)?;

	Ok(())
}

pub(super) fn app_server_method_not_found(error: &Report) -> bool {
	let text = error.to_string().to_lowercase();

	text.contains("-32601") || text.contains("method not found")
}
