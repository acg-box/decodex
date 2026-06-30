use std::{
	collections::VecDeque,
	env,
	ffi::OsString,
	fmt::{self, Display, Formatter},
	io::{self, BufRead as _, BufReader, Write as _},
	path::{Path, PathBuf},
	process::{Child, ChildStdin, Command, Stdio},
	sync::{
		Arc, Mutex,
		mpsc::{self, Receiver, RecvTimeoutError},
	},
	thread,
	time::Duration,
};

use color_eyre::{Report, eyre};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{self, Value};

use crate::git_credentials::{GitCredentialEnvironment, GitSigningConfig};

const APP_SERVER_STDERR_TAIL_LINES: usize = 20;
const CODEX_APP_SERVER_BINARY: &str = "codex";
const CODEX_HOME_ENV_VAR: &str = "CODEX_HOME";
const CODEX_SQLITE_HOME_ENV_VAR: &str = "CODEX_SQLITE_HOME";
const CODEX_HOME_DIR_NAME: &str = ".codex";

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct AppServerProcessEnv {
	git: GitCredentialEnvironment,
	codex_home_policy: AppServerCodexHomePolicy,
}
impl AppServerProcessEnv {
	#[cfg(test)]
	pub(crate) fn with_github_credentials(
		github_token_env_var: String,
		github_token: String,
	) -> Self {
		Self {
			git: GitCredentialEnvironment::with_github_credentials(
				github_token_env_var,
				github_token,
			),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}

	pub(crate) fn with_github_credentials_and_signing_config(
		github_token_env_var: String,
		github_token: String,
		signing_config: GitSigningConfig,
	) -> Self {
		Self {
			git: GitCredentialEnvironment::with_github_credentials_and_signing_config(
				github_token_env_var,
				github_token,
				signing_config,
			),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}

	pub(crate) fn resolve_codex_home_env(
		&self,
	) -> crate::prelude::Result<ResolvedAppServerCodexHomeEnv> {
		match &self.codex_home_policy {
			AppServerCodexHomePolicy::SharedDefault => resolve_shared_codex_home_env(),
			#[cfg(test)]
			AppServerCodexHomePolicy::Explicit(home_env) => Ok(home_env.clone()),
		}
	}

	pub(crate) fn apply_to(
		&self,
		command: &mut Command,
	) -> crate::prelude::Result<ResolvedAppServerCodexHomeEnv> {
		self.git.apply_to(command);

		let codex_home_env = self.resolve_codex_home_env()?;

		codex_home_env.apply_to(command)?;

		Ok(codex_home_env)
	}

	#[cfg(test)]
	fn with_codex_home_for_test(home_env: ResolvedAppServerCodexHomeEnv) -> Self {
		Self {
			git: GitCredentialEnvironment::default(),
			codex_home_policy: AppServerCodexHomePolicy::Explicit(home_env),
		}
	}
}

impl Default for AppServerProcessEnv {
	fn default() -> Self {
		Self {
			git: GitCredentialEnvironment::default(),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedAppServerCodexHomeEnv {
	codex_home: PathBuf,
	sqlite_home: PathBuf,
}
impl ResolvedAppServerCodexHomeEnv {
	pub(crate) fn new(codex_home: PathBuf, sqlite_home: PathBuf) -> crate::prelude::Result<Self> {
		validate_codex_home_path(CODEX_HOME_ENV_VAR, &codex_home)?;
		validate_codex_home_path(CODEX_SQLITE_HOME_ENV_VAR, &sqlite_home)?;

		Ok(Self { codex_home, sqlite_home })
	}

	pub(crate) fn codex_home(&self) -> &Path {
		&self.codex_home
	}

	#[cfg(test)]
	fn sqlite_home(&self) -> &Path {
		&self.sqlite_home
	}

	fn apply_to(&self, command: &mut Command) -> crate::prelude::Result<()> {
		let codex_home = path_env_value(CODEX_HOME_ENV_VAR, &self.codex_home)?;
		let sqlite_home = path_env_value(CODEX_SQLITE_HOME_ENV_VAR, &self.sqlite_home)?;

		command.env_remove(CODEX_HOME_ENV_VAR);
		command.env_remove(CODEX_SQLITE_HOME_ENV_VAR);
		command.env(CODEX_HOME_ENV_VAR, codex_home);
		command.env(CODEX_SQLITE_HOME_ENV_VAR, sqlite_home);

		Ok(())
	}
}

#[derive(Debug)]
pub(crate) struct AppServerHomePreflightFailure {
	details: String,
	kind: AppServerHomePreflightFailureKind,
}
impl AppServerHomePreflightFailure {
	pub(crate) fn resolution_failed(details: String) -> Self {
		Self { details, kind: AppServerHomePreflightFailureKind::ResolutionFailed }
	}

	pub(crate) fn initialize_mismatch(resolved_home: String, expected_home: String) -> Self {
		Self {
			details: format!(
				"app_server_protocol_failure: initialize codexHome `{resolved_home}` did not match expected shared Codex home `{expected_home}`; Decodex blocked dispatch before thread/start so Codex state is not split across homes."
			),
			kind: AppServerHomePreflightFailureKind::InitializeMismatch,
		}
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerHomePreflightFailureKind::ResolutionFailed => {
				"app_server_codex_home_preflight_failed"
			},
			AppServerHomePreflightFailureKind::InitializeMismatch => {
				"app_server_codex_home_mismatch"
			},
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"inspect the local Decodex and Codex home sharing, restart `decodex serve`, {recovery_gate}"
		)
	}
}

impl Display for AppServerHomePreflightFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.details)
	}
}

impl std::error::Error for AppServerHomePreflightFailure {}

#[derive(Debug)]
pub(crate) struct AppServerOutputTimeout;
impl Display for AppServerOutputTimeout {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str("Timed out while waiting for app-server output.")
	}
}

