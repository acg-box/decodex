use std::{
	collections::{BTreeMap, HashMap},
	time::Duration,
};

use serde::{Deserialize, Deserializer, Serialize, de::Error};
use serde_json::Value;

use crate::agent::{
	app_server::REQUEST_TIMEOUT,
	tracker_tool_bridge::{DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec},
};

#[derive(Default)]
pub(in crate::agent::app_server) struct RunOutcome {
	pub(in crate::agent::app_server) final_output: String,
	pub(in crate::agent::app_server) turn_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct InitializeParams {
	#[serde(rename = "clientInfo")]
	pub(in crate::agent::app_server) client_info: ClientInfo,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) capabilities: Option<InitializeCapabilities>,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct ClientInfo {
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) version: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct InitializeCapabilities {
	#[serde(rename = "experimentalApi", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) experimental_api: Option<bool>,
	#[serde(default, rename = "optOutNotificationMethods", skip_serializing_if = "Vec::is_empty")]
	pub(in crate::agent::app_server) opt_out_notification_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct InitializeResponse {
	#[serde(rename = "userAgent")]
	pub(in crate::agent::app_server) user_agent: String,
	#[allow(dead_code)]
	#[serde(rename = "codexHome")]
	pub(in crate::agent::app_server) codex_home: String,
	#[allow(dead_code)]
	#[serde(rename = "platformFamily")]
	pub(in crate::agent::app_server) platform_family: String,
	#[allow(dead_code)]
	#[serde(rename = "platformOs")]
	pub(in crate::agent::app_server) platform_os: String,
}

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

#[derive(Debug, Default, Serialize)]
pub(in crate::agent::app_server) struct ThreadStartRequest {
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) base_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) config: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	#[serde(rename = "dynamicTools", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) dynamic_tools: Option<Vec<AppServerDynamicToolSpec>>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) developer_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) ephemeral: Option<bool>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model_provider: Option<String>,
	#[serde(rename = "serviceName", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) service_name: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(in crate::agent::app_server) struct ThreadResumeRequest {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model: Option<String>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model_provider: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	#[serde(rename = "approvalPolicy", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) approval_policy: Option<Value>,
	#[serde(rename = "approvalsReviewer", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) approvals_reviewer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) sandbox: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) config: Option<Value>,
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) base_instructions: Option<String>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) developer_instructions: Option<String>,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct ThreadArchiveRequest {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadArchiveResponse {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalSetParams {
	pub(in crate::agent::app_server) thread_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) objective: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) status: Option<ThreadGoalStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) token_budget: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalGetParams {
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalClearParams {
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoal {
	#[allow(dead_code)]
	pub(in crate::agent::app_server) created_at: i64,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) objective: String,
	pub(in crate::agent::app_server) status: ThreadGoalStatus,
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) time_used_seconds: i64,
	pub(in crate::agent::app_server) token_budget: Option<i64>,
	pub(in crate::agent::app_server) tokens_used: i64,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalSetResponse {
	pub(in crate::agent::app_server) goal: ThreadGoal,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalGetResponse {
	pub(in crate::agent::app_server) goal: Option<ThreadGoal>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalClearResponse {
	pub(in crate::agent::app_server) cleared: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadSessionResponse {
	pub(in crate::agent::app_server) thread: Thread,
	pub(in crate::agent::app_server) model: String,
	#[serde(rename = "modelProvider")]
	pub(in crate::agent::app_server) model_provider: String,
	#[serde(rename = "serviceTier")]
	pub(in crate::agent::app_server) _service_tier: Option<Value>,
	pub(in crate::agent::app_server) cwd: String,
	#[serde(default, rename = "instructionSources")]
	pub(in crate::agent::app_server) _instruction_sources: Vec<String>,
	#[serde(rename = "approvalPolicy")]
	pub(in crate::agent::app_server) approval_policy: Value,
	#[serde(rename = "approvalsReviewer")]
	pub(in crate::agent::app_server) approvals_reviewer: String,
	pub(in crate::agent::app_server) sandbox: Value,
	#[serde(rename = "reasoningEffort")]
	pub(in crate::agent::app_server) _reasoning_effort: Option<String>,
}
impl ThreadSessionResponse {
	pub(in crate::agent::app_server) fn effective_config(&self) -> EffectiveThreadConfig {
		EffectiveThreadConfig {
			model: self.model.clone(),
			model_provider: self.model_provider.clone(),
			cwd: self.cwd.clone(),
			approval_policy: externally_tagged_value_name(&self.approval_policy)
				.unwrap_or_else(|| String::from("unknown")),
			approvals_reviewer: self.approvals_reviewer.clone(),
			sandbox_mode: externally_tagged_value_name(&self.sandbox)
				.unwrap_or_else(|| String::from("unknown")),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::app_server) struct EffectiveThreadConfig {
	pub(in crate::agent::app_server) model: String,
	pub(in crate::agent::app_server) model_provider: String,
	pub(in crate::agent::app_server) cwd: String,
	pub(in crate::agent::app_server) approval_policy: String,
	pub(in crate::agent::app_server) approvals_reviewer: String,
	pub(in crate::agent::app_server) sandbox_mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(in crate::agent::app_server) struct Thread {
	pub(in crate::agent::app_server) id: String,
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct SkillsListParams {
	pub(in crate::agent::app_server) cwds: Vec<String>,
	pub(in crate::agent::app_server) force_reload: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) per_cwd_extra_user_roots: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct SkillsListResponse {
	pub(in crate::agent::app_server) data: Vec<SkillsListEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillsListEntry {
	pub(in crate::agent::app_server) cwd: String,
	pub(in crate::agent::app_server) errors: Vec<SkillErrorInfo>,
	pub(in crate::agent::app_server) skills: Vec<SkillMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillErrorInfo {
	pub(in crate::agent::app_server) message: String,
	pub(in crate::agent::app_server) path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillMetadata {
	pub(in crate::agent::app_server) enabled: bool,
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) scope: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct PluginListParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwds: Option<Vec<String>>,
	#[serde(rename = "marketplaceKinds", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) marketplace_kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct PluginListResponse {
	pub(in crate::agent::app_server) marketplaces: Vec<PluginMarketplaceEntry>,
	#[serde(default, rename = "marketplaceLoadErrors")]
	pub(in crate::agent::app_server) marketplace_load_errors: Vec<MarketplaceLoadErrorInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct PluginMarketplaceEntry {
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) plugins: Vec<PluginSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct PluginSummary {
	pub(in crate::agent::app_server) enabled: bool,
	pub(in crate::agent::app_server) id: String,
	pub(in crate::agent::app_server) installed: bool,
	pub(in crate::agent::app_server) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct MarketplaceLoadErrorInfo {
	#[serde(rename = "marketplacePath")]
	pub(in crate::agent::app_server) marketplace_path: String,
	pub(in crate::agent::app_server) message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ListMcpServerStatusParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cursor: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) detail: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ListMcpServerStatusResponse {
	pub(in crate::agent::app_server) data: Vec<McpServerStatusSummary>,
	#[serde(rename = "nextCursor")]
	pub(in crate::agent::app_server) next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct McpServerStatusSummary {
	#[serde(rename = "authStatus")]
	pub(in crate::agent::app_server) auth_status: String,
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) tools: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnStatusPayload {
	pub(in crate::agent::app_server) id: String,
	pub(in crate::agent::app_server) status: String,
	pub(in crate::agent::app_server) error: Option<TurnError>,
}

#[derive(Debug)]
pub(in crate::agent::app_server) struct TurnError {
	pub(in crate::agent::app_server) message: String,
	pub(in crate::agent::app_server) codex_error_info: Option<String>,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) additional_details: Option<Value>,
}
impl<'de> Deserialize<'de> for TurnError {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = Value::deserialize(deserializer)?;
		let entries = value
			.as_object()
			.ok_or_else(|| Error::custom("expected app-server turn error object"))?;
		let message = entries
			.get("message")
			.and_then(string_like_json_value)
			.ok_or_else(|| Error::custom("expected app-server turn error message"))?;
		let codex_error_info = entries
			.get("codexErrorInfo")
			.or_else(|| entries.get("codex_error_info"))
			.and_then(string_like_json_value);
		let additional_details =
			entries.get("additionalDetails").or_else(|| entries.get("additional_details")).cloned();

		Ok(Self { message, codex_error_info, additional_details })
	}
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ChatgptAuthTokensRefreshParams {
	pub(in crate::agent::app_server) reason: Option<String>,
	pub(in crate::agent::app_server) previous_account_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ChatgptAuthTokensRefreshResponse {
	pub(in crate::agent::app_server) access_token: String,
	pub(in crate::agent::app_server) chatgpt_account_id: String,
	pub(in crate::agent::app_server) chatgpt_plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadStatusChangedNotification {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) status: ThreadStatus,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadStatus {
	#[serde(rename = "type")]
	pub(in crate::agent::app_server) kind: String,
	#[serde(default, rename = "activeFlags")]
	pub(in crate::agent::app_server) active_flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct AgentMessageDeltaNotification {
	pub(in crate::agent::app_server) delta: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ItemCompletedNotification {
	pub(in crate::agent::app_server) item: CompletedItem,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct CompletedItem {
	#[serde(rename = "type")]
	pub(in crate::agent::app_server) kind: String,
	pub(in crate::agent::app_server) text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnCompletedNotification {
	#[allow(dead_code)]
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: Option<String>,
	pub(in crate::agent::app_server) turn: TurnStatusPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalUpdatedNotification {
	pub(in crate::agent::app_server) goal: ThreadGoal,
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) turn_id: Option<String>,
}

#[derive(Debug)]
pub(in crate::agent::app_server) struct ErrorNotification {
	pub(in crate::agent::app_server) error: TurnError,
	pub(in crate::agent::app_server) will_retry: Option<bool>,
	pub(in crate::agent::app_server) thread_id: Option<String>,
	pub(in crate::agent::app_server) turn_id: Option<String>,
}
impl<'de> Deserialize<'de> for ErrorNotification {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = Value::deserialize(deserializer)?;
		let entries = value
			.as_object()
			.ok_or_else(|| Error::custom("expected app-server error notification object"))?;
		let error_value = entries
			.get("error")
			.ok_or_else(|| Error::custom("expected app-server error notification error"))?;
		let error = TurnError::deserialize(error_value.clone()).map_err(Error::custom)?;
		let will_retry =
			entries.get("willRetry").or_else(|| entries.get("will_retry")).and_then(Value::as_bool);
		let thread_id = entries
			.get("threadId")
			.or_else(|| entries.get("thread_id"))
			.and_then(string_like_json_value);
		let turn_id = entries
			.get("turnId")
			.or_else(|| entries.get("turn_id"))
			.and_then(string_like_json_value);

		Ok(Self { error, will_retry, thread_id, turn_id })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum ThreadGoalStatus {
	Active,
	Paused,
	Blocked,
	UsageLimited,
	BudgetLimited,
	Complete,
}
impl ThreadGoalStatus {
	pub(in crate::agent::app_server) const fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::Paused => "paused",
			Self::Blocked => "blocked",
			Self::UsageLimited => "usageLimited",
			Self::BudgetLimited => "budgetLimited",
			Self::Complete => "complete",
		}
	}
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum LoginAccountParams {
	#[serde(rename = "chatgptAuthTokens", rename_all = "camelCase")]
	ChatgptAuthTokens {
		access_token: String,
		chatgpt_account_id: String,
		#[serde(skip_serializing_if = "Option::is_none")]
		chatgpt_plan_type: Option<String>,
	},
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum LoginAccountResponse {
	#[serde(rename = "chatgptAuthTokens")]
	ChatgptAuthTokens {},
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum CommandExecutionApprovalDecision {
	Decline,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum FileChangeApprovalDecision {
	Decline,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum McpServerElicitationAction {
	Decline,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum PermissionGrantScope {
	#[default]
	Turn,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum UserInput {
	#[serde(rename = "text")]
	Text { text: String },
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

fn string_like_json_value(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		Value::Object(entries) => {
			["message", "text", "id", "codexErrorInfo", "type", "kind", "code", "reason", "name"]
				.iter()
				.find_map(|key| entries.get(*key).and_then(string_like_json_value))
				.or_else(|| {
					(entries.len() == 1)
						.then(|| entries.values().next().and_then(string_like_json_value))
						.flatten()
				})
		},
		Value::Array(items) => items.iter().find_map(string_like_json_value),
		_ => None,
	}
}

fn externally_tagged_value_name(value: &Value) -> Option<String> {
	match value {
		Value::String(value) if !value.is_empty() => Some(value.clone()),
		Value::Object(object) => object
			.get("type")
			.and_then(Value::as_str)
			.map(str::to_owned)
			.or_else(|| (object.len() == 1).then(|| object.keys().next().cloned()).flatten()),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn externally_tagged_values_prefer_explicit_type_field() {
		assert_eq!(
			super::externally_tagged_value_name(&serde_json::json!({ "type": "dangerFullAccess" })),
			Some(String::from("dangerFullAccess"))
		);
	}

	#[test]
	fn error_notifications_keep_codex_error_info_without_retry_flag() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": "usage limit exceeded",
				"codexErrorInfo": "usageLimitExceeded"
			},
			"threadId": "thread-1",
			"turnId": "turn-1"
		}))
		.expect("error notification should parse");

		assert_eq!(notification.error.codex_error_info.as_deref(), Some("usageLimitExceeded"));
		assert_eq!(notification.will_retry, None);
	}

	#[test]
	fn error_notifications_stringify_structured_error_fields() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": {
					"kind": "protocolFailure",
					"detail": "unexpected response"
				},
				"codexErrorInfo": {
					"type": "appServerProtocolMismatch"
				}
			},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": false
		}))
		.expect("structured error notification should parse");

		assert!(notification.error.message.contains("protocolFailure"));
		assert!(
			notification
				.error
				.codex_error_info
				.as_deref()
				.is_some_and(|value| value.contains("appServerProtocolMismatch"))
		);
		assert_eq!(notification.will_retry, Some(false));
	}

	#[test]
	fn error_notifications_accept_structured_string_fields() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": {
					"type": "streamDisconnected",
					"message": "stream disconnected"
				},
				"codexErrorInfo": {
					"type": "transientNetworkError"
				}
			},
			"threadId": { "id": "thread-1" },
			"turnId": { "id": "turn-1" },
			"willRetry": true
		}))
		.expect("structured error notification should parse");

		assert_eq!(notification.error.message, "stream disconnected");
		assert_eq!(notification.error.codex_error_info.as_deref(), Some("transientNetworkError"));
		assert_eq!(notification.thread_id.as_deref(), Some("thread-1"));
		assert_eq!(notification.turn_id.as_deref(), Some("turn-1"));
		assert_eq!(notification.will_retry, Some(true));
	}

	#[test]
	fn chatgpt_auth_tokens_login_uses_app_server_protocol_shape() {
		let value = serde_json::to_value(super::LoginAccountParams::ChatgptAuthTokens {
			access_token: String::from("access"),
			chatgpt_account_id: String::from("acct_1"),
			chatgpt_plan_type: Some(String::from("pro")),
		})
		.expect("login params should serialize");

		assert_eq!(
			value,
			serde_json::json!({
				"type": "chatgptAuthTokens",
				"accessToken": "access",
				"chatgptAccountId": "acct_1",
				"chatgptPlanType": "pro"
			})
		);
	}

	#[test]
	fn command_exec_request_timeout_includes_process_timeout() {
		let params = super::CommandExecParams {
			command: vec![String::from("/bin/sh")],
			cwd: None,
			timeout_ms: Some(1_000),
			output_bytes_cap: Some(128),
		};

		assert_eq!(
			params.request_timeout(),
			std::time::Duration::from_millis(1_000) + super::REQUEST_TIMEOUT
		);
	}
}
