use crate::{
	agent::{
		app_server::{
			THREAD_SESSION_REQUEST_TIMEOUT,
			protocol::{
				AppServerClient, ThreadResumeRequest, ThreadSessionResponse, ThreadStartRequest,
			},
		},
		json_rpc::{JsonRpcConnection, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

impl AppServerClient {
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
}
