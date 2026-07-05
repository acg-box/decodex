use crate::{
	agent::{
		app_server::{dynamic_tools, runtime_types::AppServerRunRequest, turn_loop::resolution},
		tracker_tool_bridge::TurnCompletionStatus,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::turn_loop::resolution) fn resolve_turn_completion_without_phase_goal(
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
			if resolution::continuation_boundary_reached(request.continuation_guard, turn_count)? {
				return Ok(Some(true));
			}

			Ok(None)
		},
	}
}
