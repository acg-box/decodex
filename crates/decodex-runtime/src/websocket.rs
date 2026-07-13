//! Maintained Axum WebSocket transport, resumable publication, and session policy.

use std::{
	collections::{HashMap, VecDeque},
	fmt::{Display, Formatter},
	net::SocketAddr,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use axum::{
	Router,
	extract::{
		State,
		ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
	},
	response::Response,
	routing,
};
use futures_util::{
	SinkExt as _, StreamExt as _,
	stream::{SplitSink, SplitStream},
};
use tokio::{
	net::TcpListener,
	sync::{Mutex, mpsc, mpsc::Receiver, oneshot},
	task::{JoinError, JoinHandle},
	time,
};

use crate::{Application, ApplicationPublication};
use decodex_protocol::{
	self, CausationId, ClientCommandId, ClientHello, ClientMessage, CommandEnvelope, CommandError,
	CommandOutcome, CommandReceipt, CommandResultEnvelope, CorrelationId, Cursor,
	EndpointPolicyError, EventEnvelope, IdempotencyKey, LoopbackEndpoint, ProtocolVersion,
	ReceiptDisposition, ReconnectMode, Refusal, RefusalEnvelope, ResumeCursor, ServerId,
	ServerMessage, ServerWelcome, SnapshotEnvelope, SnapshotItem, SupportedVersions, WireText,
};

const WS_PATH: &str = "/v1/ws";

/// Bounded transport settings. None of these enable remote binding.
#[derive(Clone, Debug)]
pub struct ServerConfig {
	/// Maximum number of events retained for cursor resume.
	pub replay_capacity: usize,
	/// Maximum number of pending messages for one client.
	pub outbound_queue_capacity: usize,
	/// Maximum number of logical commands retained for lifetime deduplication.
	pub receipt_capacity: usize,
	/// Maximum number of small-state items in one snapshot.
	pub maximum_snapshot_items: usize,
	/// Maximum accepted UTF-8 message size.
	pub maximum_message_bytes: usize,
	/// Time allowed for the mandatory first hello message.
	pub hello_timeout: Duration,
	/// Time allowed for one WebSocket write.
	pub write_timeout: Duration,
}
impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			replay_capacity: 1_024,
			outbound_queue_capacity: 64,
			receipt_capacity: 4_096,
			maximum_snapshot_items: 1_024,
			maximum_message_bytes: 256 * 1_024,
			hello_timeout: Duration::from_secs(5),
			write_timeout: Duration::from_secs(5),
		}
	}
}

/// A running loopback server and its graceful-shutdown handle.
pub struct BoundServer {
	address: SocketAddr,
	shutdown_sender: Option<oneshot::Sender<()>>,
	task: Option<JoinHandle<Result<(), ServerError>>>,
}
impl BoundServer {
	/// Return the operating-system-selected listening address.
	pub const fn address(&self) -> SocketAddr {
		self.address
	}

	/// Request graceful shutdown and wait for the server task.
	pub async fn shutdown(mut self) -> Result<(), ServerError> {
		if let Some(sender) = self.shutdown_sender.take() {
			let _ = sender.send(());
		}

		self.wait_task().await?;

		Ok(())
	}

	async fn wait(mut self) -> Result<(), ServerError> {
		self.wait_task().await
	}

	async fn wait_task(&mut self) -> Result<(), ServerError> {
		let task = self.task.take().expect("bound server task already consumed");

		task.await.map_err(ServerError::Join)??;

		Ok(())
	}
}

impl Drop for BoundServer {
	fn drop(&mut self) {
		if let Some(sender) = self.shutdown_sender.take() {
			let _ = sender.send(());
		}
	}
}

