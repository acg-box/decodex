use std::{
	collections::VecDeque,
	io::{BufRead as _, BufReader, Error, Write as _},
	process::{Child, ChildStdin, Command},
	sync::{
		Arc, Mutex,
		mpsc::{self, Receiver, RecvTimeoutError},
	},
	thread,
	time::Duration,
};

use color_eyre::{Report, eyre};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{self, Value};

use crate::{
	agent::json_rpc::{
		environment::{self, APP_SERVER_STDERR_TAIL_LINES, AppServerProcessEnv},
		errors::{AppServerOutputTimeout, AppServerTransportFailure},
		wire::{JsonRpcMessage, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

pub(crate) struct JsonRpcConnection {
	pub(super) child: Child,
	pub(super) stdin: ChildStdin,
	pub(super) stdout_rx: Receiver<String>,
	pub(super) stderr_tail: Arc<Mutex<VecDeque<String>>>,
	pub(super) pending_messages: VecDeque<WireMessage>,
	pub(super) next_request_id: i64,
}
impl JsonRpcConnection {
	pub(crate) fn spawn_app_server(
		listen: &str,
		process_env: &AppServerProcessEnv,
	) -> Result<Self> {
		let mut command = Command::new(environment::app_server_command_program());
		let _codex_home_env =
			environment::configure_app_server_command(&mut command, listen, process_env)?;
		let mut child = command.spawn()?;
		let stdin =
			child.stdin.take().ok_or_else(|| eyre::eyre!("Failed to capture app-server stdin."))?;
		let stdout = child
			.stdout
			.take()
			.ok_or_else(|| eyre::eyre!("Failed to capture app-server stdout."))?;
		let stderr = child
			.stderr
			.take()
			.ok_or_else(|| eyre::eyre!("Failed to capture app-server stderr."))?;
		let (stdout_tx, stdout_rx) = mpsc::channel();
		let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
		let _stdout_task = thread::spawn(move || {
			let reader = BufReader::new(stdout);

			for line in reader.lines() {
				match line {
					Ok(line) => {
						let line: String = line;

						if line.trim().is_empty() {
							continue;
						}
						if stdout_tx.send(line).is_err() {
							break;
						}
					},
					Err(error) => {
						tracing::warn!(?error, "Failed to read app-server stdout.");

						break;
					},
				}
			}
		});
		let stderr_tail_writer = Arc::clone(&stderr_tail);
		let _stderr_task = thread::spawn(move || {
			let reader = BufReader::new(stderr);

			for line in reader.lines() {
				match line {
					Ok(line) => {
						let line: String = line;
						let trimmed_line = line.trim().to_owned();

						if trimmed_line.is_empty() {
							continue;
						}

						match stderr_tail_writer.lock() {
							Ok(mut tail) => {
								if tail.len() == APP_SERVER_STDERR_TAIL_LINES {
									tail.pop_front();
								}

								tail.push_back(trimmed_line);
							},
							Err(error) => {
								tracing::warn!(?error, "Failed to retain app-server stderr tail.");
							},
						}

						tracing::warn!(stderr = %line, "codex app-server stderr");
					},
					Err(error) => {
						tracing::warn!(?error, "Failed to read app-server stderr.");

						break;
					},
				}
			}
		});

		Ok(Self {
			child,
			stdin,
			stdout_rx,
			stderr_tail,
			pending_messages: VecDeque::new(),
			next_request_id: 1,
		})
	}

	#[allow(dead_code)]
	pub(crate) fn request<P, T>(&mut self, method: &str, params: &P, timeout: Duration) -> Result<T>
	where
		P: Serialize,
		T: DeserializeOwned,
	{
		self.request_with_handler(method, params, timeout, |_connection, _message, request| {
			eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `{method}`.",
				request.method
			);
		})
	}

	pub(crate) fn request_with_handler<P, T, F>(
		&mut self,
		method: &str,
		params: &P,
		timeout: Duration,
		mut handle_request: F,
	) -> Result<T>
	where
		P: Serialize,
		T: DeserializeOwned,
		F: FnMut(&mut Self, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		let request_id = self.next_request_id;
		let expected_id = Value::from(request_id);

		self.next_request_id += 1;

		self.send_value(&serde_json::json!({
			"id": request_id,
			"method": method,
			"params": params,
		}))?;

		loop {
			let wire_message = self.read_message(Some(timeout))?;

			match &wire_message.message {
				JsonRpcMessage::Notification(_) => self.pending_messages.push_back(wire_message),
				JsonRpcMessage::Response(response) if response.id == expected_id => {
					return Ok(serde_json::from_value(response.result.clone())?);
				},
				JsonRpcMessage::Error(error) if error.id == expected_id => {
					let data = error
						.error
						.data
						.as_ref()
						.map_or_else(String::new, |data| format!(" data: {data}"));

					return Err(eyre::eyre!(
						"`{method}` failed with {}: {}{}",
						error.error.code,
						error.error.message,
						data
					));
				},
				JsonRpcMessage::Request(request) => handle_request(self, &wire_message, request)?,
				JsonRpcMessage::Response(response) => {
					tracing::debug!(
						method,
						response_id = %response.id,
						expected_id = %expected_id,
						"Recorded and ignored orphan app-server JSON-RPC response while waiting for request."
					);
				},
				JsonRpcMessage::Error(error) => {
					return Err(eyre::eyre!(
						"Received an unexpected JSON-RPC error while waiting for `{method}`: id {} failed with {}: {}",
						error.id,
						error.error.code,
						error.error.message
					));
				},
			}
		}
	}

	pub(crate) fn notify<P>(&mut self, method: &str, params: Option<&P>) -> Result<()>
	where
		P: Serialize,
	{
		let value = match params {
			Some(params) => serde_json::json!({
				"method": method,
				"params": params,
			}),
			None => serde_json::json!({ "method": method }),
		};

		self.send_value(&value)
	}

	pub(crate) fn recv(&mut self, timeout: Option<Duration>) -> Result<WireMessage> {
		if let Some(message) = self.pending_messages.pop_front() {
			return Ok(message);
		}

		self.read_message(timeout)
	}

	pub(crate) fn respond<R>(&mut self, id: &Value, result: &R) -> Result<()>
	where
		R: Serialize,
	{
		self.send_value(&serde_json::json!({
			"id": id,
			"result": result,
		}))
	}

	pub(crate) fn respond_error(&mut self, id: &Value, code: i64, message: &str) -> Result<()> {
		self.send_value(&serde_json::json!({
			"id": id,
			"error": {
				"code": code,
				"message": message,
			},
		}))
	}

	pub(crate) fn drain_pending(&mut self) -> Vec<WireMessage> {
		self.pending_messages.drain(..).collect()
	}

	fn send_value(&mut self, value: &Value) -> Result<()> {
		let payload = serde_json::to_string(value)?;

		if let Err(error) = writeln!(self.stdin, "{payload}") {
			return Err(self.app_server_stdin_error("write", error));
		}
		if let Err(error) = self.stdin.flush() {
			return Err(self.app_server_stdin_error("flush", error));
		}

		Ok(())
	}

	fn read_message(&mut self, timeout: Option<Duration>) -> Result<WireMessage> {
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

impl Drop for JsonRpcConnection {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}
