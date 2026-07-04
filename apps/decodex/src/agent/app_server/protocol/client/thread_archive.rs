use crate::{
	agent::app_server::{
		REQUEST_TIMEOUT,
		protocol::{AppServerClient, ThreadArchiveRequest, ThreadArchiveResponse},
	},
	prelude::Result,
};

impl AppServerClient {
	pub(in crate::agent::app_server) fn archive_thread(
		&mut self,
		params: ThreadArchiveRequest,
	) -> Result<ThreadArchiveResponse> {
		self.connection.request("thread/archive", &params, REQUEST_TIMEOUT)
	}
}
