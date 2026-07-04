use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			dynamic_tools, phase_goal,
			phase_goal::{
				AppServerPhaseGoalFailure, PhaseGoalRunStatus, PhaseGoalRuntime,
				PhaseGoalTransition,
			},
			protocol::{AppServerClient, ThreadGoal, ThreadGoalStatus},
			runtime_types::{AppServerRunRequest, RunRecorder, TurnContinuationGuard},
		},
		tracker_tool_bridge::TurnCompletionStatus,
	},
	prelude::Result,
};

struct TurnResolutionContext<'a, 'run> {
	client: &'a mut AppServerClient,
	recorder: &'a mut RunRecorder<'run>,
	request: &'a AppServerRunRequest<'run>,
	thread_id: &'a str,
	turn_count: u32,
}

#[derive(Clone, Copy)]
struct CompletionSignals {
	status: TurnCompletionStatus,
	terminal_signal: bool,
}

pub(in crate::agent::app_server) fn continuation_boundary_reached(
	continuation_guard: Option<&dyn TurnContinuationGuard>,
	turn_count: u32,
) -> Result<bool> {
	let Some(continuation_guard) = continuation_guard else {
		return Ok(false);
	};

	if continuation_guard.should_continue_turn(turn_count)? {
		return Ok(false);
	}

	continuation_guard.validate_continuation_boundary(turn_count)?;

	Ok(true)
}

pub(super) fn resolve_turn_completion<'run>(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'run>,
	request: &AppServerRunRequest<'run>,
	phase_goal_runtime: &mut Option<PhaseGoalRuntime<'run>>,
	thread_id: &str,
	turn_count: u32,
	final_output: &str,
) -> Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let completion_status =
		dynamic_tools::classify_turn_completion(request.dynamic_tool_handler, final_output)?;
	let signals = CompletionSignals {
		status: completion_status,
		terminal_signal: dynamic_tools::has_terminal_completion_signal(
			request.dynamic_tool_handler,
		),
	};

	if phase_goal_runtime.is_some() {
		return resolve_phase_goal_turn_completion(
			TurnResolutionContext { client, recorder, request, thread_id, turn_count },
			phase_goal_runtime,
			signals,
		);
	}

	resolve_turn_completion_without_phase_goal(request, turn_count, signals.status, final_output)
		.map(|result| result.map(|continuation_pending| (continuation_pending, None)))
}

fn resolve_phase_goal_turn_completion(
	context: TurnResolutionContext<'_, '_>,
	phase_goal_runtime: &mut Option<PhaseGoalRuntime<'_>>,
	signals: CompletionSignals,
) -> Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let observed_goal_result = {
		let runtime = phase_goal_runtime
			.as_ref()
			.expect("phase goal runtime should be present after is_some check");

		phase_goal::get_thread_phase_goal(
			context.client,
			context.recorder,
			context.thread_id,
			runtime,
		)
	};
	let observed_goal = match observed_goal_result {
		Ok(goal) => goal,
		Err(error) if phase_goal::app_server_method_not_found(&error) => {
			return Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/get"))
				.wrap_err(error));
		},
		Err(error) => return Err(error),
	};
	let runtime = phase_goal_runtime
		.as_mut()
		.expect("phase goal runtime should still be present after goal status read");
	let observed_status = PhaseGoalRunStatus {
		phase: runtime.active_goal.phase,
		status: observed_goal.status.as_str().to_owned(),
	};

	if observed_goal.status == ThreadGoalStatus::Complete {
		return resolve_completed_phase_goal_turn(
			context,
			runtime,
			signals,
			observed_status,
			&observed_goal,
		);
	}
	if signals.status == TurnCompletionStatus::Complete && signals.terminal_signal {
		phase_goal::clear_thread_phase_goal_best_effort(
			context.client,
			context.recorder,
			context.thread_id,
		);

		return Ok(Some((false, Some(observed_status))));
	}
	if context.turn_count >= context.request.max_turns {
		return Ok(Some((true, Some(observed_status))));
	}
	if continuation_boundary_reached(context.request.continuation_guard, context.turn_count)? {
		return Ok(Some((true, Some(observed_status))));
	}

	Ok(None)
}

fn resolve_completed_phase_goal_turn(
	context: TurnResolutionContext<'_, '_>,
	runtime: &mut PhaseGoalRuntime<'_>,
	signals: CompletionSignals,
	observed_status: PhaseGoalRunStatus,
	observed_goal: &ThreadGoal,
) -> Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let transition = runtime.controller.phase_goal_completed(runtime.active_goal.phase)?;

	phase_goal::record_phase_goal_completed(
		context.recorder,
		runtime.active_goal.phase,
		observed_goal,
	)?;

	match transition {
		PhaseGoalTransition::Continue(next_goal) => {
			if signals.status == TurnCompletionStatus::Complete && signals.terminal_signal {
				return Ok(Some((false, Some(observed_status))));
			}

			phase_goal::set_thread_phase_goal(
				context.client,
				context.recorder,
				context.thread_id,
				&next_goal,
			)?;

			runtime.active_goal = next_goal;

			if context.turn_count >= context.request.max_turns {
				return Ok(Some((true, Some(observed_status))));
			}
			if continuation_boundary_reached(
				context.request.continuation_guard,
				context.turn_count,
			)? {
				return Ok(Some((true, Some(observed_status))));
			}

			Ok(None)
		},
		PhaseGoalTransition::CompleteRun => {
			if signals.status == TurnCompletionStatus::Complete && signals.terminal_signal {
				phase_goal::clear_thread_phase_goal_best_effort(
					context.client,
					context.recorder,
					context.thread_id,
				);

				return Ok(Some((false, Some(observed_status))));
			}

			Err(Report::new(AppServerPhaseGoalFailure::missing_terminal_path(
				runtime.active_goal.phase,
			)))
		},
	}
}

fn resolve_turn_completion_without_phase_goal(
	request: &AppServerRunRequest<'_>,
	turn_count: u32,
	completion_status: TurnCompletionStatus,
	final_output: &str,
) -> Result<Option<bool>> {
	match completion_status {
		TurnCompletionStatus::Complete => Ok(Some(false)),
		TurnCompletionStatus::Continue => {
			if request.max_turns <= 1 {
				dynamic_tools::reject_nonterminal_single_turn_completion(
					request.dynamic_tool_handler,
					final_output,
				)?;
			}
			if turn_count >= request.max_turns {
				return Ok(Some(true));
			}
			if continuation_boundary_reached(request.continuation_guard, turn_count)? {
				return Ok(Some(true));
			}

			Ok(None)
		},
	}
}
