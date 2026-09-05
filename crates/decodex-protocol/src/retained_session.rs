//! Retained, resumable WebSocket client session without retry or persistence policy.

use std::{
	fmt::{Debug, Display, Formatter},
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU64, Ordering},
	},
	time::Duration,
};

use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use tokio::{sync::Notify, time};
use tokio_tungstenite::{
	WebSocketStream,
	tungstenite::{Message, protocol::WebSocketConfig},
};

use crate::{
	CURRENT_VERSION, ClientHello, ClientMessage, CommandEnvelope, CommandReceipt,
	CommandResultEnvelope, Cursor, EventEnvelope, LocalTransportAuthority, LocalTransportRefusal,
	LocalTransportStream, ProtocolVersion, QueryEnvelope, QueryResultEnvelope, ReconnectMode,
	Refusal, RefusalEnvelope, ResumeCursor, ServerId, ServerInstanceId, ServerMessage,
	ServerWelcome, SnapshotEnvelope,
};

type Socket = WebSocketStream<LocalTransportStream>;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 256 * 1_024;
const MAX_SNAPSHOT_ITEMS: usize = 1_024;
// This URI is WebSocket handshake metadata only. The client passes an already
// admitted Unix stream, so this value cannot resolve or dial a TCP endpoint.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Explicit local transport and authority inputs for one retained session.
///
/// This value contains no filesystem, profile-selection, retry, or cache policy.
#[derive(Clone, Eq, PartialEq)]
pub struct RetainedSessionConfig {
	local_transport: LocalTransportAuthority,
	expected_server_id: ServerId,
	operation_timeout: Duration,
}
impl RetainedSessionConfig {
	/// Bind one already validated local authority to a stable server identity pin.
	pub const fn new(
		local_transport: LocalTransportAuthority,
		expected_server_id: ServerId,
	) -> Self {
		Self { local_transport, expected_server_id, operation_timeout: OPERATION_TIMEOUT }
	}

	/// Stable server identity selected by the same typed profile as the endpoint.
	pub const fn expected_server_id(&self) -> &ServerId {
		&self.expected_server_id
	}
}
impl Debug for RetainedSessionConfig {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("RetainedSessionConfig")
			.field("local_transport", &"<redacted>")
			.field("identity_pinned", &true)
			.field("operation_timeout", &self.operation_timeout)
			.finish()
	}
}

/// Exact resumable position after complete application by the consumer.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SessionCheckpoint {
	server_id: ServerId,
	instance_id: ServerInstanceId,
	cursor: Cursor,
}
impl SessionCheckpoint {
	/// Restore one exact server, publication-instance, and cursor tuple.
	pub const fn new(server_id: ServerId, instance_id: ServerInstanceId, cursor: Cursor) -> Self {
		Self { server_id, instance_id, cursor }
	}

	/// Stable server identity that issued this checkpoint.
	pub const fn server_id(&self) -> &ServerId {
		&self.server_id
	}

	/// Publication instance that issued this checkpoint.
	pub const fn instance_id(&self) -> &ServerInstanceId {
		&self.instance_id
	}

	/// Last snapshot or event cursor completely applied by the consumer.
	pub const fn cursor(&self) -> Cursor {
		self.cursor
	}

	fn as_resume_cursor(&self) -> ResumeCursor {
		ResumeCursor {
			server_id: self.server_id.clone(),
			instance_id: Some(self.instance_id.clone()),
			cursor: self.cursor,
		}
	}
}

/// Cooperative cancellation shared with one retained session.
///
/// No task is spawned. An in-flight operation observes cancellation, drops the owned
/// socket, and returns a closed failure.
#[derive(Clone, Debug, Default)]
pub struct SessionCancellation {
	inner: Arc<(AtomicBool, Notify)>,
}
impl SessionCancellation {
	/// Create an uncancelled signal.
	pub fn new() -> Self {
		Self::default()
	}

	/// Cancel current and future operations.
	pub fn cancel(&self) {
		if !self.inner.0.swap(true, Ordering::AcqRel) {
			self.inner.1.notify_waiters();
		}
	}

	/// Whether cancellation has already been requested.
	pub fn is_cancelled(&self) -> bool {
		self.inner.0.load(Ordering::Acquire)
	}

	async fn cancelled(&self) {
		loop {
			let notified = self.inner.1.notified();

			if self.is_cancelled() {
				return;
			}

			notified.await;
		}
	}
}

/// Opaque proof that one checkpoint-bearing delivery was completely applied.
#[derive(Debug, Eq, PartialEq)]
pub struct ApplicationConfirmation {
	session_id: u64,
	checkpoint: SessionCheckpoint,
}