impl std::error::Error for AppServerOutputTimeout {}

#[derive(Debug)]
pub(crate) struct AppServerTransportFailure {
	details: String,
	phase: Option<&'static str>,
	retryable_startup: bool,
}
impl AppServerTransportFailure {
	pub(crate) fn new(details: String) -> Self {
		Self { details, phase: None, retryable_startup: false }
	}

	pub(crate) fn with_phase(
		details: String,
		phase: &'static str,
		retryable_startup: bool,
	) -> Self {
		Self { details, phase: Some(phase), retryable_startup }
	}

	pub(crate) fn error_class(&self) -> &'static str {
		"app_server_transport_disconnected"
	}

	pub(crate) fn is_retryable_startup(&self) -> bool {
		self.retryable_startup
	}

	pub(crate) fn retry_next_action(&self) -> String {
		if let Some(phase) = self.phase {
			format!(
				"app-server transport disconnected during `{phase}` before a durable turn was running; decodex will restart the app-server and retry automatically"
			)
		} else {
			String::from("decodex will retry the app-server transport failure automatically")
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		let phase = self.phase.map_or_else(String::new, |phase| format!(" during `{phase}`"));

		format!(
			"inspect the local app-server stderr tail and process exit status, resolve the Codex app-server transport failure{phase} manually, {recovery_gate}"
		)
	}
}

impl Display for AppServerTransportFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.details)
	}
}

impl std::error::Error for AppServerTransportFailure {}

pub(crate) struct JsonRpcConnection {
	child: Child,
	stdin: ChildStdin,
	stdout_rx: Receiver<String>,
	stderr_tail: Arc<Mutex<VecDeque<String>>>,
	pending_messages: VecDeque<WireMessage>,
	next_request_id: i64,
}
impl JsonRpcConnection {
	pub(crate) fn spawn_app_server(
		listen: &str,
		process_env: &AppServerProcessEnv,
	) -> crate::prelude::Result<Self> {
		let mut command = Command::new(app_server_command_program());
		let _codex_home_env = configure_app_server_command(&mut command, listen, process_env)?;
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
	pub(crate) fn request<P, T>(
		&mut self,
		method: &str,
		params: &P,
		timeout: Duration,
	) -> crate::prelude::Result<T>
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
	) -> crate::prelude::Result<T>
	where
		P: Serialize,
		T: DeserializeOwned,
		F: FnMut(&mut Self, &WireMessage, &JsonRpcRequest) -> crate::prelude::Result<()>,
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

	pub(crate) fn notify<P>(
		&mut self,
		method: &str,
		params: Option<&P>,
	) -> crate::prelude::Result<()>
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

	pub(crate) fn recv(
		&mut self,
		timeout: Option<Duration>,
	) -> crate::prelude::Result<WireMessage> {
		if let Some(message) = self.pending_messages.pop_front() {
			return Ok(message);
		}

		self.read_message(timeout)
	}

