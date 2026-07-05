use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	agent::{
		decodex_tool_bridge::DecodexRunContext,
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::Result,
};

pub(crate) const DECODEX_RUN_CONTEXT_TOOL_NAME: &str = "decodex_run_context";
pub(crate) const DECODEX_RUN_CONTEXT_NAMESPACE: &str = "decodex";

/// Client-side Decodex tools that are local to one app-server run attempt.
pub(crate) struct DecodexToolBridge<'a> {
	tracker_tools: &'a dyn DynamicToolHandler,
	run_context: DecodexRunContext,
}
impl<'a> DecodexToolBridge<'a> {
	pub(crate) fn new(
		tracker_tools: &'a dyn DynamicToolHandler,
		run_context: DecodexRunContext,
	) -> Self {
		Self { tracker_tools, run_context }
	}

	fn handle_run_context(&self, arguments: Value) -> DynamicToolCallResponse {
		if let Err(error) = serde_json::from_value::<EmptyToolArgs>(arguments) {
			return DynamicToolCallResponse::failure(format!(
				"Invalid `{DECODEX_RUN_CONTEXT_TOOL_NAME}` arguments: {error}"
			));
		}

		match serde_json::to_string(&self.run_context) {
			Ok(response) => DynamicToolCallResponse::success(response),
			Err(error) => DynamicToolCallResponse::failure(format!(
				"Failed to serialize `{DECODEX_RUN_CONTEXT_TOOL_NAME}` response: {error}"
			)),
		}
	}
}

impl DynamicToolHandler for DecodexToolBridge<'_> {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut specs = self.tracker_tools.tool_specs();
		let mut run_context_spec = DynamicToolSpec::new(
			DECODEX_RUN_CONTEXT_TOOL_NAME,
			"Return the current Decodex run, issue, branch, worktree, and repo-gate context for this app-server attempt.",
			serde_json::json!({
				"type": "object",
				"properties": {},
				"additionalProperties": false
			}),
		)
		.deferred();

		run_context_spec.namespace = Some(String::from(DECODEX_RUN_CONTEXT_NAMESPACE));

		specs.push(run_context_spec);

		specs
	}

	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse {
		self.handle_call_with_namespace(None, tool_name, arguments)
	}

	fn handle_call_with_namespace(
		&self,
		namespace: Option<&str>,
		tool_name: &str,
		arguments: Value,
	) -> DynamicToolCallResponse {
		if namespace == Some(DECODEX_RUN_CONTEXT_NAMESPACE)
			&& tool_name == DECODEX_RUN_CONTEXT_TOOL_NAME
		{
			return self.handle_run_context(arguments);
		}

		self.tracker_tools.handle_call_with_namespace(namespace, tool_name, arguments)
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		self.tracker_tools.classify_turn_completion(final_output)
	}

	fn has_terminal_completion_signal(&self) -> bool {
		self.tracker_tools.has_terminal_completion_signal()
	}

	fn validate_turn_completion(&self, final_output: &str) -> Result<()> {
		self.tracker_tools.validate_turn_completion(final_output)
	}
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyToolArgs {}
