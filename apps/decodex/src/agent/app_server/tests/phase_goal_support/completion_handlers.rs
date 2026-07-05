use std::cell::RefCell;

use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::tests) struct ContinueTokenCompletionHandler;
impl DynamicToolHandler for ContinueTokenCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(if final_output.trim() == "CONTINUE" {
			TurnCompletionStatus::Continue
		} else {
			TurnCompletionStatus::Complete
		})
	}
}

#[derive(Default)]
pub(in crate::agent::app_server::tests) struct TerminalTokenCompletionHandler {
	terminal_seen: RefCell<bool>,
}
impl DynamicToolHandler for TerminalTokenCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(match final_output.trim() {
			"CONTINUE" => TurnCompletionStatus::Continue,
			"TERMINAL" => {
				self.terminal_seen.replace(true);

				TurnCompletionStatus::Complete
			},
			_ => TurnCompletionStatus::Complete,
		})
	}

	fn has_terminal_completion_signal(&self) -> bool {
		*self.terminal_seen.borrow()
	}
}