	pub(crate) fn respond<R>(&mut self, id: &Value, result: &R) -> crate::prelude::Result<()>
	where
		R: Serialize,
	{
		self.send_value(&serde_json::json!({
			"id": id,
			"result": result,
		}))
	}

	pub(crate) fn respond_error(
		&mut self,
		id: &Value,
		code: i64,
		message: &str,
	) -> crate::prelude::Result<()> {
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

	fn send_value(&mut self, value: &Value) -> crate::prelude::Result<()> {
		let payload = serde_json::to_string(value)?;

		if let Err(error) = writeln!(self.stdin, "{payload}") {
			return Err(self.app_server_stdin_error("write", error));
		}
		if let Err(error) = self.stdin.flush() {
			return Err(self.app_server_stdin_error("flush", error));
		}

		Ok(())
	}

	fn read_message(&mut self, timeout: Option<Duration>) -> crate::prelude::Result<WireMessage> {
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

	fn app_server_stdin_error(&mut self, operation: &str, error: io::Error) -> Report {
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

#[derive(Clone, Debug)]
pub(crate) struct WireMessage {
	pub(crate) raw: String,
	pub(crate) message: JsonRpcMessage,
}
impl WireMessage {
	fn parse(raw: String) -> crate::prelude::Result<Self> {
		let value: Value = serde_json::from_str(&raw)?;
		let message = if value.get("method").is_some() && value.get("id").is_some() {
			JsonRpcMessage::Request(serde_json::from_value(value)?)
		} else if value.get("method").is_some() {
			JsonRpcMessage::Notification(serde_json::from_value(value)?)
		} else if value.get("error").is_some() {
			JsonRpcMessage::Error(serde_json::from_value(value)?)
		} else if value.get("result").is_some() {
			JsonRpcMessage::Response(serde_json::from_value(value)?)
		} else {
			return Err(eyre::eyre!("Received an unrecognized JSON-RPC payload: {raw}"));
		};

		Ok(Self { raw, message })
	}
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
	pub(crate) id: Value,
	pub(crate) method: String,
	#[serde(default)]
	pub(crate) params: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcNotification {
	pub(crate) method: String,
	#[serde(default)]
	pub(crate) params: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcResponse {
	pub(crate) id: Value,
	pub(crate) result: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcError {
	pub(crate) id: Value,
	pub(crate) error: JsonRpcErrorPayload,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcErrorPayload {
	pub(crate) code: i64,
	pub(crate) message: String,

	pub(crate) data: Option<Value>,
}

#[derive(Clone, Debug)]
pub(crate) enum JsonRpcMessage {
	Request(JsonRpcRequest),
	Notification(JsonRpcNotification),
	Response(JsonRpcResponse),
	Error(JsonRpcError),
}

#[derive(Debug)]
enum AppServerHomePreflightFailureKind {
	ResolutionFailed,
	InitializeMismatch,
}

#[derive(Clone, Eq, PartialEq)]
enum AppServerCodexHomePolicy {
	SharedDefault,
	#[cfg(test)]
	Explicit(ResolvedAppServerCodexHomeEnv),
}

pub(crate) fn app_server_command_program() -> PathBuf {
	app_server_command_program_from_env(env::var_os("PATH"), env::var_os("HOME"))
}

fn resolve_shared_codex_home_env() -> crate::prelude::Result<ResolvedAppServerCodexHomeEnv> {
	resolve_shared_codex_home_env_from_home(env::var_os("HOME"))
}

fn resolve_shared_codex_home_env_from_home(
	home: Option<OsString>,
) -> crate::prelude::Result<ResolvedAppServerCodexHomeEnv> {
	let Some(home) = home else {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))));
	};
	let home = PathBuf::from(home);

	if home.as_os_str().is_empty() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is empty, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))));
	}
	if !home.is_absolute() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: HOME `{}` is not absolute, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
			home.display()
		))));
	}

	let codex_home = home.join(CODEX_HOME_DIR_NAME);

	ResolvedAppServerCodexHomeEnv::new(codex_home.clone(), codex_home)
}

fn validate_codex_home_path(name: &str, path: &Path) -> crate::prelude::Result<()> {
	if path.as_os_str().is_empty() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} resolved to an empty path."
		))));
	}
	if !path.is_absolute() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} `{}` is not absolute.",
			path.display()
		))));
	}

	path_env_value(name, path).map(|_| ())
}

