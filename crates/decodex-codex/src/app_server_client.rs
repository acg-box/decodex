//! Multiplexed app-server stdio transport. The caller owns authorization, environment,
//! event consumption, and process lifetime. No request is retried by this transport.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, fmt, process::Stdio};
use tokio::{
	io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
	process::{Child, Command},
	sync::{mpsc, oneshot, watch},
};

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_REQUESTS: usize = 256;
const MAX_BUFFERED_EVENTS: usize = 256;

/// Server request identifiers must be echoed without conversion.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
	/// Numeric JSON-RPC identity.
	Number(i64),
	/// Opaque string identity; never parse it as a number.
	String(String),
}

/// Error payloads are available to the caller but are never included in Debug or Display.
#[derive(Clone, Deserialize, Serialize)]
pub struct RpcError {
	/// Provider error code safe for diagnostics.
	pub code: i64,
	/// Provider message, which can contain private request context.
	pub message: String,
	/// Optional private error details.
	pub data: Option<Value>,
}

impl fmt::Debug for RpcError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("RpcError").field("code", &self.code).finish_non_exhaustive()
	}
}

/// A transport failure after submission is an unknown dispatch outcome, never retry authority.
#[derive(Clone, Debug)]
pub enum ClientError {
	/// The connection has closed or was revoked.
	Closed,
	/// An input/output operation failed; details are intentionally omitted.
	Io,
	/// A frame did not conform to the JSON-RPC contract.
	InvalidFrame,
	/// A frame exceeded the fixed wire bound.
	FrameTooLarge,
	/// A request or event queue exceeded its bound.
	CapacityExceeded,
	/// The provider rejected a request.
	Remote(RpcError),
}

impl fmt::Display for ClientError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "app-server: {self:?}")
	}
}
impl std::error::Error for ClientError {}

/// The owner must continuously consume this stream. It contains server notifications,
/// approval requests, and tool requests; no default approval response is sent.
/// A full event queue closes the connection with CapacityExceeded rather than silently
/// dropping events and continuing with incomplete execution evidence.
pub enum ServerEvent {
	/// One asynchronous notification, correlated by its provider parameters.
	Notification {
		/// Provider method name.
		method: String,
		/// Notification parameters; the owner controls disclosure.
		params: Value,
	},
	/// A server request that needs an explicit owner response.
	Request {
		/// Exact identity to echo in the response.
		id: RequestId,
		/// Provider method name.
		method: String,
		/// Request parameters; no implied approval is granted.
		params: Value,
	},
	/// A response with no matching local request, retained for reconciliation.
	UnmatchedResponse {
		/// Unmatched provider identity.
		id: RequestId,
		/// Provider response without interpretation or replay.
		result: Result<Value, RpcError>,
	},
	/// Terminal connection failure.
	Closed(ClientError),
}

type Reply = oneshot::Sender<Result<Value, ClientError>>;
enum Outbound {
	Request { method: String, params: Value, reply: Reply },
	Message { value: Value, reply: Reply },
	Shutdown { reply: Reply },
}

/// Clones share one connection and one request-ID namespace across independent threads.
#[derive(Clone)]
pub struct AppServerClient {
	outbound: mpsc::Sender<Outbound>,
	closed: watch::Sender<bool>,
}

/// Explicit process owner. Dropping a client or completing a turn never kills this process.
#[must_use = "Keep the process owner and explicitly shut it down when the session ends"]
pub struct AppServerProcess {
	child: Child,
	client: AppServerClient,
}

impl AppServerProcess {
	/// Return the exact child PID while the process owner retains it.
	pub fn id(&self) -> Option<u32> {
		self.child.id()
	}

	/// Stop the exact child if still live, reap it, and close the transport.
	pub async fn shutdown(&mut self) -> Result<(), ClientError> {
		match self.child.try_wait().map_err(|_| ClientError::Io)? {
			Some(_) => {},
			None => {
				self.child.start_kill().map_err(|_| ClientError::Io)?;
				self.child.wait().await.map_err(|_| ClientError::Io)?;
			},
		}
		let _ = self.client.shutdown().await;
		Ok(())
	}
}

