use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodexRunContext {
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) branch: String,
	pub(crate) worktree_path: String,
	pub(crate) max_turns: u32,
	pub(crate) default_canonicalize_commands: Vec<String>,
	pub(crate) default_verify_commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyToolArgs {}

#[cfg(test)]
mod tests {
	use serde_json::Value;

	use crate::agent::{
		decodex_tool_bridge::{
			DECODEX_RUN_CONTEXT_NAMESPACE, DECODEX_RUN_CONTEXT_TOOL_NAME, DecodexRunContext,
			DecodexToolBridge,
		},
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler, DynamicToolSpec,
		},
	};

	struct FakeTrackerTools;
	impl DynamicToolHandler for FakeTrackerTools {
		fn tool_specs(&self) -> Vec<DynamicToolSpec> {
			vec![DynamicToolSpec::new(
				"issue_comment",
				"Add a comment.",
				serde_json::json!({
					"type": "object",
					"additionalProperties": false
				}),
			)]
		}

		fn handle_call(&self, tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
			DynamicToolCallResponse::success(format!("delegated {tool_name}"))
		}
	}

	fn sample_bridge() -> DecodexToolBridge<'static> {
		DecodexToolBridge::new(
			&FakeTrackerTools,
			DecodexRunContext {
				run_id: String::from("run-1"),
				attempt_number: 2,
				issue_id: String::from("issue-1"),
				issue_identifier: String::from("XY-449"),
				branch: String::from("y/decodex-xy-449"),
				worktree_path: String::from("/tmp/worktree"),
				max_turns: 3,
				default_canonicalize_commands: vec![String::from("cargo make fmt")],
				default_verify_commands: vec![String::from("cargo make test")],
			},
		)
	}

	#[test]
	fn publishes_deferred_run_context_tool_with_protocol_safe_name() {
		let specs = sample_bridge().tool_specs();
		let run_context = specs
			.iter()
			.find(|spec| spec.name == DECODEX_RUN_CONTEXT_TOOL_NAME)
			.expect("run context tool should be published");
		let protocol_identifier_is_safe = |name: &str| {
			!name.is_empty()
				&& name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
		};

		assert!(run_context.defer_loading);
		assert_eq!(run_context.namespace.as_deref(), Some(DECODEX_RUN_CONTEXT_NAMESPACE));
		assert!(protocol_identifier_is_safe(&run_context.name));
		assert!(protocol_identifier_is_safe(
			run_context.namespace.as_deref().expect("run context tool should be namespaced")
		));
		assert!(specs.iter().all(|spec| protocol_identifier_is_safe(&spec.name)));
		assert!(
			specs
				.iter()
				.filter_map(|spec| spec.namespace.as_deref())
				.all(protocol_identifier_is_safe)
		);
	}

	#[test]
	fn returns_run_context_json() {
		let response = sample_bridge().handle_call_with_namespace(
			Some(DECODEX_RUN_CONTEXT_NAMESPACE),
			DECODEX_RUN_CONTEXT_TOOL_NAME,
			serde_json::json!({}),
		);

		assert!(response.success);

		let [DynamicToolContentItem::InputText { text }] = response.content_items.as_slice() else {
			panic!("run context should return one text item");
		};
		let value: Value = serde_json::from_str(text).expect("run context should be JSON");

		assert_eq!(value["runId"], "run-1");
		assert_eq!(value["issueIdentifier"], "XY-449");
		assert_eq!(value["defaultVerifyCommands"][0], "cargo make test");
	}

	#[test]
	fn validates_run_context_arguments() {
		let response = sample_bridge().handle_call_with_namespace(
			Some(DECODEX_RUN_CONTEXT_NAMESPACE),
			DECODEX_RUN_CONTEXT_TOOL_NAME,
			serde_json::json!({ "unexpected": true }),
		);

		assert!(!response.success);
		assert!(matches!(
			response.content_items.as_slice(),
			[DynamicToolContentItem::InputText { text }]
				if text.contains("Invalid `decodex_run_context` arguments")
		));
	}

	#[test]
	fn delegates_tracker_tools() {
		let response = sample_bridge().handle_call("issue_comment", serde_json::json!({}));

		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText {
				text: String::from("delegated issue_comment")
			}]
		);
	}
}