fn path_env_value(name: &str, path: &Path) -> crate::prelude::Result<String> {
	path.to_str().map(str::to_owned).ok_or_else(|| {
		Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} `{}` is not valid UTF-8.",
			path.display()
		)))
	})
}

fn configure_app_server_command(
	command: &mut Command,
	listen: &str,
	process_env: &AppServerProcessEnv,
) -> crate::prelude::Result<ResolvedAppServerCodexHomeEnv> {
	command
		.args(["app-server", "--listen", listen])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());

	process_env.apply_to(command)
}

fn app_server_command_program_from_env(
	path_env: Option<OsString>,
	home: Option<OsString>,
) -> PathBuf {
	if let Some(path_env) = path_env {
		for path_entry in env::split_paths(&path_env) {
			let candidate = path_entry.join(CODEX_APP_SERVER_BINARY);

			if candidate.is_file() {
				return candidate;
			}
		}
	}
	if let Some(home) = home {
		let home = PathBuf::from(home);

		for relative_candidate in
			[[".local", "bin", CODEX_APP_SERVER_BINARY], [".cargo", "bin", CODEX_APP_SERVER_BINARY]]
		{
			let candidate = relative_candidate
				.iter()
				.fold(home.clone(), |path, component| path.join(*component));

			if candidate.is_file() {
				return candidate;
			}
		}
	}

	PathBuf::from(CODEX_APP_SERVER_BINARY)
}

#[cfg(test)]
mod tests {
	use std::{
		collections::{HashMap, VecDeque},
		ffi::OsString,
		fs,
		path::PathBuf,
		process::{Command, Stdio},
		sync::{Arc, Mutex, mpsc},
		time::Duration,
	};

	use crate::agent::json_rpc::{
		AppServerHomePreflightFailure, AppServerOutputTimeout, AppServerProcessEnv,
		AppServerTransportFailure, JsonRpcConnection, JsonRpcMessage,
		ResolvedAppServerCodexHomeEnv, WireMessage,
	};

	#[test]
	fn parses_notification_messages() {
		let message = WireMessage::parse(
			r#"{"method":"thread/status/changed","params":{"threadId":"thread-1"}}"#.to_owned(),
		)
		.expect("notification should parse");

		match message.message {
			JsonRpcMessage::Notification(notification) => {
				assert_eq!(notification.method, "thread/status/changed");
				assert_eq!(notification.params["threadId"], serde_json::json!("thread-1"));
			},
			other => panic!("unexpected message: {other:?}"),
		}
	}

