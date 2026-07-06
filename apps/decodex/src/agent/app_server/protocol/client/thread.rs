use crate::{
	agent::{
		app_server::{
			REQUEST_TIMEOUT, THREAD_SESSION_REQUEST_TIMEOUT,
			protocol::{
				ThreadArchiveRequest, ThreadArchiveResponse, ThreadGoalClearParams,
				ThreadGoalClearResponse, ThreadGoalGetParams, ThreadGoalGetResponse,
				ThreadGoalSetParams, ThreadGoalSetResponse, ThreadResumeRequest,
				ThreadSessionResponse, ThreadStartRequest, client::AppServerClient,
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
}