/// A loopback WebSocket server over one application-service implementation.
pub struct ProtocolServer<A>
where
	A: Application,
{
	inner: Arc<ServerInner<A>>,
}
impl<A> ProtocolServer<A>
where
	A: Application,
{
	/// Build a server with one lifetime identity and application owner.
	pub fn new(server_id: ServerId, application: A, config: ServerConfig) -> Self {
		assert!(config.replay_capacity > 0, "replay capacity must be non-zero");
		assert!(config.outbound_queue_capacity > 0, "outbound queue capacity must be non-zero");
		assert!(config.receipt_capacity > 0, "receipt capacity must be non-zero");

		Self {
			inner: Arc::new(ServerInner {
				server_id,
				application,
				config,
				connection_ids: AtomicU64::new(1),
				state: Mutex::new(PublicationState::default()),
			}),
		}
	}

	/// Bind and spawn the service after validating the address as loopback.
	pub async fn bind(self, address: SocketAddr) -> Result<BoundServer, ServerError> {
		let endpoint = LoopbackEndpoint::new(address).map_err(ServerError::EndpointPolicy)?;
		let listener = TcpListener::bind(endpoint.address()).await.map_err(ServerError::Io)?;
		let local_address = listener.local_addr().map_err(ServerError::Io)?;
		let router = Router::new().route(WS_PATH, routing::any(upgrade::<A>)).with_state(self);
		let (shutdown_sender, shutdown_receiver) = oneshot::channel();
		let task = tokio::spawn(async move {
			axum::serve(listener, router)
				.with_graceful_shutdown(async {
					let _ = shutdown_receiver.await;
				})
				.await
				.map_err(ServerError::Io)
		});

		Ok(BoundServer {
			address: local_address,
			shutdown_sender: Some(shutdown_sender),
			task: Some(task),
		})
	}

	/// Run the service until it is externally stopped.
	pub async fn run(self, address: SocketAddr) -> Result<(), ServerError> {
		self.bind(address).await?.wait().await
	}
}

