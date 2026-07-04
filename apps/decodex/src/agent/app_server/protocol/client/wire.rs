use std::time::Duration;

use crate::{
	agent::{app_server::protocol::AppServerClient, json_rpc::WireMessage},
	prelude::Result,
};

impl AppServerClient {
	pub(in crate::agent::app_server) fn recv(
		&mut self,
		timeout: Option<Duration>,
	) -> Result<WireMessage> {
		self.connection.recv(timeout)
	}

	pub(in crate::agent::app_server) fn drain_pending(&mut self) -> Vec<WireMessage> {
		self.connection.drain_pending()
	}
}
