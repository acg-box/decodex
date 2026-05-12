use std::{
	collections::{BTreeMap, HashMap},
	env,
	time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	agent::{
		app_server::REQUEST_TIMEOUT,
		json_rpc::{AppServerProcessEnv, JsonRpcConnection, JsonRpcRequest, WireMessage},
		tracker_tool_bridge::{DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec},
	},
	prelude::Result,
};

pub(super) struct AppServerClient {
	pub(super) connection: JsonRpcConnection,
}
impl AppServerClient {
	pub(super) fn spawn(listen: &str, process_env: &AppServerProcessEnv) -> Result<Self> {
		Ok(Self { connection: JsonRpcConnection::spawn_app_server(listen, process_env)? })
	}

	#[allow(dead_code)]
	pub(super) fn initialize(
		&mut self,
		enable_experimental_api: bool,
	) -> Result<InitializeResponse> {
		self.initialize_with_handler(enable_experimental_api, |_connection, _message, request| {
			color_eyre::eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `initialize`.",
				request.method
			);
		})
	}

	pub(super) fn initialize_with_handler<H>(
		&mut self,
		enable_experimental_api: bool,
		handler: H,
	) -> Result<InitializeResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler(
			"initialize",
			&InitializeParams {
				client_info: ClientInfo {
					name: env!("CARGO_PKG_NAME").to_owned(),
					version: env!("CARGO_PKG_VERSION").to_owned(),
				},
				capabilities: enable_experimental_api.then_some(InitializeCapabilities {
					experimental_api: Some(true),
					opt_out_notification_methods: Vec::new(),
				}),
			},
			REQUEST_TIMEOUT,
			handler,
		)
	}

	pub(super) fn mark_initialized(&mut self) -> Result<()> {
		self.connection.notify::<Value>("initialized", None)
	}

	pub(super) fn login_account_with_handler<H>(
		&mut self,
		params: LoginAccountParams,
		handler: H,
	) -> Result<LoginAccountResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler(
			"account/login/start",
			&params,
			REQUEST_TIMEOUT,
			handler,
		)
	}

	#[allow(dead_code)]
	pub(super) fn start_thread(
		&mut self,
		params: ThreadStartRequest,
	) -> Result<ThreadSessionResponse> {
		self.start_thread_with_handler(params, |_connection, _message, request| {
			color_eyre::eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `thread/start`.",
				request.method
			);
		})
	}

	pub(super) fn start_thread_with_handler<H>(
		&mut self,
		params: ThreadStartRequest,
		handler: H,
	) -> Result<ThreadSessionResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("thread/start", &params, REQUEST_TIMEOUT, handler)
	}

	#[allow(dead_code)]
	pub(super) fn resume_thread(
		&mut self,
		params: ThreadResumeRequest,
	) -> Result<ThreadSessionResponse> {
		self.resume_thread_with_handler(params, |_connection, _message, request| {
			color_eyre::eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `thread/resume`.",
				request.method
			);
		})
	}

	pub(super) fn resume_thread_with_handler<H>(
		&mut self,
		params: ThreadResumeRequest,
		handler: H,
	) -> Result<ThreadSessionResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("thread/resume", &params, REQUEST_TIMEOUT, handler)
	}

	#[allow(dead_code)]
	pub(super) fn start_turn(&mut self, params: TurnStartRequest) -> Result<TurnStartResponse> {
		self.start_turn_with_handler(params, |_connection, _message, request| {
			color_eyre::eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `turn/start`.",
				request.method
			);
		})
	}

	pub(super) fn start_turn_with_handler<H>(
		&mut self,
		params: TurnStartRequest,
		handler: H,
	) -> Result<TurnStartResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("turn/start", &params, REQUEST_TIMEOUT, handler)
	}

	pub(super) fn command_exec(
		&mut self,
		params: &CommandExecParams,
	) -> Result<CommandExecResponse> {
		self.connection.request("command/exec", params, params.request_timeout())
	}

	pub(super) fn read_config(&mut self, params: &ConfigReadParams) -> Result<ConfigReadResponse> {
		self.connection.request("config/read", params, REQUEST_TIMEOUT)
	}

	pub(super) fn list_models(&mut self, params: &ModelListParams) -> Result<ModelListResponse> {
		self.connection.request("model/list", params, REQUEST_TIMEOUT)
	}

	pub(super) fn read_model_provider_capabilities(
		&mut self,
	) -> Result<ModelProviderCapabilitiesReadResponse> {
		self.connection.request(
			"modelProvider/capabilities/read",
			&ModelProviderCapabilitiesReadParams {},
			REQUEST_TIMEOUT,
		)
	}

	pub(super) fn list_skills(&mut self, params: &SkillsListParams) -> Result<SkillsListResponse> {
		self.connection.request("skills/list", params, REQUEST_TIMEOUT)
	}

	pub(super) fn list_plugins(&mut self, params: &PluginListParams) -> Result<PluginListResponse> {
		self.connection.request("plugin/list", params, REQUEST_TIMEOUT)
	}

	pub(super) fn list_mcp_server_status(
		&mut self,
		params: &ListMcpServerStatusParams,
		timeout: Duration,
	) -> Result<ListMcpServerStatusResponse> {
		self.connection.request("mcpServerStatus/list", params, timeout)
	}

	pub(super) fn recv(&mut self, timeout: Option<Duration>) -> Result<WireMessage> {
		self.connection.recv(timeout)
	}

	#[allow(dead_code)]
	pub(super) fn respond<R>(&mut self, id: &Value, result: &R) -> Result<()>
	where
		R: Serialize,
	{
		self.connection.respond(id, result)
	}

	#[allow(dead_code)]
	pub(super) fn respond_error(&mut self, id: &Value, code: i64, message: &str) -> Result<()> {
		self.connection.respond_error(id, code, message)
	}

	pub(super) fn drain_pending(&mut self) -> Vec<WireMessage> {
		self.connection.drain_pending()
	}
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub(super) enum LoginAccountParams {
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
pub(super) enum LoginAccountResponse {
	#[serde(rename = "chatgptAuthTokens")]
	ChatgptAuthTokens {},
}

#[derive(Default)]
pub(super) struct RunOutcome {
	pub(super) final_output: String,
}

#[derive(Debug, Serialize)]
pub(super) struct InitializeParams {
	#[serde(rename = "clientInfo")]
	pub(super) client_info: ClientInfo,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) capabilities: Option<InitializeCapabilities>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClientInfo {
	pub(super) name: String,
	pub(super) version: String,
}

#[derive(Debug, Serialize)]
pub(super) struct InitializeCapabilities {
	#[serde(rename = "experimentalApi", skip_serializing_if = "Option::is_none")]
	pub(super) experimental_api: Option<bool>,
	#[serde(default, rename = "optOutNotificationMethods", skip_serializing_if = "Vec::is_empty")]
	pub(super) opt_out_notification_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct InitializeResponse {
	#[serde(rename = "userAgent")]
	pub(super) user_agent: String,
	#[allow(dead_code)]
	#[serde(rename = "codexHome")]
	pub(super) codex_home: String,
	#[allow(dead_code)]
	#[serde(rename = "platformFamily")]
	pub(super) platform_family: String,
	#[allow(dead_code)]
	#[serde(rename = "platformOs")]
	pub(super) platform_os: String,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct ThreadStartRequest {
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(super) base_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) config: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cwd: Option<String>,
	#[serde(rename = "dynamicTools", skip_serializing_if = "Option::is_none")]
	pub(super) dynamic_tools: Option<Vec<DynamicToolSpec>>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(super) developer_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) ephemeral: Option<bool>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(super) model_provider: Option<String>,
	#[serde(rename = "serviceName", skip_serializing_if = "Option::is_none")]
	pub(super) service_name: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct ThreadResumeRequest {
	#[serde(rename = "threadId")]
	pub(super) thread_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) model: Option<String>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(super) model_provider: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cwd: Option<String>,
	#[serde(rename = "approvalPolicy", skip_serializing_if = "Option::is_none")]
	pub(super) approval_policy: Option<Value>,
	#[serde(rename = "approvalsReviewer", skip_serializing_if = "Option::is_none")]
	pub(super) approvals_reviewer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) sandbox: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) config: Option<Value>,
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(super) base_instructions: Option<String>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(super) developer_instructions: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ThreadSessionResponse {
	pub(super) thread: Thread,
	pub(super) model: String,
	#[serde(rename = "modelProvider")]
	pub(super) model_provider: String,
	#[serde(rename = "serviceTier")]
	pub(super) _service_tier: Option<Value>,
	pub(super) cwd: String,
	#[serde(default, rename = "instructionSources")]
	pub(super) _instruction_sources: Vec<String>,
	#[serde(rename = "approvalPolicy")]
	pub(super) approval_policy: Value,
	#[serde(rename = "approvalsReviewer")]
	pub(super) approvals_reviewer: String,
	pub(super) sandbox: Value,
	#[serde(rename = "reasoningEffort")]
	pub(super) _reasoning_effort: Option<String>,
}
impl ThreadSessionResponse {
	pub(super) fn effective_config(&self) -> EffectiveThreadConfig {
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
pub(super) struct EffectiveThreadConfig {
	pub(super) model: String,
	pub(super) model_provider: String,
	pub(super) cwd: String,
	pub(super) approval_policy: String,
	pub(super) approvals_reviewer: String,
	pub(super) sandbox_mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct Thread {
	pub(super) id: String,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct TurnStartRequest {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cwd: Option<String>,
	pub(super) input: Vec<UserInput>,
	#[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
	pub(super) output_schema: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) summary: Option<String>,
	#[serde(rename = "threadId")]
	pub(super) thread_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnStartResponse {
	pub(super) turn: TurnStatusPayload,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandExecParams {
	pub(super) command: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cwd: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) timeout_ms: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) output_bytes_cap: Option<u64>,
}
impl CommandExecParams {
	fn request_timeout(&self) -> Duration {
		self.timeout_ms
			.map(Duration::from_millis)
			.map(|timeout| timeout.saturating_add(REQUEST_TIMEOUT))
			.unwrap_or(REQUEST_TIMEOUT)
	}
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandExecResponse {
	pub(super) exit_code: i32,
	pub(super) stdout: String,
	pub(super) stderr: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigReadParams {
	pub(super) cwd: Option<String>,
	pub(super) include_layers: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ConfigReadResponse {
	pub(super) config: RuntimeConfigSummary,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub(super) struct RuntimeConfigSummary {
	pub(super) model: Option<String>,
	#[serde(rename = "model_provider")]
	pub(super) model_provider: Option<String>,
	pub(super) approval_policy: Option<Value>,
	pub(super) sandbox_mode: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelListParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cursor: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) include_hidden: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelListResponse {
	pub(super) data: Vec<ModelSummary>,
	#[serde(rename = "nextCursor")]
	pub(super) next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct ModelSummary {
	pub(super) id: String,
	pub(super) model: String,
	#[serde(rename = "displayName")]
	pub(super) display_name: String,
	#[serde(rename = "isDefault")]
	pub(super) is_default: bool,
	pub(super) hidden: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ModelProviderCapabilitiesReadParams {}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct ModelProviderCapabilitiesReadResponse {
	#[serde(rename = "imageGeneration")]
	pub(super) image_generation: bool,
	#[serde(rename = "namespaceTools")]
	pub(super) namespace_tools: bool,
	#[serde(rename = "webSearch")]
	pub(super) web_search: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SkillsListParams {
	pub(super) cwds: Vec<String>,
	pub(super) force_reload: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) per_cwd_extra_user_roots: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillsListResponse {
	pub(super) data: Vec<SkillsListEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct SkillsListEntry {
	pub(super) cwd: String,
	pub(super) errors: Vec<SkillErrorInfo>,
	pub(super) skills: Vec<SkillMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct SkillErrorInfo {
	pub(super) message: String,
	pub(super) path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct SkillMetadata {
	pub(super) enabled: bool,
	pub(super) name: String,
	pub(super) scope: String,
}

#[derive(Debug, Serialize)]
pub(super) struct PluginListParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cwds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PluginListResponse {
	pub(super) marketplaces: Vec<PluginMarketplaceEntry>,
	#[serde(default, rename = "marketplaceLoadErrors")]
	pub(super) marketplace_load_errors: Vec<MarketplaceLoadErrorInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct PluginMarketplaceEntry {
	pub(super) name: String,
	pub(super) plugins: Vec<PluginSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct PluginSummary {
	pub(super) enabled: bool,
	pub(super) id: String,
	pub(super) installed: bool,
	pub(super) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(super) struct MarketplaceLoadErrorInfo {
	#[serde(rename = "marketplacePath")]
	pub(super) marketplace_path: String,
	pub(super) message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListMcpServerStatusParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cursor: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) detail: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ListMcpServerStatusResponse {
	pub(super) data: Vec<McpServerStatusSummary>,
	#[serde(rename = "nextCursor")]
	pub(super) next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct McpServerStatusSummary {
	#[serde(rename = "authStatus")]
	pub(super) auth_status: String,
	pub(super) name: String,
	pub(super) tools: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnStatusPayload {
	pub(super) id: String,
	pub(super) status: String,
	pub(super) error: Option<TurnError>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnError {
	pub(super) message: String,
	#[serde(rename = "codexErrorInfo")]
	pub(super) codex_error_info: Option<String>,
	#[allow(dead_code)]
	#[serde(rename = "additionalDetails")]
	pub(super) additional_details: Option<Value>,
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatgptAuthTokensRefreshParams {
	pub(super) reason: Option<String>,
	pub(super) previous_account_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatgptAuthTokensRefreshResponse {
	pub(super) access_token: String,
	pub(super) chatgpt_account_id: String,
	pub(super) chatgpt_plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadStatusChangedNotification {
	#[serde(rename = "threadId")]
	pub(super) thread_id: String,
	pub(super) status: ThreadStatus,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadStatus {
	#[serde(rename = "type")]
	pub(super) kind: String,
	#[serde(default, rename = "activeFlags")]
	pub(super) active_flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentMessageDeltaNotification {
	pub(super) delta: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemCompletedNotification {
	pub(super) item: CompletedItem,
}

#[derive(Debug, Deserialize)]
pub(super) struct CompletedItem {
	#[serde(rename = "type")]
	pub(super) kind: String,
	pub(super) text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnCompletedNotification {
	#[allow(dead_code)]
	#[serde(rename = "threadId")]
	pub(super) thread_id: Option<String>,
	pub(super) turn: TurnStatusPayload,
}

#[derive(Debug, Deserialize)]
pub(super) struct ErrorNotification {
	pub(super) error: TurnError,
	#[serde(rename = "willRetry")]
	pub(super) will_retry: Option<bool>,
	#[serde(rename = "threadId")]
	pub(super) thread_id: Option<String>,
	#[serde(rename = "turnId")]
	pub(super) turn_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct DynamicToolCallParams {
	pub(super) arguments: Value,
	#[serde(rename = "callId")]
	pub(super) call_id: String,
	pub(super) namespace: Option<String>,
	#[serde(rename = "threadId")]
	pub(super) thread_id: String,
	pub(super) tool: String,
	#[serde(rename = "turnId")]
	pub(super) turn_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandExecutionRequestApprovalResponse {
	pub(super) decision: CommandExecutionApprovalDecision,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum CommandExecutionApprovalDecision {
	Decline,
}

#[derive(Debug, Serialize)]
pub(super) struct FileChangeRequestApprovalResponse {
	pub(super) decision: FileChangeApprovalDecision,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum FileChangeApprovalDecision {
	Decline,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolRequestUserInputResponse {
	pub(super) answers: HashMap<String, ToolRequestUserInputAnswer>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolRequestUserInputAnswer {
	pub(super) answers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpServerElicitationRequestResponse {
	pub(super) action: McpServerElicitationAction,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) content: Option<Value>,
	#[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
	pub(super) meta: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum McpServerElicitationAction {
	Decline,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PermissionsRequestApprovalResponse {
	pub(super) permissions: GrantedPermissionProfile,
	pub(super) scope: PermissionGrantScope,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GrantedPermissionProfile {}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum PermissionGrantScope {
	#[default]
	Turn,
}

pub(super) struct ProbeDynamicToolHandler;
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

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum UserInput {
	#[serde(rename = "text")]
	Text { text: String },
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