/// A single retained WebSocket connection with no retry, filesystem, or cache owner.
pub struct RetainedSession {
	socket: Option<Socket>,
	expected_server_id: ServerId,
	instance_id: ServerInstanceId,
	welcome_high_water: Cursor,
	replay_high_water: Option<Cursor>,
	next_cursor: Option<Cursor>,
	checkpoint: Option<SessionCheckpoint>,
	pending_confirmation: Option<ApplicationConfirmation>,
	initial_snapshot: Option<SnapshotEnvelope>,
	cancellation: SessionCancellation,
	operation_timeout: Duration,
	session_id: u64,
}
impl RetainedSession {
	/// Connect, negotiate the exact current version, and verify session identity before
	/// any application delivery can be observed.
	pub async fn connect(
		config: RetainedSessionConfig,
		checkpoint: Option<SessionCheckpoint>,
		cancellation: SessionCancellation,
	) -> Result<Self, RetainedSessionFailure> {
		if cancellation.is_cancelled() {
			return Err(RetainedSessionFailure::Cancelled);
		}
		if checkpoint
			.as_ref()
			.is_some_and(|checkpoint| checkpoint.server_id != config.expected_server_id)
		{
			return Err(RetainedSessionFailure::CheckpointIdentityMismatch);
		}

		let websocket_config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(16 * 1_024)
			.max_write_buffer_size(MAX_MESSAGE_BYTES)
			.max_message_size(Some(MAX_MESSAGE_BYTES))
			.max_frame_size(Some(MAX_MESSAGE_BYTES));
		let stream =
			bounded(&cancellation, config.operation_timeout, config.local_transport.connect())
				.await?
				.map_err(map_local_transport_failure)?;
		let connect = tokio_tungstenite::client_async_with_config(
			LOCAL_WEBSOCKET_URI,
			stream,
			Some(websocket_config),
		);
		let (mut socket, _) = bounded(&cancellation, config.operation_timeout, connect)
			.await?
			.map_err(map_connect_error)?;
		let hello = ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: Some(config.expected_server_id.clone()),
			resume: checkpoint.as_ref().map(SessionCheckpoint::as_resume_cursor),
		});

		send_message(&mut socket, hello, &cancellation, config.operation_timeout).await?;

		let welcome =
			match receive_message(&mut socket, &cancellation, config.operation_timeout).await? {
				ServerMessage::Welcome(welcome) => welcome,
				ServerMessage::Refusal(refusal) => {
					return Err(refusal_failure(&config.expected_server_id, refusal));
				},
				_ => return Err(RetainedSessionFailure::Malformed),
			};

		verify_welcome(&config.expected_server_id, &welcome)?;

		let instance_id = welcome
			.instance_id
			.clone()
			.ok_or(RetainedSessionFailure::PublicationIdentityUnavailable)?;
		let (replay_high_water, checkpoint, expect_snapshot) =
			match (&checkpoint, welcome.reconnect) {
				(None, ReconnectMode::Snapshot) => (None, None, true),
				(Some(checkpoint), ReconnectMode::Resume)
					if checkpoint.instance_id == instance_id
						&& checkpoint.cursor <= welcome.cursor =>
					(Some(welcome.cursor), Some(checkpoint.clone()), false),
				(Some(_), ReconnectMode::Resume) => {
					return Err(RetainedSessionFailure::CheckpointIdentityMismatch);
				},
				(Some(_), ReconnectMode::SnapshotFallback) => (None, None, true),
				_ => return Err(RetainedSessionFailure::Malformed),
			};
		let initial_snapshot =
			if expect_snapshot {
				let snapshot =
					match receive_message(&mut socket, &cancellation, config.operation_timeout)
						.await?
					{
						ServerMessage::Snapshot(snapshot) => snapshot,
						ServerMessage::Refusal(refusal) => {
							return Err(refusal_failure(&config.expected_server_id, refusal));
						},
						_ => return Err(RetainedSessionFailure::Malformed),
					};

				verify_snapshot(&config.expected_server_id, welcome.cursor, &snapshot)?;

				Some(snapshot)
			} else {
				None
			};
		let next_cursor = checkpoint.as_ref().and_then(|checkpoint| next(checkpoint.cursor));

		Ok(Self {
			socket: Some(socket),
			expected_server_id: config.expected_server_id,
			instance_id,
			welcome_high_water: welcome.cursor,
			replay_high_water,
			next_cursor,
			checkpoint,
			pending_confirmation: None,
			initial_snapshot,
			cancellation,
			operation_timeout: config.operation_timeout,
			session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
		})
	}

	/// Informational high-water observed during the verified welcome handshake.
	///
	/// This value never advances [`Self::checkpoint`].
	pub const fn welcome_high_water(&self) -> Cursor {
		self.welcome_high_water
	}

	/// Last checkpoint explicitly confirmed as completely applied.
	pub const fn checkpoint(&self) -> Option<&SessionCheckpoint> {
		self.checkpoint.as_ref()
	}

	#[cfg(test)]
	fn set_operation_timeout(&mut self, operation_timeout: Duration) {
		self.operation_timeout = operation_timeout;
	}

	/// Send one exact-version command with a bounded write.
	pub async fn send_command(
		&mut self,
		command: CommandEnvelope,
	) -> Result<(), RetainedSessionFailure> {
		if command.version != CURRENT_VERSION {
			return Err(version_failure(command.version));
		}

		self.send(ClientMessage::Command(command)).await
	}

	/// Send one exact-version live query with a bounded write.
	pub async fn send_query(&mut self, query: QueryEnvelope) -> Result<(), RetainedSessionFailure> {
		if query.version != CURRENT_VERSION {
			return Err(version_failure(query.version));
		}

		self.send(ClientMessage::Query(query)).await
	}

	/// Receive the next verified delivery without imposing an idle-session deadline.
	///
	/// The deadline begins only when this operation is called. A snapshot or event blocks
	/// the next receive and every send until its application confirmation is returned.
	pub async fn next(&mut self) -> Result<SessionDelivery, RetainedSessionFailure> {
		if self.pending_confirmation.is_some() {
			return Err(RetainedSessionFailure::ApplicationConfirmationRequired);
		}

		if let Some(snapshot) = self.initial_snapshot.take() {
			return Ok(self.checkpoint_delivery(snapshot));
		}

		let message = self.receive().await?;

		match message {
			ServerMessage::Event(event) => self.event_delivery(event),
			ServerMessage::CommandReceipt(receipt) => {
				self.verify_envelope(receipt.version, &receipt.server_id)?;

				Ok(SessionDelivery::CommandReceipt(receipt))
			},
			ServerMessage::CommandResult(result) => {
				self.verify_envelope(result.version, &result.server_id)?;

				Ok(SessionDelivery::CommandResult(result))
			},
			ServerMessage::QueryResult(result) => {
				self.verify_envelope(result.version, &result.server_id)?;

				Ok(SessionDelivery::QueryResult(result))
			},
			ServerMessage::Refusal(refusal) =>
				Err(refusal_failure(&self.expected_server_id, refusal)),
			ServerMessage::Welcome(_)
			| ServerMessage::Snapshot(_)
			| ServerMessage::AccountLogin(_) => Err(RetainedSessionFailure::Malformed),
		}
		.inspect_err(|_| {
			self.terminate();
		})
	}

	/// Confirm complete application and atomically advance the resumable checkpoint.
	pub fn confirm_applied(
		&mut self,
		confirmation: ApplicationConfirmation,
	) -> Result<&SessionCheckpoint, RetainedSessionFailure> {
		if self.pending_confirmation.as_ref() != Some(&confirmation) {
			return Err(RetainedSessionFailure::ApplicationConfirmationMismatch);
		}

		let checkpoint = confirmation.checkpoint;

		self.pending_confirmation = None;
		self.next_cursor = next(checkpoint.cursor);
		self.checkpoint = Some(checkpoint);

		Ok(self.checkpoint.as_ref().expect("checkpoint was just installed"))
	}

	/// Perform a bounded close handshake and consume the owned socket.
	pub async fn close(mut self) -> Result<(), RetainedSessionFailure> {
		let Some(mut socket) = self.socket.take() else {
			return Ok(());
		};
		let cancellation = self.cancellation.clone();
		let close = async {
			socket.send(Message::Close(None)).await.map_err(map_transport_error)?;

			while let Some(message) = socket.next().await {
				match message.map_err(map_transport_error)? {
					Message::Close(_) => return Ok(()),
					Message::Ping(payload) =>
						socket.send(Message::Pong(payload)).await.map_err(map_transport_error)?,
					Message::Pong(_) => {},
					Message::Text(_) | Message::Binary(_) | Message::Frame(_) => {
						return Err(RetainedSessionFailure::Malformed);
					},
				}
			}

			Ok(())
		};

		bounded(&cancellation, self.operation_timeout, close).await?
	}

	async fn send(&mut self, message: ClientMessage) -> Result<(), RetainedSessionFailure> {
		if self.pending_confirmation.is_some() {
			return Err(RetainedSessionFailure::ApplicationConfirmationRequired);
		}

		let Some(socket) = self.socket.as_mut() else {
			return Err(RetainedSessionFailure::Closed);
		};
		let result =
			send_message(socket, message, &self.cancellation, self.operation_timeout).await;

		if result.is_err() {
			self.terminate();
		}

		result
	}

	async fn receive(&mut self) -> Result<ServerMessage, RetainedSessionFailure> {
		receive_owned(&mut self.socket, &self.cancellation, self.operation_timeout).await
	}

	fn checkpoint_delivery(&mut self, snapshot: SnapshotEnvelope) -> SessionDelivery {
		let confirmation = self.confirmation(snapshot.cursor);

		SessionDelivery::Snapshot { snapshot, confirmation }
	}

	fn event_delivery(
		&mut self,
		event: EventEnvelope,
	) -> Result<SessionDelivery, RetainedSessionFailure> {
		self.verify_identity(event.version, &event.server_id)?;

		if self.next_cursor != Some(event.cursor) {
			return Err(RetainedSessionFailure::PublicationOrder);
		}
		if self.replay_high_water.is_some_and(|high_water| event.cursor > high_water) {
			self.replay_high_water = None;
		}

		let confirmation = self.confirmation(event.cursor);

		Ok(SessionDelivery::Event { event, confirmation })
	}

	fn confirmation(&mut self, cursor: Cursor) -> ApplicationConfirmation {
		let confirmation = ApplicationConfirmation {
			session_id: self.session_id,
			checkpoint: SessionCheckpoint::new(
				self.expected_server_id.clone(),
				self.instance_id.clone(),
				cursor,
			),
		};

		self.pending_confirmation = Some(ApplicationConfirmation {
			session_id: confirmation.session_id,
			checkpoint: confirmation.checkpoint.clone(),
		});

		confirmation
	}

	fn verify_envelope(
		&self,
		version: ProtocolVersion,
		server_id: &ServerId,
	) -> Result<(), RetainedSessionFailure> {
		self.verify_identity(version, server_id)?;

		if self
			.replay_high_water
			.is_some_and(|high_water| self.next_cursor.is_some_and(|cursor| cursor <= high_water))
		{
			return Err(RetainedSessionFailure::PublicationOrder);
		}

		Ok(())
	}

	fn verify_identity(
		&self,
		version: ProtocolVersion,
		server_id: &ServerId,
	) -> Result<(), RetainedSessionFailure> {
		if version != CURRENT_VERSION {
			return Err(version_failure(version));
		}
		if server_id != &self.expected_server_id {
			return Err(RetainedSessionFailure::ServerIdentityMismatch);
		}

		Ok(())
	}

	fn terminate(&mut self) {
		self.socket.take();
	}
}