	#[test]
	fn parses_response_messages() {
		let message =
			WireMessage::parse(r#"{"id":1,"result":{"userAgent":"decodex-test"}}"#.to_owned())
				.expect("response should parse");

		match message.message {
			JsonRpcMessage::Response(response) => {
				assert_eq!(response.id, serde_json::json!(1));
				assert_eq!(response.result["userAgent"], serde_json::json!("decodex-test"));
			},
			other => panic!("unexpected message: {other:?}"),
		}
	}

	#[test]
	fn request_wait_ignores_orphan_response_before_expected_response() {
		let mut connection = test_connection_with_messages([
			r#"{"id":99,"result":{"late":true}}"#,
			r#"{"id":1,"result":{"ok":true}}"#,
		]);
		let response: serde_json::Value = connection
			.request_with_handler(
				"thread/start",
				&serde_json::json!({}),
				Duration::from_secs(1),
				|_, _, _| Ok(()),
			)
			.expect("orphan response should not fail the pending request");

		assert_eq!(response, serde_json::json!({"ok": true}));
	}

	#[test]
	fn app_server_command_inherits_noninteractive_git_environment() {
		let process_env = AppServerProcessEnv::with_github_credentials(
			String::from("GITHUB_PAT_Y"),
			String::from("ghp_test_token"),
		);
		let mut command = Command::new("codex");

		super::configure_app_server_command(&mut command, "stdio://", &process_env)
			.expect("app-server command should configure");

		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
		let envs = command
			.get_envs()
			.filter_map(|(key, value)| {
				Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
			})
			.collect::<HashMap<_, _>>();

		assert_eq!(args, ["app-server", "--listen", "stdio://"]);
		assert_eq!(envs.get("GH_TOKEN").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GITHUB_TOKEN").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GITHUB_PAT_Y").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
		assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
		assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
		assert!(!envs.contains_key("GIT_ASKPASS"));
		assert_eq!(envs.get("GIT_CONFIG_COUNT").map(String::as_str), Some("11"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_0").map(String::as_str), Some("credential.helper"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_0").map(String::as_str), Some(""));
		assert_eq!(envs.get("GIT_CONFIG_KEY_1").map(String::as_str), Some("credential.helper"));
		assert!(
			envs.get("GIT_CONFIG_VALUE_1").is_some_and(
				|value| value.contains("github.com") && value.contains("x-access-token")
			)
		);
		assert_eq!(
			envs.get("GIT_CONFIG_KEY_2").map(String::as_str),
			Some("url.https://github.com/.insteadOf")
		);
		assert_eq!(envs.get("GIT_CONFIG_VALUE_2").map(String::as_str), Some("git@github.com:"));
		assert_eq!(
			envs.get("GIT_CONFIG_KEY_7").map(String::as_str),
			Some("url.https://github.com/.insteadOf")
		);
		assert_eq!(
			envs.get("GIT_CONFIG_VALUE_7").map(String::as_str),
			Some("ssh://git@github.com-y/")
		);
		assert_eq!(envs.get("GIT_CONFIG_KEY_8").map(String::as_str), Some("commit.gpgsign"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_8").map(String::as_str), Some("false"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_9").map(String::as_str), Some("tag.gpgsign"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_9").map(String::as_str), Some("false"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_10").map(String::as_str), Some("user.signingkey"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_10").map(String::as_str), Some(""));
	}

	#[test]
	fn app_server_program_falls_back_to_home_local_codex_when_path_is_sparse() {
		let temp_dir = tempfile::tempdir().expect("tempdir should create");
		let local_bin = temp_dir.path().join(".local/bin");

		fs::create_dir_all(&local_bin).expect("local bin should create");

		let codex_path = local_bin.join("codex");

		fs::write(&codex_path, "#!/bin/sh\n").expect("codex fixture should write");

		let resolved = super::app_server_command_program_from_env(
			Some(OsString::from("/usr/bin:/bin")),
			Some(temp_dir.path().as_os_str().to_owned()),
		);

		assert_eq!(resolved, codex_path);
	}

	#[test]
	fn app_server_command_does_not_rewrite_git_urls_without_credentials() {
		let mut command = Command::new("codex");

		super::configure_app_server_command(
			&mut command,
			"stdio://",
			&AppServerProcessEnv::default(),
		)
		.expect("app-server command should configure");

		let envs = command
			.get_envs()
			.filter_map(|(key, value)| {
				Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
			})
			.collect::<HashMap<_, _>>();

		assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
		assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
		assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
		assert!(!envs.contains_key("GIT_CONFIG_COUNT"));
		assert!(!envs.keys().any(|key| key.starts_with("GIT_CONFIG_KEY_")));
		assert!(!envs.keys().any(|key| key.starts_with("GIT_CONFIG_VALUE_")));
	}

	#[test]
	fn app_server_command_sets_shared_codex_homes() {
		let shared_home = PathBuf::from("/Users/test/.codex");
		let home_env = ResolvedAppServerCodexHomeEnv::new(shared_home.clone(), shared_home.clone())
			.expect("test home should validate");
		let process_env = AppServerProcessEnv::with_codex_home_for_test(home_env);
		let mut command = Command::new("codex");
		let resolved = super::configure_app_server_command(&mut command, "stdio://", &process_env)
			.expect("app-server command should configure");
		let envs = command
			.get_envs()
			.filter_map(|(key, value)| {
				Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
			})
			.collect::<HashMap<_, _>>();

		assert_eq!(resolved.codex_home(), shared_home.as_path());
		assert_eq!(envs.get("CODEX_HOME").map(String::as_str), Some("/Users/test/.codex"));
		assert_eq!(envs.get("CODEX_SQLITE_HOME").map(String::as_str), Some("/Users/test/.codex"));
	}

	#[test]
	fn shared_codex_home_resolution_uses_home_dot_codex_for_state() {
		let resolved =
			super::resolve_shared_codex_home_env_from_home(Some(OsString::from("/Users/test")))
				.expect("absolute HOME should resolve");

		assert_eq!(resolved.codex_home(), PathBuf::from("/Users/test/.codex").as_path());
		assert_eq!(resolved.sqlite_home(), PathBuf::from("/Users/test/.codex").as_path());
	}

	#[test]
	fn app_server_command_overrides_ambient_codex_home_leakage() {
		let shared_home = PathBuf::from("/Users/test/.codex");
		let home_env = ResolvedAppServerCodexHomeEnv::new(shared_home.clone(), shared_home)
			.expect("test home should validate");
		let process_env = AppServerProcessEnv::with_codex_home_for_test(home_env);
		let mut command = Command::new("codex");

		command.env("CODEX_HOME", "/tmp/per-account-codex-home");
		command.env("CODEX_SQLITE_HOME", "/tmp/per-account-codex-state");

		super::configure_app_server_command(&mut command, "stdio://", &process_env)
			.expect("app-server command should configure");

		let envs = command
			.get_envs()
			.filter_map(|(key, value)| {
				Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
			})
			.collect::<HashMap<_, _>>();

		assert_eq!(envs.get("CODEX_HOME").map(String::as_str), Some("/Users/test/.codex"));
		assert_eq!(envs.get("CODEX_SQLITE_HOME").map(String::as_str), Some("/Users/test/.codex"));
	}

	#[test]
	fn shared_codex_home_resolution_requires_home() {
		let error = super::resolve_shared_codex_home_env_from_home(None)
			.expect_err("missing HOME should fail");

		assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
		assert!(error.to_string().contains("HOME is not set"));
	}

	#[test]
	fn shared_codex_home_resolution_rejects_invalid_home() {
		for (case_name, home, expected) in [
			("empty", OsString::from(""), "HOME is empty"),
			("relative", OsString::from("relative-home"), "is not absolute"),
		] {
			let error =
				super::resolve_shared_codex_home_env_from_home(Some(home)).expect_err(case_name);

			assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
			assert!(
				error.to_string().contains(expected),
				"unexpected error for {case_name}: {error:?}"
			);
		}
	}

	#[test]
	fn stdin_write_failures_classify_as_app_server_transport_failures() {
		let mut child = Command::new("sh")
			.args(["-c", "exit 17"])
			.stdin(Stdio::piped())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("child process should spawn");
		let stdin = child.stdin.take().expect("child stdin should be captured");
		let _status = child.wait().expect("child should exit");
		let (_stdout_tx, stdout_rx) = mpsc::channel();
		let stderr_tail =
			Arc::new(Mutex::new(VecDeque::from([String::from("fatal app-server transport test")])));
		let mut connection = JsonRpcConnection {
			child,
			stdin,
			stdout_rx,
			stderr_tail,
			pending_messages: VecDeque::new(),
			next_request_id: 1,
		};
		let error = connection
			.notify::<serde_json::Value>("thread/test", None)
			.expect_err("closed stdin should fail as transport");

		assert!(error.downcast_ref::<AppServerTransportFailure>().is_some());
		assert!(error.to_string().contains("App-server stdin write failed"));
		assert!(error.to_string().contains("fatal app-server transport test"));
	}

	#[test]
	fn output_timeouts_downcast_to_timeout_class() {
		let error = color_eyre::Report::new(AppServerOutputTimeout);

		assert!(error.downcast_ref::<AppServerOutputTimeout>().is_some());
		assert_eq!(error.to_string(), "Timed out while waiting for app-server output.");
	}

	#[test]
	fn wrapped_transport_failures_still_downcast_to_transport_class() {
		let error = color_eyre::Report::new(AppServerTransportFailure::new(String::from(
			"App-server stdout disconnected unexpectedly.",
		)))
		.wrap_err("outer context");

		assert!(error.downcast_ref::<AppServerTransportFailure>().is_some());
	}

	fn test_connection_with_messages<const N: usize>(messages: [&str; N]) -> JsonRpcConnection {
		let mut child = Command::new("sh")
			.args(["-c", "cat >/dev/null"])
			.stdin(Stdio::piped())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("child process should spawn");
		let stdin = child.stdin.take().expect("child stdin should be captured");
		let (stdout_tx, stdout_rx) = mpsc::channel();

		for message in messages {
			stdout_tx.send(message.to_owned()).expect("test message should send");
		}

		JsonRpcConnection {
			child,
			stdin,
			stdout_rx,
			stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
			pending_messages: VecDeque::new(),
			next_request_id: 1,
		}
	}
}
