use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT,
			protocol::{LoginAccountParams, LoginAccountResponse, client::AppServerClient},
		},
		json_rpc::{JsonRpcConnection, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

impl AppServerClient {
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
}