impl Debug for RetainedSession {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("RetainedSession")
			.field("connected", &self.socket.is_some())
			.field("identity_verified", &true)
			.field("checkpoint", &self.checkpoint)
			.field("confirmation_pending", &self.pending_confirmation.is_some())
			.finish()
	}
}

/// One verified application delivery in exact WebSocket order.
#[derive(Debug, Eq, PartialEq)]
pub enum SessionDelivery {
	/// Current state requiring complete application confirmation.
	Snapshot {
		/// Verified snapshot envelope.
		snapshot: SnapshotEnvelope,
		/// Opaque confirmation consumed after complete application.
		confirmation: ApplicationConfirmation,
	},
	/// Resumable publication requiring complete application confirmation.
	Event {
		/// Verified ordered event envelope.
		event: EventEnvelope,
		/// Opaque confirmation consumed after complete application.
		confirmation: ApplicationConfirmation,
	},
	/// Command attempt receipt in wire order.
	CommandReceipt(CommandReceipt),
	/// Deterministic command result in wire order.
	CommandResult(CommandResultEnvelope),
	/// Live query result in wire order.
	QueryResult(QueryResultEnvelope),
}

/// Closed retained-session failures. External endpoint, parser, socket, HTTP, and
/// server-provided text cannot inhabit this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetainedSessionFailure {
	/// Durable local policy disables the endpoint.
	LocalTransportDisabled,
	/// Remote transport remains outside this implementation.
	RemoteTransportDisabled,
	/// The platform has no accepted local kernel peer-identity implementation.
	LocalTransportUnsupported,
	/// The local directory, lock, socket, or captured identity is unsafe.
	UnsafeLocalEndpoint,
	/// The kernel did not provide an unambiguous local peer identity.
	LocalPeerIdentityUnavailable,
	/// The process or connected peer does not match the configured service UID.
	LocalPeerUidMismatch,
	/// A bounded connect, handshake, send, or close deadline elapsed.
	OperationTimeout,
	/// Cooperative cancellation terminated the owned socket operation.
	Cancelled,
	/// The session was already closed or terminated.
	Closed,
	/// The peer disconnected without a completed close operation.
	Disconnected,
	/// The server used a different protocol generation.
	ServiceVersionMismatch,
	/// The stable server identity did not match the explicit pin.
	ServerIdentityMismatch,
	/// Current protocol negotiation omitted publication-instance identity.
	PublicationIdentityUnavailable,
	/// A checkpoint was incompatible with the exact server or publication instance.
	CheckpointIdentityMismatch,
	/// A server response was not the expected bounded typed envelope.
	Malformed,
	/// The server refused the protocol operation.
	ProtocolViolation,
	/// A bounded transport or server queue limit was reached.
	Backpressure,
	/// Snapshot or event cursors were not delivered in strict order.
	PublicationOrder,
	/// Another delivery is blocked on complete application confirmation.
	ApplicationConfirmationRequired,
	/// The confirmation did not belong to the pending delivery and session.
	ApplicationConfirmationMismatch,
}
impl Display for RetainedSessionFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::LocalTransportDisabled => "retained session local transport is disabled",
			Self::RemoteTransportDisabled => "retained session remote transport is disabled",
			Self::LocalTransportUnsupported => "retained session local transport is unsupported",
			Self::UnsafeLocalEndpoint => "retained session local endpoint is unsafe",
			Self::LocalPeerIdentityUnavailable =>
				"retained session local peer identity is unavailable",
			Self::LocalPeerUidMismatch => "retained session local peer UID does not match",
			Self::OperationTimeout => "retained session operation timed out",
			Self::Cancelled => "retained session was cancelled",
			Self::Closed => "retained session is closed",
			Self::Disconnected => "retained session disconnected",
			Self::ServiceVersionMismatch => "service_version_mismatch",
			Self::ServerIdentityMismatch => "retained session server identity does not match",
			Self::PublicationIdentityUnavailable =>
				"retained session publication identity is unavailable",
			Self::CheckpointIdentityMismatch =>
				"retained session checkpoint identity does not match",
			Self::Malformed => "retained session protocol response is malformed",
			Self::ProtocolViolation => "retained session protocol operation was refused",
			Self::Backpressure => "retained session backpressure limit was reached",
			Self::PublicationOrder => "retained session publication order is invalid",
			Self::ApplicationConfirmationRequired =>
				"retained session delivery requires application confirmation",
			Self::ApplicationConfirmationMismatch =>
				"retained session application confirmation does not match",
		})
	}
}

