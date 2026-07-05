mod boundary;
mod completion;
mod phase_goal;

use crate::{
	agent::{
		app_server::{
			dynamic_tools,
			phase_goal::{PhaseGoalRunStatus, PhaseGoalRuntime},
			protocol::AppServerClient,
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
	boundary::continuation_boundary_reached(continuation_guard, turn_count)
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
		return phase_goal::resolve_phase_goal_turn_completion(
			TurnResolutionContext { client, recorder, request, thread_id, turn_count },
			phase_goal_runtime,
			signals,
		);
	}

	completion::resolve_turn_completion_without_phase_goal(
		request,
		turn_count,
		signals.status,
		final_output,
	)
	.map(|result| result.map(|continuation_pending| (continuation_pending, None)))
}
