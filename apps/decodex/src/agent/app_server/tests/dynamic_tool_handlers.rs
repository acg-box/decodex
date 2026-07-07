use std::cell::RefCell;

use serde_json::Value;

use crate::{
	agent::{
		app_server::TurnContinuationGuard,
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::Result,
};

pub(super) struct InvalidToolNameHandler;
impl DynamicToolHandler for InvalidToolNameHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"invalid.tool",
			"Invalid test tool.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::success(String::from("unused"))
	}
}

pub(super) struct EmptyToolResponseHandler;
impl DynamicToolHandler for EmptyToolResponseHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"empty_response",
			"Return an invalid empty response.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse { content_items: Vec::new(), success: true }
	}
}

pub(super) struct FailingToolHandler;
impl DynamicToolHandler for FailingToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"failing_tool",
			"Return a normal tool failure response.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("tool rejected the request"))
	}
}

pub(super) struct HiddenCheckpointToolHandler {
	pub(super) called: RefCell<bool>,
}
impl DynamicToolHandler for HiddenCheckpointToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"issue_review_handoff",
			"Declared handoff tool.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		self.called.replace(true);

		DynamicToolCallResponse::success(format!("called {tool_name}"))
	}
}

pub(super) struct NamespacedDynamicToolHandler {
	pub(super) seen_namespace: RefCell<Option<String>>,
}
impl DynamicToolHandler for NamespacedDynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut spec = DynamicToolSpec::new(
			"tracker_tool",
			"Test namespaced tool.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		);

		spec.namespace = Some(String::from("tracker"));

		vec![spec]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("namespace should be forwarded"))
	}

	fn handle_call_with_namespace(
		&self,
		namespace: Option<&str>,
		_tool_name: &str,
		_arguments: Value,
	) -> DynamicToolCallResponse {
		self.seen_namespace.replace(namespace.map(str::to_owned));

		DynamicToolCallResponse::success(String::from("ok"))
	}
}

pub(super) struct LiveResumeDynamicToolHandler;
impl DynamicToolHandler for LiveResumeDynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"echo_resume",
			"Echo the provided integration text.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"text": { "type": "string" }
				},
				"required": ["text"],
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse {
		if tool_name != "echo_resume" {
			return DynamicToolCallResponse::failure(format!(
				"Unexpected live integration tool `{tool_name}`."
			));
		}

		let Some(text) = arguments.get("text").and_then(Value::as_str) else {
			return DynamicToolCallResponse::failure(String::from(
				"`echo_resume` requires a string `text` argument.",
			));
		};

		DynamicToolCallResponse::success(text.to_owned())
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(match final_output.trim() {
			"CONTINUE" => TurnCompletionStatus::Continue,
			_ => TurnCompletionStatus::Complete,
		})
	}
}

pub(super) struct LiveResumeBoundaryGuard;
impl TurnContinuationGuard for LiveResumeBoundaryGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}
}