impl std::error::Error for RetainedSessionFailure {}

fn verify_welcome(
	expected_server_id: &ServerId,
	welcome: &ServerWelcome,
) -> Result<(), RetainedSessionFailure> {
	if welcome.version != CURRENT_VERSION {
		return Err(version_failure(welcome.version));
	}
	if &welcome.server_id != expected_server_id {
		return Err(RetainedSessionFailure::ServerIdentityMismatch);
	}

	Ok(())
}

fn verify_snapshot(
	expected_server_id: &ServerId,
	welcome_cursor: Cursor,
	snapshot: &SnapshotEnvelope,
) -> Result<(), RetainedSessionFailure> {
	if snapshot.version != CURRENT_VERSION {
		return Err(version_failure(snapshot.version));
	}
	if &snapshot.server_id != expected_server_id {
		return Err(RetainedSessionFailure::ServerIdentityMismatch);
	}
	if snapshot.cursor != welcome_cursor || snapshot.items.len() > MAX_SNAPSHOT_ITEMS {
		return Err(RetainedSessionFailure::Malformed);
	}

	Ok(())
}

fn refusal_failure(
	expected_server_id: &ServerId,
	refusal: RefusalEnvelope,
) -> RetainedSessionFailure {
	if &refusal.server_id != expected_server_id {
		return RetainedSessionFailure::ServerIdentityMismatch;
	}

	match refusal.refusal {
		Refusal::ServiceVersionMismatch { .. } => RetainedSessionFailure::ServiceVersionMismatch,
		Refusal::ServerIdentityMismatch { .. } => RetainedSessionFailure::ServerIdentityMismatch,
		Refusal::ProtocolViolation { .. } => RetainedSessionFailure::ProtocolViolation,
		Refusal::Backpressure { .. } => RetainedSessionFailure::Backpressure,
	}
}

fn version_failure(_version: ProtocolVersion) -> RetainedSessionFailure {
	RetainedSessionFailure::ServiceVersionMismatch
}

fn map_connect_error(error: tokio_tungstenite::tungstenite::Error) -> RetainedSessionFailure {
	match error {
		tokio_tungstenite::tungstenite::Error::Capacity(_) => RetainedSessionFailure::Backpressure,
		tokio_tungstenite::tungstenite::Error::Protocol(_)
		| tokio_tungstenite::tungstenite::Error::Utf8(_)
		| tokio_tungstenite::tungstenite::Error::Http(_)
		| tokio_tungstenite::tungstenite::Error::HttpFormat(_) => RetainedSessionFailure::Malformed,
		_ => RetainedSessionFailure::Disconnected,
	}
}

fn map_local_transport_failure(failure: LocalTransportRefusal) -> RetainedSessionFailure {
	match failure {
		LocalTransportRefusal::Disabled => RetainedSessionFailure::LocalTransportDisabled,
		LocalTransportRefusal::InvalidPolicy | LocalTransportRefusal::ConfigurationUnavailable =>
			RetainedSessionFailure::UnsafeLocalEndpoint,
		LocalTransportRefusal::UnsupportedPlatform =>
			RetainedSessionFailure::LocalTransportUnsupported,
		LocalTransportRefusal::EffectiveUidMismatch | LocalTransportRefusal::PeerUidMismatch =>
			RetainedSessionFailure::LocalPeerUidMismatch,
		LocalTransportRefusal::UnsafeDirectory
		| LocalTransportRefusal::UnsafeEndpoint
		| LocalTransportRefusal::EndpointReplaced => RetainedSessionFailure::UnsafeLocalEndpoint,
		LocalTransportRefusal::PeerCredentialsUnavailable =>
			RetainedSessionFailure::LocalPeerIdentityUnavailable,
		LocalTransportRefusal::EndpointUnavailable | LocalTransportRefusal::EndpointInUse =>
			RetainedSessionFailure::Disconnected,
	}
}

fn map_transport_error(error: tokio_tungstenite::tungstenite::Error) -> RetainedSessionFailure {
	match error {
		tokio_tungstenite::tungstenite::Error::Capacity(_) => RetainedSessionFailure::Backpressure,
		tokio_tungstenite::tungstenite::Error::Protocol(_)
		| tokio_tungstenite::tungstenite::Error::Utf8(_) => RetainedSessionFailure::Malformed,
		_ => RetainedSessionFailure::Disconnected,
	}
}

fn next(cursor: Cursor) -> Option<Cursor> {
	cursor.0.checked_add(1).map(Cursor)
}

async fn bounded<F, T>(
	cancellation: &SessionCancellation,
	timeout: Duration,
	operation: F,
) -> Result<T, RetainedSessionFailure>
where
	F: Future<Output = T>,
{
	let selected = async {
		tokio::select! {
			biased;

			() = cancellation.cancelled() => Err(RetainedSessionFailure::Cancelled),
			result = operation => Ok(result),
		}
	};

	time::timeout(timeout, selected).await.map_err(|_| RetainedSessionFailure::OperationTimeout)?
}