impl AppServerClient {
	/// The caller supplies executable, arguments, cwd, and environment. This function
	/// never reads or writes authentication. Stderr is discarded to prevent secret logging.
	pub fn spawn(
		command: &mut Command,
	) -> Result<(Self, mpsc::Receiver<ServerEvent>, AppServerProcess), ClientError> {
		let mut child = command
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.kill_on_drop(false)
			.spawn()
			.map_err(|_| ClientError::Io)?;
		let stdin = child.stdin.take().ok_or(ClientError::Io)?;
		let stdout = child.stdout.take().ok_or(ClientError::Io)?;
		let (client, events) = Self::from_io(stdout, stdin);
		let process = AppServerProcess { child, client: client.clone() };
		Ok((client, events, process))
	}

	/// Attach an existing transport. Useful for caller-owned processes and tests.
	pub fn from_io<R, W>(reader: R, writer: W) -> (Self, mpsc::Receiver<ServerEvent>)
	where
		R: AsyncRead + Unpin + Send + 'static,
		W: AsyncWrite + Unpin + Send + 'static,
	{
		let (outbound, commands) = mpsc::channel(64);
		let (events, receiver) = mpsc::channel(MAX_BUFFERED_EVENTS);
		let (closed, cancellation) = watch::channel(false);
		tokio::spawn(run(reader, writer, commands, events, cancellation));
		(Self { outbound, closed }, receiver)
	}

	/// Attach a caller-owned framed transport after its private initialization. The caller
	/// must remove credential callbacks before forwarding frames and enforce wire bounds.
	/// `next_request_id` is the first unused ID on this connection.
	pub fn from_framed(
		next_request_id: i64,
		incoming: mpsc::Receiver<Result<Value, ClientError>>,
		outgoing: mpsc::Sender<Value>,
	) -> Result<(Self, mpsc::Receiver<ServerEvent>), ClientError> {
		if next_request_id < 1 {
			return Err(ClientError::InvalidFrame);
		}
		let (outbound, commands) = mpsc::channel(64);
		let (events, receiver) = mpsc::channel(MAX_BUFFERED_EVENTS);
		let (closed, cancellation) = watch::channel(false);
		tokio::spawn(run_frames(
			FrameSink::Channel(outgoing),
			commands,
			events,
			incoming,
			next_request_id - 1,
			cancellation,
		));
		Ok((Self { outbound, closed }, receiver))
	}

	/// Revoke all clones without waiting. Already submitted requests remain ambiguous.
	pub fn close(&self) {
		self.closed.send_replace(true);
	}

	/// Send one RPC and await its correlated reply without automatic retry.
	pub async fn request(
		&self,
		method: impl Into<String>,
		params: Value,
	) -> Result<Value, ClientError> {
		if *self.closed.borrow() {
			return Err(ClientError::Closed);
		}
		let (reply, result) = oneshot::channel();
		self.outbound
			.send(Outbound::Request { method: method.into(), params, reply })
			.await
			.map_err(|_| ClientError::Closed)?;
		result.await.unwrap_or(Err(ClientError::Closed))
	}

	async fn message(&self, value: Value) -> Result<(), ClientError> {
		if *self.closed.borrow() {
			return Err(ClientError::Closed);
		}
		let (reply, result) = oneshot::channel();
		self.outbound
			.send(Outbound::Message { value, reply })
			.await
			.map_err(|_| ClientError::Closed)?;
		result.await.unwrap_or(Err(ClientError::Closed)).map(|_| ())
	}

	/// Complete the initialization handshake on a fresh connection only.
	pub async fn initialize(&self, params: Value) -> Result<Value, ClientError> {
		let result = self.request("initialize", params).await?;
		self.notify("initialized", Value::Null).await?;
		Ok(result)
	}

	/// Send a notification without allocating a request identity.
	pub async fn notify(&self, method: &str, params: Value) -> Result<(), ClientError> {
		self.message(json!({"method": method, "params": params})).await
	}

	/// Return a result for one exact server request.
	pub async fn respond(&self, id: RequestId, result: Value) -> Result<(), ClientError> {
		self.message(json!({"id": id, "result": result})).await
	}

