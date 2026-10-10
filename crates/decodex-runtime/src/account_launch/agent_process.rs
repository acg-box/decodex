//! Retained protocol bridge for an already admitted and initialized account-bound child.
//! The existing supervisor keeps process ownership. Credential callbacks remain on the
//! zeroizing synchronous path and never enter the general-purpose event channel.

#[cfg(all(test, unix))]
#[path = "agent_process_native_tests.rs"]
pub(crate) mod native_tests;

use std::{
	collections::HashSet,
	io::{self, ErrorKind, Write},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::RecvTimeoutError,
	},
	thread::{Builder, JoinHandle},
	time::Duration,
};

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc::{
	self, Sender,
	error::{TryRecvError, TrySendError},
};

use crate::account_launch::process::{AccountBinding, InboundFrame, SupervisedProcess};
use decodex_codex::{
	app_server_client::{self, AppServerClient, ClientError, RequestId, ServerEvent},
	schema::ACCOUNT_REFRESH_CALLBACK_METHOD,
};

const BRIDGE_CAPACITY: usize = 64;

pub(super) struct AgentProcessBridge {
	cancelled: Arc<AtomicBool>,
	client: AppServerClient,
	thread: Option<JoinHandle<()>>,
}
impl AgentProcessBridge {
	pub(super) fn start(
		stdin: Box<dyn Write + Send>,
		stdout: std::sync::mpsc::Receiver<InboundFrame>,
		binding: AccountBinding,
		protocol_limit_exceeded: Arc<AtomicBool>,
		next_request_id: i64,
		config_warnings: Vec<Value>,
	) -> Result<(Self, AppServerClient, mpsc::Receiver<ServerEvent>), ClientError> {
		let (incoming, frames) = mpsc::channel(BRIDGE_CAPACITY);
		let (outgoing, commands) = mpsc::channel(BRIDGE_CAPACITY);
		let (client, events) = AppServerClient::from_framed(next_request_id, frames, outgoing)?;

		client.bind_native_home(binding.codex_home().to_path_buf())?;

		let cancelled = Arc::new(AtomicBool::new(false));
		let worker_cancelled = Arc::clone(&cancelled);
		let worker = Builder::new()
			.name("decodex-agent-account-bridge".into())
			.spawn(move || {
				let terminal = incoming.clone();
				let mut writer: Box<dyn Write + Send> = Box::new(RevocableWriter {
					inner: stdin,
					cancelled: Arc::clone(&worker_cancelled),
				});

				for warning in config_warnings {
					if incoming.blocking_send(Ok(warning)).is_err() {
						return;
					}
				}

				let result = pump(
					&mut writer,
					stdout,
					commands,
					incoming,
					&worker_cancelled,
					&protocol_limit_exceeded,
					Some(binding.codex_home()),
					|writer, id, bytes| {
						SupervisedProcess::service_inbound_request(
							&binding,
							writer,
							id,
							ACCOUNT_REFRESH_CALLBACK_METHOD,
							bytes,
						)
						.map_err(|_| ClientError::Io)
					},
				);

				finish_bridge(writer, terminal, result);
			})
			.map_err(|_| {
				client.close();

				ClientError::Io
			})?;

		Ok((Self { cancelled, client: client.clone(), thread: Some(worker) }, client, events))
	}

	pub(super) fn close(&self) {
		self.cancelled.store(true, Ordering::Release);
		self.client.close();
	}
}

impl Drop for AgentProcessBridge {
	fn drop(&mut self) {
		self.close();

		if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
			&& let Some(worker) = self.thread.take()
		{
			let _ = worker.join();
		}
		// A callback already inside AccountService may finish later. Its writer is revoked,
		// its client is closed, and the supervisor still owns process death and quarantine.
	}
}

struct RevocableWriter {
	inner: Box<dyn Write + Send>,
	cancelled: Arc<AtomicBool>,
}
impl Write for RevocableWriter {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		if self.cancelled.load(Ordering::Acquire) {
			return Err(ErrorKind::BrokenPipe.into());
		}

		self.inner.write(bytes)
	}

	fn flush(&mut self) -> io::Result<()> {
		if self.cancelled.load(Ordering::Acquire) {
			return Err(ErrorKind::BrokenPipe.into());
		}

		self.inner.flush()
	}
}

#[derive(Deserialize)]
struct Header<'a> {
	id: Option<RequestId>,
	#[serde(borrow)]
	method: Option<&'a str>,
}

fn finish_bridge(
	writer: Box<dyn Write + Send>,
	terminal: Sender<Result<Value, ClientError>>,
	result: Result<(), ClientError>,
) {
	// Release stdin before a potentially blocked terminal event delivery. EOF lets
	// Codex shut down its threads and helpers even if the consumer stopped polling.
	drop(writer);

	// Preserve queued evidence before transport EOF. Revocation closes the client
	// separately, including when an AccountService callback is still pending.
	let _ = terminal.blocking_send(Err(result.err().unwrap_or(ClientError::Closed)));
}

