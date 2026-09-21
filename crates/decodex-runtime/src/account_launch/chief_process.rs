//! Retained protocol bridge for an already admitted and initialized account-bound child.
//! The existing supervisor keeps process ownership. Credential callbacks remain on the
//! zeroizing synchronous path and never enter the general-purpose event channel.

use super::process::{AccountBinding, InboundFrame, SupervisedProcess};
use decodex_codex::{
	app_server_client::{AppServerClient, ClientError, RequestId, ServerEvent},
	schema::ACCOUNT_REFRESH_CALLBACK_METHOD,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
	collections::HashSet,
	io::{self, Write},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{Receiver, RecvTimeoutError},
	},
	thread::{self, JoinHandle},
	time::Duration,
};
use tokio::sync::mpsc;

const BRIDGE_CAPACITY: usize = 64;

#[cfg(all(test, unix))]
#[path = "chief_process_native_tests.rs"]
mod native_tests;

pub(super) struct ChiefProcessBridge {
	cancelled: Arc<AtomicBool>,
	client: AppServerClient,
	thread: Option<JoinHandle<()>>,
}

impl ChiefProcessBridge {
	pub(super) fn start(
		stdin: Box<dyn Write + Send>,
		stdout: Receiver<InboundFrame>,
		binding: AccountBinding,
		protocol_limit_exceeded: Arc<AtomicBool>,
		next_request_id: i64,
		config_warnings: Vec<Value>,
	) -> Result<(Self, AppServerClient, mpsc::Receiver<ServerEvent>), ClientError> {
		let (incoming, frames) = mpsc::channel(BRIDGE_CAPACITY);
		let (outgoing, commands) = mpsc::channel(BRIDGE_CAPACITY);
		let (client, events) = AppServerClient::from_framed(next_request_id, frames, outgoing)?;
		let cancelled = Arc::new(AtomicBool::new(false));
		let worker_cancelled = Arc::clone(&cancelled);
		let worker = thread::Builder::new()
			.name("decodex-chief-account-bridge".into())
			.spawn(move || {
				let mut writer: Box<dyn Write + Send> = Box::new(RevocableWriter {
					inner: stdin,
					cancelled: Arc::clone(&worker_cancelled),
				});
				let terminal = incoming.clone();
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

fn finish_bridge(
	writer: Box<dyn Write + Send>,
	terminal: mpsc::Sender<Result<Value, ClientError>>,
	result: Result<(), ClientError>,
) {
	// Release stdin before a potentially blocked terminal event delivery. EOF lets
	// Codex shut down its threads and helpers even if the consumer stopped polling.
	drop(writer);
	// Preserve queued evidence before transport EOF. Revocation closes the client
	// separately, including when an AccountService callback is still pending.
	let _ = terminal.blocking_send(Err(result.err().unwrap_or(ClientError::Closed)));
}

impl Drop for ChiefProcessBridge {
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
			return Err(io::ErrorKind::BrokenPipe.into());
		}
		self.inner.write(bytes)
	}

	fn flush(&mut self) -> io::Result<()> {
		if self.cancelled.load(Ordering::Acquire) {
			return Err(io::ErrorKind::BrokenPipe.into());
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

fn pump(
	writer: &mut Box<dyn Write + Send>,
	stdout: Receiver<InboundFrame>,
	mut commands: mpsc::Receiver<Value>,
	events: mpsc::Sender<Result<Value, ClientError>>,
	cancelled: &AtomicBool,
	protocol_limit_exceeded: &AtomicBool,
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
				Err(mpsc::error::TryRecvError::Empty) => break,
				Err(mpsc::error::TryRecvError::Disconnected) => return Err(ClientError::Closed),
			};
			validate_outbound(&value, &mut requests)?;
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
			mpsc::error::TrySendError::Full(_) => ClientError::CapacityExceeded,
			mpsc::error::TrySendError::Closed(_) => ClientError::Closed,
		})?;
	}
}

fn validate_outbound(value: &Value, requests: &mut HashSet<RequestId>) -> Result<(), ClientError> {
	if let Some(method) = value.get("method") {
		if method == "config/batchWrite" {
			return if decodex_codex::app_server_client::is_app_link_settings_write(&value["params"])
			{
				Ok(())
			} else {
				Err(ClientError::InvalidFrame)
			};
		}
		if method == "config/read" {
			let params = &value["params"];
			return if params.as_object().is_some_and(|p| p.len() == 2)
				&& params["includeLayers"] == true
				&& params["cwd"].as_str().is_some_and(|p| std::path::Path::new(p).is_absolute())
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
					| "thread/realtime/stop"
					| "model/list" | "experimentalFeature/list"
					| "thread/start"
					| "thread/resume"
					| "thread/goal/get"
					| "thread/unarchive"
					| "thread/read" | "thread/list"
					| "thread/turns/list"
					| "thread/items/list"
					| "thread/timeline/list"
					| "thread/attachment/list"
					| "thread/attachment/add"
					| "thread/attachment/remove"
					| "thread/inject_items"
					| "account/usage/read"
					| "mcpServerStatus/list"
					| "mcpServer/oauth/login"
					| "plugin/installed"
					| "plugin/read" | "plugin/list"
					| "plugin/install"
					| "plugin/reconcile"
					| "app/list" | "config/mcpServer/reload"
					| "thread/archive"
					| "thread/approveGuardianDeniedAction"
					| "turn/start" | "turn/steer"
					| "turn/interrupt"
			)
		) {
			return Err(ClientError::InvalidFrame);
		}
	} else {
		let id = serde_json::from_value(value.get("id").cloned().ok_or(ClientError::InvalidFrame)?)
			.map_err(|_| ClientError::InvalidFrame)?;
		if !requests.remove(&id) {
			return Err(ClientError::InvalidFrame);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	use std::sync::mpsc as sync_mpsc;

	#[cfg(unix)]
	#[test]
	fn blocked_terminal_delivery_does_not_keep_child_stdin_open() {
		use std::io::Read as _;
		let (writer, mut child_stdin) = std::os::unix::net::UnixStream::pair().unwrap();
		child_stdin.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
		let (terminal, mut events) = mpsc::channel(1);
		terminal.try_send(Ok(json!({"method":"turn/completed"}))).unwrap();
		let worker = thread::spawn(move || finish_bridge(Box::new(writer), terminal, Ok(())));
		assert_eq!(child_stdin.read(&mut [0_u8; 1]).unwrap(), 0);
		assert_eq!(events.blocking_recv().unwrap().unwrap()["method"], "turn/completed");
		assert!(matches!(events.blocking_recv(), Some(Err(ClientError::Closed))));
		worker.join().unwrap();
	}

	#[test]
	fn refresh_callback_is_consumed_privately_before_event_conversion() {
		let (sender, stdout) = sync_mpsc::sync_channel(4);
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
		let result = pump(
			&mut writer,
			stdout,
			commands,
			events,
			&AtomicBool::new(false),
			&AtomicBool::new(false),
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
		let (sender, stdout) = sync_mpsc::sync_channel(1);
		sender.send(InboundFrame::fixture(br#"{"method":"item/agentMessage/delta","params":{"delta":"first\nsecond \"quoted\""}}"#)).unwrap();
		drop(sender);
		let (_outgoing, commands) = mpsc::channel(1);
		let (events, mut receiver) = mpsc::channel(1);
		let mut writer: Box<dyn Write + Send> = Box::new(io::sink());
		let _ = pump(
			&mut writer,
			stdout,
			commands,
			events,
			&AtomicBool::new(false),
			&AtomicBool::new(false),
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
			validate_outbound(&json!({"id":41,"method":"initialize","params":{}}), &mut requests)
				.is_err()
		);
		assert!(
			validate_outbound(
				&json!({"id":41,"method":"account/login/start","params":{}}),
				&mut requests
			)
			.is_err()
		);
		assert!(validate_outbound(&json!({"id":17,"result":{}}), &mut requests).is_err());
		for method in ["thread/turns/list", "thread/items/list"] {
			assert!(
				validate_outbound(&json!({"id":43,"method":method,"params":{}}), &mut requests)
					.is_ok()
			);
		}
		requests.insert(RequestId::String("approval".into()));
		assert!(
			validate_outbound(
				&json!({"id":"approval","result":{"decision":"decline"}}),
				&mut requests
			)
			.is_ok()
		);
		assert!(validate_outbound(&json!({"id":"approval","result":{}}), &mut requests).is_err());
		assert!(
			validate_outbound(&json!({"id":42,"method":"turn/steer","params":{}}), &mut requests)
				.is_ok()
		);
	}

	#[test]
	fn metadata_reads_are_allowed_but_feature_changes_remain_private() {
		let mut requests = HashSet::new();
		assert!(
			validate_outbound(
				&json!({"id":42,"method":"experimentalFeature/list","params":{"limit":100}}),
				&mut requests
			)
			.is_ok()
		);
		for method in ["experimentalFeature/enablement/set", "config/value/write", "memory/reset"] {
			assert!(
				validate_outbound(&json!({"id":43,"method":method,"params":{}}), &mut requests)
					.is_err()
			);
		}
		assert!(
			validate_outbound(&json!({"id":44,"method":"turn/start","params":{}}), &mut requests)
				.is_ok()
		);
	}

	#[test]
	fn account_settings_bridge_accepts_only_versioned_single_leaf_edits() {
		let mut requests = HashSet::new();
		let valid = json!({"id":42,"method":"config/batchWrite","params":{
			"filePath":"/fixture/config.toml","expectedVersion":"v1","reloadUserConfig":true,
			"edits":[{"keyPath":"apps.\"日历.app\".links.\"work.\\\"link\\\\one\".approvals_reviewer",
				"mergeStrategy":"replace","value":"auto_review"}]}});
		assert!(validate_outbound(&valid, &mut requests).is_ok());
		for path in [
			"approvals_reviewer",
			"apps.app.links.work.approvals_reviewer",
			"apps.\"app\".links.\"work\".enabled",
			"apps.\"app\".links.\"work\".approvals_reviewer.extra",
			"apps.\"\".links.\"work\".approvals_reviewer",
			"apps.\"app\".links.\"work",
		] {
			let mut invalid = valid.clone();
			invalid["params"]["edits"][0]["keyPath"] = json!(path);
			assert!(validate_outbound(&invalid, &mut requests).is_err(), "{path}");
		}
		for (pointer, replacement) in [
			("/params/expectedVersion", Value::Null),
			("/params/filePath", json!("relative.toml")),
			("/params/reloadUserConfig", json!(false)),
			("/params/edits/0/mergeStrategy", json!("upsert")),
			("/params/edits/0/value", json!("future_unknown")),
		] {
			let mut invalid = valid.clone();
			*invalid.pointer_mut(pointer).unwrap() = replacement;
			assert!(validate_outbound(&invalid, &mut requests).is_err(), "{pointer}");
		}
		let mut invalid = valid.clone();
		invalid["params"]["edits"]
			.as_array_mut()
			.unwrap()
			.push(json!({"keyPath":"model","value":"other","mergeStrategy":"replace"}));
		assert!(validate_outbound(&invalid, &mut requests).is_err());
		let mut clear = valid;
		clear["params"]["edits"][0]["value"] = Value::Null;
		assert!(validate_outbound(&clear, &mut requests).is_ok());
		assert!(
			validate_outbound(
				&json!({"id":44,"method":"config/read","params":{"cwd":"/fixture","includeLayers":true}}),
				&mut requests
			)
			.is_ok()
		);
		assert!(
			validate_outbound(
				&json!({"id":44,"method":"config/read","params":{"cwd":"relative","includeLayers":true}}),
				&mut requests
			)
			.is_err()
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
	async fn bridge_drop_closes_pending_rpc_without_killing_a_peer_process() {
		let (_sender, stdout) = sync_mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = ChiefProcessBridge::start(
			Box::new(io::sink()),
			stdout,
			binding,
			Arc::new(AtomicBool::new(false)),
			41,
			Vec::new(),
		)
		.unwrap();
		let pending_client = client.clone();
		let pending =
			tokio::spawn(
				async move { pending_client.thread_read(json!({"threadId":"peer"})).await },
			);
		drop(bridge);
		assert!(matches!(
			tokio::time::timeout(Duration::from_secs(2), pending).await.unwrap().unwrap(),
			Err(ClientError::Closed)
		));
		assert!(matches!(client.thread_read(json!({})).await, Err(ClientError::Closed)));
	}
	#[cfg(unix)]
	#[tokio::test]
	async fn native_timeline_read_crosses_retained_bridge_and_keeps_peer_available() {
		use std::io::{BufRead, BufReader};
		let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
		reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
		let (sender, stdout) = sync_mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = ChiefProcessBridge::start(
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
				("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
				(
					"thread/timeline/list",
					json!({"data":[{"type":"item","position":1,"turnId":"turn","item":{"id":"item","type":"userMessage","content":[{"type":"text","text":"visible"},{"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(2*1024*1024))}]}}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}),
				),
				("thread/read", json!({"thread":{"id":"peer","historyMode":"paginated"}})),
			] {
				let Some(Ok(line)) = lines.next() else {
					return;
				};
				let request: Value = serde_json::from_str(&line).unwrap();
				assert_eq!(request["method"], method);
				let response =
					serde_json::to_vec(&json!({"id":request["id"],"result":result})).unwrap();
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
		assert!(page["data"][0]["item"]["content"][1]["url"].as_str().unwrap().len() > 1024 * 1024);
		let peer = client.thread_read(json!({"threadId":"peer"})).await.unwrap();
		assert_eq!(peer["thread"]["id"], "peer");
		server.join().unwrap();
		drop(bridge);
	}
	#[cfg(unix)]
	#[tokio::test]
	async fn existing_usage_resources_and_integrations_use_retained_bridge() {
		use std::io::{BufRead, BufReader};
		let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
		reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
		let (sender, stdout) = sync_mpsc::sync_channel(4);
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			"/tmp/.codex".into(),
		);
		let (bridge, client, _events) = ChiefProcessBridge::start(
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
				("thread/goal/get", json!({"goal":null})),
				(
					"account/usage/read",
					json!({"threadUsage":{"threadId":"thread","estimatedUsageCreditsMicros":17,"groups":[]}}),
				),
				("thread/attachment/list", json!({"data":[],"nextCursor":null})),
				("mcpServerStatus/list", json!({"data":[],"nextCursor":null})),
				("plugin/installed", json!({"marketplaces":[],"marketplaceLoadErrors":[]})),
				(
					"plugin/reconcile",
					json!({"changedPlugins":[],"failedRemotePluginIds":[],"failedMaterializationRemotePluginIds":[]}),
				),
				("config/mcpServer/reload", json!({})),
				("app/list", json!({"data":[],"nextCursor":null})),
			] {
				let Some(Ok(line)) = lines.next() else {
					return;
				};
				let request: Value = serde_json::from_str(&line).unwrap();
				assert_eq!(request["method"], method);
				let response =
					serde_json::to_vec(&json!({"id":request["id"],"result":result})).unwrap();
				sender.send(InboundFrame::fixture(&response)).unwrap();
			}
		});
		assert!(
			client
				.request("thread/goal/get", serde_json::json!({"threadId":"thread"}))
				.await
				.unwrap()["goal"]
				.is_null()
		);
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
			json!([])
		);
		assert!(client.refresh_integrations("thread").await.unwrap());
		server.join().unwrap();
		drop(bridge);
	}
}
