use color_eyre::Report;

use crate::{
	agent::app_server::{
		AppServerClient, AppServerPhaseGoalFailure, AppServerRunRequest, PhaseGoalController,
		PhaseGoalKind, PhaseGoalSpec, RunRecorder, ThreadGoal, ThreadGoalClearParams,
		ThreadGoalGetParams, ThreadGoalSetParams, ThreadGoalStatus,
		serde_json::{self, Value},
	},
	prelude::Result,
};

pub(crate) struct PhaseGoalRuntime<'a> {
	pub(crate) controller: &'a dyn PhaseGoalController,
	pub(crate) active_goal: PhaseGoalSpec,
}

pub(crate) fn initialize_phase_goal_runtime<'a>(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &'a AppServerRunRequest<'_>,
	thread_id: &str,
) -> Result<Option<PhaseGoalRuntime<'a>>> {
	let Some(controller) = request.phase_goal_controller else {
		return Ok(None);
	};
	let Some(active_goal) = controller.initial_phase_goal()? else {
		return Ok(None);
	};

	match set_thread_phase_goal(client, recorder, thread_id, &active_goal) {
		Ok(()) => Ok(Some(PhaseGoalRuntime { controller, active_goal })),
		Err(error) if app_server_method_not_found(&error) =>
			Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/set"))
				.wrap_err(error)),
		Err(error) => Err(error),
	}
}

pub(crate) fn set_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	goal: &PhaseGoalSpec,
) -> Result<()> {
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

pub(crate) fn get_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	runtime: &PhaseGoalRuntime<'_>,
) -> Result<ThreadGoal> {
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

pub(crate) fn clear_thread_phase_goal_best_effort(
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

pub(crate) fn record_phase_goal_completed(
	recorder: &mut RunRecorder<'_>,
	phase: PhaseGoalKind,
	goal: &ThreadGoal,
) -> Result<()> {
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

pub(crate) fn app_server_method_not_found(error: &Report) -> bool {
	let text = error.to_string().to_lowercase();

	text.contains("-32601") || text.contains("method not found")
}

fn record_phase_goal_private_event(
	recorder: &mut RunRecorder<'_>,
	event_type: &str,
	phase: PhaseGoalKind,
	payload: &Value,
) -> Result<()> {
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
