mod connection;
mod environment;
mod errors;
mod wire;

#[cfg(test)] pub(crate) use self::wire::JsonRpcErrorPayload;
pub(crate) use self::{
	connection::JsonRpcConnection,
	environment::{AppServerProcessEnv, ResolvedAppServerCodexHomeEnv, app_server_command_program},
	errors::{AppServerHomePreflightFailure, AppServerOutputTimeout, AppServerTransportFailure},
	wire::{JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, WireMessage},
};

#[cfg(test)]
use self::environment::{
	app_server_command_program_from_env, configure_app_server_command,
	resolve_shared_codex_home_env_from_home,
};

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
