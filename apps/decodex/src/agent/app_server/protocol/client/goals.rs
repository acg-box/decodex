use crate::{
	agent::app_server::{
		REQUEST_TIMEOUT,
		protocol::{
			AppServerClient, ThreadGoalClearParams, ThreadGoalClearResponse, ThreadGoalGetParams,
			ThreadGoalGetResponse, ThreadGoalSetParams, ThreadGoalSetResponse,
		},
	},
	prelude::Result,
};

impl AppServerClient {
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
