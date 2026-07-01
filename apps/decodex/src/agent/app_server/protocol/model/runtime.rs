use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::app_server::REQUEST_TIMEOUT;

use super::{TurnStatusPayload, UserInput};

#[derive(Debug, Default, Serialize)]
pub(in crate::agent::app_server) struct TurnStartRequest {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	pub(in crate::agent::app_server) input: Vec<UserInput>,
	#[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) output_schema: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) summary: Option<String>,
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnStartResponse {
	pub(in crate::agent::app_server) turn: TurnStatusPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct TurnInterruptRequest {
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) turn_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct TurnSteerRequest {
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) expected_turn_id: String,
	pub(in crate::agent::app_server) input: Vec<UserInput>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnSteerResponse {
	#[serde(rename = "turnId")]
	pub(in crate::agent::app_server) turn_id: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct CommandExecParams {
	pub(in crate::agent::app_server) command: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) timeout_ms: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) output_bytes_cap: Option<u64>,
}
impl CommandExecParams {
	pub(in crate::agent::app_server) fn request_timeout(&self) -> Duration {
		self.timeout_ms
			.map(Duration::from_millis)
			.map(|timeout| timeout.saturating_add(REQUEST_TIMEOUT))
			.unwrap_or(REQUEST_TIMEOUT)
	}
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct CommandExecResponse {
	pub(in crate::agent::app_server) exit_code: i32,
	pub(in crate::agent::app_server) stdout: String,
	pub(in crate::agent::app_server) stderr: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ConfigReadParams {
	pub(in crate::agent::app_server) cwd: Option<String>,
	pub(in crate::agent::app_server) include_layers: bool,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ConfigReadResponse {
	pub(in crate::agent::app_server) config: RuntimeConfigSummary,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct RuntimeConfigSummary {
	pub(in crate::agent::app_server) model: Option<String>,
	#[serde(rename = "model_provider")]
	pub(in crate::agent::app_server) model_provider: Option<String>,
	pub(in crate::agent::app_server) approval_policy: Option<Value>,
	pub(in crate::agent::app_server) sandbox_mode: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ModelListParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cursor: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) include_hidden: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ModelListResponse {
	pub(in crate::agent::app_server) data: Vec<ModelSummary>,
	#[serde(rename = "nextCursor")]
	pub(in crate::agent::app_server) next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct ModelSummary {
	pub(in crate::agent::app_server) id: String,
	pub(in crate::agent::app_server) model: String,
	#[serde(rename = "displayName")]
	pub(in crate::agent::app_server) display_name: String,
	#[serde(rename = "isDefault")]
	pub(in crate::agent::app_server) is_default: bool,
	pub(in crate::agent::app_server) hidden: bool,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct ModelProviderCapabilitiesReadParams {}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct ModelProviderCapabilitiesReadResponse {
	#[serde(rename = "imageGeneration")]
	pub(in crate::agent::app_server) image_generation: bool,
	#[serde(rename = "namespaceTools")]
	pub(in crate::agent::app_server) namespace_tools: bool,
	#[serde(rename = "webSearch")]
	pub(in crate::agent::app_server) web_search: bool,
}