impl<A> ProtocolServer<A>
where
	A: Application,
{
	async fn handle_connection(&self, mut socket: WebSocket) {
		let Some(hello) = self.receive_hello(&mut socket).await else {
			return;
		};
		let negotiated = match hello.version.negotiate() {
			Ok(version) => version,
			Err(refusal) => {
				let _ = self
					.send_direct(
						&mut socket,
						ServerMessage::Refusal(RefusalEnvelope {
							server_id: self.inner.server_id.clone(),
							refusal: Refusal::UnsupportedVersion(refusal),
						}),
					)
					.await;

				return;
			},
		};
		let connection_id = self.inner.connection_ids.fetch_add(1, Ordering::Relaxed);
		let (sender, receiver) = mpsc::channel(self.inner.config.outbound_queue_capacity);
		let initial = match self.prepare_session(connection_id, sender, hello, negotiated).await {
			Ok(messages) => messages,
			Err(refusal) => {
				let _ = self
					.send_direct(
						&mut socket,
						ServerMessage::Refusal(RefusalEnvelope {
							server_id: self.inner.server_id.clone(),
							refusal,
						}),
					)
					.await;

				return;
			},
		};

		for message in initial {
			if !self.send_direct(&mut socket, message).await {
				self.remove_subscriber(connection_id).await;

				return;
			}
		}

		let (socket_sender, socket_receiver) = socket.split();
		let reader = self.read_commands(socket_receiver, connection_id, negotiated);
		let writer = self.write_messages(socket_sender, receiver);

		tokio::pin!(reader, writer);

		tokio::select! {
			() = &mut writer => {},
			backpressure = &mut reader => {
				if backpressure {
					writer.await;
				}
			},
		}

		self.remove_subscriber(connection_id).await;
	}

	async fn receive_hello(&self, socket: &mut WebSocket) -> Option<ClientHello> {
		let received =
			time::timeout(self.inner.config.hello_timeout, socket.recv()).await.ok()??;
		let message = received.ok()?;
		let Message::Text(text) = message else {
			let refusal =
				protocol_refusal(&self.inner.server_id, "first message must be text hello");
			let _ = self.send_direct(socket, refusal).await;

			return None;
		};

		match decodex_protocol::decode_client_message(&text) {
			Ok(ClientMessage::Hello(hello)) => Some(hello),
			Ok(ClientMessage::Command(_)) => {
				let refusal =
					protocol_refusal(&self.inner.server_id, "hello is required before command");
				let _ = self.send_direct(socket, refusal).await;

				None
			},
			Err(_) => {
				let refusal = protocol_refusal(
					&self.inner.server_id,
					"message is not a valid client envelope",
				);
				let _ = self.send_direct(socket, refusal).await;

				None
			},
		}
	}

	async fn prepare_session(
		&self,
		connection_id: u64,
		sender: mpsc::Sender<ServerMessage>,
		hello: ClientHello,
		negotiated: ProtocolVersion,
	) -> Result<Vec<ServerMessage>, Refusal> {
		let state = self.inner.state.lock().await;
		let snapshot_items = self.inner.application.snapshot().await;
		let mut state = state;

		if snapshot_items.len() > self.inner.config.maximum_snapshot_items {
			return Err(Refusal::ProtocolViolation {
				message: bounded_text("application snapshot exceeds the bounded item limit"),
			});
		}

		let (reconnect, mut messages) =
			self.reconnect_messages(&state, hello.resume.as_ref(), negotiated, snapshot_items);

		messages.insert(
			0,
			ServerMessage::Welcome(ServerWelcome {
				version: negotiated,
				supported: SupportedVersions::current(),
				server_id: self.inner.server_id.clone(),
				cursor: state.cursor,
				reconnect,
			}),
		);
		state.subscribers.insert(connection_id, Subscriber { sender, version: negotiated });

		Ok(messages)
	}

	fn reconnect_messages(
		&self,
		state: &PublicationState,
		resume: Option<&ResumeCursor>,
		version: ProtocolVersion,
		snapshot_items: Vec<SnapshotItem>,
	) -> (ReconnectMode, Vec<ServerMessage>) {
		let can_resume = resume.is_some_and(|resume| {
			resume.server_id == self.inner.server_id
				&& resume.cursor <= state.cursor
				&& state
					.events
					.front()
					.is_none_or(|oldest| resume.cursor.0.saturating_add(1) >= oldest.cursor.0)
		});

		if can_resume {
			let cursor = resume.expect("checked above").cursor;
			let deltas = state
				.events
				.iter()
				.filter(|event| event.cursor > cursor)
				.cloned()
				.map(|mut event| {
					event.version = version;

					ServerMessage::Event(event)
				})
				.collect();

			return (ReconnectMode::Resume, deltas);
		}

		let reconnect = if resume.is_some() {
			ReconnectMode::SnapshotFallback
		} else {
			ReconnectMode::Snapshot
		};

		(
			reconnect,
			vec![ServerMessage::Snapshot(SnapshotEnvelope {
				version,
				server_id: self.inner.server_id.clone(),
				cursor: state.cursor,
				items: snapshot_items,
			})],
		)
	}

	async fn read_commands(
		&self,
		mut receiver: SplitStream<WebSocket>,
		connection_id: u64,
		negotiated: ProtocolVersion,
	) -> bool {
		while let Some(Ok(message)) = receiver.next().await {
			let Message::Text(text) = message else {
				if matches!(message, Message::Close(_)) {
					return false;
				}

				continue;
			};
			let Ok(client_message) = decodex_protocol::decode_client_message(&text) else {
				if !self
					.enqueue(
						connection_id,
						protocol_refusal(
							&self.inner.server_id,
							"message is not a valid client envelope",
						),
					)
					.await
				{
					return true;
				}

				continue;
			};

			match client_message {
				ClientMessage::Hello(_) => {
					if !self
						.enqueue(
							connection_id,
							protocol_refusal(&self.inner.server_id, "hello may be sent only once"),
						)
						.await
					{
						return true;
					}
				},
				ClientMessage::Command(command) => {
					if command.version != negotiated {
						if !self
							.enqueue(
								connection_id,
								protocol_refusal(
									&self.inner.server_id,
									"command version differs from negotiated version",
								),
							)
							.await
						{
							return true;
						}

						continue;
					}
					if !self.execute_command(connection_id, command, negotiated).await {
						return true;
					}
				},
			}
		}

		false
	}

	async fn execute_command(
		&self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
	) -> bool {
		let server = self.clone();
		let task = tokio::spawn(async move {
			server.execute_command_owned(connection_id, command, version).await
		});

		task.await.is_ok_and(|delivered| delivered)
	}

	async fn execute_command_owned(
		&self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
	) -> bool {
		let fingerprint = serde_json::to_vec(&(&command.expected_revision, &command.payload))
			.expect("typed command serialization cannot fail");
		let mut state = self.inner.state.lock().await;

		if let Some(stored) = state.receipts.get(&command.idempotency_key).cloned() {
			let receipt = CommandReceipt {
				version,
				server_id: self.inner.server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				disposition: ReceiptDisposition::Duplicate,
				original_client_command_id: stored.original_client_command_id,
			};
			let mut result = stored.result;

			result.version = version;
			result.client_command_id = command.client_command_id;

			if stored.fingerprint != fingerprint {
				result.outcome = CommandOutcome::Rejected;
				result.entity_revision = None;
				result.payload = None;
				result.error = Some(CommandError::IdempotencyConflict);
			}

			return enqueue_locked(
				&mut state,
				connection_id,
				ServerMessage::CommandReceipt(receipt),
			) && enqueue_locked(
				&mut state,
				connection_id,
				ServerMessage::CommandResult(result),
			);
		}

		if state.receipts.len() >= self.inner.config.receipt_capacity {
			let receipt = CommandReceipt {
				version,
				server_id: self.inner.server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				disposition: ReceiptDisposition::Refused,
				original_client_command_id: command.client_command_id.clone(),
			};
			let result = CommandResultEnvelope {
				version,
				server_id: self.inner.server_id.clone(),
				client_command_id: command.client_command_id,
				idempotency_key: command.idempotency_key,
				outcome: CommandOutcome::Rejected,
				entity_revision: None,
				payload: None,
				error: Some(CommandError::IdempotencyCapacityExceeded {
					capacity: self.inner.config.receipt_capacity,
				}),
			};

			return enqueue_locked(
				&mut state,
				connection_id,
				ServerMessage::CommandReceipt(receipt),
			) && enqueue_locked(
				&mut state,
				connection_id,
				ServerMessage::CommandResult(result),
			);
		}

		let correlation = (command.correlation_id.clone(), command.causation_id.clone());
		let execution = self.inner.application.execute(&command).await;
		let (result, publication) =
			result_from_execution(&self.inner.server_id, &command, version, execution);
		let stored = StoredCommand {
			fingerprint,
			original_client_command_id: command.client_command_id.clone(),
			result: result.clone(),
		};

		state.receipts.insert(command.idempotency_key.clone(), stored);

		let receipt = CommandReceipt {
			version,
			server_id: self.inner.server_id.clone(),
			client_command_id: command.client_command_id,
			idempotency_key: command.idempotency_key,
			disposition: ReceiptDisposition::Executed,
			original_client_command_id: result.client_command_id.clone(),
		};
		let delivered =
			enqueue_locked(&mut state, connection_id, ServerMessage::CommandReceipt(receipt))
				&& enqueue_locked(&mut state, connection_id, ServerMessage::CommandResult(result));

		if let Some(publication) = publication {
			self.publish_locked(&mut state, version, correlation, publication);
		}

		delivered && state.subscribers.contains_key(&connection_id)
	}

	fn publish_locked(
		&self,
		state: &mut PublicationState,
		version: ProtocolVersion,
		correlation: (CorrelationId, Option<CausationId>),
		publication: ApplicationPublication,
	) {
		state.cursor.0 = state.cursor.0.checked_add(1).expect("protocol cursor exhausted");

		let event = EventEnvelope {
			version,
			server_id: self.inner.server_id.clone(),
			cursor: state.cursor,
			channel: publication.channel,
			entity_id: publication.entity_id,
			entity_revision: publication.entity_revision,
			correlation_id: correlation.0,
			causation_id: correlation.1,
			payload: publication.event,
		};

		state.events.push_back(event.clone());

		while state.events.len() > self.inner.config.replay_capacity {
			state.events.pop_front();
		}

		state.subscribers.retain(|_, subscriber| {
			let mut event = event.clone();

			event.version = subscriber.version;

			subscriber.sender.try_send(ServerMessage::Event(event)).is_ok()
		});
	}

	async fn enqueue(&self, connection_id: u64, message: ServerMessage) -> bool {
		let mut state = self.inner.state.lock().await;

		enqueue_locked(&mut state, connection_id, message)
	}

	async fn remove_subscriber(&self, connection_id: u64) {
		self.inner.state.lock().await.subscribers.remove(&connection_id);
	}

	async fn write_messages(
		&self,
		mut sender: SplitSink<WebSocket, Message>,
		mut receiver: Receiver<ServerMessage>,
	) {
		while let Some(message) = receiver.recv().await {
			let Ok(encoded) = decodex_protocol::encode_server_message(&message) else {
				return;
			};

			if encoded.len() > self.inner.config.maximum_message_bytes {
				self.send_split_close(&mut sender, 1_009, "outbound message exceeds bounded size")
					.await;

				return;
			}

			let write_result = time::timeout(
				self.inner.config.write_timeout,
				sender.send(Message::Text(encoded.into())),
			)
			.await;

			if !matches!(write_result, Ok(Ok(()))) {
				return;
			}
		}

		self.send_split_close(&mut sender, 1_013, "bounded outbound queue exceeded").await;
	}

	async fn send_direct(&self, socket: &mut WebSocket, message: ServerMessage) -> bool {
		let Ok(encoded) = decodex_protocol::encode_server_message(&message) else {
			return false;
		};

		if encoded.len() > self.inner.config.maximum_message_bytes {
			self.send_socket_close(socket, 1_009, "outbound message exceeds bounded size").await;

			return false;
		}

		time::timeout(self.inner.config.write_timeout, socket.send(Message::Text(encoded.into())))
			.await
			.is_ok_and(|result| result.is_ok())
	}

	async fn send_split_close(
		&self,
		sender: &mut SplitSink<WebSocket, Message>,
		code: u16,
		reason: &'static str,
	) {
		let close = sender.send(Message::Close(Some(CloseFrame { code, reason: reason.into() })));
		let _ = time::timeout(self.inner.config.write_timeout, close).await;
	}

	async fn send_socket_close(&self, socket: &mut WebSocket, code: u16, reason: &'static str) {
		let close = socket.send(Message::Close(Some(CloseFrame { code, reason: reason.into() })));
		let _ = time::timeout(self.inner.config.write_timeout, close).await;
	}
}