async fn send_message(
	socket: &mut Socket,
	message: ClientMessage,
	cancellation: &SessionCancellation,
	timeout: Duration,
) -> Result<(), RetainedSessionFailure> {
	let encoded = serde_json::to_string(&message)
		.expect("typed bounded client message serialization cannot fail");

	if encoded.len() > MAX_MESSAGE_BYTES {
		return Err(RetainedSessionFailure::Backpressure);
	}

	bounded(cancellation, timeout, socket.send(Message::Text(encoded.into())))
		.await?
		.map_err(map_transport_error)
}

async fn receive_message(
	socket: &mut Socket,
	cancellation: &SessionCancellation,
	timeout: Duration,
) -> Result<ServerMessage, RetainedSessionFailure> {
	bounded(cancellation, timeout, receive_frame(socket, cancellation, timeout)).await?
}

async fn receive_owned<S>(
	socket: &mut Option<S>,
	cancellation: &SessionCancellation,
	timeout: Duration,
) -> Result<ServerMessage, RetainedSessionFailure>
where
	S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
		+ Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
		+ Unpin,
{
	let result = match socket.as_mut() {
		Some(socket) => receive_frame(socket, cancellation, timeout).await,
		None => return Err(RetainedSessionFailure::Closed),
	};

	if result.is_err() {
		socket.take();
	}

	result
}

async fn receive_frame<S>(
	socket: &mut S,
	cancellation: &SessionCancellation,
	timeout: Duration,
) -> Result<ServerMessage, RetainedSessionFailure>
where
	S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
		+ Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
		+ Unpin,
{
	let receive = async {
		loop {
			let message = cancellable(cancellation, socket.next())
				.await?
				.ok_or(RetainedSessionFailure::Disconnected)?
				.map_err(map_transport_error)?;

			match message {
				Message::Text(text) => {
					return serde_json::from_str(&text)
						.map_err(|_| RetainedSessionFailure::Malformed);
				},
				Message::Ping(payload) =>
					bounded(cancellation, timeout, socket.send(Message::Pong(payload)))
						.await?
						.map_err(map_transport_error)?,
				Message::Pong(_) => {},
				Message::Close(_) => return Err(RetainedSessionFailure::Disconnected),
				Message::Binary(_) | Message::Frame(_) => {
					return Err(RetainedSessionFailure::Malformed);
				},
			}
		}
	};

	receive.await
}

