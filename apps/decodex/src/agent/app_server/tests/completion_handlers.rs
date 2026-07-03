use serde_json::Value;

use crate::{
	agent::{
		app_server::TurnContinuationGuard,
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::{Result, eyre},
};

pub(super) struct RejectingCompletionHandler;
impl DynamicToolHandler for RejectingCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn validate_turn_completion(&self, _final_output: &str) -> Result<()> {
		Err(eyre::eyre!("terminal finalization missing"))
	}
}

pub(super) struct ContinuingCompletionHandler;
impl DynamicToolHandler for ContinuingCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, _final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(TurnCompletionStatus::Continue)
	}

	fn validate_turn_completion(&self, _final_output: &str) -> Result<()> {
		Err(eyre::eyre!("terminal finalization missing"))
	}
}

pub(super) struct YieldingContinuationGuard;
impl TurnContinuationGuard for YieldingContinuationGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}
}

pub(super) struct RejectingContinuationGuard;
impl TurnContinuationGuard for RejectingContinuationGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}

	fn validate_continuation_boundary(&self, turn_count: u32) -> Result<()> {
		Err(eyre::eyre!("turn {turn_count} hit an invalid continuation boundary"))
	}
}