impl<A> Clone for ProtocolServer<A>
where
	A: Application,
{
	fn clone(&self) -> Self {
		Self { inner: Arc::clone(&self.inner) }
	}
}

struct ServerInner<A>
where
	A: Application,
{
	server_id: ServerId,
	application: A,
	config: ServerConfig,
	connection_ids: AtomicU64,
	state: Mutex<PublicationState>,
}

#[derive(Default)]
struct PublicationState {
	cursor: Cursor,
	events: VecDeque<EventEnvelope>,
	receipts: HashMap<IdempotencyKey, StoredCommand>,
	subscribers: HashMap<u64, Subscriber>,
}

struct Subscriber {
	sender: mpsc::Sender<ServerMessage>,
	version: ProtocolVersion,
}

#[derive(Clone)]
struct StoredCommand {
	fingerprint: Vec<u8>,
	original_client_command_id: ClientCommandId,
	result: CommandResultEnvelope,
}

/// Failure to validate, bind, run, or join the loopback server.
#[derive(Debug)]
pub enum ServerError {
	/// The requested listener was outside the loopback boundary.
	EndpointPolicy(EndpointPolicyError),
	/// Socket or serving I/O failed.
	Io(std::io::Error),
	/// The spawned server task failed to join.
	Join(JoinError),
}
impl std::error::Error for ServerError {}

