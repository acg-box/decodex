use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec,
};

use super::{
	CommandExecutionApprovalDecision, FileChangeApprovalDecision, McpServerElicitationAction,
	PermissionGrantScope,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::app_server) struct AppServerDynamicToolNamespaceTool {
	#[serde(rename = "type")]
	kind: &'static str,
	description: String,
	#[serde(rename = "deferLoading", default, skip_serializing_if = "std::ops::Not::not")]
	defer_loading: bool,
	#[serde(rename = "inputSchema")]
	input_schema: Value,
	name: String,
}
impl AppServerDynamicToolNamespaceTool {
	fn from_spec(spec: &DynamicToolSpec) -> Self {
		Self {
			kind: "function",
			description: spec.description.clone(),
			defer_loading: spec.defer_loading,
			input_schema: spec.input_schema.clone(),
			name: spec.name.clone(),
		}
	}
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct DynamicToolCallParams {
	pub(in crate::agent::app_server) arguments: Value,
	#[serde(rename = "callId")]
	pub(in crate::agent::app_server) call_id: String,
	pub(in crate::agent::app_server) namespace: Option<String>,
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) tool: String,
	#[serde(rename = "turnId")]
	pub(in crate::agent::app_server) turn_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct CommandExecutionRequestApprovalResponse {
	pub(in crate::agent::app_server) decision: CommandExecutionApprovalDecision,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct FileChangeRequestApprovalResponse {
	pub(in crate::agent::app_server) decision: FileChangeApprovalDecision,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ToolRequestUserInputResponse {
	pub(in crate::agent::app_server) answers: HashMap<String, ToolRequestUserInputAnswer>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ToolRequestUserInputAnswer {
	pub(in crate::agent::app_server) answers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct McpServerElicitationRequestResponse {
	pub(in crate::agent::app_server) action: McpServerElicitationAction,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) content: Option<Value>,
	#[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) meta: Option<Value>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct PermissionsRequestApprovalResponse {
	pub(in crate::agent::app_server) permissions: GrantedPermissionProfile,
	pub(in crate::agent::app_server) scope: PermissionGrantScope,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct GrantedPermissionProfile {}

pub(in crate::agent::app_server) struct ProbeDynamicToolHandler;
impl DynamicToolHandler for ProbeDynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"echo_probe",
			"Echo the provided text back to the model.",
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
		if tool_name != "echo_probe" {
			return DynamicToolCallResponse::failure(format!(
				"Unexpected probe tool `{tool_name}`."
			));
		}

		let Some(text) = arguments.get("text").and_then(Value::as_str) else {
			return DynamicToolCallResponse::failure(String::from(
				"`echo_probe` requires a string `text` argument.",
			));
		};

		DynamicToolCallResponse::success(text.to_owned())
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum AppServerDynamicToolSpec {
	#[serde(rename = "function")]
	Function {
		description: String,
		#[serde(rename = "deferLoading", default, skip_serializing_if = "std::ops::Not::not")]
		defer_loading: bool,
		#[serde(rename = "inputSchema")]
		input_schema: Value,
		name: String,
	},
	#[serde(rename = "namespace")]
	Namespace { description: String, name: String, tools: Vec<AppServerDynamicToolNamespaceTool> },
}
impl AppServerDynamicToolSpec {
	fn function_from_spec(spec: &DynamicToolSpec) -> Self {
		Self::Function {
			description: spec.description.clone(),
			defer_loading: spec.defer_loading,
			input_schema: spec.input_schema.clone(),
			name: spec.name.clone(),
		}
	}
}

pub(in crate::agent::app_server) fn app_server_dynamic_tool_specs(
	tool_specs: &[DynamicToolSpec],
) -> Vec<AppServerDynamicToolSpec> {
	let mut app_server_specs = Vec::new();
	let mut namespace_tools = BTreeMap::<String, Vec<AppServerDynamicToolNamespaceTool>>::new();

	for spec in tool_specs {
		if let Some(namespace) = spec.namespace.as_deref() {
			namespace_tools
				.entry(namespace.to_owned())
				.or_default()
				.push(AppServerDynamicToolNamespaceTool::from_spec(spec));
		} else {
			app_server_specs.push(AppServerDynamicToolSpec::function_from_spec(spec));
		}
	}
	for (namespace, tools) in namespace_tools {
		app_server_specs.push(AppServerDynamicToolSpec::Namespace {
			description: format!("Dynamic tools in the {namespace} namespace."),
			name: namespace,
			tools,
		});
	}

	app_server_specs
}
