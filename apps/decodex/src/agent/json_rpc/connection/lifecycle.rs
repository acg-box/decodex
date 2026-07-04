use std::{
	collections::VecDeque,
	io::{BufRead as _, BufReader},
	process::Command,
	sync::{Arc, Mutex, mpsc},
	thread,
};

use color_eyre::eyre;

use crate::{
	agent::json_rpc::{
		connection::JsonRpcConnection,
		environment::{self, APP_SERVER_STDERR_TAIL_LINES, AppServerProcessEnv},
	},
	prelude::Result,
};

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
}

impl Drop for JsonRpcConnection {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}
