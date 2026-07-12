use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			phase_goal,
			phase_goal::{
				AppServerPhaseGoalFailure, PhaseGoalRunStatus, PhaseGoalRuntime,
				PhaseGoalTransition,
			},
			protocol::{ThreadGoal, ThreadGoalStatus},
			turn_loop::resolution::{self, CompletionSignals, TurnResolutionContext},
		},
		tracker_tool_bridge::TurnCompletionStatus,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::turn_loop::resolution) fn resolve_phase_goal_turn_completion(
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
	if resolution::continuation_boundary_reached(
		context.request.continuation_guard,
		context.turn_count,
	)? {
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
			if resolution::continuation_boundary_reached(
				context.request.continuation_guard,
				context.turn_count,
			)? {
				return Ok(Some((true, Some(observed_status))));
			}

			Ok(None)
		},
		PhaseGoalTransition::ScheduleContinuation(next_goal) => {
			phase_goal::set_thread_phase_goal(
				context.client,
				context.recorder,
				context.thread_id,
				&next_goal,
			)?;
			runtime.active_goal = next_goal;
			Ok(Some((true, Some(observed_status))))
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
