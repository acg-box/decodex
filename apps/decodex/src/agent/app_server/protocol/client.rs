mod account;
mod commands;
mod goals;
mod resources;
mod thread_archive;
mod thread_session;
mod turn_control;
mod turn_start;
mod wire;

use std::env;

use serde_json::Value;

use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT,
			protocol::{ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse},
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
}