#[allow(clippy::too_many_arguments)] // Keep bridge I/O, revocation and credential callback explicit.
fn pump(
	writer: &mut Box<dyn Write + Send>,
	stdout: std::sync::mpsc::Receiver<InboundFrame>,
	mut commands: mpsc::Receiver<Value>,
	events: Sender<Result<Value, ClientError>>,
	cancelled: &AtomicBool,
	protocol_limit_exceeded: &AtomicBool,
	native_home: Option<&std::path::Path>,
	mut refresh: impl FnMut(&mut Box<dyn Write + Send>, u64, &[u8]) -> Result<(), ClientError>,
) -> Result<(), ClientError> {
	let mut requests = HashSet::new();

	loop {
		if cancelled.load(Ordering::Acquire) {
			return Err(ClientError::Closed);
		}

		for _ in 0..BRIDGE_CAPACITY {
			let value = match commands.try_recv() {
				Ok(value) => value,
				Err(TryRecvError::Empty) => break,
				Err(TryRecvError::Disconnected) => return Err(ClientError::Closed),
			};

			if let Err(error) = validate_outbound(&value, &mut requests)
				.and_then(|_| validate_goal_attachment_owner(&value, native_home))
			{
				if value.get("method").and_then(Value::as_str).is_some()
					&& let Some(id) = value
						.get("id")
						.cloned()
						.and_then(|id| serde_json::from_value::<RequestId>(id).ok())
				{
					// Refuse this local RPC before writing any bytes. Optional UI reads
					// must not close the conversation's shared transport.
					events.try_send(Ok(serde_json::json!({"id":id,"error":{"code":-32_601,"message":"Method is not available on the Agent connection."}})))
                        .map_err(|_| ClientError::CapacityExceeded)?;

					continue;
				}

				return Err(error);
			}

			SupervisedProcess::write_bound_json(writer, &value).map_err(|_| ClientError::Io)?;
		}

		let frame = match stdout.recv_timeout(Duration::from_millis(5)) {
			Ok(frame) => frame.into_contiguous(),
			Err(RecvTimeoutError::Timeout) => continue,
			Err(RecvTimeoutError::Disconnected) => {
				return Err(if protocol_limit_exceeded.load(Ordering::Acquire) {
					ClientError::FrameTooLarge
				} else {
					ClientError::Closed
				});
			},
		};
		let header: Header<'_> =
			serde_json::from_slice(&frame).map_err(|_| ClientError::InvalidFrame)?;

		if header.method == Some(ACCOUNT_REFRESH_CALLBACK_METHOD) {
			SupervisedProcess::validate_zero_scratch_json(&frame)
				.map_err(|_| ClientError::InvalidFrame)?;

			let Some(RequestId::Number(id)) = header.id else {
				return Err(ClientError::InvalidFrame);
			};
			let id = u64::try_from(id).map_err(|_| ClientError::InvalidFrame)?;

			refresh(writer, id, &frame)?;

			continue;
		}
		if header.method.is_some_and(|method| method.starts_with("account/")) {
			// Account events belong to AccountService. Unknown account requests fail closed.
			if header.id.is_some() {
				return Err(ClientError::InvalidFrame);
			}

			continue;
		}
		if header.method.is_some()
			&& let Some(id) = header.id
			&& (requests.len() >= BRIDGE_CAPACITY || !requests.insert(id))
		{
			return Err(ClientError::CapacityExceeded);
		}

		let value = serde_json::from_slice(&frame).map_err(|_| ClientError::InvalidFrame)?;

		events.try_send(Ok(value)).map_err(|error| match error {
			TrySendError::Full(_) => ClientError::CapacityExceeded,
			TrySendError::Closed(_) => ClientError::Closed,
		})?;
	}
}

fn validate_goal_attachment_owner(
	value: &Value,
	native_home: Option<&std::path::Path>,
) -> Result<(), ClientError> {
	let method = value["method"].as_str().unwrap_or("");

	if !matches!(method, "fs/createDirectory" | "fs/writeFile") {
		return Ok(());
	}

	let home = native_home.ok_or(ClientError::InvalidFrame)?;
	let path =
		std::path::Path::new(value["params"]["path"].as_str().ok_or(ClientError::InvalidFrame)?);
	let directory = if method == "fs/writeFile" {
		path.parent().ok_or(ClientError::InvalidFrame)?
	} else {
		path
	};

	if directory.parent() != Some(home.join("attachments").as_path()) {
		return Err(ClientError::InvalidFrame);
	}

	Ok(())
}