async fn cancellable<F, T>(
	cancellation: &SessionCancellation,
	operation: F,
) -> Result<T, RetainedSessionFailure>
where
	F: Future<Output = T>,
{
	tokio::select! {
		biased;

		() = cancellation.cancelled() => Err(RetainedSessionFailure::Cancelled),
		result = operation => Ok(result),
	}
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
	use std::{
		future::Future,
		pin::Pin,
		sync::{
			Arc,
			atomic::{AtomicBool, Ordering},
		},
		task::{Context, Poll},
		time::Duration,
	};

	use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
	use tempfile::TempDir;
	use tokio::{
		sync::oneshot,
		task::{self, JoinHandle},
	};
	use tokio_tungstenite::{self, WebSocketStream, tungstenite::Message};

	use crate::{
		CURRENT_VERSION, Channel, ClientCommandId, ClientHello, ClientMessage, CommandEnvelope,
		CommandOutcome, CommandPayload, CommandReceipt, CommandResultEnvelope, CorrelationId,
		Cursor, EntityId, EntityRevision, EventEnvelope, EventPayload, IdempotencyKey,
		LocalTransportAuthority, LocalTransportStream, ProtocolVersion, ReceiptDisposition,
		ReconnectMode, Refusal, RefusalEnvelope, ServerId, ServerInstanceId, ServerMessage,
		ServerWelcome, SnapshotEnvelope, SnapshotItem, WireText,
		retained_session::{
			ApplicationConfirmation, MAX_MESSAGE_BYTES, RetainedSession, RetainedSessionConfig,
			RetainedSessionFailure, SessionCancellation, SessionCheckpoint, SessionDelivery,
		},
	};
	use decodex_core::{DecodexRoot, LocalTrustPolicy};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
	const OTHER_SERVER_ID: &str = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
	const INSTANCE_ID: &str = "118f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
	const NEW_INSTANCE_ID: &str = "218f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	struct StalledPongSocket {
		inbound: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
		pong_write_polled: Arc<AtomicBool>,
		dropped: Arc<AtomicBool>,
	}

	impl Stream for StalledPongSocket {
		type Item = Result<Message, tokio_tungstenite::tungstenite::Error>;

		fn poll_next(
			mut self: Pin<&mut Self>,
			_context: &mut Context<'_>,
		) -> Poll<Option<Self::Item>> {
			Poll::Ready(self.inbound.take())
		}
	}

	impl Sink<Message> for StalledPongSocket {
		type Error = tokio_tungstenite::tungstenite::Error;

		fn poll_ready(
			self: Pin<&mut Self>,
			_context: &mut Context<'_>,
		) -> Poll<Result<(), Self::Error>> {
			self.pong_write_polled.store(true, Ordering::Release);

			Poll::Pending
		}

		fn start_send(self: Pin<&mut Self>, _item: Message) -> Result<(), Self::Error> {
			unreachable!("the stalled sink never becomes writable")
		}

		fn poll_flush(
			self: Pin<&mut Self>,
			_context: &mut Context<'_>,
		) -> Poll<Result<(), Self::Error>> {
			Poll::Pending
		}

		fn poll_close(
			self: Pin<&mut Self>,
			_context: &mut Context<'_>,
		) -> Poll<Result<(), Self::Error>> {
			Poll::Ready(Ok(()))
		}
	}

	impl Drop for StalledPongSocket {
		fn drop(&mut self) {
			self.dropped.store(true, Ordering::Release);
		}
	}

	fn server_id(value: &str) -> ServerId {
		ServerId::new(value).expect("test operation must succeed")
	}

	fn instance_id(value: &str) -> ServerInstanceId {
		ServerInstanceId::new(value).expect("test operation must succeed")
	}

	fn checkpoint(instance: &str, cursor: u64) -> SessionCheckpoint {
		SessionCheckpoint::new(server_id(SERVER_ID), instance_id(instance), Cursor(cursor))
	}

	fn welcome(
		server: &str,
		instance: Option<&str>,
		cursor: u64,
		reconnect: ReconnectMode,
	) -> ServerMessage {
		ServerMessage::Welcome(ServerWelcome {
			version: CURRENT_VERSION,
			server_id: server_id(server),
			instance_id: instance.map(instance_id),
			cursor: Cursor(cursor),
			reconnect,
		})
	}

	fn snapshot(server: &str, cursor: u64) -> ServerMessage {
		ServerMessage::Snapshot(SnapshotEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id(server),
			cursor: Cursor(cursor),
			items: vec![SnapshotItem::SystemState {
				entity_id: EntityId::new("system").expect("test operation must succeed"),
				revision: EntityRevision(cursor),
				status: WireText::new("ready").expect("test operation must succeed"),
			}],
		})
	}

	fn event(cursor: u64) -> ServerMessage {
		ServerMessage::Event(EventEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id(SERVER_ID),
			cursor: Cursor(cursor),
			channel: Channel::SystemHealth,
			entity_id: EntityId::new("system").expect("test operation must succeed"),
			entity_revision: EntityRevision(cursor),
			correlation_id: CorrelationId::new(format!("correlation-{cursor}"))
				.expect("test operation must succeed"),
			causation_id: None,
			payload: EventPayload::SystemObservationRefreshed {
				status: WireText::new(format!("event-{cursor}"))
					.expect("test operation must succeed"),
			},
		})
	}

	fn receipt() -> ServerMessage {
		ServerMessage::CommandReceipt(CommandReceipt {
			version: CURRENT_VERSION,
			server_id: server_id(SERVER_ID),
			client_command_id: ClientCommandId::new("command-1")
				.expect("test operation must succeed"),
			idempotency_key: IdempotencyKey::new("key-1").expect("test operation must succeed"),
			disposition: ReceiptDisposition::Executed,
			original_client_command_id: ClientCommandId::new("command-1")
				.expect("test operation must succeed"),
		})
	}

	fn command() -> CommandEnvelope {
		CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("command-1")
				.expect("test operation must succeed"),
			idempotency_key: IdempotencyKey::new("key-1").expect("test operation must succeed"),
			expected_revision: None,
			correlation_id: CorrelationId::new("correlation-command")
				.expect("test operation must succeed"),
			causation_id: None,
			payload: CommandPayload::RefreshSystemObservation {
				entity_id: EntityId::new("system").expect("test operation must succeed"),
			},
		}
	}

	fn result() -> ServerMessage {
		ServerMessage::CommandResult(CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id(SERVER_ID),
			client_command_id: ClientCommandId::new("command-1")
				.expect("test operation must succeed"),
			idempotency_key: IdempotencyKey::new("key-1").expect("test operation must succeed"),
			outcome: CommandOutcome::Rejected,
			entity_revision: None,
			payload: None,
			error: None,
		})
	}

	fn local_transport() -> (TempDir, LocalTransportAuthority) {
		let temp = TempDir::new().expect("test operation must succeed");
		let root = DecodexRoot::new(
			temp.path().canonicalize().expect("test operation must succeed").join(".decodex"),
		)
		.expect("test operation must succeed");
		let paths = root.paths();

		paths.ensure_layout().expect("test operation must succeed");

		// SAFETY: `geteuid` has no arguments or failure return.
		let service_owner_uid = unsafe { libc::geteuid() };
		let authority =
			LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(service_owner_uid))
				.expect("test operation must succeed");

		(temp, authority)
	}

	#[test]
	fn retained_session_config_uses_only_local_authority_and_the_identity_pin() {
		let (temp, authority) = local_transport();
		let config = RetainedSessionConfig::new(authority, server_id(SERVER_ID));
		let debug = format!("{config:?}");

		assert_eq!(config.expected_server_id(), &server_id(SERVER_ID));
		assert!(!debug.contains(temp.path().to_string_lossy().as_ref()));
	}

	async fn send(socket: &mut WebSocketStream<LocalTransportStream>, message: ServerMessage) {
		let text = serde_json::to_string(&message).expect("test operation must succeed");

		socket.send(Message::Text(text.into())).await.expect("test operation must succeed");
	}

	async fn hello(socket: &mut WebSocketStream<LocalTransportStream>) -> ClientHello {
		let message = socket
			.next()
			.await
			.expect("test operation must succeed")
			.expect("test operation must succeed");
		let Message::Text(text) = message else { panic!("expected text hello") };
		let ClientMessage::Hello(hello) =
			serde_json::from_str(&text).expect("test operation must succeed")
		else {
			panic!("expected typed hello")
		};

		assert_eq!(hello.version, CURRENT_VERSION);
		assert_eq!(hello.expected_server_id.as_ref(), Some(&server_id(SERVER_ID)));

		hello
	}

	async fn fixture<F, Fut>(handler: F) -> (RetainedSessionConfig, JoinHandle<()>, TempDir)
	where
		F: FnOnce(WebSocketStream<LocalTransportStream>) -> Fut + Send + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let task = tokio::spawn(async move {
			let stream = listener.accept().await.expect("test operation must succeed");
			let socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			handler(socket).await;
			listener.cleanup().expect("test operation must succeed");
		});
		let config = RetainedSessionConfig::new(authority, server_id(SERVER_ID));

		(config, task, temp)
	}

	#[tokio::test]
	async fn stalled_pong_write_times_out_and_releases_transport_ownership() {
		let pong_write_polled = Arc::new(AtomicBool::new(false));
		let dropped = Arc::new(AtomicBool::new(false));
		let cancellation = SessionCancellation::new();
		let mut socket = Some(StalledPongSocket {
			inbound: Some(Ok(Message::Ping(vec![1, 2, 3].into()))),
			pong_write_polled: Arc::clone(&pong_write_polled),
			dropped: Arc::clone(&dropped),
		});

		assert_eq!(
			super::receive_owned(&mut socket, &cancellation, Duration::ZERO).await.unwrap_err(),
			RetainedSessionFailure::OperationTimeout
		);
		assert!(pong_write_polled.load(Ordering::Acquire));
		assert!(socket.is_none());
		assert!(dropped.load(Ordering::Acquire));
		assert!(!cancellation.is_cancelled());
	}

	#[tokio::test]
	async fn snapshot_high_water_advances_only_after_exact_application_confirmation() {
		let (config, task, _temp) = fixture(|mut socket| async move {
			assert!(hello(&mut socket).await.resume.is_none());

			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 7, ReconnectMode::Snapshot))
				.await;
			send(&mut socket, snapshot(SERVER_ID, 7)).await;
		})
		.await;
		let mut session = RetainedSession::connect(config, None, SessionCancellation::new())
			.await
			.expect("test operation must succeed");

		assert_eq!(session.welcome_high_water(), Cursor(7));
		assert_eq!(session.checkpoint(), None);

		let SessionDelivery::Snapshot { snapshot, confirmation } =
			session.next().await.expect("test operation must succeed")
		else {
			panic!("expected snapshot")
		};

		assert_eq!(snapshot.cursor, Cursor(7));
		assert_eq!(session.checkpoint(), None);
		assert_eq!(
			session.next().await.unwrap_err(),
			RetainedSessionFailure::ApplicationConfirmationRequired
		);

		let wrong_confirmation = ApplicationConfirmation {
			session_id: confirmation.session_id + 1,
			checkpoint: confirmation.checkpoint.clone(),
		};

		assert_eq!(
			session.confirm_applied(wrong_confirmation).unwrap_err(),
			RetainedSessionFailure::ApplicationConfirmationMismatch
		);
		assert_eq!(session.checkpoint(), None);

		let applied = session.confirm_applied(confirmation).expect("test operation must succeed");

		assert_eq!(applied.server_id(), &server_id(SERVER_ID));
		assert_eq!(applied.instance_id(), &instance_id(INSTANCE_ID));
		assert_eq!(applied.cursor(), Cursor(7));

		drop(session);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn resume_delivers_events_receipt_and_result_strictly_in_wire_order() {
		let resume = checkpoint(INSTANCE_ID, 1);
		let expected_resume = resume.clone();
		let (config, task, _temp) = fixture(move |mut socket| async move {
			let actual = hello(&mut socket).await.resume.expect("test operation must succeed");

			assert_eq!(actual.server_id, expected_resume.server_id);
			assert_eq!(actual.instance_id.as_ref(), Some(&expected_resume.instance_id));
			assert_eq!(actual.cursor, Cursor(1));

			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 3, ReconnectMode::Resume))
				.await;
			send(&mut socket, event(2)).await;
			send(&mut socket, event(3)).await;

			let Message::Text(message) = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed")
			else {
				panic!("expected command")
			};

			assert_eq!(
				serde_json::from_str::<ClientMessage>(&message)
					.expect("test operation must succeed"),
				ClientMessage::Command(command())
			);

			send(&mut socket, receipt()).await;
			send(&mut socket, result()).await;
		})
		.await;
		let mut session =
			RetainedSession::connect(config, Some(resume), SessionCancellation::new())
				.await
				.expect("test operation must succeed");

		assert_eq!(session.checkpoint().expect("test operation must succeed").cursor(), Cursor(1));

		let SessionDelivery::Event { event, confirmation } =
			session.next().await.expect("test operation must succeed")
		else {
			panic!("expected first event")
		};

		assert_eq!(event.cursor, Cursor(2));

		session.confirm_applied(confirmation).expect("test operation must succeed");

		let SessionDelivery::Event { event, confirmation } =
			session.next().await.expect("test operation must succeed")
		else {
			panic!("expected second event")
		};

		assert_eq!(event.cursor, Cursor(3));
		assert_eq!(session.checkpoint().expect("test operation must succeed").cursor(), Cursor(2));
		assert_eq!(
			session.next().await.unwrap_err(),
			RetainedSessionFailure::ApplicationConfirmationRequired
		);

		session.confirm_applied(confirmation).expect("test operation must succeed");
		session.send_command(command()).await.expect("test operation must succeed");

		assert!(matches!(
			session.next().await.expect("test operation must succeed"),
			SessionDelivery::CommandReceipt(_)
		));
		assert!(matches!(
			session.next().await.expect("test operation must succeed"),
			SessionDelivery::CommandResult(_)
		));

		drop(session);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn cursor_gaps_terminate_delivery_before_out_of_order_application_data() {
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 2, ReconnectMode::Resume))
				.await;
			send(&mut socket, event(2)).await;
		})
		.await;
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		assert_eq!(session.next().await.unwrap_err(), RetainedSessionFailure::PublicationOrder);
		assert_eq!(session.next().await.unwrap_err(), RetainedSessionFailure::Closed);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn stale_publication_instance_falls_back_without_reusing_the_old_checkpoint() {
		let old_checkpoint = checkpoint(INSTANCE_ID, 5);
		let (config, task, _temp) = fixture(|mut socket| async move {
			assert_eq!(
				hello(&mut socket).await.resume.expect("test operation must succeed").instance_id,
				Some(instance_id(INSTANCE_ID))
			);

			send(
				&mut socket,
				welcome(SERVER_ID, Some(NEW_INSTANCE_ID), 8, ReconnectMode::SnapshotFallback),
			)
			.await;
			send(&mut socket, snapshot(SERVER_ID, 8)).await;
		})
		.await;
		let mut session =
			RetainedSession::connect(config, Some(old_checkpoint), SessionCancellation::new())
				.await
				.expect("test operation must succeed");

		assert_eq!(session.checkpoint(), None);

		let SessionDelivery::Snapshot { confirmation, .. } =
			session.next().await.expect("test operation must succeed")
		else {
			panic!("expected fallback snapshot")
		};
		let checkpoint =
			session.confirm_applied(confirmation).expect("test operation must succeed");

		assert_eq!(checkpoint.instance_id(), &instance_id(NEW_INSTANCE_ID));
		assert_eq!(checkpoint.cursor(), Cursor(8));

		drop(session);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn server_and_checkpoint_identity_changes_fail_closed_before_application_data() {
		let (_temp, authority) = local_transport();
		let invalid_config = RetainedSessionConfig::new(authority, server_id(SERVER_ID));
		let wrong_checkpoint =
			SessionCheckpoint::new(server_id(OTHER_SERVER_ID), instance_id(INSTANCE_ID), Cursor(1));

		assert_eq!(
			RetainedSession::connect(
				invalid_config,
				Some(wrong_checkpoint),
				SessionCancellation::new()
			)
			.await
			.unwrap_err(),
			RetainedSessionFailure::CheckpointIdentityMismatch
		);

		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(
				&mut socket,
				welcome(OTHER_SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Snapshot),
			)
			.await;
		})
		.await;

		assert_eq!(
			RetainedSession::connect(config, None, SessionCancellation::new()).await.unwrap_err(),
			RetainedSessionFailure::ServerIdentityMismatch
		);

		task.await.expect("test operation must succeed");

		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(NEW_INSTANCE_ID), 1, ReconnectMode::Resume))
				.await;
		})
		.await;

		assert_eq!(
			RetainedSession::connect(
				config,
				Some(checkpoint(INSTANCE_ID, 1)),
				SessionCancellation::new()
			)
			.await
			.unwrap_err(),
			RetainedSessionFailure::CheckpointIdentityMismatch
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn malformed_and_refused_frames_collapse_to_closed_failures_without_server_text() {
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;

			socket
				.send(Message::Binary(vec![1, 2, 3].into()))
				.await
				.expect("test operation must succeed");
		})
		.await;

		assert_eq!(
			RetainedSession::connect(config, None, SessionCancellation::new()).await.unwrap_err(),
			RetainedSessionFailure::Malformed
		);

		task.await.expect("test operation must succeed");

		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(
				&mut socket,
				ServerMessage::Refusal(RefusalEnvelope {
					server_id: server_id(SERVER_ID),
					refusal: Refusal::ProtocolViolation {
						message: WireText::new("untrusted server detail")
							.expect("test operation must succeed"),
					},
				}),
			)
			.await;
		})
		.await;
		let failure =
			RetainedSession::connect(config, None, SessionCancellation::new()).await.unwrap_err();

		assert_eq!(failure, RetainedSessionFailure::ProtocolViolation);
		assert!(!failure.to_string().contains("untrusted"));

		task.await.expect("test operation must succeed");

		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;

			let ServerMessage::Welcome(mut wrong_version) =
				welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Snapshot)
			else {
				unreachable!()
			};

			wrong_version.version = ProtocolVersion { major: 2, minor: 0 };

			send(&mut socket, ServerMessage::Welcome(wrong_version)).await;
		})
		.await;

		assert_eq!(
			RetainedSession::connect(config, None, SessionCancellation::new()).await.unwrap_err(),
			RetainedSessionFailure::ServiceVersionMismatch
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn idle_session_remains_owned_until_the_consumer_requests_more_data() {
		let (release_sender, release_receiver) = oneshot::channel();
		let (idle_sender, idle_receiver) = oneshot::channel();
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;

			idle_sender.send(()).expect("test operation must succeed");

			tokio::select! {
				biased;

				result = release_receiver => result.expect("test operation must succeed"),
				message = socket.next() => panic!("idle session emitted or closed: {message:?}"),
			}

			send(&mut socket, event(1)).await;
		})
		.await;
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		session.set_operation_timeout(std::time::Duration::ZERO);
		idle_receiver.await.expect("test operation must succeed");

		task::yield_now().await;

		release_sender.send(()).expect("test operation must succeed");

		let SessionDelivery::Event { event, .. } =
			session.next().await.expect("test operation must succeed")
		else {
			panic!("expected event after idle release")
		};

		assert_eq!(event.cursor, Cursor(1));

		drop(session);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn cancellation_terminates_an_inflight_receive_and_drops_the_owned_socket() {
		let (ready_sender, ready_receiver) = oneshot::channel();
		let (closed_sender, closed_receiver) = oneshot::channel();
		let (config, server_task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;

			ready_sender.send(()).expect("test operation must succeed");

			while socket.next().await.is_some() {}

			closed_sender.send(()).expect("test operation must succeed");
		})
		.await;
		let cancellation = SessionCancellation::new();
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			cancellation.clone(),
		)
		.await
		.expect("test operation must succeed");

		ready_receiver.await.expect("test operation must succeed");

		let client_task = tokio::spawn(async move { session.next().await });

		task::yield_now().await;

		cancellation.cancel();

		assert_eq!(
			client_task.await.expect("test operation must succeed").unwrap_err(),
			RetainedSessionFailure::Cancelled
		);

		closed_receiver.await.expect("test operation must succeed");
		server_task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn oversized_input_and_server_backpressure_are_closed_and_bounded() {
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;

			// The client can close as soon as it detects the oversized frame.
			let _send_result =
				socket.send(Message::Text("x".repeat(MAX_MESSAGE_BYTES + 1).into())).await;
		})
		.await;
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		assert_eq!(session.next().await.unwrap_err(), RetainedSessionFailure::Backpressure);

		task.await.expect("test operation must succeed");

		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;
			send(
				&mut socket,
				ServerMessage::Refusal(RefusalEnvelope {
					server_id: server_id(SERVER_ID),
					refusal: Refusal::Backpressure { queue_capacity: 1 },
				}),
			)
			.await;
		})
		.await;
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		assert_eq!(session.next().await.unwrap_err(), RetainedSessionFailure::Backpressure);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn close_completes_one_bounded_handshake_without_a_detached_socket() {
		let (closed_sender, closed_receiver) = oneshot::channel();
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;

			let message = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");

			assert!(matches!(message, Message::Close(_)));

			socket.flush().await.expect("test operation must succeed");
			closed_sender.send(()).expect("test operation must succeed");
		})
		.await;
		let session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		session.close().await.expect("test operation must succeed");
		closed_receiver.await.expect("test operation must succeed");
		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn zero_deadline_bounds_close_and_still_drops_the_owned_socket() {
		let (closed_sender, closed_receiver) = oneshot::channel();
		let (release_sender, release_receiver) = oneshot::channel();
		let (config, task, _temp) = fixture(|mut socket| async move {
			hello(&mut socket).await;
			send(&mut socket, welcome(SERVER_ID, Some(INSTANCE_ID), 0, ReconnectMode::Resume))
				.await;

			release_receiver.await.expect("test operation must succeed");

			while socket.next().await.is_some() {}

			closed_sender.send(()).expect("test operation must succeed");
		})
		.await;
		let mut session = RetainedSession::connect(
			config,
			Some(checkpoint(INSTANCE_ID, 0)),
			SessionCancellation::new(),
		)
		.await
		.expect("test operation must succeed");

		session.set_operation_timeout(std::time::Duration::ZERO);

		assert_eq!(session.close().await.unwrap_err(), RetainedSessionFailure::OperationTimeout);

		release_sender.send(()).expect("test operation must succeed");
		closed_receiver.await.expect("test operation must succeed");
		task.await.expect("test operation must succeed");
	}
}