	/// Return an explicit error for one exact server request.
	pub async fn respond_error(&self, id: RequestId, error: RpcError) -> Result<(), ClientError> {
		self.message(json!({"id": id, "error": error})).await
	}

	/// Request a new independent provider thread.
	pub async fn thread_start(&self, params: Value) -> Result<Value, ClientError> {
		self.request("thread/start", params).await
	}

	/// Load an existing provider thread without starting a turn.
	pub async fn thread_resume(&self, params: Value) -> Result<Value, ClientError> {
		self.request("thread/resume", params).await
	}

	/// Read provider thread evidence without executing work.
	pub async fn thread_read(&self, params: Value) -> Result<Value, ClientError> {
		self.request("thread/read", params).await
	}

	/// Submit one turn; the owner must fence uncertain acceptance.
	pub async fn turn_start(&self, params: Value) -> Result<Value, ClientError> {
		self.request("turn/start", params).await
	}

	/// Send additional input to the exact active turn selected by the owner.
	pub async fn turn_steer(&self, params: Value) -> Result<Value, ClientError> {
		self.request("turn/steer", params).await
	}

	/// Request interruption of an exact turn without terminating peer threads.
	pub async fn turn_interrupt(&self, params: Value) -> Result<Value, ClientError> {
		self.request("turn/interrupt", params).await
	}

	/// Close this connection for all clones. Does not itself terminate an owned process.
	pub async fn shutdown(&self) -> Result<(), ClientError> {
		let (reply, result) = oneshot::channel();
		self.outbound.send(Outbound::Shutdown { reply }).await.map_err(|_| ClientError::Closed)?;
		result.await.unwrap_or(Err(ClientError::Closed)).map(|_| ())
	}
}

async fn write_frame<W: AsyncWrite + Unpin>(
	writer: &mut W,
	value: Value,
) -> Result<(), ClientError> {
	let mut bytes = serde_json::to_vec(&value).map_err(|_| ClientError::InvalidFrame)?;
	if bytes.len() > MAX_FRAME_BYTES {
		return Err(ClientError::FrameTooLarge);
	}
	bytes.push(b'\n');
	tokio::time::timeout(std::time::Duration::from_secs(30), async {
		writer.write_all(&bytes).await.map_err(|_| ClientError::Io)?;
		writer.flush().await.map_err(|_| ClientError::Io)
	})
	.await
	.map_err(|_| ClientError::Io)?
}

async fn run<R, W>(
	reader: R,
	writer: W,
	commands: mpsc::Receiver<Outbound>,
	events: mpsc::Sender<ServerEvent>,
	cancellation: watch::Receiver<bool>,
) where
	R: AsyncRead + Unpin + Send + 'static,
	W: AsyncWrite + Unpin + Send + 'static,
{
	let (frames_tx, frames) = mpsc::channel(64);
	// A dedicated reader makes cancellation of the actor select safe, even mid-frame.
	let reader_task = tokio::spawn(async move {
		let mut reader = BufReader::new(reader);
		loop {
			let mut bytes = Vec::new();
			let read = (&mut reader)
				.take((MAX_FRAME_BYTES + 1) as u64)
				.read_until(b'\n', &mut bytes)
				.await;
			let frame = match read {
				Err(_) => Err(ClientError::Io),
				Ok(0) => Err(ClientError::Closed),
				Ok(_) if bytes.len() > MAX_FRAME_BYTES => Err(ClientError::FrameTooLarge),
				Ok(_) if bytes.last() != Some(&b'\n') => Err(ClientError::InvalidFrame),
				Ok(_) =>
					serde_json::from_slice::<Value>(&bytes).map_err(|_| ClientError::InvalidFrame),
			};
			let terminal = frame.is_err();
			if frames_tx.send(frame).await.is_err() || terminal {
				break;
			}
		}
	});
	run_frames(FrameSink::Io(Box::new(writer)), commands, events, frames, 0, cancellation).await;
	reader_task.abort();
}

