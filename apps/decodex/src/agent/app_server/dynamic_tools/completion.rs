use crate::{
	agent::app_server::{DynamicToolHandler, TurnCompletionStatus, eyre},
	prelude::Result,
};

pub(in crate::agent::app_server) fn classify_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> Result<TurnCompletionStatus> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		return dynamic_tool_handler.classify_turn_completion(final_output);
	}

	Ok(TurnCompletionStatus::Complete)
}

pub(in crate::agent::app_server) fn has_terminal_completion_signal(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
) -> bool {
	dynamic_tool_handler.is_some_and(DynamicToolHandler::has_terminal_completion_signal)
}

pub(in crate::agent::app_server) fn reject_nonterminal_single_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> Result<()> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		dynamic_tool_handler.validate_turn_completion(final_output)?;
	}

	eyre::bail!(
		"Turn completed without a terminal completion path while same-thread continuation is disabled."
	);
}
