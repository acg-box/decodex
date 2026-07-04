use crate::{
	agent::app_server::protocol::{AppServerClient, CommandExecParams, CommandExecResponse},
	prelude::Result,
};

impl AppServerClient {
	pub(in crate::agent::app_server) fn command_exec(
		&mut self,
		params: &CommandExecParams,
	) -> Result<CommandExecResponse> {
		self.connection.request("command/exec", params, params.request_timeout())
	}
}
