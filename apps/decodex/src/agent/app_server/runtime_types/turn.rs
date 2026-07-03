use crate::prelude::Result;

pub(crate) trait TurnContinuationGuard {
	fn should_continue_turn(&self, turn_count: u32) -> Result<bool>;
	fn validate_continuation_boundary(&self, _turn_count: u32) -> Result<()> {
		Ok(())
	}
}
