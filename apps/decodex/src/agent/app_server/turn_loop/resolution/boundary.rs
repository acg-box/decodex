use crate::{agent::app_server::runtime_types::TurnContinuationGuard, prelude::Result};

pub(in crate::agent::app_server::turn_loop::resolution) fn continuation_boundary_reached(
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
