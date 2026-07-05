use std::{env, time::Duration};

use serde_json::Value;

use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT, THREAD_SESSION_REQUEST_TIMEOUT,
			protocol::{
				ClientInfo, CommandExecParams, CommandExecResponse, ConfigReadParams,
				ConfigReadResponse, InitializeCapabilities, InitializeParams, InitializeResponse,
				ListMcpServerStatusParams, ListMcpServerStatusResponse, LoginAccountParams,
				LoginAccountResponse, ModelListParams, ModelListResponse,
				ModelProviderCapabilitiesReadParams, ModelProviderCapabilitiesReadResponse,
				PluginListParams, PluginListResponse, SkillsListParams, SkillsListResponse,
				ThreadArchiveRequest, ThreadArchiveResponse, ThreadGoalClearParams,
				ThreadGoalClearResponse, ThreadGoalGetParams, ThreadGoalGetResponse,
				ThreadGoalSetParams, ThreadGoalSetResponse, ThreadResumeRequest,
				ThreadSessionResponse, ThreadStartRequest, TurnInterruptRequest, TurnStartRequest,
				TurnStartResponse, TurnSteerRequest, TurnSteerResponse,
			},
		},
		json_rpc::{AppServerProcessEnv, JsonRpcConnection, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

pub(in crate::agent::app_server) struct AppServerClient {
	pub(in crate::agent::app_server) connection: JsonRpcConnection,
}
impl AppServerClient {
	pub(in crate::agent::app_server) fn spawn(
		listen: &str,
		process_env: &AppServerProcessEnv,
	) -> Result<Self> {
		Ok(Self { connection: JsonRpcConnection::spawn_app_server(listen, process_env)? })
	}

	pub(in crate::agent::app_server) fn initialize(
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

	pub(in crate::agent::app_server) fn initialize_with_handler<H>(
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

	pub(in crate::agent::app_server) fn mark_initialized(&mut self) -> Result<()> {
		self.connection.notify::<Value>("initialized", None)
	}

	pub(in crate::agent::app_server) fn recv(
		&mut self,
		timeout: Option<Duration>,
	) -> Result<WireMessage> {
		self.connection.recv(timeout)
	}

	pub(in crate::agent::app_server) fn drain_pending(&mut self) -> Vec<WireMessage> {
		self.connection.drain_pending()
	}

	pub(in crate::agent::app_server) fn login_account_with_handler<H>(
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

	pub(in crate::agent::app_server) fn command_exec(
		&mut self,
		params: &CommandExecParams,
	) -> Result<CommandExecResponse> {
		self.connection.request("command/exec", params, params.request_timeout())
	}

	pub(in crate::agent::app_server) fn read_config(
		&mut self,
		params: &ConfigReadParams,
	) -> Result<ConfigReadResponse> {
		self.connection.request("config/read", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_models(
		&mut self,
		params: &ModelListParams,
	) -> Result<ModelListResponse> {
		self.connection.request("model/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn read_model_provider_capabilities(
		&mut self,
	) -> Result<ModelProviderCapabilitiesReadResponse> {
		self.connection.request(
			"modelProvider/capabilities/read",
			&ModelProviderCapabilitiesReadParams {},
			REQUEST_TIMEOUT,
		)
	}

	pub(in crate::agent::app_server) fn list_skills(
		&mut self,
		params: &SkillsListParams,
	) -> Result<SkillsListResponse> {
		self.connection.request("skills/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_plugins(
		&mut self,
		params: &PluginListParams,
	) -> Result<PluginListResponse> {
		self.connection.request("plugin/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_mcp_server_status(
		&mut self,
		params: &ListMcpServerStatusParams,
		timeout: Duration,
	) -> Result<ListMcpServerStatusResponse> {
		self.connection.request("mcpServerStatus/list", params, timeout)
	}

	pub(in crate::agent::app_server) fn start_thread_with_handler<H>(
		&mut self,
		params: ThreadStartRequest,
		handler: H,
	) -> Result<ThreadSessionResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler(
			"thread/start",
			&params,
			THREAD_SESSION_REQUEST_TIMEOUT,
			handler,
		)
	}

	pub(in crate::agent::app_server) fn resume_thread_with_handler<H>(
		&mut self,
		params: ThreadResumeRequest,
		handler: H,
	) -> Result<ThreadSessionResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler(
			"thread/resume",
			&params,
			THREAD_SESSION_REQUEST_TIMEOUT,
			handler,
		)
	}

	pub(in crate::agent::app_server) fn archive_thread(
		&mut self,
		params: ThreadArchiveRequest,
	) -> Result<ThreadArchiveResponse> {
		self.connection.request("thread/archive", &params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn set_thread_goal(
		&mut self,
		params: ThreadGoalSetParams,
	) -> Result<ThreadGoalSetResponse> {
		self.connection.request("thread/goal/set", &params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn get_thread_goal(
		&mut self,
		params: ThreadGoalGetParams,
	) -> Result<ThreadGoalGetResponse> {
		self.connection.request("thread/goal/get", &params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn clear_thread_goal(
		&mut self,
		params: ThreadGoalClearParams,
	) -> Result<ThreadGoalClearResponse> {
		self.connection.request("thread/goal/clear", &params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn start_turn_with_handler<H>(
		&mut self,
		params: TurnStartRequest,
		handler: H,
	) -> Result<TurnStartResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("turn/start", &params, REQUEST_TIMEOUT, handler)
	}

	pub(in crate::agent::app_server) fn interrupt_turn_with_handler<H>(
		&mut self,
		params: TurnInterruptRequest,
		handler: H,
	) -> Result<Value>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("turn/interrupt", &params, REQUEST_TIMEOUT, handler)
	}

	pub(in crate::agent::app_server) fn steer_turn_with_handler<H>(
		&mut self,
		params: TurnSteerRequest,
		handler: H,
	) -> Result<TurnSteerResponse>
	where
		H: FnMut(&mut JsonRpcConnection, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		self.connection.request_with_handler("turn/steer", &params, REQUEST_TIMEOUT, handler)
	}
}
