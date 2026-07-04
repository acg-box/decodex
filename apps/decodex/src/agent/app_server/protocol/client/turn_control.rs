use serde_json::Value;

use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT,
			protocol::{
				AppServerClient, TurnInterruptRequest, TurnSteerRequest, TurnSteerResponse,
			},
		},
		json_rpc::{JsonRpcConnection, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

impl AppServerClient {
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