fn validate_outbound(value: &Value, requests: &mut HashSet<RequestId>) -> Result<(), ClientError> {
	if let Some(method) = value.get("method") {
		return validate_outbound_method(method, &value["params"]);
	}

	let id = serde_json::from_value(value.get("id").cloned().ok_or(ClientError::InvalidFrame)?)
		.map_err(|_| ClientError::InvalidFrame)?;

	if !requests.remove(&id) {
		return Err(ClientError::InvalidFrame);
	}

	Ok(())
}

fn validate_outbound_method(method: &Value, params: &Value) -> Result<(), ClientError> {
	if method == "thread/fork" {
		return validate_fork_request(params);
	}

	if let Some(method @ ("fs/createDirectory" | "fs/writeFile")) = method.as_str() {
		return if app_server_client::is_goal_attachment_write(method, params) {
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}

	if method == "thread/readState/update" {
		return if app_server_client::is_thread_read_state_update(params) {
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if method == "thread/goal/set" {
		return if app_server_client::is_native_goal_update(params) {
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if method == "config/batchWrite" {
		return if app_server_client::is_hook_settings_write(params)
			|| app_server_client::is_app_tool_exposure_write(params)
			|| app_server_client::is_realtime_voice_write(params)
			|| app_server_client::is_search_mode_write(params)
		{
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if method == "thread/settings/update" {
		return if app_server_client::is_thread_model_selection(params)
			|| app_server_client::is_thread_model_recovery_update(params)
			|| app_server_client::is_thread_permission_selection(params)
		{
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if method == "account/sendAddCreditsNudgeEmail" {
		return if params.as_object().is_some_and(|p| p.len() == 1)
			&& matches!(params["creditType"].as_str(), Some("credits" | "usage_limit"))
		{
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if method == "turn/settings/update" {
		return if app_server_client::is_live_reviewer_update(params)
			|| app_server_client::is_live_model_update(params)
		{
			Ok(())
		} else {
			Err(ClientError::InvalidFrame)
		};
	}
	if !matches!(
		method.as_str(),
		Some(
			"getAuthStatus"
				| "thread/realtime/start"
				| "thread/realtime/appendSpeech"
				| "thread/realtime/stop"
				| "thread/realtime/listVoices"
				| "model/list"
				| "experimentalFeature/list"
				| "skills/extraRoots/set"
				| "skills/list"
				| "server/diagnostics"
				| "permissionProfile/list"
				| "hooks/list"
				| "config/read"
				| "configRequirements/read"
				| "thread/start"
				| "thread/resume"
				| "thread/inject_items"
				| "thread/attachment/list"
				| "mcpServerStatus/list"
				| "plugin/installed"
				| "app/installed"
				| "app/list"
				| "app/read"
				| "account/usage/read"
				| "thread/unarchive"
				| "thread/revert"
				| "thread/unsubscribe"
				| "thread/read"
				| "thread/list"
				| "thread/backgroundTerminals/list"
				| "thread/backgroundTerminals/terminate"
				| "thread/search"
				| "thread/searchOccurrences"
				| "thread/goal/get"
				| "thread/turns/list"
				| "thread/items/list"
				| "thread/items/read"
				| "thread/timeline/list"
				| "thread/attachment/add"
				| "thread/attachment/remove"
				| "mcpServer/oauth/login"
				| "thread/archive"
				| "thread/approveGuardianDeniedAction"
				| "turn/start"
				| "turn/steer"
				| "turn/interrupt"
		)
	) {
		return Err(ClientError::InvalidFrame);
	}

	Ok(())
}

fn validate_fork_request(params: &Value) -> Result<(), ClientError> {
	let identity = |value: &Value| {
		value.as_str().is_some_and(|id| {
			!id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
		})
	};

	if params.as_object().is_some_and(|p| {
		p.len() == 5
			&& p.keys().all(|key| {
				matches!(
					key.as_str(),
					"threadId"
						| "beforeTurnId"
						| "lastTurnId"
						| "deferGoalContinuation"
						| "excludeTurns"
						| "modelProvider"
				)
			})
	}) && identity(&params["threadId"])
		&& identity(&params["modelProvider"])
		&& (identity(&params["beforeTurnId"]) ^ identity(&params["lastTurnId"]))
		&& params["deferGoalContinuation"] == true
		&& params["excludeTurns"] == true
	{
		Ok(())
	} else {
		Err(ClientError::InvalidFrame)
	}
}

#[cfg(test)]
mod tests {
	#[cfg(test)] use std::io::Read as _;
	#[cfg(unix)]
	#[cfg(test)]
	use std::io::{BufRead as _, BufReader};
	use std::{io, mem, thread};

	use tokio::{sync::oneshot, time};

	use crate::account_launch::agent_process::{
		self, AccountBinding, AgentProcessBridge, Arc, AtomicBool, ClientError, Duration, HashSet,
		InboundFrame, Ordering, RequestId, RevocableWriter, Value, Write, mpsc,
	};

	#[test]
	fn fork_bridge_preserves_explicit_boundary_and_deferred_goal() {
		for boundary in ["beforeTurnId", "lastTurnId"] {
			let mut params = serde_json::json!({"threadId":"source","deferGoalContinuation":true,"excludeTurns":true,"modelProvider":"source-provider"});

			params[boundary] = serde_json::json!("selected");

			assert!(
				agent_process::validate_outbound_method(&serde_json::json!("thread/fork"), &params)
					.is_ok()
			);

			for (field, value) in [
				("path", serde_json::json!("/unreviewed/history.jsonl")),
				("deferGoalContinuation", serde_json::json!(false)),
				("excludeTurns", serde_json::json!(false)),
				("threadId", serde_json::json!("")),
				("modelProvider", serde_json::json!(null)),
				("modelProvider", serde_json::json!("\n")),
				(
					if boundary == "beforeTurnId" { "lastTurnId" } else { "beforeTurnId" },
					serde_json::json!("other"),
				),
			] {
				let mut invalid = params.clone();

				invalid[field] = value;

				assert!(
					agent_process::validate_outbound_method(
						&serde_json::json!("thread/fork"),
						&invalid
					)
					.is_err()
				);
			}
		}
	}

	#[test]
	fn account_notification_bridge_accepts_only_exact_native_purposes() {
		for (params, valid) in [
			(serde_json::json!({"creditType":"credits"}), true),
			(serde_json::json!({"creditType":"usage_limit"}), true),
			(serde_json::json!({"creditType":"future"}), false),
			(serde_json::json!({"creditType":"credits","accountId":"other"}), false),
			(serde_json::json!({"creditType":null}), false),
			(serde_json::json!({}), false),
		] {
			assert_eq!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":2,
				"method":"account/sendAddCreditsNudgeEmail","params":params}),
					&mut HashSet::new()
				)
				.is_ok(),
				valid
			);
		}
	}

	#[cfg(unix)]
	#[test]
	fn live_model_bridge_preserves_exact_turn_and_separate_edit_scope() {
		let frame = serde_json::json!({"id":42,"method":"turn/settings/update","params":{
            "threadId":"thread","turnId":"turn","model":"selected-model","effort":"high"}});
		let mut requests = HashSet::new();

		assert!(agent_process::validate_outbound(&frame, &mut requests).is_ok());

		for (field, value) in [
			("approvalPolicy", serde_json::json!("never")),
			("approvalsReviewer", serde_json::json!("auto_review")),
			("serviceTier", serde_json::json!("fast")),
		] {
			let mut mixed = frame.clone();

			mixed["params"][field] = value;

			assert!(agent_process::validate_outbound(&mixed, &mut requests).is_err());
		}

		let mut saved = frame;

		saved["method"] = serde_json::json!("thread/settings/update");

		assert!(agent_process::validate_outbound(&saved, &mut requests).is_err());
	}

	#[cfg(unix)]
	#[test]
	fn live_reviewer_bridge_preserves_the_narrow_edit_scope() {
		let mut requests = HashSet::new();
		let mut frame = serde_json::json!({"id":42,"method":"turn/settings/update","params":{
			"threadId":"thread","turnId":"turn","approvalsReviewer":"user"}});

		assert!(agent_process::validate_outbound(&frame, &mut requests).is_ok());

		frame["params"]["approvalPolicy"] = serde_json::json!("never");

		assert!(agent_process::validate_outbound(&frame, &mut requests).is_err());

		frame["method"] = serde_json::json!("thread/settings/update");

		assert!(agent_process::validate_outbound(&frame, &mut requests).is_err());
	}

	#[test]
	fn permission_bridge_rejects_unrelated_setting_changes() {
		let mut request = serde_json::json!({"id":42,"method":"thread/settings/update","params":{"threadId":"thread","permissions":"scoped"}});

		assert!(agent_process::validate_outbound(&request, &mut HashSet::new()).is_ok());

		for field in [
			"model",
			"sandboxPolicy",
			"sandbox",
			"config",
			"approvalPolicy",
			"approvalsReviewer",
			"cwd",
		] {
			request["params"][field] = serde_json::json!("unrelated");

			assert!(agent_process::validate_outbound(&request, &mut HashSet::new()).is_err());

			request["params"].as_object_mut().unwrap().remove(field);
		}
	}

	#[test]
	fn goal_attachments_stay_in_the_admitted_native_home() {
		let home = std::path::Path::new("/fixture/.codex");
		let request = serde_json::json!({"method":"fs/writeFile","params":{"path":"/fixture/.codex/attachments/20000000-0000-4000-8000-000000000002/goal-objective.md","dataBase64":"b2JqZWN0aXZl"}});

		assert!(agent_process::validate_outbound(&request, &mut HashSet::new()).is_ok());
		assert!(agent_process::validate_goal_attachment_owner(&request, Some(home)).is_ok());
		assert!(
			agent_process::validate_goal_attachment_owner(
				&request,
				Some(std::path::Path::new("/other/.codex"))
			)
			.is_err()
		);
		assert!(agent_process::validate_goal_attachment_owner(&request, None).is_err());

		for path in [
			"/fixture/.codex/auth.json",
			"/fixture/.codex/attachments/../goal-objective.md",
			"/fixture/.codex/attachments/20000000-0000-4000-8000-000000000002/config.toml",
		] {
			let mut invalid = request.clone();

			invalid["params"]["path"] = serde_json::json!(path);

			assert!(agent_process::validate_outbound(&invalid, &mut HashSet::new()).is_err());
		}
	}

	#[test]
	fn goal_bridge_requires_explicit_valid_edits_and_keeps_clear_unsupported() {
		for (method, allowed) in
			[("thread/goal/get", true), ("thread/goal/set", false), ("thread/goal/clear", false)]
		{
			assert_eq!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":1,"method":method,"params":{"threadId":"thread"}}),
					&mut HashSet::new()
				)
				.is_ok(),
				allowed
			);
		}
		for edit in [
			serde_json::json!({"objective":"Updated objective"}),
			serde_json::json!({"origin":"user", "objective":"Updated objective", "status":"paused", "tokenBudget":75}),
			serde_json::json!({"tokenBudget":1_234}),
			serde_json::json!({"tokenBudget":null}),
			serde_json::json!({"status":"paused"}),
		] {
			let mut params = edit;

			params["threadId"] = serde_json::json!("thread");

			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":2,"method":"thread/goal/set","params":params}),
					&mut HashSet::new()
				)
				.is_ok()
			);
		}
		for edit in [
			serde_json::json!({"status":"usageLimited"}),
			serde_json::json!({"origin":"user"}),
			serde_json::json!({"origin":"automatic", "objective":"Not a user edit"}),
			serde_json::json!({"tokenBudget":0}),
			serde_json::json!({"objective":""}),
			serde_json::json!({"objective":"New", "cwd":"/other"}),
		] {
			let mut params = edit;

			params["threadId"] = serde_json::json!("thread");

			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":3,"method":"thread/goal/set","params":params}),
					&mut HashSet::new()
				)
				.is_err()
			);
		}
	}

	#[test]
	fn rejected_optional_request_does_not_close_the_shared_transport() {
		let (sender, stdout) = std::sync::mpsc::sync_channel(2);

		sender.send(InboundFrame::fixture(br#"{"id":2,"result":{"data":[]}}"#)).unwrap();

		drop(sender);

		let (outgoing, commands) = mpsc::channel(4);

		outgoing
			.try_send(serde_json::json!({"id":1,"method":"account/login/start","params":{}}))
			.unwrap();
		outgoing.try_send(serde_json::json!({"id":2,"method":"thread/list","params":{}})).unwrap();

		let (events, mut receiver) = mpsc::channel(4);
		let mut writer: Box<dyn Write + Send> = Box::new(io::sink());
		let result = agent_process::pump(
			&mut writer,
			stdout,
			commands,
			events,
			&AtomicBool::new(false),
			&AtomicBool::new(false),
			None,
			|_, _, _| panic!("no refresh expected"),
		);

		assert!(matches!(result, Err(ClientError::Closed)));

		let denied = receiver.try_recv().unwrap().unwrap();

		assert_eq!(denied["id"], 1);
		assert_eq!(denied["error"]["code"], -32_601);
		assert_eq!(receiver.try_recv().unwrap().unwrap()["id"], 2);
	}

	#[test]
	fn conversation_metadata_reads_use_the_native_bridge() {
		for disabled in [serde_json::json!([]), serde_json::json!(["sample@test"])] {
			assert!(agent_process::validate_outbound(
				&serde_json::json!({"id":1,"method":"thread/settings/update","params":{"threadId":"thread","disabledPluginIds":disabled}}),
				&mut HashSet::new()
			).is_err(), "Retired local plugin selection");
		}
		for method in [
			"plugin/list",
			"plugin/read",
			"plugin/install",
			"plugin/reconcile",
			"config/mcpServer/reload",
			"mcpServer/resource/read",
			"mcpServer/tool/call",
		] {
			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":1,"method":method,"params":{}}),
					&mut HashSet::new()
				)
				.is_err(),
				"Retired client operation: {method}"
			);
		}
		for method in [
			"thread/attachment/list",
			"mcpServerStatus/list",
			"app/read",
			"plugin/installed",
			"app/list",
			"account/usage/read",
		] {
			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":1,"method":method,"params":{}}),
					&mut HashSet::new()
				)
				.is_ok(),
				"{method}"
			);
		}
	}

	#[test]
	fn native_context_injection_is_admitted_without_opening_account_methods() {
		let mut requests = HashSet::new();

		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":1,"method":"thread/inject_items","params":{"threadId":"fixture","items":[]}}),
				&mut requests
			)
			.is_ok()
		);
		assert!(matches!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":2,"method":"account/login/start","params":{}}),
				&mut requests
			),
			Err(ClientError::InvalidFrame)
		));
	}

	#[test]
	fn blocked_terminal_delivery_does_not_keep_child_stdin_open() {
		let (writer, mut child_stdin) = std::os::unix::net::UnixStream::pair().unwrap();

		child_stdin.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

		let (terminal, mut events) = mpsc::channel(1);

		terminal.try_send(Ok(serde_json::json!({"method":"turn/completed"}))).unwrap();

		let worker =
			thread::spawn(move || agent_process::finish_bridge(Box::new(writer), terminal, Ok(())));

		assert_eq!(child_stdin.read(&mut [0_u8; 1]).unwrap(), 0);
		assert_eq!(events.blocking_recv().unwrap().unwrap()["method"], "turn/completed");
		assert!(matches!(events.blocking_recv(), Some(Err(ClientError::Closed))));

		worker.join().unwrap();
	}

	#[test]
	fn refresh_callback_is_consumed_privately_before_event_conversion() {
		let (sender, stdout) = std::sync::mpsc::sync_channel(4);

		sender.send(InboundFrame::fixture(br#"{"id":17,"method":"account/chatgptAuthTokens/refresh","params":{"reason":"unauthorized","previousAccountId":"test-account"}}"#)).unwrap();
		sender
			.send(InboundFrame::fixture(
				br#"{"method":"account/updated","params":{"privateField":"test-value"}}"#,
			))
			.unwrap();
		sender
			.send(InboundFrame::fixture(
				br#"{"method":"turn/completed","params":{"threadId":"peer"}}"#,
			))
			.unwrap();

		drop(sender);

		let (_outgoing, commands) = mpsc::channel(4);
		let (events, mut receiver) = mpsc::channel(4);
		let mut writer: Box<dyn Write + Send> = Box::new(io::sink());
		let mut refreshed = false;
		let result = agent_process::pump(
			&mut writer,
			stdout,
			commands,
			events,
			&AtomicBool::new(false),
			&AtomicBool::new(false),
			None,
			|_, id, bytes| {
				assert_eq!(id, 17);
				assert!(bytes.windows(b"unauthorized".len()).any(|part| part == b"unauthorized"));

				refreshed = true;

				Ok(())
			},
		);

		assert!(refreshed);
		assert!(matches!(result, Err(ClientError::Closed)));
		assert_eq!(receiver.try_recv().unwrap().unwrap()["method"], "turn/completed");
		assert!(matches!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Disconnected)));
	}

	#[test]
	fn ordinary_text_preserves_json_escapes() {
		let (sender, stdout) = std::sync::mpsc::sync_channel(1);

		sender.send(InboundFrame::fixture(br#"{"method":"item/agentMessage/delta","params":{"delta":"first\nsecond \"quoted\""}}"#)).unwrap();

		drop(sender);

		let (_outgoing, commands) = mpsc::channel(1);
		let (events, mut receiver) = mpsc::channel(1);
		let mut writer: Box<dyn Write + Send> = Box::new(io::sink());
		let _ = agent_process::pump(
			&mut writer,
			stdout,
			commands,
			events,
			&AtomicBool::new(false),
			&AtomicBool::new(false),
			None,
			|_, _, _| panic!("not an account callback"),
		);

		assert_eq!(
			receiver.try_recv().unwrap().unwrap()["params"]["delta"],
			"first\nsecond \"quoted\""
		);
	}

	#[test]
	fn retained_capability_rejects_reauthentication_and_unowned_responses() {
		let mut requests = HashSet::new();

		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":41,"method":"initialize","params":{}}),
				&mut requests
			)
			.is_err()
		);
		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":41,"method":"account/login/start","params":{}}),
				&mut requests
			)
			.is_err()
		);
		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":17,"result":{}}),
				&mut requests
			)
			.is_err()
		);

		for method in ["thread/turns/list", "thread/items/list", "thread/items/read"] {
			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":43,"method":method,"params":{}}),
					&mut requests
				)
				.is_ok()
			);
		}

		requests.insert(RequestId::String("approval".into()));

		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":"approval","result":{"decision":"decline"}}),
				&mut requests
			)
			.is_ok()
		);
		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":"approval","result":{}}),
				&mut requests
			)
			.is_err()
		);
		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":42,"method":"turn/steer","params":{}}),
				&mut requests
			)
			.is_ok()
		);
	}

	#[test]
	fn hook_config_writes_do_not_admit_other_config_or_file_targets() {
		let params = serde_json::json!({"edits":[{"keyPath":"hooks.state.\"plugin.key\".enabled","value":false,"mergeStrategy":"replace"}],"expectedVersion":"reviewed-version","reloadUserConfig":true});
		let mut requests = HashSet::new();
		let mut request = serde_json::json!({"id":45,"method":"config/batchWrite","params":params});

		assert!(agent_process::validate_outbound(&request, &mut requests).is_ok());

		request["params"]["filePath"] = serde_json::json!("/other/config.toml");

		assert!(agent_process::validate_outbound(&request, &mut requests).is_err());

		request["params"].as_object_mut().unwrap().remove("filePath");

		request["params"]["edits"][0]["keyPath"] = serde_json::json!("bypass_hook_trust");

		assert!(agent_process::validate_outbound(&request, &mut requests).is_err());
	}

	#[test]
	fn retired_connector_approval_writes_are_rejected() {
		for field in ["approvals_reviewer", "default_tools_approval_mode"] {
			for value in [serde_json::json!("auto"), Value::Null] {
				let request = serde_json::json!({"id":42,"method":"config/batchWrite","params":{
					"filePath":"/fixture/config.toml","expectedVersion":"v1","reloadUserConfig":true,
					"edits":[{"keyPath":format!("apps.\"calendar\".links.\"work\".{field}"),
						"mergeStrategy":"replace","value":value}]}});

				assert!(agent_process::validate_outbound(&request, &mut HashSet::new()).is_err());
			}
		}
	}

	#[test]
	fn connector_exposure_bridge_preserves_the_single_connector_boundary() {
		let frame = serde_json::json!({"id":48,"method":"config/batchWrite","params":{
			"filePath":"/fixture/config.toml","expectedVersion":"v1","reloadUserConfig":true,
			"edits":[{"keyPath":"apps.\"connector.with.dot\".omit_tools_from","value":["deferred"],"mergeStrategy":"replace"}]}});
		let mut requests = HashSet::new();

		assert!(agent_process::validate_outbound(&frame, &mut requests).is_ok());

		for path in [
			"apps.\"_default\".omit_tools_from",
			"apps.\"connector.with.dot\".links.\"work\".omit_tools_from",
			"apps.\"connector.with.dot\".enabled",
		] {
			let mut changed = frame.clone();

			changed["params"]["edits"][0]["keyPath"] = serde_json::json!(path);

			assert!(agent_process::validate_outbound(&changed, &mut requests).is_err());
		}

		let mut changed = frame;

		changed["params"]["edits"]
			.as_array_mut()
			.unwrap()
			.push(serde_json::json!({"keyPath":"model","value":"other","mergeStrategy":"replace"}));

		assert!(agent_process::validate_outbound(&changed, &mut requests).is_err());
	}

	#[test]
	fn metadata_reads_are_allowed_but_feature_changes_remain_private() {
		let mut requests = HashSet::new();

		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":42,"method":"experimentalFeature/list","params":{"limit":100}}),
				&mut requests
			)
			.is_ok()
		);

		for method in ["experimentalFeature/enablement/set", "config/value/write", "memory/reset"] {
			assert!(
				agent_process::validate_outbound(
					&serde_json::json!({"id":43,"method":method,"params":{}}),
					&mut requests
				)
				.is_err()
			);
		}

		assert!(
			agent_process::validate_outbound(
				&serde_json::json!({"id":44,"method":"turn/start","params":{}}),
				&mut requests
			)
			.is_ok()
		);
	}

	#[test]
	fn supervisor_revocation_blocks_a_late_callback_write() {
		let cancelled = Arc::new(AtomicBool::new(false));
		let mut writer =
			RevocableWriter { inner: Box::new(io::sink()), cancelled: Arc::clone(&cancelled) };

		writer.write_all(b"before").unwrap();
		cancelled.store(true, Ordering::Release);

		assert_eq!(writer.write_all(b"after").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
		assert_eq!(writer.flush().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
	}

	#[tokio::test]
	async fn bridge_drop_closes_an_in_flight_rpc() {
		struct RequestWriter {
			bytes: Vec<u8>,
			flushed: Option<tokio::sync::oneshot::Sender<Vec<u8>>>,
		}

		impl Write for RequestWriter {
			fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
				self.bytes.extend_from_slice(bytes);

				Ok(bytes.len())
			}

			fn flush(&mut self) -> io::Result<()> {
				if let Some(flushed) = self.flushed.take() {
					let _ = flushed.send(mem::take(&mut self.bytes));
				}

				Ok(())
			}
		}

		let (flushed, request) = oneshot::channel();
		let (_sender, stdout) = std::sync::mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = AgentProcessBridge::start(
			Box::new(RequestWriter { bytes: Vec::new(), flushed: Some(flushed) }),
			stdout,
			binding,
			Arc::new(AtomicBool::new(false)),
			41,
			Vec::new(),
		)
		.unwrap();
		let pending_client = client.clone();
		let pending = tokio::spawn(async move {
			pending_client.thread_read(serde_json::json!({"threadId":"peer"})).await
		});
		let frame = time::timeout(Duration::from_secs(2), request).await.unwrap().unwrap();
		let frame: Value = serde_json::from_slice(&frame).unwrap();

		assert_eq!(frame["method"], "thread/read");
		assert_eq!(frame["params"]["threadId"], "peer");
		assert!(!pending.is_finished(), "request must await a response before bridge shutdown");

		drop(bridge);

		assert!(matches!(
			time::timeout(Duration::from_secs(2), pending).await.unwrap().unwrap(),
			Err(ClientError::Closed)
		));
		assert!(matches!(
			client.thread_read(serde_json::json!({})).await,
			Err(ClientError::Closed)
		));
	}
	#[cfg(unix)]
	#[tokio::test]
	async fn native_timeline_read_crosses_retained_bridge_and_keeps_peer_available() {
		let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();

		reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

		let (sender, stdout) = std::sync::mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = AgentProcessBridge::start(
			Box::new(writer),
			stdout,
			binding,
			Arc::new(AtomicBool::new(false)),
			41,
			vec![],
		)
		.unwrap();
		let server = thread::spawn(move || {
			let mut lines = BufReader::new(reader).lines();

			for (method, result) in [
				(
					"thread/read",
					serde_json::json!({"thread":{"id":"thread","historyMode":"paginated"}}),
				),
				(
					"thread/timeline/list",
					serde_json::json!({"data":[{"type":"item","position":1,"turnId":"turn","item":{"id":"item","type":"userMessage","content":[{"type":"text","text":"visible"},{"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(2*1_024*1_024))}]}}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}),
				),
				(
					"thread/read",
					serde_json::json!({"thread":{"id":"peer","historyMode":"paginated"}}),
				),
			] {
				let Some(Ok(line)) = lines.next() else {
					return;
				};
				let request: Value = serde_json::from_str(&line).unwrap();

				assert_eq!(request["method"], method);

				let response =
					serde_json::to_vec(&serde_json::json!({"id":request["id"],"result":result}))
						.unwrap();

				sender.send(InboundFrame::fixture(&response)).unwrap();
			}
		});
		let page = tokio::time::timeout(
			Duration::from_secs(3),
			client.thread_timeline_page("thread", None, 30),
		)
		.await
		.unwrap();

		assert!(page.is_ok(), "retained bridge rejected native timeline: {page:?}");

		let page = page.unwrap();

		assert_eq!(page["data"][0]["item"]["content"][0]["text"], "visible");
		assert!(
			page["data"][0]["item"]["content"][1]["url"].as_str().unwrap().len() > 1_024 * 1_024
		);

		let peer = client.thread_read(serde_json::json!({"threadId":"peer"})).await.unwrap();

		assert_eq!(peer["thread"]["id"], "peer");

		server.join().unwrap();

		drop(bridge);
	}
	#[cfg(unix)]
	#[tokio::test]
	async fn existing_usage_resources_and_integrations_use_retained_bridge() {
		let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();

		reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

		let (sender, stdout) = std::sync::mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = AgentProcessBridge::start(
			Box::new(writer),
			stdout,
			binding,
			Arc::new(AtomicBool::new(false)),
			41,
			vec![],
		)
		.unwrap();
		let server = thread::spawn(move || {
			let mut lines = BufReader::new(reader).lines();

			for (method, result) in [
				(
					"account/usage/read",
					serde_json::json!({"threadUsage":{"threadId":"thread","estimatedUsageCreditsMicros":17,"groups":[]}}),
				),
				("thread/attachment/list", serde_json::json!({"data":[],"nextCursor":null})),
				("mcpServerStatus/list", serde_json::json!({"data":[],"nextCursor":null})),
				(
					"plugin/installed",
					serde_json::json!({"marketplaces":[],"marketplaceLoadErrors":[]}),
				),
			] {
				let Some(Ok(line)) = lines.next() else {
					return;
				};
				let request: Value = serde_json::from_str(&line).unwrap();

				assert_eq!(request["method"], method);

				let response =
					serde_json::to_vec(&serde_json::json!({"id":request["id"],"result":result}))
						.unwrap();

				sender.send(InboundFrame::fixture(&response)).unwrap();
			}
		});

		assert_eq!(
			client
				.thread_usage_estimate("thread")
				.await
				.unwrap()
				.unwrap()
				.estimated_usage_credits_micros,
			17
		);
		assert!(client.thread_attachments("thread").await.unwrap().is_empty());
		assert!(client.mcp_server_statuses("thread").await.unwrap().is_empty());
		assert_eq!(
			client.installed_plugins_for_directory("/project").await.unwrap()["marketplaces"],
			serde_json::json!([])
		);

		server.join().unwrap();

		drop(bridge);
	}
}