enum FrameSink {
	Io(Box<dyn AsyncWrite + Unpin + Send>),
	Channel(mpsc::Sender<Value>),
}
impl FrameSink {
	async fn write(&mut self, value: Value) -> Result<(), ClientError> {
		match self {
			Self::Io(writer) => write_frame(writer, value).await,
			Self::Channel(sender) => {
				if serde_json::to_vec(&value).map_err(|_| ClientError::InvalidFrame)?.len()
					> MAX_FRAME_BYTES
				{
					return Err(ClientError::FrameTooLarge);
				}
				sender.try_send(value).map_err(|error| match error {
					mpsc::error::TrySendError::Full(_) => ClientError::CapacityExceeded,
					mpsc::error::TrySendError::Closed(_) => ClientError::Closed,
				})
			},
		}
	}
}

async fn run_frames(
	mut writer: FrameSink,
	mut commands: mpsc::Receiver<Outbound>,
	events: mpsc::Sender<ServerEvent>,
	mut frames: mpsc::Receiver<Result<Value, ClientError>>,
	mut sequence: i64,
	mut cancellation: watch::Receiver<bool>,
) {
	let mut pending: HashMap<RequestId, Reply> = HashMap::new();
	let reason = loop {
		tokio::select! {
			biased;
			_ = cancellation.changed() => { break ClientError::Closed; },
			command = commands.recv() => {
				let Some(command) = command else { break ClientError::Closed; };
				match command {
					Outbound::Shutdown { reply } => {
						let _ = reply.send(Ok(Value::Null));
						break ClientError::Closed;
					},
					Outbound::Message { value, reply } => {
						let result = writer.write(value).await;
						let _ = reply.send(result.clone().map(|_| Value::Null));
						if let Err(error) = result { break error; }
					},
					Outbound::Request { method, params, reply } => {
						if pending.len() >= MAX_PENDING_REQUESTS {
							let _ = reply.send(Err(ClientError::CapacityExceeded));
							continue;
						}
						let Some(next) = sequence.checked_add(1) else { break ClientError::Closed; };
						sequence = next;
						let id = RequestId::Number(sequence);
						pending.insert(id.clone(), reply);
						if let Err(error) = writer.write(json!({"id": id, "method": method, "params": params})).await { break error; }
					},
				}
			},
			frame = frames.recv() => {
				let frame = match frame { Some(Ok(frame)) => frame, Some(Err(error)) => break error, None => break ClientError::Closed };
				if let Err(error) = dispatch(frame, &mut pending, &events) { break error; }
			},
		}
	};
	drop(writer);
	commands.close();
	for (_, reply) in pending {
		let _ = reply.send(Err(reason.clone()));
	}
	while let Some(command) = commands.recv().await {
		let reply = match command {
			Outbound::Request { reply, .. }
			| Outbound::Message { reply, .. }
			| Outbound::Shutdown { reply } => reply,
		};
		let _ = reply.send(Err(reason.clone()));
	}
	let _ = events.send(ServerEvent::Closed(reason)).await;
}

