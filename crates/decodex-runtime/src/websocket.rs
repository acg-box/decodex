//! Direct owned WebSocket lifecycle over the same-UID local transport.
//!
//! One server-owned publication actor owns command execution, receipts, replay, subscribers,
//! application-event ordering, and shutdown drain. Session and service tasks remain in one
//! bounded-accounting `JoinSet`; application command futures never enter that task set.

use std::{
	collections::{HashMap, VecDeque},
	fmt::{Display, Formatter},
	future::Future,
	panic::AssertUnwindSafe,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use futures_util::{
	FutureExt as _, SinkExt as _, StreamExt as _,
	stream::{SplitSink, SplitStream},
};
use tokio::{
	sync::{mpsc, mpsc::Receiver, oneshot, watch},
	task::{AbortHandle, Id as TokioTaskId, JoinError, JoinHandle, JoinSet},
	time,
};
use tokio_tungstenite::{
	WebSocketStream, accept_hdr_async_with_config,
	tungstenite::{
		Error as TungsteniteError, Message,
		error::ProtocolError,
		handshake::server::{ErrorResponse, Request, Response},
		http::StatusCode,
		protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
	},
};

use crate::{Application, ApplicationEventPublication, ApplicationPublication};
use decodex_core::ServerIdentity;
use decodex_protocol::{
	self, CausationId, ClientCommandId, ClientHello, ClientMessage, CommandEnvelope, CommandError,
	CommandOutcome, CommandReceipt, CommandResultEnvelope, CorrelationId, Cursor, EventEnvelope,
	IdempotencyKey, LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
	LocalTransportStream, ProtocolVersion, QueryEnvelope, QueryResultEnvelope, ReceiptDisposition,
	ReconnectMode, Refusal, RefusalEnvelope, ResumeCursor, ServerId, ServerInstanceId,
	ServerMessage, ServerWelcome, SnapshotEnvelope, SnapshotItem, SupportedVersions, WireText,
};

const WS_PATH: &str = "/v1/ws";
const PUBLICATION_INSTANCE_MINIMUM_VERSION: ProtocolVersion =
	ProtocolVersion { major: 2, minor: 0 };

const fn supports_publication_instance(version: ProtocolVersion) -> bool {
	version.major == PUBLICATION_INSTANCE_MINIMUM_VERSION.major
}

type WebSocket = WebSocketStream<LocalTransportStream>;
type OwnedFuture = Pin<Box<dyn Future<Output = OwnedTaskResult> + Send + 'static>>;

const PRODUCT_MAXIMUM_SESSION_TASKS: usize = 64;

// Tungstenite fixes this callback's error type to the full HTTP response.
#[allow(clippy::result_large_err)]
fn validate_websocket_route(
	request: &Request,
	response: Response,
) -> Result<Response, ErrorResponse> {
	if request.uri().path() == WS_PATH && request.uri().query().is_none() {
		return Ok(response);
	}

	let mut refusal = ErrorResponse::new(Some("WebSocket route is unavailable".to_owned()));

	*refusal.status_mut() = StatusCode::NOT_FOUND;

	Err(refusal)
}

/// Bounded transport and lifecycle settings. None of these enable remote binding.
#[derive(Clone, Debug)]
pub struct ServerConfig {
	/// Maximum number of events retained for cursor resume.
	pub replay_capacity: usize,
	/// Maximum number of pending messages for one client and pending command submissions.
	pub outbound_queue_capacity: usize,
	/// Maximum number of logical commands retained for lifetime deduplication.
	pub receipt_capacity: usize,
	/// Maximum number of small-state items in one snapshot.
	pub maximum_snapshot_items: usize,
	/// Maximum accepted UTF-8 message size.
	pub maximum_message_bytes: usize,
	/// Time allowed for the mandatory first hello message.
	pub hello_timeout: Duration,
	/// Time allowed for one local WebSocket message or close write.
	pub write_timeout: Duration,
	/// One non-extendable deadline for all owned task quiescence after stopping starts.
	pub shutdown_timeout: Duration,
	/// Maximum concurrent admitted WebSocket tasks, including incomplete handshakes.
	pub maximum_concurrent_sessions: usize,
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
			shutdown_timeout: Duration::from_secs(5),
			maximum_concurrent_sessions: PRODUCT_MAXIMUM_SESSION_TASKS,
		}
	}
}

/// Stable lifecycle-local identity assigned before one owned task is spawned.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpawnId(pub u64);

/// Closed task kind for the one owned runtime task set.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OwnedTaskKind {
	/// One admitted WebSocket session, including handshake and queries.
	Session,
	/// One daemon-local service future under the established lifecycle authority.
	Service,
}

/// Stable identity and kind of one owned task.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OwnedTaskIdentity {
	/// Monotonic lifecycle-local spawn identity.
	pub spawn_id: SpawnId,
	/// Closed owned task kind.
	pub kind: OwnedTaskKind,
}

/// Deterministic primary termination class.
///
/// Rank from highest to lowest is cleanup refusal, endpoint refusal, owner
/// integrity failure, child panic, unexpected child failure, forced task deadline,
/// actor-command deadline, and requested shutdown. Task-class ties select the lowest stable
/// spawn ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationPrimary {
	/// Exact requested shutdown completed without an abnormal fact.
	RequestedShutdown,
	/// The absolute deadline classified the lowest identified abort-safe task.
	ForcedDeadline(OwnedTaskIdentity),
	/// The absolute server deadline elapsed while one actor-owned application command was active.
	ActorCommandDeadline,
	/// An owned task or its local transport ended unexpectedly without a panic or deadline.
	ChildFailure(OwnedTaskIdentity),
	/// An owned task panicked.
	ChildPanic(OwnedTaskIdentity),
	/// Stable task accounting or identity became inconsistent.
	OwnerIntegrity,
	/// The published listener failed a point-in-time authority check.
	EndpointRefusal(LocalTransportRefusal),
	/// Exact cleanup refused to remove the retained publication.
	CleanupRefusal(LocalTransportRefusal),
}

/// Bounded deterministic readback for one complete lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminationReceipt {
	/// Highest-ranked termination fact after all tasks and cleanup were harvested.
	pub primary: TerminationPrimary,
	/// Number of session tasks assigned a stable spawn ID.
	pub spawned_sessions: u64,
	/// Number of daemon service tasks assigned a stable spawn ID.
	pub spawned_services: u64,
	/// Number of fresh application commands admitted by the publication actor.
	pub actor_commands_admitted: u64,
	/// Number of actor-owned application commands settled without dropping their future.
	pub actor_commands_settled: u64,
	/// Deterministic command state at the absolute server deadline.
	pub actor_command_deadline: ActorCommandDeadlineClass,
	/// Number of `JoinSet` results harvested before cleanup.
	pub harvested_tasks: u64,
	/// Number of owned tasks that completed with an exact expected lifecycle outcome.
	pub expected_tasks: u64,
	/// Number of owned task panics.
	pub panicked_tasks: u64,
	/// Number of unexpected non-panic task or local transport failures.
	pub failed_tasks: u64,
	/// Number of tasks classified at the absolute deadline.
	pub forced_cancelled_tasks: u64,
	/// Number of stable owner-accounting failures.
	pub owner_integrity_failures: u64,
	/// Lowest stable identity among panicked tasks.
	pub lowest_panicked: Option<OwnedTaskIdentity>,
	/// Lowest stable identity among unexpected failed tasks.
	pub lowest_failed: Option<OwnedTaskIdentity>,
	/// Lowest stable identity among tasks classified at the absolute deadline.
	pub lowest_forced: Option<OwnedTaskIdentity>,
	/// Listener-invalidating point-in-time refusal, if observed.
	pub endpoint_refusal: Option<LocalTransportRefusal>,
	/// Identity-checked cleanup refusal, if observed.
	pub cleanup_refusal: Option<LocalTransportRefusal>,
}

impl TerminationReceipt {
	/// Whether the receipt proves requested shutdown, complete harvesting, and exact cleanup.
	pub const fn is_success(self) -> bool {
		matches!(self.primary, TerminationPrimary::RequestedShutdown)
			&& self.panicked_tasks == 0
			&& self.failed_tasks == 0
			&& self.forced_cancelled_tasks == 0
			&& self.owner_integrity_failures == 0
			&& self.endpoint_refusal.is_none()
			&& self.cleanup_refusal.is_none()
			&& matches!(
				self.actor_command_deadline,
				ActorCommandDeadlineClass::NoActiveCommand
					| ActorCommandDeadlineClass::SettledBeforeDeadline
			) && self.actor_commands_admitted == self.actor_commands_settled
			&& self.harvested_tasks == self.spawned_sessions.saturating_add(self.spawned_services)
			&& self.expected_tasks == self.harvested_tasks
	}
}

/// Actor-command settlement at the one absolute server deadline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActorCommandDeadlineClass {
	/// Shutdown began with no active actor-owned application command.
	#[default]
	NoActiveCommand,
	/// The active command settled before the absolute server deadline.
	SettledBeforeDeadline,
	/// The deadline elapsed, then the command settled during application-owner shutdown.
	DeadlineElapsedThenSettledDuringApplicationShutdown,
	/// Application settlement and event EOF completed while the command remained unsettled.
	ApplicationSettledWithCommandUnsettled,
}

/// A running same-UID local server and its cancellation-safe lifecycle handle.
pub struct BoundServer {
	shutdown_sender: Option<oneshot::Sender<()>>,
	task: Option<JoinHandle<TerminationReceipt>>,
}

impl BoundServer {
	/// Request shutdown and wait for complete owned-task and cleanup readback.
	pub async fn shutdown(&mut self) -> Result<TerminationReceipt, ServerError> {
		if let Some(sender) = self.shutdown_sender.take() {
			let _ = sender.send(());
		}

		self.wait_task().await
	}

	/// Wait until the server stops and returns its complete lifecycle readback.
	pub async fn wait(&mut self) -> Result<TerminationReceipt, ServerError> {
		self.wait_task().await
	}

	async fn wait_task(&mut self) -> Result<TerminationReceipt, ServerError> {
		let task = self.task.as_mut().ok_or(ServerError::LifecycleUnavailable)?;
		let joined = task.await;

		self.task.take();
		let receipt = joined.map_err(ServerError::LifecycleJoin)?;

		if receipt.is_success() { Ok(receipt) } else { Err(ServerError::Terminated(receipt)) }
	}
}

impl Drop for BoundServer {
	fn drop(&mut self) {
		if let Some(sender) = self.shutdown_sender.take() {
			let _ = sender.send(());
		}
	}
}