impl Display for ServerError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::EndpointPolicy(error) => Display::fmt(error, formatter),
			Self::Io(error) => write!(formatter, "loopback server I/O failed: {error}"),
			Self::Join(error) => write!(formatter, "loopback server task failed: {error}"),
		}
	}
}

fn result_from_execution(
	server_id: &ServerId,
	command: &CommandEnvelope,
	version: ProtocolVersion,
	execution: Result<ApplicationPublication, CommandError>,
) -> (CommandResultEnvelope, Option<ApplicationPublication>) {
	match execution {
		Ok(publication) => (
			CommandResultEnvelope {
				version,
				server_id: server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				outcome: CommandOutcome::Succeeded,
				entity_revision: Some(publication.entity_revision),
				payload: Some(publication.result.clone()),
				error: None,
			},
			Some(publication),
		),
		Err(error) => (
			CommandResultEnvelope {
				version,
				server_id: server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				outcome: CommandOutcome::Rejected,
				entity_revision: None,
				payload: None,
				error: Some(error),
			},
			None,
		),
	}
}

fn enqueue_locked(
	state: &mut PublicationState,
	connection_id: u64,
	message: ServerMessage,
) -> bool {
	let Some(subscriber) = state.subscribers.get(&connection_id) else {
		return false;
	};

	if subscriber.sender.try_send(message).is_ok() {
		true
	} else {
		state.subscribers.remove(&connection_id);

		false
	}
}

fn protocol_refusal(server_id: &ServerId, message: &str) -> ServerMessage {
	ServerMessage::Refusal(RefusalEnvelope {
		server_id: server_id.clone(),
		refusal: Refusal::ProtocolViolation { message: bounded_text(message) },
	})
}

fn bounded_text(message: &str) -> WireText {
	WireText::new(message).expect("internal protocol message is bounded")
}

async fn upgrade<A>(ws: WebSocketUpgrade, State(server): State<ProtocolServer<A>>) -> Response
where
	A: Application,
{
	let maximum = server.inner.config.maximum_message_bytes;

	ws.max_message_size(maximum)
		.max_frame_size(maximum)
		.on_upgrade(move |socket| async move { server.handle_connection(socket).await })
}