fn dispatch(
	frame: Value,
	pending: &mut HashMap<RequestId, Reply>,
	events: &mpsc::Sender<ServerEvent>,
) -> Result<(), ClientError> {
	let object = frame.as_object().ok_or(ClientError::InvalidFrame)?;
	let id = object
		.get("id")
		.map(|id| {
			serde_json::from_value::<RequestId>(id.clone()).map_err(|_| ClientError::InvalidFrame)
		})
		.transpose()?;
	let event = if let Some(method) = object.get("method") {
		if object.contains_key("result") || object.contains_key("error") {
			return Err(ClientError::InvalidFrame);
		}
		let method = method.as_str().ok_or(ClientError::InvalidFrame)?.to_owned();
		let params = object.get("params").cloned().unwrap_or(Value::Null);
		match id {
			Some(id) => ServerEvent::Request { id, method, params },
			None => ServerEvent::Notification { method, params },
		}
	} else {
		let id = id.ok_or(ClientError::InvalidFrame)?;
		let result = match (object.get("result"), object.get("error")) {
			(Some(result), None) => Ok(result.clone()),
			(None, Some(error)) => Err(serde_json::from_value::<RpcError>(error.clone())
				.map_err(|_| ClientError::InvalidFrame)?),
			_ => return Err(ClientError::InvalidFrame),
		};
		if let Some(reply) = pending.remove(&id) {
			let _ = reply.send(result.map_err(ClientError::Remote));
			return Ok(());
		}
		ServerEvent::UnmatchedResponse { id, result }
	};
	events.try_send(event).map_err(|error| match error {
		mpsc::error::TrySendError::Full(_) => ClientError::CapacityExceeded,
		mpsc::error::TrySendError::Closed(_) => ClientError::Closed,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::{
		io::{DuplexStream, ReadHalf, WriteHalf},
		time::{Duration, timeout},
	};

	fn connection() -> (
		AppServerClient,
		mpsc::Receiver<ServerEvent>,
		BufReader<ReadHalf<DuplexStream>>,
		WriteHalf<DuplexStream>,
	) {
		let (client, server) = tokio::io::duplex(65536);
		let (read, write) = tokio::io::split(client);
		let (client, events) = AppServerClient::from_io(read, write);
		let (read, write) = tokio::io::split(server);
		(client, events, BufReader::new(read), write)
	}

	async fn read(reader: &mut BufReader<ReadHalf<DuplexStream>>) -> Value {
		let mut line = String::new();
		timeout(Duration::from_secs(2), reader.read_line(&mut line)).await.unwrap().unwrap();
		serde_json::from_str(&line).unwrap()
	}

	#[tokio::test]
	async fn interleaved_threads_out_of_order_replies_and_parent_completion_preserve_peer() {
		let (client, mut events, mut reader, mut writer) = connection();
		let parent = client.clone();
		let first =
			tokio::spawn(async move { parent.turn_start(json!({"threadId":"parent"})).await });
		let first_wire = read(&mut reader).await;
		let peer = client.clone();
		let second = tokio::spawn(async move { peer.turn_start(json!({"threadId":"peer"})).await });
		let second_wire = read(&mut reader).await;
		assert_ne!(first_wire["id"], second_wire["id"]);
		write_frame(
			&mut writer,
			json!({"id": second_wire["id"], "result":{"turn":{"id":"peer-turn"}}}),
		)
		.await
		.unwrap();
		write_frame(&mut writer, json!({"method":"turn/completed","params":{"threadId":"parent"}}))
			.await
			.unwrap();
		write_frame(
			&mut writer,
			json!({"method":"item/agentMessage/delta","params":{"threadId":"peer","delta":"still working"}}),
		)
		.await
		.unwrap();
		write_frame(
			&mut writer,
			json!({"id": first_wire["id"], "result":{"turn":{"id":"parent-turn"}}}),
		)
		.await
		.unwrap();
		assert_eq!(second.await.unwrap().unwrap()["turn"]["id"], "peer-turn");
		assert_eq!(first.await.unwrap().unwrap()["turn"]["id"], "parent-turn");
		assert!(
			matches!(events.recv().await, Some(ServerEvent::Notification { method, params }) if method == "turn/completed" && params["threadId"] == "parent")
		);
		assert!(
			matches!(events.recv().await, Some(ServerEvent::Notification { params, .. }) if params["threadId"] == "peer")
		);
		let next_client = client.clone();
		let next = tokio::spawn(async move {
			next_client
				.turn_steer(json!({"threadId":"peer","expectedTurnId":"peer-turn","input":[]}))
				.await
		});
		let next_wire = read(&mut reader).await;
		assert_eq!(next_wire["method"], "turn/steer");
		write_frame(&mut writer, json!({"id":next_wire["id"],"result":{}})).await.unwrap();
		next.await.unwrap().unwrap();
		client.shutdown().await.unwrap();
	}

	#[tokio::test]
	async fn approval_requests_are_preserved_and_only_explicitly_answered() {
		let (client, mut events, mut reader, mut writer) = connection();
		write_frame(&mut writer, json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"threadId":"peer","command":"echo test"}})).await.unwrap();
		let event = events.recv().await.unwrap();
		let ServerEvent::Request { id, method, params } = event else {
			panic!("expected request");
		};
		assert_eq!(method, "item/commandExecution/requestApproval");
		assert_eq!(params["threadId"], "peer");
		// There is no automatic response, including for an unknown request method.
		let mut byte = [0];
		assert!(timeout(Duration::from_millis(20), reader.read(&mut byte)).await.is_err());
		client.respond(id, json!({"decision":"decline"})).await.unwrap();
		assert_eq!(
			read(&mut reader).await,
			json!({"id":"approval-1","result":{"decision":"decline"}})
		);
		client.shutdown().await.unwrap();
	}

	#[tokio::test]
	async fn eof_drains_every_pending_request_without_replay() {
		let (client, mut events, mut reader, writer) = connection();
		let first_client = client.clone();
		let first =
			tokio::spawn(async move { first_client.thread_read(json!({"threadId":"one"})).await });
		read(&mut reader).await;
		let second_client = client.clone();
		let second =
			tokio::spawn(async move { second_client.turn_start(json!({"threadId":"two"})).await });
		read(&mut reader).await;
		drop(writer);
		drop(reader);
		assert!(matches!(
			timeout(Duration::from_secs(2), first).await.unwrap().unwrap(),
			Err(ClientError::Closed)
		));
		assert!(matches!(second.await.unwrap(), Err(ClientError::Closed)));
		assert!(matches!(events.recv().await, Some(ServerEvent::Closed(ClientError::Closed))));
		assert!(matches!(client.thread_start(json!({})).await, Err(ClientError::Closed)));
	}

	#[tokio::test]
	async fn malformed_response_fails_pending_requests() {
		let (client, mut events, mut reader, mut writer) = connection();
		let requester = client.clone();
		let request = tokio::spawn(async move { requester.thread_start(json!({})).await });
		let wire = read(&mut reader).await;
		write_frame(
			&mut writer,
			json!({"id":wire["id"], "result":{}, "error":{"code":1,"message":"private"}}),
		)
		.await
		.unwrap();
		assert!(matches!(request.await.unwrap(), Err(ClientError::InvalidFrame)));
		assert!(matches!(
			events.recv().await,
			Some(ServerEvent::Closed(ClientError::InvalidFrame))
		));
	}

	#[tokio::test]
	async fn framed_attachment_preserves_initialization_ids_and_revokes_clones() {
		let (incoming, frames) = mpsc::channel(4);
		let (outgoing, mut requests) = mpsc::channel(4);
		let (client, _events) = AppServerClient::from_framed(41, frames, outgoing).unwrap();
		let first_client = client.clone();
		let first =
			tokio::spawn(async move { first_client.thread_read(json!({"threadId":"one"})).await });
		let frame = requests.recv().await.unwrap();
		assert_eq!(frame["id"], 41);
		incoming.send(Ok(json!({"id":41,"result":{}}))).await.unwrap();
		first.await.unwrap().unwrap();
		let peer = client.clone();
		let pending = tokio::spawn(async move { peer.turn_start(json!({"threadId":"two"})).await });
		assert_eq!(requests.recv().await.unwrap()["id"], 42);
		client.close();
		assert!(matches!(pending.await.unwrap(), Err(ClientError::Closed)));
		assert!(matches!(client.thread_start(json!({})).await, Err(ClientError::Closed)));
	}

	#[tokio::test]
	async fn event_overflow_closes_connection_with_explicit_failure() {
		let (client, mut events, mut reader, mut writer) = connection();
		let request = tokio::spawn(async move { client.turn_start(json!({})).await });
		read(&mut reader).await;
		for _ in 0..=MAX_BUFFERED_EVENTS {
			write_frame(&mut writer, json!({"method":"tick","params":{}})).await.unwrap();
		}
		assert!(matches!(
			timeout(Duration::from_secs(2), request).await.unwrap().unwrap(),
			Err(ClientError::CapacityExceeded)
		));
		for _ in 0..MAX_BUFFERED_EVENTS {
			assert!(matches!(events.recv().await, Some(ServerEvent::Notification { .. })));
		}
		assert!(matches!(
			events.recv().await,
			Some(ServerEvent::Closed(ClientError::CapacityExceeded))
		));
	}

	#[cfg(unix)]
	#[tokio::test]
	async fn process_survives_turn_completion_until_explicit_shutdown() {
		let mut command = Command::new("/bin/sh");
		command.args(["-c", "printf '%s\\n' '{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"parent\"}}'; exec cat"]);
		let (_client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		assert!(
			matches!(timeout(Duration::from_secs(2), events.recv()).await.unwrap(), Some(ServerEvent::Notification { method, .. }) if method == "turn/completed")
		);
		assert!(process.child.try_wait().unwrap().is_none());
		process.shutdown().await.unwrap();
		assert!(process.child.try_wait().unwrap().is_some());
	}

	/// Explicit paid smoke: set DECODEX_SMOKE_MODEL and DECODEX_SMOKE_CWD, then select
	/// this ignored test by exact name. No alternate model is selected.
	#[tokio::test]
	#[ignore = "explicitly submits two small provider turns using the configured account"]
	async fn real_two_thread_smoke() {
		let model = std::env::var("DECODEX_SMOKE_MODEL").expect("set DECODEX_SMOKE_MODEL");
		let cwd = std::env::var("DECODEX_SMOKE_CWD").expect("set DECODEX_SMOKE_CWD");
		let executable = std::env::var("DECODEX_SMOKE_CODEX").unwrap_or_else(|_| "codex".into());
		let mut command = Command::new(executable);
		command.arg("app-server").current_dir(&cwd);
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		let result = timeout(Duration::from_secs(120), async {
            client.initialize(json!({"clientInfo":{"name":"decodex-transport-smoke","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
            let models = client.request("model/list", json!({})).await?;
            if !models["data"].as_array().is_some_and(|models| models.iter().any(|entry| entry["model"] == model)) {
                return Err(ClientError::InvalidFrame);
            }
            let params = json!({"model":model,"cwd":cwd,"sandbox":"read-only","approvalPolicy":"never","ephemeral":true,"config":{"model_reasoning_effort":"medium"}});
            let (one, two) = tokio::try_join!(client.thread_start(params.clone()), client.thread_start(params))?;
            for thread in [&one, &two] {
                if thread["model"] != model || thread["reasoningEffort"] != "medium" {
                    return Err(ClientError::InvalidFrame);
                }
            }
            let one = one["thread"]["id"].as_str().ok_or(ClientError::InvalidFrame)?;
            let two = two["thread"]["id"].as_str().ok_or(ClientError::InvalidFrame)?;
            if one == two { return Err(ClientError::InvalidFrame); }
            let input = json!([{"type":"text","text":"Reply with exactly OK. Do not use any tools."}]);
            tokio::try_join!(client.turn_start(json!({"threadId":one,"input":input,"effort":"medium"})), client.turn_start(json!({"threadId":two,"input":input,"effort":"medium"})))?;
            let mut completed = std::collections::HashSet::new();
            while completed.len() < 2 {
                match events.recv().await {
                    Some(ServerEvent::Notification { method, params }) if method == "turn/completed" => {
                        let thread = params["threadId"].as_str().ok_or(ClientError::InvalidFrame)?;
                        if thread != one && thread != two { return Err(ClientError::InvalidFrame); }
                        if params["turn"]["status"] != "completed" { return Err(ClientError::InvalidFrame); }
                        completed.insert(thread.to_owned());
                    },
                    Some(ServerEvent::Request { id, .. }) => {
                        client.respond_error(id, RpcError { code: -32601, message: "Smoke test does not execute tools".into(), data: None }).await?;
                    },
                    Some(ServerEvent::Closed(error)) => return Err(error),
                    None => return Err(ClientError::Closed),
                    _ => {},
                }
            }
            client.thread_read(json!({"threadId":two})).await?;
            Ok::<_, ClientError>(())
        }).await;
		process.shutdown().await.unwrap();
		assert!(matches!(result, Ok(Ok(()))), "live smoke failed: {result:?}");
	}

	/// Opt in with cargo test -p decodex-codex app_server_client::tests::real_app_server_smoke --
	/// --ignored. This only initializes the installed server; it does not submit a provider turn.
	#[tokio::test]
	#[ignore = "requires an installed codex executable"]
	async fn real_app_server_smoke() {
		let mut command = Command::new("codex");
		command.arg("app-server");
		let (client, _events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		let result = timeout(Duration::from_secs(15), client.initialize(json!({"clientInfo":{"name":"decodex-transport-smoke","version":"0.1.0"},"capabilities":{"experimentalApi":true}}))).await;
		process.shutdown().await.unwrap();
		assert!(matches!(result, Ok(Ok(_))));
	}
}
