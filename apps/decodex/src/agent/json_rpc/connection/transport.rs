use std::{
	io::{Error, Write as _},
	sync::mpsc::RecvTimeoutError,
	time::Duration,
};

use color_eyre::Report;
use serde_json::Value;

use crate::{
	agent::json_rpc::{
		connection::JsonRpcConnection,
		errors::{AppServerOutputTimeout, AppServerTransportFailure},
		wire::WireMessage,
	},
	prelude::Result,
};

impl JsonRpcConnection {
	pub(in crate::agent::json_rpc::connection) fn send_value(
		&mut self,
		value: &Value,
	) -> Result<()> {
		let payload = serde_json::to_string(value)?;

		if let Err(error) = writeln!(self.stdin, "{payload}") {
			return Err(self.app_server_stdin_error("write", error));
		}
		if let Err(error) = self.stdin.flush() {
			return Err(self.app_server_stdin_error("flush", error));
		}

		Ok(())
	}

	pub(in crate::agent::json_rpc::connection) fn read_message(
		&mut self,
		timeout: Option<Duration>,
	) -> Result<WireMessage> {
		let raw = match timeout {
			Some(timeout) => match self.stdout_rx.recv_timeout(timeout) {
				Ok(raw) => raw,
				Err(RecvTimeoutError::Timeout) => {
					return Err(Report::new(AppServerOutputTimeout));
				},
				Err(RecvTimeoutError::Disconnected) => {
					return Err(self.app_server_disconnect_error());
				},
			},
			None => self.stdout_rx.recv().map_err(|_| self.app_server_disconnect_error())?,
		};

		WireMessage::parse(raw)
	}

	fn app_server_disconnect_error(&mut self) -> Report {
		let details = self.app_server_transport_error_details(
			"App-server stdout disconnected unexpectedly".to_owned(),
		);

		Report::new(AppServerTransportFailure::new(details))
	}

	fn app_server_stdin_error(&mut self, operation: &str, error: Error) -> Report {
		let details = self.app_server_transport_error_details(format!(
			"App-server stdin {operation} failed: {error}"
		));

		Report::new(AppServerTransportFailure::new(details))
	}

	fn app_server_transport_error_details(&mut self, summary: String) -> String {
		let process_status = match self.child.try_wait() {
			Ok(Some(status)) => format!("process exited with `{status}`"),
			Ok(None) => String::from("process was still running"),
			Err(error) => format!("failed to inspect process status: {error}"),
		};
		let stderr_tail = self.stderr_tail_snapshot();
		let mut details = format!("{summary} ({process_status}).");

		if !stderr_tail.is_empty() {
			details.push_str(" Recent app-server stderr tail:");

			for line in stderr_tail {
				details.push_str("\n  ");
				details.push_str(&line);
			}
		}

		details
	}

	fn stderr_tail_snapshot(&self) -> Vec<String> {
		match self.stderr_tail.lock() {
			Ok(tail) => tail.iter().cloned().collect(),
			Err(error) => {
				tracing::warn!(?error, "Failed to read app-server stderr tail.");

				Vec::new()
			},
		}
	}
}