/// A same-UID local WebSocket server over one application implementation.
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
	/// Build a server with a stable host identity and a fresh publication epoch.
	pub fn new(server_id: ServerId, application: A, config: ServerConfig) -> Self {
		assert!(config.replay_capacity > 0, "replay capacity must be non-zero");
		assert!(config.outbound_queue_capacity > 0, "outbound queue capacity must be non-zero");
		assert!(config.receipt_capacity > 0, "receipt capacity must be non-zero");
		assert!(A::EVENT_CAPACITY > 0, "application event capacity must be non-zero");
		assert!(!config.shutdown_timeout.is_zero(), "shutdown timeout must be non-zero");
		assert!(
			(1..=PRODUCT_MAXIMUM_SESSION_TASKS).contains(&config.maximum_concurrent_sessions),
			"maximum concurrent sessions must be between 1 and 64",
		);

		Self {
			inner: Arc::new(ServerInner {
				server_id,
				instance_id: ServerIdentity::generate()
					.ok()
					.and_then(|identity| ServerInstanceId::new(identity.as_str()).ok()),
				application,
				config,
				connection_ids: AtomicU64::new(1),
			}),
		}
	}

	/// Publish the endpoint and spawn the one top-level lifecycle owner.
	pub async fn bind(
		self,
		transport: LocalTransportAuthority,
	) -> Result<BoundServer, ServerError> {
		let listener = transport.bind().await.map_err(ServerError::LocalTransport)?;

		Ok(self.bind_listener(listener))
	}

	pub(crate) fn bind_listener(self, listener: LocalTransportListener) -> BoundServer {
		let (shutdown_sender, shutdown_receiver) = oneshot::channel();
		let task = tokio::spawn(self.serve_owned(listener, shutdown_receiver));

		BoundServer { shutdown_sender: Some(shutdown_sender), task: Some(task) }
	}

	async fn serve_owned(
		self,
		listener: LocalTransportListener,
		shutdown_receiver: oneshot::Receiver<()>,
	) -> TerminationReceipt {
		OwnerLoop::new(self, listener, shutdown_receiver).run().await
	}

	async fn handle_stream(
		&self,
		stream: LocalTransportStream,
		connection_id: u64,
		actor_sender: mpsc::Sender<PublicationRequest>,
		mut stop: watch::Receiver<bool>,
	) -> SessionTaskCompletion {
		let config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(0)
			.max_write_buffer_size(self.inner.config.maximum_message_bytes.saturating_add(1))
			.max_message_size(Some(self.inner.config.maximum_message_bytes))
			.max_frame_size(Some(self.inner.config.maximum_message_bytes));
		let handshake = time::timeout(
			self.inner.config.hello_timeout,
			accept_hdr_async_with_config(stream, validate_websocket_route, Some(config)),
		);
		let socket = tokio::select! {
			biased;

			() = stopped(&mut stop) => return SessionTaskCompletion::unregistered(connection_id),
			result = handshake => match result {
				Ok(Ok(socket)) => socket,
				_ => return SessionTaskCompletion::unregistered(connection_id),
			},
		};

		self.handle_connection(socket, connection_id, actor_sender, stop).await
	}

	async fn handle_connection(
		&self,
		mut socket: WebSocket,
		connection_id: u64,
		actor_sender: mpsc::Sender<PublicationRequest>,
		mut stop: watch::Receiver<bool>,
	) -> SessionTaskCompletion {
		let hello = match self.receive_hello(&mut socket, &mut stop).await {
			InitialHello::Received(hello) => hello,
			InitialHello::Stopped => return SessionTaskCompletion::unregistered(connection_id),
			InitialHello::Failed => return SessionTaskCompletion::unregistered(connection_id),
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

				return SessionTaskCompletion::unregistered(connection_id);
			},
		};

		if let Some(expected) = hello.expected_server_id.as_ref()
			&& expected != &self.inner.server_id
		{
			let _ = self
				.send_direct(
					&mut socket,
					ServerMessage::Refusal(RefusalEnvelope {
						server_id: self.inner.server_id.clone(),
						refusal: Refusal::ServerIdentityMismatch {
							expected: expected.clone(),
							actual: self.inner.server_id.clone(),
						},
					}),
				)
				.await;

			return SessionTaskCompletion::unregistered(connection_id);
		}

		let (sender, receiver) = mpsc::channel(self.inner.config.outbound_queue_capacity);
		let (seal_sender, seal_receiver) = oneshot::channel();
		let initial = match self
			.prepare_session(connection_id, sender, seal_sender, hello, negotiated, &actor_sender)
			.await
		{
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

				return SessionTaskCompletion::unregistered(connection_id);
			},
		};

		RegisteredSessionIo::new(
			self.clone(),
			socket,
			RegisteredSessionContext {
				outbound_receiver: receiver,
				seal_receiver,
				connection_id,
				negotiated,
				actor_sender,
				stop,
			},
		)
		.run(initial)
		.await
	}

	async fn receive_hello(
		&self,
		socket: &mut WebSocket,
		stop: &mut watch::Receiver<bool>,
	) -> InitialHello {
		let received = tokio::select! {
			biased;

			() = stopped(stop) => return InitialHello::Stopped,
			received = time::timeout(self.inner.config.hello_timeout, socket.next()) => received,
		};
		let Ok(Some(Ok(message))) = received else {
			return InitialHello::Failed;
		};
		let Message::Text(text) = message else {
			let refusal =
				protocol_refusal(&self.inner.server_id, "first message must be text hello");
			let _ = self.send_direct(socket, refusal).await;

			return InitialHello::Failed;
		};

		match decodex_protocol::decode_client_message(&text) {
			Ok(ClientMessage::Hello(hello)) => InitialHello::Received(hello),
			Ok(ClientMessage::Command(_)) => {
				let refusal =
					protocol_refusal(&self.inner.server_id, "hello is required before command");
				let _ = self.send_direct(socket, refusal).await;

				InitialHello::Failed
			},
			Ok(ClientMessage::Query(_)) => {
				let refusal =
					protocol_refusal(&self.inner.server_id, "hello is required before a query");
				let _ = self.send_direct(socket, refusal).await;

				InitialHello::Failed
			},
			Err(_) => {
				let refusal = protocol_refusal(
					&self.inner.server_id,
					"message is not a valid client envelope",
				);
				let _ = self.send_direct(socket, refusal).await;

				InitialHello::Failed
			},
		}
	}

	async fn prepare_session(
		&self,
		connection_id: u64,
		sender: mpsc::Sender<OutboundItem>,
		seal_sender: oneshot::Sender<SessionSealReason>,
		hello: ClientHello,
		negotiated: ProtocolVersion,
		actor_sender: &mpsc::Sender<PublicationRequest>,
	) -> Result<Vec<ServerMessage>, Refusal> {
		let (reply, result) = oneshot::channel();
		actor_sender
			.send(PublicationRequest::Register {
				connection_id,
				sender,
				seal_sender,
				hello,
				version: negotiated,
				reply,
			})
			.await
			.map_err(|_| Refusal::ProtocolViolation {
				message: bounded_text("server publication owner is unavailable"),
			})?;
		result.await.map_err(|_| Refusal::ProtocolViolation {
			message: bounded_text("server publication owner is unavailable"),
		})?
	}

	fn reconnect_messages(
		&self,
		state: &PublicationState,
		resume: Option<&ResumeCursor>,
		version: ProtocolVersion,
		snapshot_items: Vec<SnapshotItem>,
	) -> (ReconnectMode, Vec<ServerMessage>) {
		let can_resume = resume.is_some_and(|resume| {
			supports_publication_instance(version)
				&& self.inner.instance_id.as_ref() == resume.instance_id.as_ref()
				&& self.inner.instance_id.is_some()
				&& resume.server_id == self.inner.server_id
				&& resume.cursor <= state.cursor
				&& state
					.events
					.front()
					.is_none_or(|oldest| resume.cursor.0.saturating_add(1) >= oldest.cursor.0)
		});

		if can_resume {
			let cursor = resume.expect("the resume value was checked above").cursor;
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

	async fn read_messages(
		&self,
		receiver: &mut SplitStream<WebSocket>,
		connection_id: u64,
		negotiated: ProtocolVersion,
		actor_sender: &mpsc::Sender<PublicationRequest>,
		mut stop: watch::Receiver<bool>,
	) -> SessionReaderCompletion {
		loop {
			let received = tokio::select! {
				biased;

				() = stopped(&mut stop) => {
					return SessionReaderCompletion::Reason(SessionSealReason::ServerShutdown);
				},
				received = receiver.next() => received,
			};
			let Some(Ok(message)) = received else {
				return SessionReaderCompletion::Reason(SessionSealReason::PeerDisconnected);
			};
			let Message::Text(text) = message else {
				if matches!(message, Message::Close(_)) {
					return SessionReaderCompletion::PeerClose;
				}

				continue;
			};
			let Ok(client_message) = decodex_protocol::decode_client_message(&text) else {
				if !self
					.enqueue(
						actor_sender,
						connection_id,
						protocol_refusal(
							&self.inner.server_id,
							"message is not a valid client envelope",
						),
					)
					.await
				{
					return SessionReaderCompletion::Reason(session_ingress_failure_reason(&stop));
				}

				continue;
			};

			match client_message {
				ClientMessage::Hello(_) => {
					if !self
						.enqueue(
							actor_sender,
							connection_id,
							protocol_refusal(&self.inner.server_id, "hello may be sent only once"),
						)
						.await
					{
						return SessionReaderCompletion::Reason(session_ingress_failure_reason(
							&stop,
						));
					}
				},
				ClientMessage::Command(command) => {
					if command.version != negotiated || !command.payload.is_supported_in(negotiated)
					{
						if !self
							.enqueue(
								actor_sender,
								connection_id,
								protocol_refusal(
									&self.inner.server_id,
									"command is unavailable in the negotiated protocol version",
								),
							)
							.await
						{
							return SessionReaderCompletion::Reason(
								session_ingress_failure_reason(&stop),
							);
						}

						continue;
					}
					if !self
						.submit_command(connection_id, command, negotiated, actor_sender, &mut stop)
						.await
					{
						return SessionReaderCompletion::Reason(session_ingress_failure_reason(
							&stop,
						));
					}
				},
				ClientMessage::Query(query) => {
					if query.version != negotiated || !query.payload.is_supported_in(negotiated) {
						if !self
							.enqueue(
								actor_sender,
								connection_id,
								protocol_refusal(
									&self.inner.server_id,
									"query is unavailable in the negotiated protocol version",
								),
							)
							.await
						{
							return SessionReaderCompletion::Reason(
								session_ingress_failure_reason(&stop),
							);
						}

						continue;
					}
					if !self.execute_query(actor_sender, connection_id, query, negotiated).await {
						return SessionReaderCompletion::Reason(session_ingress_failure_reason(
							&stop,
						));
					}
				},
			}
		}
	}

	async fn execute_query(
		&self,
		actor_sender: &mpsc::Sender<PublicationRequest>,
		connection_id: u64,
		query: QueryEnvelope,
		version: ProtocolVersion,
	) -> bool {
		let payload = self.inner.application.query(&query).await;
		let result = QueryResultEnvelope {
			version,
			server_id: self.inner.server_id.clone(),
			query_id: query.query_id,
			payload,
		};

		self.enqueue(actor_sender, connection_id, ServerMessage::QueryResult(result)).await
	}

	async fn submit_command(
		&self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
		actor_sender: &mpsc::Sender<PublicationRequest>,
		stop: &mut watch::Receiver<bool>,
	) -> bool {
		let (result_sender, result_receiver) = oneshot::channel();
		let submitted = tokio::select! {
			biased;

			() = stopped(stop) => return false,
			submitted = actor_sender.send(PublicationRequest::Command {
				connection_id,
				command,
				version,
				reply: result_sender,
			}) => submitted,
		};

		if submitted.is_err() {
			return false;
		}

		tokio::select! {
			biased;

			() = stopped(stop) => false,
			delivered = result_receiver => delivered.unwrap_or(false),
		}
	}

	async fn enqueue(
		&self,
		actor_sender: &mpsc::Sender<PublicationRequest>,
		connection_id: u64,
		message: ServerMessage,
	) -> bool {
		let (reply, result) = oneshot::channel();
		if actor_sender
			.send(PublicationRequest::Enqueue {
				connection_id,
				message: Box::new(message),
				reply,
			})
			.await
			.is_err()
		{
			return false;
		}
		result.await.unwrap_or(false)
	}

	async fn send_direct(&self, socket: &mut WebSocket, message: ServerMessage) -> bool {
		self.send_direct_result(socket, message).await.is_ok()
	}

	async fn send_direct_result(
		&self,
		socket: &mut WebSocket,
		message: ServerMessage,
	) -> Result<(), SessionTransportFailure> {
		let encoded = decodex_protocol::encode_server_message(&message)
			.map_err(|_| SessionTransportFailure::InitialPrefixEncoding)?;

		if encoded.len() > self.inner.config.maximum_message_bytes {
			self.send_socket_close(socket, 1_009, "outbound message exceeds bounded size").await;

			return Err(SessionTransportFailure::InitialPrefixTooLarge);
		}

		time::timeout(self.inner.config.write_timeout, socket.send(Message::Text(encoded.into())))
			.await
			.map_err(|_| SessionTransportFailure::InitialPrefixWrite)?
			.map_err(|_| SessionTransportFailure::InitialPrefixWrite)
	}

	async fn send_socket_close(&self, socket: &mut WebSocket, code: u16, reason: &'static str) {
		let close = socket.send(Message::Close(Some(CloseFrame {
			code: CloseCode::from(code),
			reason: reason.into(),
		})));
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
	instance_id: Option<ServerInstanceId>,
	application: A,
	config: ServerConfig,
	connection_ids: AtomicU64,
}

struct RegisteredSessionContext {
	outbound_receiver: Receiver<OutboundItem>,
	seal_receiver: oneshot::Receiver<SessionSealReason>,
	connection_id: u64,
	negotiated: ProtocolVersion,
	actor_sender: mpsc::Sender<PublicationRequest>,
	stop: watch::Receiver<bool>,
}

#[derive(Clone, Copy)]
struct RegisteredSessionTransportContext {
	connection_id: u64,
	write_timeout: Duration,
	maximum_message_bytes: usize,
}

struct RegisteredSessionIo<A>
where
	A: Application,
{
	server: ProtocolServer<A>,
	socket_writer: SplitSink<WebSocket, Message>,
	socket_reader: SplitStream<WebSocket>,
	context: RegisteredSessionContext,
	frozen_reader: Option<SessionReaderCompletion>,
	locally_written: SessionOrdinalProgress,
}

impl<A> RegisteredSessionIo<A>
where
	A: Application,
{
	fn new(
		server: ProtocolServer<A>,
		socket: WebSocket,
		context: RegisteredSessionContext,
	) -> Self {
		let (socket_writer, socket_reader) = socket.split();

		Self {
			server,
			socket_writer,
			socket_reader,
			context,
			frozen_reader: None,
			locally_written: SessionOrdinalProgress::Empty,
		}
	}

	async fn run(mut self, initial: Vec<ServerMessage>) -> SessionTaskCompletion {
		if let Err((cause, seal_reason)) = self.write_initial_prefix(initial).await {
			return SessionTaskCompletion::registered(
				self.context.connection_id,
				seal_reason,
				SessionTransportDisposition::failed(self.locally_written, cause),
			);
		}

		let Self {
			server,
			mut socket_writer,
			mut socket_reader,
			context:
				RegisteredSessionContext {
					mut outbound_receiver,
					mut seal_receiver,
					connection_id,
					negotiated,
					actor_sender,
					stop,
				},
			mut frozen_reader,
			mut locally_written,
		} = self;
		let transport = RegisteredSessionTransportContext {
			connection_id,
			write_timeout: server.inner.config.write_timeout,
			maximum_message_bytes: server.inner.config.maximum_message_bytes,
		};
		let mut peer_close_reply_flushed = false;
		let reader = server.read_messages(
			&mut socket_reader,
			transport.connection_id,
			negotiated,
			&actor_sender,
			stop,
		);
		tokio::pin!(reader);

		loop {
			let item = Self::next_outbound_item(
				&mut outbound_receiver,
				&mut frozen_reader,
				reader.as_mut(),
			)
			.await;
			let Some(item) = item else {
				return Self::finish_drained_session(
					&mut socket_writer,
					transport.write_timeout,
					transport.connection_id,
					&mut seal_receiver,
					frozen_reader,
					peer_close_reply_flushed,
					locally_written,
				)
				.await;
			};

			let encoded = match Self::prepare_outbound_message(
				transport,
				&mut socket_writer,
				&mut seal_receiver,
				frozen_reader,
				locally_written,
				&item.message,
			)
			.await
			{
				Ok(encoded) => encoded,
				Err(completion) => return completion,
			};

			if socket_writer.feed(Message::Text(encoded.into())).await.is_err() {
				return Self::transport_failure(
					transport.connection_id,
					&mut seal_receiver,
					frozen_reader,
					locally_written,
					SessionTransportFailure::MessageWrite,
				);
			}

			let flush = time::timeout(transport.write_timeout, socket_writer.flush());
			tokio::pin!(flush);
			let flush_result = if frozen_reader.is_some() {
				flush.await
			} else {
				tokio::select! {
					biased;

					result = &mut flush => result,
					completion = &mut reader => {
						let peer_close = completion.is_peer_close();
						frozen_reader = Some(completion);
						outbound_receiver.close();

						let result = flush.await;
						if peer_close && matches!(&result, Ok(Ok(()))) {
							peer_close_reply_flushed = true;
						}

						result
					},
				}
			};
			if frozen_reader.is_some_and(SessionReaderCompletion::is_peer_close)
				&& matches!(
					&flush_result,
					Ok(Err(TungsteniteError::Protocol(ProtocolError::SendAfterClosing)))
				) {
				let _ =
					Self::flush_peer_close_reply(&mut socket_writer, transport.write_timeout).await;
			}
			if !matches!(flush_result, Ok(Ok(()))) {
				return Self::transport_failure(
					transport.connection_id,
					&mut seal_receiver,
					frozen_reader,
					locally_written,
					SessionTransportFailure::MessageWrite,
				);
			}
			locally_written = SessionOrdinalProgress::Through(item.ordinal);
		}
	}

	async fn next_outbound_item<F>(
		outbound_receiver: &mut Receiver<OutboundItem>,
		frozen_reader: &mut Option<SessionReaderCompletion>,
		mut reader: Pin<&mut F>,
	) -> Option<OutboundItem>
	where
		F: Future<Output = SessionReaderCompletion>,
	{
		loop {
			if frozen_reader.is_some() {
				return outbound_receiver.recv().await;
			}
			match outbound_receiver.try_recv() {
				Ok(item) => return Some(item),
				Err(mpsc::error::TryRecvError::Disconnected) => return None,
				Err(mpsc::error::TryRecvError::Empty) => {},
			}
			tokio::select! {
				completion = reader.as_mut() => {
					*frozen_reader = Some(completion);
					outbound_receiver.close();
				},
				item = outbound_receiver.recv() => return item,
			}
		}
	}

	async fn finish_drained_session(
		socket_writer: &mut SplitSink<WebSocket, Message>,
		write_timeout: Duration,
		connection_id: u64,
		seal_receiver: &mut oneshot::Receiver<SessionSealReason>,
		frozen_reader: Option<SessionReaderCompletion>,
		peer_close_reply_flushed: bool,
		locally_written: SessionOrdinalProgress,
	) -> SessionTaskCompletion {
		let seal_reason = Self::finalize_seal_reason(
			seal_receiver,
			frozen_reader.map(SessionReaderCompletion::requested_seal),
		);
		let close_sent = match (seal_reason, frozen_reader) {
			(SessionSealReason::OutboundFull, _) =>
				Self::send_close(
					socket_writer,
					write_timeout,
					1_013,
					"bounded outbound queue exceeded",
				)
				.await,
			(_, Some(SessionReaderCompletion::PeerClose)) if peer_close_reply_flushed => true,
			(_, Some(SessionReaderCompletion::PeerClose)) =>
				Self::flush_peer_close_reply(socket_writer, write_timeout).await,
			(_, Some(SessionReaderCompletion::Reason(_)) | None) =>
				Self::send_close(socket_writer, write_timeout, 1_000, "server session sealed").await,
		};
		let transport = if close_sent {
			SessionTransportDisposition::drained(locally_written)
		} else {
			SessionTransportDisposition::failed(
				locally_written,
				SessionTransportFailure::CloseWrite,
			)
		};
		SessionTaskCompletion::registered(connection_id, seal_reason, transport)
	}

	async fn prepare_outbound_message(
		transport: RegisteredSessionTransportContext,
		socket_writer: &mut SplitSink<WebSocket, Message>,
		seal_receiver: &mut oneshot::Receiver<SessionSealReason>,
		frozen_reader: Option<SessionReaderCompletion>,
		locally_written: SessionOrdinalProgress,
		message: &ServerMessage,
	) -> Result<String, SessionTaskCompletion> {
		match Self::encode_outbound_message(message, transport.maximum_message_bytes) {
			Ok(encoded) => Ok(encoded),
			Err(cause) => {
				if cause == SessionTransportFailure::MessageTooLarge {
					let seal_reason = Self::finalize_seal_reason(
						seal_receiver,
						Some(
							frozen_reader
								.map(SessionReaderCompletion::requested_seal)
								.unwrap_or(SessionSealReason::WriterFailed),
						),
					);
					if seal_reason == SessionSealReason::OutboundFull {
						let _ = Self::send_close(
							socket_writer,
							transport.write_timeout,
							1_013,
							"bounded outbound queue exceeded",
						)
						.await;
					} else if !frozen_reader.is_some_and(SessionReaderCompletion::is_peer_close) {
						let _ = Self::send_close(
							socket_writer,
							transport.write_timeout,
							1_009,
							"outbound message exceeds bounded size",
						)
						.await;
					}

					return Err(SessionTaskCompletion::registered(
						transport.connection_id,
						seal_reason,
						SessionTransportDisposition::failed(locally_written, cause),
					));
				}
				Err(Self::transport_failure(
					transport.connection_id,
					seal_receiver,
					frozen_reader,
					locally_written,
					cause,
				))
			},
		}
	}

	fn encode_outbound_message(
		message: &ServerMessage,
		maximum_message_bytes: usize,
	) -> Result<String, SessionTransportFailure> {
		let encoded = decodex_protocol::encode_server_message(message)
			.map_err(|_| SessionTransportFailure::MessageEncoding)?;
		if encoded.len() > maximum_message_bytes {
			return Err(SessionTransportFailure::MessageTooLarge);
		}
		Ok(encoded)
	}

	async fn write_initial_prefix(
		&mut self,
		initial: Vec<ServerMessage>,
	) -> Result<(), (SessionTransportFailure, SessionSealReason)> {
		for message in initial {
			let encoded = match decodex_protocol::encode_server_message(&message) {
				Ok(encoded) => encoded,
				Err(_) => {
					let cause = SessionTransportFailure::InitialPrefixEncoding;
					let seal_reason = Self::finalize_seal_reason(
						&mut self.context.seal_receiver,
						Some(SessionSealReason::InitialPrefixFailed),
					);

					return Err((cause, seal_reason));
				},
			};
			if encoded.len() > self.server.inner.config.maximum_message_bytes {
				let seal_reason = Self::finalize_seal_reason(
					&mut self.context.seal_receiver,
					Some(SessionSealReason::InitialPrefixFailed),
				);
				let (code, reason) = if seal_reason == SessionSealReason::OutboundFull {
					(1_013, "bounded outbound queue exceeded")
				} else {
					(1_009, "outbound message exceeds bounded size")
				};
				let _ = Self::send_close(
					&mut self.socket_writer,
					self.server.inner.config.write_timeout,
					code,
					reason,
				)
				.await;

				return Err((SessionTransportFailure::InitialPrefixTooLarge, seal_reason));
			}
			let result = time::timeout(
				self.server.inner.config.write_timeout,
				self.socket_writer.send(Message::Text(encoded.into())),
			)
			.await;
			if !matches!(result, Ok(Ok(()))) {
				let seal_reason = Self::finalize_seal_reason(
					&mut self.context.seal_receiver,
					Some(SessionSealReason::InitialPrefixFailed),
				);

				return Err((SessionTransportFailure::InitialPrefixWrite, seal_reason));
			}
		}

		Ok(())
	}

	fn transport_failure(
		connection_id: u64,
		seal_receiver: &mut oneshot::Receiver<SessionSealReason>,
		frozen_reader: Option<SessionReaderCompletion>,
		locally_written: SessionOrdinalProgress,
		cause: SessionTransportFailure,
	) -> SessionTaskCompletion {
		let seal_reason = Self::finalize_seal_reason(
			seal_receiver,
			Some(
				frozen_reader
					.map(SessionReaderCompletion::requested_seal)
					.unwrap_or(SessionSealReason::WriterFailed),
			),
		);

		SessionTaskCompletion::registered(
			connection_id,
			seal_reason,
			SessionTransportDisposition::failed(locally_written, cause),
		)
	}

	fn finalize_seal_reason(
		seal_receiver: &mut oneshot::Receiver<SessionSealReason>,
		requested: Option<SessionSealReason>,
	) -> SessionSealReason {
		match seal_receiver.try_recv() {
			Ok(SessionSealReason::OutboundClosed) => requested
				.filter(|reason| reason.canonicalizes_outbound_closed())
				.unwrap_or(SessionSealReason::OutboundClosed),
			Ok(reason) => reason,
			Err(oneshot::error::TryRecvError::Empty) =>
				requested.unwrap_or(SessionSealReason::ActorUnavailable),
			Err(oneshot::error::TryRecvError::Closed) => SessionSealReason::ActorUnavailable,
		}
	}

	async fn flush_peer_close_reply(
		socket_writer: &mut SplitSink<WebSocket, Message>,
		write_timeout: Duration,
	) -> bool {
		time::timeout(write_timeout, socket_writer.flush()).await.is_ok_and(|result| result.is_ok())
	}

	async fn send_close(
		socket_writer: &mut SplitSink<WebSocket, Message>,
		write_timeout: Duration,
		code: u16,
		reason: &'static str,
	) -> bool {
		let close = socket_writer.send(Message::Close(Some(CloseFrame {
			code: CloseCode::from(code),
			reason: reason.into(),
		})));

		time::timeout(write_timeout, close).await.is_ok_and(|result| result.is_ok())
	}
}

type ApplicationShutdownFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type RegistrationSnapshotResult = Result<Vec<SnapshotItem>, ()>;
type RegistrationSnapshotFuture =
	Pin<Box<dyn Future<Output = RegistrationSnapshotResult> + Send + 'static>>;

struct OwnerLoop<A>
where
	A: Application,
{
	server: ProtocolServer<A>,
	listener: LocalTransportListener,
	shutdown_receiver: oneshot::Receiver<()>,
	phase: OwnerPhase,
	deadline: Option<OwnerDeadline>,
	operation: Option<ActiveActorOperation>,
	state: PublicationState,
	deferred_events: VecDeque<ApplicationEventPublication>,
	tasks: OwnedTasks,
	receipt: TerminationReceiptBuilder,
	actor_sender: Option<mpsc::Sender<PublicationRequest>>,
	actor_receiver: mpsc::Receiver<PublicationRequest>,
	stop_sender: watch::Sender<bool>,
	service_stop_sender: watch::Sender<bool>,
	event_eof: bool,
	ingress_drained: bool,
	application_shutdown: Option<ApplicationShutdownFuture>,
	application_settled: bool,
	command_shutdown: CommandShutdownState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerPhase {
	Accepting,
	DrainingApplication,
	DrainingEgress,
	Closed,
}

struct OwnerDeadline {
	at: time::Instant,
	sleep: Pin<Box<time::Sleep>>,
	state: OwnerDeadlineState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerDeadlineState {
	Pending,
	Elapsed,
}

enum ActiveActorOperation {
	Command(ActiveCommand),
	RegistrationSnapshot(PendingRegistrationSnapshot),
}

struct PendingRegistrationSnapshot {
	connection_id: u64,
	sender: mpsc::Sender<OutboundItem>,
	seal_sender: oneshot::Sender<SessionSealReason>,
	hello: ClientHello,
	version: ProtocolVersion,
	base_cursor: Cursor,
	reply: oneshot::Sender<Result<Vec<ServerMessage>, Refusal>>,
	future: RegistrationSnapshotFuture,
}

enum ActiveActorOperationCompletion {
	Command(Box<ActiveCommandResult>),
	RegistrationSnapshot(RegistrationSnapshotResult),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerDirective {
	Continue,
	BeginStopping(StopCause),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopCause {
	RequestedShutdown,
	EndpointRefusal(LocalTransportRefusal),
	ActorIngressClosed,
	UnexpectedEventEof,
	OwnerIntegrity,
	ChildPanic(OwnedTaskIdentity),
	ChildFailure(OwnedTaskIdentity),
	TransportFailed(OwnedTaskIdentity),
	DeadlineClassification(OwnedTaskIdentity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandShutdownState {
	NoCommand,
	Retained,
	DeadlineElapsed,
	IntegrityRecorded,
	IntegrityRecordedAfterDeadline,
	SettledBeforeDeadline,
	SettledAfterDeadline,
}

enum AcceptingWake {
	RequestedShutdown,
	OwnedTask(Option<Result<(TokioTaskId, OwnedTaskCompletion), JoinError>>),
	Operation(ActiveActorOperationCompletion),
	Ordinary(Box<AcceptingOrdinaryWake>),
}

enum AcceptingOrdinaryWake {
	Accepted(Result<LocalTransportStream, LocalTransportRefusal>),
	Request(Option<PublicationRequest>),
	ApplicationEvent(Option<ApplicationEventPublication>),
}

enum StoppingWake {
	Deadline,
	OwnedTask(Option<Result<(TokioTaskId, OwnedTaskCompletion), JoinError>>),
	Operation(ActiveActorOperationCompletion),
	ApplicationSettled,
	Request(Option<PublicationRequest>),
	FlushDeferred,
	ApplicationEvent(Option<ApplicationEventPublication>),
}

impl<A> OwnerLoop<A>
where
	A: Application,
{
	fn combine_directives(left: OwnerDirective, right: OwnerDirective) -> OwnerDirective {
		match (left, right) {
			(OwnerDirective::BeginStopping(cause), _)
			| (_, OwnerDirective::BeginStopping(cause)) => OwnerDirective::BeginStopping(cause),
			(OwnerDirective::Continue, OwnerDirective::Continue) => OwnerDirective::Continue,
		}
	}

	fn directive_for_acceptance(acceptance: SessionAcceptance) -> OwnerDirective {
		match acceptance {
			SessionAcceptance::Sealed(SessionSealReason::OrdinalExhausted) =>
				OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
			SessionAcceptance::Accepted(_)
			| SessionAcceptance::Unavailable
			| SessionAcceptance::Sealed(
				SessionSealReason::OutboundClosed | SessionSealReason::OutboundFull,
			) => OwnerDirective::Continue,
			SessionAcceptance::Sealed(_) =>
				OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
		}
	}

	fn shutdown_refusal() -> Refusal {
		Refusal::ProtocolViolation { message: bounded_text("server is shutting down") }
	}

	fn reject_during_shutdown(&mut self, request: PublicationRequest) {
		match request {
			PublicationRequest::Register { reply, .. } => {
				let _ = reply.send(Err(Self::shutdown_refusal()));
			},
			PublicationRequest::Enqueue { reply, .. } => {
				let _ = reply.send(false);
			},
			PublicationRequest::Command { reply, .. } => {
				let _ = reply.send(false);
			},
		}
	}

	fn record_stop_cause(&mut self, cause: StopCause) {
		match cause {
			StopCause::RequestedShutdown => self.receipt.record_requested_shutdown(),
			StopCause::EndpointRefusal(refusal) => {
				self.receipt.record_endpoint_refusal(refusal);
			},
			StopCause::ActorIngressClosed
			| StopCause::UnexpectedEventEof
			| StopCause::OwnerIntegrity => self.receipt.record_owner_integrity(),
			StopCause::ChildPanic(identity) => {
				debug_assert!(
					self.receipt.lowest_panicked.is_some_and(|lowest| lowest <= identity)
				);
			},
			StopCause::ChildFailure(identity) | StopCause::TransportFailed(identity) => {
				debug_assert!(self.receipt.lowest_failed.is_some_and(|lowest| lowest <= identity));
			},
			StopCause::DeadlineClassification(identity) => {
				debug_assert!(self.receipt.lowest_forced.is_some_and(|lowest| lowest <= identity));
			},
		}
	}

	fn new(
		server: ProtocolServer<A>,
		listener: LocalTransportListener,
		shutdown_receiver: oneshot::Receiver<()>,
	) -> Self {
		let (actor_sender, actor_receiver) =
			mpsc::channel::<PublicationRequest>(server.inner.config.outbound_queue_capacity);
		let (stop_sender, _) = watch::channel(false);
		let (service_stop_sender, service_stop_receiver) = watch::channel(false);
		let event_eof = !server.inner.application.has_publication_source();
		let services = server.inner.application.daemon_service_tasks(service_stop_receiver);
		let mut owner = Self {
			server,
			listener,
			shutdown_receiver,
			phase: OwnerPhase::Accepting,
			deadline: None,
			operation: None,
			state: PublicationState::default(),
			deferred_events: VecDeque::with_capacity(A::EVENT_CAPACITY),
			tasks: OwnedTasks::new(),
			receipt: TerminationReceiptBuilder::default(),
			actor_sender: Some(actor_sender),
			actor_receiver,
			stop_sender,
			service_stop_sender,
			event_eof,
			ingress_drained: false,
			application_shutdown: None,
			application_settled: false,
			command_shutdown: CommandShutdownState::NoCommand,
		};

		for service in services {
			let future = Box::pin(async move {
				service.await;

				OwnedTaskResult::Service
			});
			if owner.tasks.spawn(OwnedTaskKind::Service, None, future, &mut owner.receipt).is_err()
			{
				owner.begin_stopping(StopCause::OwnerIntegrity);
				break;
			}
		}

		owner
	}

	async fn run(mut self) -> TerminationReceipt {
		loop {
			match self.phase {
				OwnerPhase::Accepting => {
					let directive = self.wait_accepting().await;
					self.apply_directive(directive);
				},
				OwnerPhase::DrainingApplication | OwnerPhase::DrainingEgress => {
					self.enforce_deadline();
					let directive = self.command_shutdown_integrity();
					self.apply_directive(directive);
					self.advance_stopping_phase();
					if self.phase != OwnerPhase::Closed {
						let directive = self.wait_stopping().await;
						self.apply_directive(directive);
					}
				},
				OwnerPhase::Closed => break,
			}
		}

		self.finish()
	}

	fn apply_directive(&mut self, directive: OwnerDirective) {
		if let OwnerDirective::BeginStopping(cause) = directive {
			self.begin_stopping(cause);
		}
	}

	fn begin_stopping(&mut self, cause: StopCause) {
		self.record_stop_cause(cause);
		if self.phase != OwnerPhase::Accepting {
			return;
		}

		self.phase = OwnerPhase::DrainingApplication;
		let started = time::Instant::now();
		let at =
			started.checked_add(self.server.inner.config.shutdown_timeout).unwrap_or_else(|| {
				self.receipt.record_owner_integrity();

				started
			});
		self.deadline = Some(OwnerDeadline {
			at,
			sleep: Box::pin(time::sleep_until(at)),
			state: OwnerDeadlineState::Pending,
		});

		let _ = self.stop_sender.send(true);
		self.actor_receiver.close();
		drop(self.actor_sender.take());
		self.server.inner.application.begin_shutdown();
		let _ = self.service_stop_sender.send(true);

		if let Some(operation) = self.operation.take() {
			match operation {
				ActiveActorOperation::Command(command) => {
					self.command_shutdown = CommandShutdownState::Retained;
					self.operation = Some(ActiveActorOperation::Command(command));
				},
				ActiveActorOperation::RegistrationSnapshot(snapshot) => {
					let _ = snapshot.reply.send(Err(Self::shutdown_refusal()));
					self.command_shutdown = CommandShutdownState::NoCommand;
				},
			}
		} else {
			self.command_shutdown = CommandShutdownState::NoCommand;
		}

		let application = Arc::clone(&self.server.inner);
		self.application_shutdown = Some(Box::pin(async move {
			application.application.wait_for_shutdown().await;
		}));
		self.drain_ingress();
	}

	fn drain_ingress(&mut self) {
		loop {
			match self.actor_receiver.try_recv() {
				Ok(request) => self.reject_during_shutdown(request),
				Err(mpsc::error::TryRecvError::Empty) => return,
				Err(mpsc::error::TryRecvError::Disconnected) => {
					self.ingress_drained = true;
					return;
				},
			}
		}
	}

	fn enforce_deadline(&mut self) {
		if self.phase == OwnerPhase::Accepting {
			return;
		}
		let elapsed = self.deadline.as_ref().is_some_and(|deadline| {
			deadline.state == OwnerDeadlineState::Pending && time::Instant::now() >= deadline.at
		});
		if elapsed {
			self.handle_deadline();
		}
	}

	fn handle_deadline(&mut self) {
		let Some(deadline) = self.deadline.as_mut() else {
			self.receipt.record_owner_integrity();
			return;
		};
		if deadline.state == OwnerDeadlineState::Elapsed {
			return;
		}
		deadline.state = OwnerDeadlineState::Elapsed;

		self.tasks.classify_session_deadlines(&mut self.state);
		self.tasks.abort_classified_sessions();
		self.command_shutdown = match self.command_shutdown {
			CommandShutdownState::Retained => CommandShutdownState::DeadlineElapsed,
			CommandShutdownState::IntegrityRecorded =>
				CommandShutdownState::IntegrityRecordedAfterDeadline,
			state => state,
		};
	}

	fn command_shutdown_integrity(&mut self) -> OwnerDirective {
		if !self.application_settled
			|| !self.event_eof
			|| !matches!(self.operation, Some(ActiveActorOperation::Command(_)))
		{
			return OwnerDirective::Continue;
		}

		self.command_shutdown = match self.command_shutdown {
			CommandShutdownState::Retained => CommandShutdownState::IntegrityRecorded,
			CommandShutdownState::DeadlineElapsed =>
				CommandShutdownState::IntegrityRecordedAfterDeadline,
			CommandShutdownState::IntegrityRecorded
			| CommandShutdownState::IntegrityRecordedAfterDeadline => {
				return OwnerDirective::Continue;
			},
			_ => return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
		};
		self.receipt.actor_command_deadline =
			ActorCommandDeadlineClass::ApplicationSettledWithCommandUnsettled;

		OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
	}

	fn mark_command_settled(&mut self) -> OwnerDirective {
		if self.phase == OwnerPhase::Accepting {
			return OwnerDirective::Continue;
		}
		match self.command_shutdown {
			CommandShutdownState::Retained => {
				self.command_shutdown = CommandShutdownState::SettledBeforeDeadline;
				self.receipt.actor_command_deadline =
					ActorCommandDeadlineClass::SettledBeforeDeadline;
				OwnerDirective::Continue
			},
			CommandShutdownState::DeadlineElapsed => {
				self.command_shutdown = CommandShutdownState::SettledAfterDeadline;
				self.receipt.actor_command_deadline =
					ActorCommandDeadlineClass::DeadlineElapsedThenSettledDuringApplicationShutdown;
				OwnerDirective::Continue
			},
			CommandShutdownState::IntegrityRecorded
			| CommandShutdownState::IntegrityRecordedAfterDeadline => OwnerDirective::Continue,
			_ => OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
		}
	}

	fn advance_stopping_phase(&mut self) {
		if self.phase == OwnerPhase::DrainingApplication
			&& self.operation.is_none()
			&& self.application_settled
			&& self.event_eof
			&& self.ingress_drained
			&& self.deferred_events.is_empty()
			&& !self.tasks.has_service_tasks()
		{
			self.enforce_deadline();
			self.state.seal_all(SessionSealReason::ServerDrained);
			self.phase = OwnerPhase::DrainingEgress;
		}

		if self.phase == OwnerPhase::DrainingEgress
			&& self.operation.is_none()
			&& self.deferred_events.is_empty()
			&& self.ingress_drained
			&& self.tasks.is_empty()
			&& self.tasks.identities.is_empty()
			&& self.state.subscribers.is_empty()
			&& self.state.sealed_sessions.is_empty()
		{
			self.phase = OwnerPhase::Closed;
		}
	}

	async fn wait_accepting(&mut self) -> OwnerDirective {
		let operation_active = self.operation.is_some();
		let may_poll_events = !self.event_eof
			&& (!operation_active || self.deferred_events.len() < A::EVENT_CAPACITY);
		let may_accept = self.tasks.active_session_count()
			< self.server.inner.config.maximum_concurrent_sessions;
		let has_tasks = !self.tasks.is_empty();
		let wake = {
			let listener = &mut self.listener;
			let actor_receiver = &mut self.actor_receiver;
			let application = &self.server.inner.application;
			let ordinary = async {
				tokio::select! {
					accepted = listener.accept(), if may_accept => {
						AcceptingOrdinaryWake::Accepted(accepted)
					},
					request = actor_receiver.recv(), if !operation_active => {
						AcceptingOrdinaryWake::Request(request)
					},
					publication = application.next_publication(), if may_poll_events => {
						AcceptingOrdinaryWake::ApplicationEvent(publication)
					},
					_ = std::future::pending::<()>() => unreachable!(),
				}
			};
			tokio::pin!(ordinary);
			tokio::select! {
				biased;

				_ = &mut self.shutdown_receiver => AcceptingWake::RequestedShutdown,
				joined = self.tasks.set.join_next_with_id(), if has_tasks => {
					AcceptingWake::OwnedTask(joined)
				},
				completion = poll_active_operation(&mut self.operation), if operation_active => {
					AcceptingWake::Operation(completion)
				},
				ordinary = &mut ordinary => AcceptingWake::Ordinary(Box::new(ordinary)),
			}
		};

		match wake {
			AcceptingWake::RequestedShutdown =>
				OwnerDirective::BeginStopping(StopCause::RequestedShutdown),
			AcceptingWake::OwnedTask(Some(joined)) => self.harvest_task(joined),
			AcceptingWake::OwnedTask(None) =>
				OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
			AcceptingWake::Operation(completion) => self.finish_operation(completion),
			AcceptingWake::Ordinary(ordinary) => self.handle_ordinary(*ordinary),
		}
	}

	async fn wait_stopping(&mut self) -> OwnerDirective {
		let deadline_pending = self
			.deadline
			.as_ref()
			.is_some_and(|deadline| deadline.state == OwnerDeadlineState::Pending);
		let has_tasks = !self.tasks.is_empty();
		let operation_active = self.operation.is_some();
		let application_pending = !self.application_settled;
		let may_drain_ingress = !self.ingress_drained;
		let may_flush_deferred = !operation_active && !self.deferred_events.is_empty();
		let may_poll_events = self.phase == OwnerPhase::DrainingApplication
			&& !self.event_eof
			&& (!operation_active || self.deferred_events.len() < A::EVENT_CAPACITY);
		let wake = tokio::select! {
			biased;

			_ = poll_owner_deadline(&mut self.deadline), if deadline_pending => {
				StoppingWake::Deadline
			},
			joined = self.tasks.set.join_next_with_id(), if has_tasks => {
				StoppingWake::OwnedTask(joined)
			},
			completion = poll_active_operation(&mut self.operation), if operation_active => {
				StoppingWake::Operation(completion)
			},
			_ = poll_application_shutdown(&mut self.application_shutdown),
				if application_pending => StoppingWake::ApplicationSettled,
			request = self.actor_receiver.recv(), if may_drain_ingress => {
				StoppingWake::Request(request)
			},
			_ = std::future::ready(()), if may_flush_deferred => {
					StoppingWake::FlushDeferred
			},
			publication = self.server.inner.application.next_publication(), if may_poll_events => {
				StoppingWake::ApplicationEvent(publication)
			},
			_ = std::future::pending::<()>() => unreachable!(),
		};

		match wake {
			StoppingWake::Deadline => {
				self.handle_deadline();
				OwnerDirective::Continue
			},
			StoppingWake::OwnedTask(Some(joined)) => self.harvest_task(joined),
			StoppingWake::OwnedTask(None) =>
				OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
			StoppingWake::Operation(completion) => self.finish_operation(completion),
			StoppingWake::ApplicationSettled => {
				self.application_settled = true;
				drop(self.application_shutdown.take());
				OwnerDirective::Continue
			},
			StoppingWake::Request(Some(request)) => {
				self.reject_during_shutdown(request);
				OwnerDirective::Continue
			},
			StoppingWake::Request(None) => {
				self.ingress_drained = true;
				OwnerDirective::Continue
			},
			StoppingWake::FlushDeferred => self.flush_deferred_events(),
			StoppingWake::ApplicationEvent(publication) =>
				self.handle_application_event(publication),
		}
	}

	fn handle_ordinary(&mut self, ordinary: AcceptingOrdinaryWake) -> OwnerDirective {
		match ordinary {
			AcceptingOrdinaryWake::Accepted(accepted) => self.handle_accepted(accepted),
			AcceptingOrdinaryWake::Request(Some(request)) => self.handle_request(request),
			AcceptingOrdinaryWake::Request(None) =>
				OwnerDirective::BeginStopping(StopCause::ActorIngressClosed),
			AcceptingOrdinaryWake::ApplicationEvent(publication) =>
				self.handle_application_event(publication),
		}
	}

	fn handle_accepted(
		&mut self,
		accepted: Result<LocalTransportStream, LocalTransportRefusal>,
	) -> OwnerDirective {
		match accepted {
			Ok(stream) => {
				let Some(actor_sender) = self.actor_sender.as_ref() else {
					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				};
				let connection_id =
					self.server.inner.connection_ids.fetch_add(1, Ordering::Relaxed);
				let server = self.server.clone();
				let publication_ingress = actor_sender.clone();
				let session_stop = self.stop_sender.subscribe();
				let session = Box::pin(async move {
					OwnedTaskResult::Session(
						server
							.handle_stream(stream, connection_id, publication_ingress, session_stop)
							.await,
					)
				});
				if self
					.tasks
					.spawn(OwnedTaskKind::Session, Some(connection_id), session, &mut self.receipt)
					.is_err()
				{
					OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
				} else {
					OwnerDirective::Continue
				}
			},
			Err(refusal) if refusal.invalidates_listener() =>
				OwnerDirective::BeginStopping(StopCause::EndpointRefusal(refusal)),
			Err(_) => OwnerDirective::Continue,
		}
	}

	fn handle_application_event(
		&mut self,
		publication: Option<ApplicationEventPublication>,
	) -> OwnerDirective {
		match publication {
			Some(publication) if self.operation.is_some() => {
				if self.deferred_events.len() >= A::EVENT_CAPACITY {
					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				}
				self.deferred_events.push_back(publication);

				OwnerDirective::Continue
			},
			Some(publication) => {
				self.enforce_deadline();
				self.publish_event_locked(decodex_protocol::CURRENT_VERSION, publication)
			},
			None => {
				self.event_eof = true;
				if self.phase == OwnerPhase::Accepting {
					OwnerDirective::BeginStopping(StopCause::UnexpectedEventEof)
				} else {
					OwnerDirective::Continue
				}
			},
		}
	}

	fn publish_locked(
		&mut self,
		version: ProtocolVersion,
		correlation: (CorrelationId, Option<CausationId>),
		publication: ApplicationPublication,
	) -> OwnerDirective {
		self.publish_event_locked(
			version,
			ApplicationEventPublication {
				correlation_id: correlation.0,
				causation_id: correlation.1,
				channel: publication.channel,
				entity_id: publication.entity_id,
				entity_revision: publication.entity_revision,
				event: publication.event,
			},
		)
	}

	fn publish_event_locked(
		&mut self,
		version: ProtocolVersion,
		publication: ApplicationEventPublication,
	) -> OwnerDirective {
		let Some(next_cursor) = self.state.cursor.0.checked_add(1) else {
			return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
		};
		self.state.cursor.0 = next_cursor;

		let event = EventEnvelope {
			version,
			server_id: self.server.inner.server_id.clone(),
			cursor: self.state.cursor,
			channel: publication.channel,
			entity_id: publication.entity_id,
			entity_revision: publication.entity_revision,
			correlation_id: publication.correlation_id,
			causation_id: publication.causation_id,
			payload: publication.event,
		};

		self.state.events.push_back(event.clone());
		while self.state.events.len() > self.server.inner.config.replay_capacity {
			self.state.events.pop_front();
		}

		let subscribers = self
			.state
			.subscribers
			.iter()
			.filter_map(|(connection_id, subscriber)| {
				event
					.payload
					.is_supported_in(subscriber.version)
					.then_some((*connection_id, subscriber.version))
			})
			.collect::<Vec<_>>();
		let mut directive = OwnerDirective::Continue;
		for (connection_id, version) in subscribers {
			let mut event = event.clone();

			event.version = version;
			let acceptance =
				accept_for_session(&mut self.state, connection_id, ServerMessage::Event(event));
			directive =
				Self::combine_directives(directive, Self::directive_for_acceptance(acceptance));
		}

		directive
	}

	fn handle_request(&mut self, request: PublicationRequest) -> OwnerDirective {
		if self.phase != OwnerPhase::Accepting || self.operation.is_some() {
			self.reject_during_shutdown(request);

			return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
		}

		match request {
			PublicationRequest::Register {
				connection_id,
				sender,
				seal_sender,
				hello,
				version,
				reply,
			} => {
				if self.state.contains_session(connection_id) {
					let _ = reply.send(Err(Refusal::ProtocolViolation {
						message: bounded_text("session identity is already owned"),
					}));

					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				}
				let base_cursor = self.state.cursor;
				let application = Arc::clone(&self.server.inner);
				let future = Box::pin(async move {
					AssertUnwindSafe(application.application.snapshot())
						.catch_unwind()
						.await
						.map_err(|_| ())
				});
				self.operation =
					Some(ActiveActorOperation::RegistrationSnapshot(PendingRegistrationSnapshot {
						connection_id,
						sender,
						seal_sender,
						hello,
						version,
						base_cursor,
						reply,
						future,
					}));

				OwnerDirective::Continue
			},
			PublicationRequest::Enqueue { connection_id, message, reply } => {
				let acceptance = accept_for_session(&mut self.state, connection_id, *message);
				let delivered = acceptance.is_accepted();
				let directive = Self::directive_for_acceptance(acceptance);
				let _ = reply.send(delivered);

				directive
			},
			PublicationRequest::Command { connection_id, command, version, reply } =>
				self.handle_command_request(connection_id, command, version, reply),
		}
	}

	fn handle_command_request(
		&mut self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
		reply: oneshot::Sender<bool>,
	) -> OwnerDirective {
		let fingerprint =
			serde_json::to_vec(&(version, &command.expected_revision, &command.payload))
				.expect("typed command serialization cannot fail");
		let receipt_key = (version, command.idempotency_key.clone());
		if let Some(stored) = self.state.receipts.get(&receipt_key).cloned() {
			let command_receipt = CommandReceipt {
				version,
				server_id: self.server.inner.server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				disposition: ReceiptDisposition::Duplicate,
				original_client_command_id: stored.original_client_command_id,
			};
			let mut result = stored.result;
			result.version = version;
			result.client_command_id = command.client_command_id.clone();
			if stored.fingerprint != fingerprint {
				result.outcome = CommandOutcome::Rejected;
				result.entity_revision = None;
				result.payload = None;
				result.error = Some(CommandError::IdempotencyConflict);
			}
			let receipt_acceptance = accept_for_session(
				&mut self.state,
				connection_id,
				ServerMessage::CommandReceipt(command_receipt),
			);
			let result_acceptance = accept_for_session(
				&mut self.state,
				connection_id,
				ServerMessage::CommandResult(result),
			);
			let delivered = receipt_acceptance.is_accepted() && result_acceptance.is_accepted();
			let directive = Self::combine_directives(
				Self::directive_for_acceptance(receipt_acceptance),
				Self::directive_for_acceptance(result_acceptance),
			);
			let _ = reply.send(delivered);

			return directive;
		}

		let version_receipt_count = self
			.state
			.receipts
			.keys()
			.filter(|(stored_version, _)| stored_version == &version)
			.count();
		if version_receipt_count >= self.server.inner.config.receipt_capacity {
			let command_receipt = CommandReceipt {
				version,
				server_id: self.server.inner.server_id.clone(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				disposition: ReceiptDisposition::Refused,
				original_client_command_id: command.client_command_id.clone(),
			};
			let result = CommandResultEnvelope {
				version,
				server_id: self.server.inner.server_id.clone(),
				client_command_id: command.client_command_id,
				idempotency_key: command.idempotency_key,
				outcome: CommandOutcome::Rejected,
				entity_revision: None,
				payload: None,
				error: Some(CommandError::IdempotencyCapacityExceeded {
					capacity: self.server.inner.config.receipt_capacity,
				}),
			};
			let receipt_acceptance = accept_for_session(
				&mut self.state,
				connection_id,
				ServerMessage::CommandReceipt(command_receipt),
			);
			let result_acceptance = accept_for_session(
				&mut self.state,
				connection_id,
				ServerMessage::CommandResult(result),
			);
			let delivered = receipt_acceptance.is_accepted() && result_acceptance.is_accepted();
			let directive = Self::combine_directives(
				Self::directive_for_acceptance(receipt_acceptance),
				Self::directive_for_acceptance(result_acceptance),
			);
			let _ = reply.send(delivered);

			return directive;
		}

		self.receipt.actor_commands_admitted =
			self.receipt.actor_commands_admitted.saturating_add(1);
		let application = Arc::clone(&self.server.inner);
		let execution_command = command.clone();
		let future = Box::pin(async move {
			AssertUnwindSafe(application.application.execute(&execution_command))
				.catch_unwind()
				.await
				.map_err(|_| ())
		});
		self.operation = Some(ActiveActorOperation::Command(ActiveCommand {
			connection_id,
			command,
			version,
			fingerprint,
			reply,
			future,
		}));

		OwnerDirective::Continue
	}

	fn finish_operation(&mut self, completion: ActiveActorOperationCompletion) -> OwnerDirective {
		let Some(operation) = self.operation.take() else {
			return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
		};
		match operation {
			ActiveActorOperation::Command(command) => match completion {
				ActiveActorOperationCompletion::Command(execution) =>
					self.finish_active_command(command, *execution),
				ActiveActorOperationCompletion::RegistrationSnapshot(_) => {
					self.operation = Some(ActiveActorOperation::Command(command));

					OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
				},
			},
			ActiveActorOperation::RegistrationSnapshot(snapshot) => match completion {
				ActiveActorOperationCompletion::RegistrationSnapshot(result) =>
					self.finish_registration_snapshot(snapshot, result),
				ActiveActorOperationCompletion::Command(_) => {
					self.operation = Some(ActiveActorOperation::RegistrationSnapshot(snapshot));

					OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
				},
			},
		}
	}

	fn finish_registration_snapshot(
		&mut self,
		snapshot: PendingRegistrationSnapshot,
		result: RegistrationSnapshotResult,
	) -> OwnerDirective {
		if self.phase != OwnerPhase::Accepting {
			let _ = snapshot.reply.send(Err(Self::shutdown_refusal()));

			return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
		}
		let snapshot_items = match result {
			Ok(items) => items,
			Err(()) => {
				let _ = snapshot.reply.send(Err(Refusal::ProtocolViolation {
					message: bounded_text("application snapshot failed"),
				}));

				return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
			},
		};
		if snapshot_items.len() > self.server.inner.config.maximum_snapshot_items {
			let _ = snapshot.reply.send(Err(Refusal::ProtocolViolation {
				message: bounded_text("application snapshot exceeds the bounded item limit"),
			}));

			return Self::combine_directives(
				OwnerDirective::Continue,
				self.flush_deferred_events(),
			);
		}
		if self.state.cursor != snapshot.base_cursor
			|| self.state.contains_session(snapshot.connection_id)
		{
			let _ = snapshot.reply.send(Err(Refusal::ProtocolViolation {
				message: bounded_text("registration snapshot lost its publication cut"),
			}));

			return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
		}

		let (reconnect, mut messages) = self.server.reconnect_messages(
			&self.state,
			snapshot.hello.resume.as_ref(),
			snapshot.version,
			snapshot_items,
		);
		messages.insert(
			0,
			ServerMessage::Welcome(ServerWelcome {
				version: snapshot.version,
				supported: SupportedVersions::current(),
				server_id: self.server.inner.server_id.clone(),
				instance_id: supports_publication_instance(snapshot.version)
					.then(|| self.server.inner.instance_id.clone())
					.flatten(),
				cursor: snapshot.base_cursor,
				reconnect,
			}),
		);
		self.state.subscribers.insert(
			snapshot.connection_id,
			Subscriber::new(snapshot.sender, snapshot.seal_sender, snapshot.version),
		);
		if snapshot.reply.send(Ok(messages)).is_err() {
			self.state
				.seal_session(snapshot.connection_id, SessionSealReason::RegistrationAbandoned);
		}

		self.flush_deferred_events()
	}

	fn finish_active_command(
		&mut self,
		active: ActiveCommand,
		execution: ActiveCommandResult,
	) -> OwnerDirective {
		self.receipt.actor_commands_settled = self.receipt.actor_commands_settled.saturating_add(1);
		let mut directive = self.mark_command_settled();
		self.enforce_deadline();
		let Ok(execution) = execution else {
			let _ = active.reply.send(false);
			directive = Self::combine_directives(
				directive,
				OwnerDirective::BeginStopping(StopCause::OwnerIntegrity),
			);

			return directive;
		};

		let correlation =
			(active.command.correlation_id.clone(), active.command.causation_id.clone());
		let receipt_key = (active.version, active.command.idempotency_key.clone());
		let (result, publication) = result_from_execution(
			&self.server.inner.server_id,
			&active.command,
			active.version,
			execution,
		);
		if result.outcome != CommandOutcome::AcceptanceUnknown {
			self.state.receipts.insert(
				receipt_key,
				StoredCommand {
					fingerprint: active.fingerprint,
					original_client_command_id: active.command.client_command_id.clone(),
					result: result.clone(),
				},
			);
		}
		let command_receipt = CommandReceipt {
			version: active.version,
			server_id: self.server.inner.server_id.clone(),
			client_command_id: active.command.client_command_id,
			idempotency_key: active.command.idempotency_key,
			disposition: ReceiptDisposition::Executed,
			original_client_command_id: result.client_command_id.clone(),
		};
		let receipt_acceptance = accept_for_session(
			&mut self.state,
			active.connection_id,
			ServerMessage::CommandReceipt(command_receipt),
		);
		let result_acceptance = accept_for_session(
			&mut self.state,
			active.connection_id,
			ServerMessage::CommandResult(result),
		);
		let delivered = receipt_acceptance.is_accepted() && result_acceptance.is_accepted();
		directive =
			Self::combine_directives(directive, Self::directive_for_acceptance(receipt_acceptance));
		directive =
			Self::combine_directives(directive, Self::directive_for_acceptance(result_acceptance));
		if let Some(publication) = publication.filter(ApplicationPublication::publishes_event) {
			self.enforce_deadline();
			let publication_directive =
				self.publish_locked(active.version, correlation, publication);
			directive = Self::combine_directives(directive, publication_directive);
		}
		let flush_directive = self.flush_deferred_events();
		directive = Self::combine_directives(directive, flush_directive);
		let delivered = delivered && self.state.subscribers.contains_key(&active.connection_id);
		let _ = active.reply.send(delivered);

		directive
	}

	fn flush_deferred_events(&mut self) -> OwnerDirective {
		if self.deferred_events.is_empty() {
			return OwnerDirective::Continue;
		}
		self.enforce_deadline();
		let mut directive = OwnerDirective::Continue;
		while let Some(publication) = self.deferred_events.pop_front() {
			let publication_directive =
				self.publish_event_locked(decodex_protocol::CURRENT_VERSION, publication);
			directive = Self::combine_directives(directive, publication_directive);
		}

		directive
	}

	fn harvest_task(
		&mut self,
		joined: Result<(TokioTaskId, OwnedTaskCompletion), JoinError>,
	) -> OwnerDirective {
		self.receipt.record_harvested_task();

		match joined {
			Ok((task_id, completion)) => {
				let completion_identity = completion.identity;
				let Some(record) = self.tasks.take_record(task_id) else {
					self.receipt.record_failed_task(Some(completion_identity));

					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				};
				if record.identity != completion_identity {
					if let Some(connection_id) = record.connection_id {
						self.state
							.resolve_failed_session(connection_id, record.deadline_classified);
					}
					self.receipt.record_failed_task(Some(record.identity));

					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				}
				match (record.identity.kind, completion.result) {
					(OwnedTaskKind::Service, OwnedTaskResult::Service) => {
						if self.phase == OwnerPhase::Accepting {
							self.receipt.record_failed_task(Some(completion_identity));

							OwnerDirective::BeginStopping(StopCause::ChildFailure(
								completion_identity,
							))
						} else {
							self.receipt.record_expected_task();

							OwnerDirective::Continue
						}
					},
					(OwnedTaskKind::Session, OwnedTaskResult::Session(completion))
						if record.connection_id == Some(completion.connection_id) =>
					{
						let connection_id = completion.connection_id;
						match self
							.state
							.reconcile_session_completion(completion, record.deadline_classified)
						{
							SessionCompletionClassification::Unregistered
							| SessionCompletionClassification::ExactDrained
							| SessionCompletionClassification::SessionLocal => {
								self.receipt.record_expected_task();

								OwnerDirective::Continue
							},
							SessionCompletionClassification::TransportFailed => {
								self.receipt.record_failed_task(Some(completion_identity));

								OwnerDirective::BeginStopping(StopCause::TransportFailed(
									completion_identity,
								))
							},
							SessionCompletionClassification::Deadline => {
								self.receipt.record_forced_cancelled_task(completion_identity);

								OwnerDirective::BeginStopping(StopCause::DeadlineClassification(
									completion_identity,
								))
							},
							SessionCompletionClassification::Invalid => {
								self.state.resolve_failed_session(
									connection_id,
									record.deadline_classified,
								);
								self.receipt.record_failed_task(Some(completion_identity));

								OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
							},
						}
					},
					_ => {
						if let Some(connection_id) = record.connection_id {
							self.state
								.resolve_failed_session(connection_id, record.deadline_classified);
						}
						self.receipt.record_failed_task(Some(completion_identity));

						OwnerDirective::BeginStopping(StopCause::OwnerIntegrity)
					},
				}
			},
			Err(error) => {
				let Some(record) = self.tasks.take_record(error.id()) else {
					self.receipt.record_failed_task(None);

					return OwnerDirective::BeginStopping(StopCause::OwnerIntegrity);
				};
				let identity = record.identity;
				let mut deadline_cancel = false;
				if let Some(connection_id) = record.connection_id {
					deadline_cancel = error.is_cancelled()
						&& record.deadline_classified
						&& matches!(
							self.state.reconcile_deadline_cancel(connection_id),
							SessionCompletionClassification::Deadline
						);
					if !deadline_cancel {
						self.state
							.resolve_failed_session(connection_id, record.deadline_classified);
					}
				}

				if error.is_panic() {
					self.receipt.record_panicked_task(identity);

					OwnerDirective::BeginStopping(StopCause::ChildPanic(identity))
				} else if deadline_cancel {
					self.receipt.record_forced_cancelled_task(identity);

					OwnerDirective::BeginStopping(StopCause::DeadlineClassification(identity))
				} else {
					self.receipt.record_failed_task(Some(identity));

					OwnerDirective::BeginStopping(StopCause::ChildFailure(identity))
				}
			},
		}
	}

	fn finish_task_accounting(&mut self) {
		let spawned = self.receipt.spawned_sessions.saturating_add(self.receipt.spawned_services);
		let classified = self
			.receipt
			.expected_tasks
			.saturating_add(self.receipt.panicked_tasks)
			.saturating_add(self.receipt.failed_tasks)
			.saturating_add(self.receipt.forced_cancelled_tasks);

		if !self.tasks.is_empty()
			|| !self.tasks.identities.is_empty()
			|| self.tasks.active_sessions != 0
			|| !self.state.subscribers.is_empty()
			|| !self.state.sealed_sessions.is_empty()
			|| spawned != self.receipt.harvested_tasks
			|| classified != self.receipt.harvested_tasks
			|| self.receipt.actor_commands_admitted != self.receipt.actor_commands_settled
		{
			self.receipt.record_owner_integrity();
		}
	}

	fn ensure_termination_fact(receipt: &mut TerminationReceiptBuilder) {
		if !receipt.requested_shutdown
			&& receipt.cleanup_refusal.is_none()
			&& receipt.endpoint_refusal.is_none()
			&& receipt.owner_integrity_failures == 0
			&& receipt.panicked_tasks == 0
			&& receipt.failed_tasks == 0
			&& receipt.forced_cancelled_tasks == 0
		{
			receipt.record_owner_integrity();
		}
	}

	fn finish(mut self) -> TerminationReceipt {
		debug_assert_eq!(self.phase, OwnerPhase::Closed);
		self.finish_task_accounting();
		let Self {
			server,
			listener,
			shutdown_receiver,
			phase: _,
			deadline,
			operation,
			state,
			deferred_events,
			tasks,
			mut receipt,
			actor_sender,
			actor_receiver,
			stop_sender,
			service_stop_sender,
			event_eof: _,
			ingress_drained: _,
			application_shutdown,
			application_settled: _,
			command_shutdown: _,
		} = self;
		drop(operation);
		drop(deferred_events);
		drop(state);
		drop(actor_sender);
		drop(actor_receiver);
		drop(stop_sender);
		drop(service_stop_sender);
		drop(application_shutdown);
		drop(deadline);
		drop(shutdown_receiver);
		drop(tasks);
		drop(server);

		if let Err(refusal) = listener.cleanup() {
			receipt.record_cleanup_refusal(refusal);
		}
		Self::ensure_termination_fact(&mut receipt);

		receipt.finish()
	}
}

type ActiveCommandResult = Result<Result<ApplicationPublication, CommandError>, ()>;
type ActiveCommandFuture = Pin<Box<dyn Future<Output = ActiveCommandResult> + Send + 'static>>;

struct ActiveCommand {
	connection_id: u64,
	command: CommandEnvelope,
	version: ProtocolVersion,
	fingerprint: Vec<u8>,
	reply: oneshot::Sender<bool>,
	future: ActiveCommandFuture,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SessionOrdinal(u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SessionOrdinalProgress {
	Empty,
	Through(SessionOrdinal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionAcceptance {
	Accepted(SessionOrdinal),
	Unavailable,
	Sealed(SessionSealReason),
}

impl SessionAcceptance {
	fn is_accepted(self) -> bool {
		match self {
			Self::Accepted(_ordinal) => true,
			Self::Unavailable | Self::Sealed(_) => false,
		}
	}
}

enum InitialHello {
	Received(ClientHello),
	Stopped,
	Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionReaderCompletion {
	PeerClose,
	Reason(SessionSealReason),
}

impl SessionReaderCompletion {
	fn is_peer_close(self) -> bool {
		matches!(self, Self::PeerClose)
	}

	fn requested_seal(self) -> SessionSealReason {
		match self {
			Self::PeerClose => SessionSealReason::PeerDisconnected,
			Self::Reason(reason) => reason,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionSealReason {
	ActorUnavailable,
	Deadline,
	InitialPrefixFailed,
	OrdinalExhausted,
	OutboundClosed,
	OutboundFull,
	PeerDisconnected,
	RegistrationAbandoned,
	ServerDrained,
	ServerShutdown,
	TaskFailed,
	WriterFailed,
}

impl SessionSealReason {
	fn canonicalizes_outbound_closed(self) -> bool {
		matches!(
			self,
			Self::ActorUnavailable
				| Self::PeerDisconnected
				| Self::ServerShutdown
				| Self::WriterFailed
		)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionTransportFailure {
	InitialPrefixEncoding,
	InitialPrefixTooLarge,
	InitialPrefixWrite,
	MessageEncoding,
	MessageTooLarge,
	MessageWrite,
	CloseWrite,
}

impl SessionTransportFailure {
	fn is_initial_prefix(self) -> bool {
		matches!(
			self,
			Self::InitialPrefixEncoding | Self::InitialPrefixTooLarge | Self::InitialPrefixWrite
		)
	}

	fn is_encoding(self) -> bool {
		matches!(self, Self::InitialPrefixEncoding | Self::MessageEncoding)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionTransportDisposition {
	// These variants prove local sink progress only. WebSocket writes do not prove peer receipt.
	DrainedThrough(SessionOrdinalProgress),
	TransportFailed { locally_written: SessionOrdinalProgress, cause: SessionTransportFailure },
}

impl SessionTransportDisposition {
	fn drained(locally_written: SessionOrdinalProgress) -> Self {
		Self::DrainedThrough(locally_written)
	}

	fn failed(locally_written: SessionOrdinalProgress, cause: SessionTransportFailure) -> Self {
		Self::TransportFailed { locally_written, cause }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionCompletionClassification {
	Unregistered,
	ExactDrained,
	SessionLocal,
	TransportFailed,
	Deadline,
	Invalid,
}

struct SessionTaskCompletion {
	connection_id: u64,
	registration: SessionRegistrationCompletion,
}

enum SessionRegistrationCompletion {
	Unregistered,
	Registered { seal_reason: SessionSealReason, transport: SessionTransportDisposition },
}

impl SessionTaskCompletion {
	fn unregistered(connection_id: u64) -> Self {
		Self { connection_id, registration: SessionRegistrationCompletion::Unregistered }
	}

	fn registered(
		connection_id: u64,
		seal_reason: SessionSealReason,
		transport: SessionTransportDisposition,
	) -> Self {
		Self {
			connection_id,
			registration: SessionRegistrationCompletion::Registered { seal_reason, transport },
		}
	}
}

enum OwnedTaskResult {
	Session(SessionTaskCompletion),
	Service,
}

enum PublicationRequest {
	Register {
		connection_id: u64,
		sender: mpsc::Sender<OutboundItem>,
		seal_sender: oneshot::Sender<SessionSealReason>,
		hello: ClientHello,
		version: ProtocolVersion,
		reply: oneshot::Sender<Result<Vec<ServerMessage>, Refusal>>,
	},
	Enqueue {
		connection_id: u64,
		message: Box<ServerMessage>,
		reply: oneshot::Sender<bool>,
	},
	Command {
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
		reply: oneshot::Sender<bool>,
	},
}

async fn poll_active_operation(
	operation: &mut Option<ActiveActorOperation>,
) -> ActiveActorOperationCompletion {
	match operation.as_mut().expect("guarded active operation exists") {
		ActiveActorOperation::Command(command) =>
			ActiveActorOperationCompletion::Command(Box::new(command.future.as_mut().await)),
		ActiveActorOperation::RegistrationSnapshot(snapshot) =>
			ActiveActorOperationCompletion::RegistrationSnapshot(snapshot.future.as_mut().await),
	}
}

async fn poll_owner_deadline(deadline: &mut Option<OwnerDeadline>) {
	deadline.as_mut().expect("stopping owns one deadline").sleep.as_mut().await;
}

async fn poll_application_shutdown(shutdown: &mut Option<ApplicationShutdownFuture>) {
	shutdown.as_mut().expect("application shutdown is active").as_mut().await;
}

#[derive(Default)]
struct PublicationState {
	cursor: Cursor,
	events: VecDeque<EventEnvelope>,
	receipts: HashMap<(ProtocolVersion, IdempotencyKey), StoredCommand>,
	subscribers: HashMap<u64, Subscriber>,
	sealed_sessions: HashMap<u64, SealedSession>,
}

struct Subscriber {
	sender: mpsc::Sender<OutboundItem>,
	seal_sender: oneshot::Sender<SessionSealReason>,
	version: ProtocolVersion,
	next_ordinal: SessionOrdinal,
	accepted_through: SessionOrdinalProgress,
}

impl Subscriber {
	fn new(
		sender: mpsc::Sender<OutboundItem>,
		seal_sender: oneshot::Sender<SessionSealReason>,
		version: ProtocolVersion,
	) -> Self {
		Self {
			sender,
			seal_sender,
			version,
			next_ordinal: SessionOrdinal(1),
			accepted_through: SessionOrdinalProgress::Empty,
		}
	}
}

struct OutboundItem {
	ordinal: SessionOrdinal,
	message: ServerMessage,
}

struct SealedSession {
	accepted_through: SessionOrdinalProgress,
	reason: SessionSealReason,
	deadline_classified: bool,
}

impl SealedSession {
	fn reconcile(&self, transport: SessionTransportDisposition) -> SessionCompletionClassification {
		let progress_matches = match transport {
			SessionTransportDisposition::DrainedThrough(locally_written) =>
				locally_written == self.accepted_through,
			SessionTransportDisposition::TransportFailed {
				locally_written,
				cause: SessionTransportFailure::CloseWrite,
			} => locally_written == self.accepted_through,
			SessionTransportDisposition::TransportFailed { locally_written, .. } =>
				locally_written <= self.accepted_through,
		};
		if !progress_matches {
			return SessionCompletionClassification::Invalid;
		}

		let reason_matches = match (self.reason, transport) {
			(
				SessionSealReason::InitialPrefixFailed,
				SessionTransportDisposition::TransportFailed { cause, .. },
			) => cause.is_initial_prefix(),
			(
				SessionSealReason::WriterFailed,
				SessionTransportDisposition::TransportFailed { cause, .. },
			) => !cause.is_initial_prefix(),
			(SessionSealReason::RegistrationAbandoned | SessionSealReason::TaskFailed, _) => false,
			(SessionSealReason::InitialPrefixFailed, _)
			| (SessionSealReason::WriterFailed, _)
			| (SessionSealReason::OutboundClosed, _) => false,
			_ => true,
		};
		if !reason_matches {
			return SessionCompletionClassification::Invalid;
		}
		if matches!(
			transport,
			SessionTransportDisposition::TransportFailed { cause, .. } if cause.is_encoding()
		) {
			return SessionCompletionClassification::TransportFailed;
		}
		if self.deadline_classified {
			return SessionCompletionClassification::Deadline;
		}
		match transport {
			SessionTransportDisposition::DrainedThrough(_) => match self.reason {
				SessionSealReason::ActorUnavailable =>
					SessionCompletionClassification::TransportFailed,
				SessionSealReason::Deadline
				| SessionSealReason::InitialPrefixFailed
				| SessionSealReason::OutboundClosed
				| SessionSealReason::RegistrationAbandoned
				| SessionSealReason::TaskFailed
				| SessionSealReason::WriterFailed => SessionCompletionClassification::Invalid,
				SessionSealReason::OrdinalExhausted
				| SessionSealReason::OutboundFull
				| SessionSealReason::PeerDisconnected
				| SessionSealReason::ServerDrained
				| SessionSealReason::ServerShutdown => SessionCompletionClassification::ExactDrained,
			},
			SessionTransportDisposition::TransportFailed { .. } => match self.reason {
				SessionSealReason::InitialPrefixFailed
				| SessionSealReason::OutboundFull
				| SessionSealReason::PeerDisconnected
				| SessionSealReason::ServerDrained
				| SessionSealReason::ServerShutdown
				| SessionSealReason::WriterFailed => SessionCompletionClassification::SessionLocal,
				SessionSealReason::ActorUnavailable | SessionSealReason::OrdinalExhausted =>
					SessionCompletionClassification::TransportFailed,
				SessionSealReason::Deadline
				| SessionSealReason::OutboundClosed
				| SessionSealReason::RegistrationAbandoned
				| SessionSealReason::TaskFailed => SessionCompletionClassification::Invalid,
			},
		}
	}
}

impl PublicationState {
	fn contains_session(&self, connection_id: u64) -> bool {
		self.subscribers.contains_key(&connection_id)
			|| self.sealed_sessions.contains_key(&connection_id)
	}

	fn seal_session(&mut self, connection_id: u64, reason: SessionSealReason) {
		if self.sealed_sessions.contains_key(&connection_id) {
			return;
		}
		if let Some(subscriber) = self.subscribers.remove(&connection_id) {
			let Subscriber { sender, seal_sender, accepted_through, .. } = subscriber;
			self.sealed_sessions.insert(
				connection_id,
				SealedSession {
					accepted_through,
					reason,
					deadline_classified: reason == SessionSealReason::Deadline,
				},
			);
			// Publish the first-wins reason before FIFO closure wakes the session writer.
			let _ = seal_sender.send(reason);
			drop(sender);
		}
	}

	fn seal_all(&mut self, reason: SessionSealReason) {
		let connection_ids = self.subscribers.keys().copied().collect::<Vec<_>>();
		for connection_id in connection_ids {
			self.seal_session(connection_id, reason);
		}
	}

	fn canonicalize_outbound_closed(&mut self, connection_id: u64, seal_reason: SessionSealReason) {
		if !seal_reason.canonicalizes_outbound_closed() {
			return;
		}
		let Some(sealed) = self.sealed_sessions.get_mut(&connection_id) else {
			return;
		};
		if sealed.reason == SessionSealReason::OutboundClosed {
			sealed.reason = seal_reason;
		}
	}

	fn classify_deadline(&mut self) {
		self.seal_all(SessionSealReason::Deadline);
		for sealed in self.sealed_sessions.values_mut() {
			sealed.deadline_classified = true;
		}
	}

	fn reconcile_session_completion(
		&mut self,
		completion: SessionTaskCompletion,
		deadline_classified: bool,
	) -> SessionCompletionClassification {
		match completion.registration {
			SessionRegistrationCompletion::Unregistered => {
				if self.contains_session(completion.connection_id) {
					SessionCompletionClassification::Invalid
				} else if deadline_classified {
					SessionCompletionClassification::Deadline
				} else {
					SessionCompletionClassification::Unregistered
				}
			},
			SessionRegistrationCompletion::Registered { seal_reason, transport } => {
				if self.subscribers.contains_key(&completion.connection_id) {
					self.seal_session(completion.connection_id, seal_reason);
				}
				self.canonicalize_outbound_closed(completion.connection_id, seal_reason);
				let Some(sealed) = self.sealed_sessions.get(&completion.connection_id) else {
					return SessionCompletionClassification::Invalid;
				};
				if sealed.deadline_classified != deadline_classified {
					return SessionCompletionClassification::Invalid;
				}
				let classification = sealed.reconcile(transport);
				if classification != SessionCompletionClassification::Invalid || deadline_classified
				{
					self.sealed_sessions.remove(&completion.connection_id);
				}

				classification
			},
		}
	}

	fn reconcile_deadline_cancel(&mut self, connection_id: u64) -> SessionCompletionClassification {
		if self.subscribers.contains_key(&connection_id) {
			return SessionCompletionClassification::Invalid;
		}
		match self.sealed_sessions.remove(&connection_id) {
			Some(sealed) if sealed.deadline_classified => SessionCompletionClassification::Deadline,
			Some(_) => SessionCompletionClassification::Invalid,
			None => SessionCompletionClassification::Deadline,
		}
	}

	fn seal_failed_session(&mut self, connection_id: u64) {
		self.seal_session(connection_id, SessionSealReason::TaskFailed);
	}

	fn resolve_failed_session(&mut self, connection_id: u64, deadline_classified: bool) {
		self.seal_failed_session(connection_id);
		if deadline_classified {
			if let Some(sealed) = self.sealed_sessions.get_mut(&connection_id) {
				sealed.deadline_classified = true;
			}
			self.sealed_sessions.remove(&connection_id);
		}
	}

	fn resolve_deadline_orphans(&mut self, mut task_owns_connection: impl FnMut(u64) -> bool) {
		self.sealed_sessions.retain(|connection_id, sealed| {
			debug_assert!(sealed.deadline_classified);

			task_owns_connection(*connection_id)
		});
	}
}

#[derive(Clone)]
struct StoredCommand {
	fingerprint: Vec<u8>,
	original_client_command_id: ClientCommandId,
	result: CommandResultEnvelope,
}

struct OwnedTaskCompletion {
	identity: OwnedTaskIdentity,
	result: OwnedTaskResult,
}

struct OwnedTaskRecord {
	identity: OwnedTaskIdentity,
	connection_id: Option<u64>,
	deadline_classified: bool,
	abort_handle: AbortHandle,
}

struct OwnedTasks {
	set: JoinSet<OwnedTaskCompletion>,
	identities: HashMap<TokioTaskId, OwnedTaskRecord>,
	next_spawn_id: u64,
	active_sessions: usize,
}

impl OwnedTasks {
	fn new() -> Self {
		Self {
			set: JoinSet::new(),
			identities: HashMap::new(),
			next_spawn_id: 1,
			active_sessions: 0,
		}
	}

	fn spawn(
		&mut self,
		kind: OwnedTaskKind,
		connection_id: Option<u64>,
		future: OwnedFuture,
		receipt: &mut TerminationReceiptBuilder,
	) -> Result<(), ()> {
		if (kind == OwnedTaskKind::Session) != connection_id.is_some() {
			return Err(());
		}
		let next = self.next_spawn_id.checked_add(1).ok_or(())?;
		let identity = OwnedTaskIdentity { spawn_id: SpawnId(self.next_spawn_id), kind };
		self.next_spawn_id = next;

		let abort_handle = self.set.spawn(async move {
			let result = future.await;

			OwnedTaskCompletion { identity, result }
		});
		let task_id = abort_handle.id();
		let record =
			OwnedTaskRecord { identity, connection_id, deadline_classified: false, abort_handle };
		let prior = self.identities.insert(task_id, record);

		if prior.is_some() {
			return Err(());
		}

		receipt.record_spawn(kind);
		if kind == OwnedTaskKind::Session {
			self.active_sessions = self.active_sessions.checked_add(1).ok_or(())?;
		}

		Ok(())
	}

	fn active_session_count(&self) -> usize {
		self.active_sessions
	}

	fn take_record(&mut self, task_id: TokioTaskId) -> Option<OwnedTaskRecord> {
		let record = self.identities.remove(&task_id)?;
		if record.identity.kind == OwnedTaskKind::Session {
			self.active_sessions = self.active_sessions.checked_sub(1)?;
		}

		Some(record)
	}

	fn classify_session_deadlines(&mut self, state: &mut PublicationState) {
		state.classify_deadline();
		for record in self.identities.values_mut() {
			if record.identity.kind == OwnedTaskKind::Session {
				record.deadline_classified = true;
			}
		}
		let identities = &self.identities;
		state.resolve_deadline_orphans(|connection_id| {
			identities.values().any(|record| record.connection_id == Some(connection_id))
		});
	}

	fn is_empty(&self) -> bool {
		self.set.is_empty()
	}

	fn has_service_tasks(&self) -> bool {
		self.identities.values().any(|record| record.identity.kind == OwnedTaskKind::Service)
	}

	fn abort_classified_sessions(&self) {
		for record in self.identities.values() {
			if record.identity.kind == OwnedTaskKind::Session && record.deadline_classified {
				record.abort_handle.abort();
			}
		}
	}
}

#[derive(Default)]
struct TerminationReceiptBuilder {
	requested_shutdown: bool,
	spawned_sessions: u64,
	spawned_services: u64,
	actor_commands_admitted: u64,
	actor_commands_settled: u64,
	actor_command_deadline: ActorCommandDeadlineClass,
	harvested_tasks: u64,
	expected_tasks: u64,
	panicked_tasks: u64,
	failed_tasks: u64,
	forced_cancelled_tasks: u64,
	owner_integrity_failures: u64,
	lowest_panicked: Option<OwnedTaskIdentity>,
	lowest_failed: Option<OwnedTaskIdentity>,
	lowest_forced: Option<OwnedTaskIdentity>,
	endpoint_refusal: Option<LocalTransportRefusal>,
	cleanup_refusal: Option<LocalTransportRefusal>,
}

impl TerminationReceiptBuilder {
	fn record_spawn(&mut self, kind: OwnedTaskKind) {
		match kind {
			OwnedTaskKind::Session => {
				self.spawned_sessions = self.spawned_sessions.saturating_add(1);
			},
			OwnedTaskKind::Service => {
				self.spawned_services = self.spawned_services.saturating_add(1);
			},
		}
	}

	fn record_requested_shutdown(&mut self) {
		self.requested_shutdown = true;
	}

	fn record_harvested_task(&mut self) {
		self.harvested_tasks = self.harvested_tasks.saturating_add(1);
	}

	fn record_expected_task(&mut self) {
		self.expected_tasks = self.expected_tasks.saturating_add(1);
	}

	fn record_panicked_task(&mut self, identity: OwnedTaskIdentity) {
		self.panicked_tasks = self.panicked_tasks.saturating_add(1);
		record_lowest(&mut self.lowest_panicked, identity);
	}

	fn record_failed_task(&mut self, identity: Option<OwnedTaskIdentity>) {
		self.failed_tasks = self.failed_tasks.saturating_add(1);
		if let Some(identity) = identity {
			record_lowest(&mut self.lowest_failed, identity);
		}
	}

	fn record_forced_cancelled_task(&mut self, identity: OwnedTaskIdentity) {
		self.forced_cancelled_tasks = self.forced_cancelled_tasks.saturating_add(1);
		record_lowest(&mut self.lowest_forced, identity);
	}

	fn record_owner_integrity(&mut self) {
		self.owner_integrity_failures = self.owner_integrity_failures.saturating_add(1);
	}

	fn record_endpoint_refusal(&mut self, refusal: LocalTransportRefusal) {
		self.endpoint_refusal = select_refusal(self.endpoint_refusal, refusal);
	}

	fn record_cleanup_refusal(&mut self, refusal: LocalTransportRefusal) {
		self.cleanup_refusal = select_refusal(self.cleanup_refusal, refusal);
	}

	fn finish(self) -> TerminationReceipt {
		let primary = if let Some(refusal) = self.cleanup_refusal {
			TerminationPrimary::CleanupRefusal(refusal)
		} else if let Some(refusal) = self.endpoint_refusal {
			TerminationPrimary::EndpointRefusal(refusal)
		} else if self.owner_integrity_failures > 0 {
			TerminationPrimary::OwnerIntegrity
		} else if let Some(identity) = self.lowest_panicked {
			TerminationPrimary::ChildPanic(identity)
		} else if let Some(identity) = self.lowest_failed {
			TerminationPrimary::ChildFailure(identity)
		} else if let Some(identity) = self.lowest_forced {
			TerminationPrimary::ForcedDeadline(identity)
		} else if matches!(
			self.actor_command_deadline,
			ActorCommandDeadlineClass::DeadlineElapsedThenSettledDuringApplicationShutdown
		) {
			TerminationPrimary::ActorCommandDeadline
		} else {
			TerminationPrimary::RequestedShutdown
		};

		TerminationReceipt {
			primary,
			spawned_sessions: self.spawned_sessions,
			spawned_services: self.spawned_services,
			actor_commands_admitted: self.actor_commands_admitted,
			actor_commands_settled: self.actor_commands_settled,
			actor_command_deadline: self.actor_command_deadline,
			harvested_tasks: self.harvested_tasks,
			expected_tasks: self.expected_tasks,
			panicked_tasks: self.panicked_tasks,
			failed_tasks: self.failed_tasks,
			forced_cancelled_tasks: self.forced_cancelled_tasks,
			owner_integrity_failures: self.owner_integrity_failures,
			lowest_panicked: self.lowest_panicked,
			lowest_failed: self.lowest_failed,
			lowest_forced: self.lowest_forced,
			endpoint_refusal: self.endpoint_refusal,
			cleanup_refusal: self.cleanup_refusal,
		}
	}
}

/// Failure to publish, own, or read back the local server lifecycle.
#[derive(Debug)]
pub enum ServerError {
	/// The local transport refused publication before the lifecycle task existed.
	LocalTransport(LocalTransportRefusal),
	/// The top-level lifecycle task itself failed to join.
	LifecycleJoin(JoinError),
	/// The lifecycle handle was already consumed.
	LifecycleUnavailable,
	/// The lifecycle completed with a deterministic abnormal receipt.
	Terminated(TerminationReceipt),
}

impl std::error::Error for ServerError {}

impl Display for ServerError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::LocalTransport(refusal) => Display::fmt(refusal, formatter),
			Self::LifecycleJoin(_) => formatter.write_str("server lifecycle task failed"),
			Self::LifecycleUnavailable => formatter.write_str("server lifecycle is unavailable"),
			Self::Terminated(receipt) => {
				write!(formatter, "server lifecycle terminated: {:?}", receipt.primary)
			},
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
		Err(error) => {
			let outcome = if matches!(&error, CommandError::AcceptanceUnknown) {
				CommandOutcome::AcceptanceUnknown
			} else {
				CommandOutcome::Rejected
			};

			(
				CommandResultEnvelope {
					version,
					server_id: server_id.clone(),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					outcome,
					entity_revision: None,
					payload: None,
					error: Some(error),
				},
				None,
			)
		},
	}
}

fn accept_for_session(
	state: &mut PublicationState,
	connection_id: u64,
	message: ServerMessage,
) -> SessionAcceptance {
	// This is the sole per-session capacity decision; dropping the actor-owned sender seals FIFO.
	let Some(subscriber) = state.subscribers.get_mut(&connection_id) else {
		return SessionAcceptance::Unavailable;
	};
	let ordinal = subscriber.next_ordinal;
	let decision = if let Some(next_ordinal) = ordinal.0.checked_add(1).map(SessionOrdinal) {
		match subscriber.sender.try_send(OutboundItem { ordinal, message }) {
			Ok(()) => {
				subscriber.next_ordinal = next_ordinal;
				subscriber.accepted_through = SessionOrdinalProgress::Through(ordinal);

				Ok(ordinal)
			},
			Err(mpsc::error::TrySendError::Full(_)) => Err(SessionSealReason::OutboundFull),
			Err(mpsc::error::TrySendError::Closed(_)) => Err(SessionSealReason::OutboundClosed),
		}
	} else {
		Err(SessionSealReason::OrdinalExhausted)
	};
	match decision {
		Ok(ordinal) => SessionAcceptance::Accepted(ordinal),
		Err(reason) => {
			state.seal_session(connection_id, reason);

			SessionAcceptance::Sealed(reason)
		},
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

fn session_ingress_failure_reason(stop: &watch::Receiver<bool>) -> SessionSealReason {
	if *stop.borrow() {
		SessionSealReason::ServerShutdown
	} else {
		SessionSealReason::ActorUnavailable
	}
}

fn record_lowest(current: &mut Option<OwnedTaskIdentity>, candidate: OwnedTaskIdentity) {
	if current.is_none_or(|identity| candidate < identity) {
		*current = Some(candidate);
	}
}

fn select_refusal(
	current: Option<LocalTransportRefusal>,
	candidate: LocalTransportRefusal,
) -> Option<LocalTransportRefusal> {
	match current {
		Some(current) if refusal_rank(current) <= refusal_rank(candidate) => Some(current),
		_ => Some(candidate),
	}
}

const fn refusal_rank(refusal: LocalTransportRefusal) -> u8 {
	match refusal {
		LocalTransportRefusal::Disabled => 0,
		LocalTransportRefusal::InvalidPolicy => 1,
		LocalTransportRefusal::ConfigurationUnavailable => 2,
		LocalTransportRefusal::UnsupportedPlatform => 3,
		LocalTransportRefusal::EffectiveUidMismatch => 4,
		LocalTransportRefusal::UnsafeDirectory => 5,
		LocalTransportRefusal::UnsafeEndpoint => 6,
		LocalTransportRefusal::EndpointUnavailable => 7,
		LocalTransportRefusal::EndpointInUse => 8,
		LocalTransportRefusal::EndpointReplaced => 9,
		LocalTransportRefusal::PeerCredentialsUnavailable => 10,
		LocalTransportRefusal::PeerUidMismatch => 11,
	}
}

async fn stopped(receiver: &mut watch::Receiver<bool>) {
	loop {
		if *receiver.borrow_and_update() {
			return;
		}
		if receiver.changed().await.is_err() {
			return;
		}
	}
}
