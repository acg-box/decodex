use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT,
			protocol::{AppServerClient, TurnStartRequest, TurnStartResponse},
		},
		json_rpc::{JsonRpcConnection, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

impl AppServerClient {
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
}
