//! Direct owned WebSocket lifecycle over the same-UID local transport.
//!
//! One top-level lifecycle owns the published listener and directly polls every
//! daemon-local service future. One `JoinSet` owns every session and command task.
//! Shutdown creates one absolute deadline, harvests completed tasks explicitly,
//! aborts once at the deadline, and continues `join_next_with_id` until the set is
//! empty. The lifecycle then stops and drains every service future. Endpoint cleanup
//! starts only after all of those owners are gone. The listener closes before its
//! namespace lock is released.

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
	stream::{FuturesUnordered, SplitSink, SplitStream},
};
use tokio::{
	sync::{Mutex, mpsc, mpsc::Receiver, oneshot, watch},
	task::{Id as TokioTaskId, JoinError, JoinHandle, JoinSet},
	time,
};
use tokio_tungstenite::{
	WebSocketStream, accept_hdr_async_with_config,
	tungstenite::{
		Message,
		handshake::server::{ErrorResponse, Request, Response},
		http::StatusCode,
		protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
	},
};

use crate::{Application, ApplicationPublication};
use decodex_core::ServerIdentity;
use decodex_protocol::{
	self, CURRENT_VERSION, CausationId, ClientCommandId, ClientHello, ClientMessage,
	CommandEnvelope, CommandError, CommandOutcome, CommandReceipt, CommandResultEnvelope,
	CorrelationId, Cursor, EventEnvelope, IdempotencyKey, LocalTransportAuthority,
	LocalTransportListener, LocalTransportRefusal, LocalTransportStream, ProtocolVersion,
	QueryEnvelope, QueryResultEnvelope, ReceiptDisposition, ReconnectMode, Refusal,
	RefusalEnvelope, ResumeCursor, ServerId, ServerInstanceId, ServerMessage, ServerWelcome,
	SnapshotEnvelope, SnapshotItem, SupportedVersions, WireText,
};

const WS_PATH: &str = "/v1/ws";

type WebSocket = WebSocketStream<LocalTransportStream>;
type OwnedFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

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
	/// Time allowed for one WebSocket write or a peer response to a server close.
	pub write_timeout: Duration,
	/// One non-extendable deadline for all owned task quiescence after stopping starts.
	pub shutdown_timeout: Duration,
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
	/// One application command submitted by an admitted session.
	Command,
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
/// integrity failure, child panic, unexpected child failure, forced deadline,
/// and requested shutdown. Task-class ties select the lowest stable spawn ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationPrimary {
	/// Exact requested shutdown completed without an abnormal fact.
	RequestedShutdown,
	/// The absolute deadline forced cancellation of the lowest identified task.
	ForcedDeadline(OwnedTaskIdentity),
	/// An owned task ended unexpectedly without a panic or deadline abort.
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
	/// Number of command tasks assigned a stable spawn ID.
	pub spawned_commands: u64,
	/// Number of `JoinSet` results harvested before cleanup.
	pub harvested_tasks: u64,
	/// Number of owned tasks that returned normally.
	pub expected_tasks: u64,
	/// Number of owned task panics.
	pub panicked_tasks: u64,
	/// Number of unexpected non-panic task failures.
	pub failed_tasks: u64,
	/// Number of deadline-aborted tasks.
	pub forced_cancelled_tasks: u64,
	/// Number of stable owner-accounting failures.
	pub owner_integrity_failures: u64,
	/// Lowest stable identity among panicked tasks.
	pub lowest_panicked: Option<OwnedTaskIdentity>,
	/// Lowest stable identity among unexpected failed tasks.
	pub lowest_failed: Option<OwnedTaskIdentity>,
	/// Lowest stable identity among deadline-aborted tasks.
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
			&& self.harvested_tasks == self.spawned_sessions.saturating_add(self.spawned_commands)
			&& self.expected_tasks == self.harvested_tasks
	}
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
		assert!(!config.shutdown_timeout.is_zero(), "shutdown timeout must be non-zero");

		Self {
			inner: Arc::new(ServerInner {
				server_id,
				instance_id: ServerIdentity::generate()
					.ok()
					.and_then(|identity| ServerInstanceId::new(identity.as_str()).ok()),
				application,
				config,
				connection_ids: AtomicU64::new(1),
				state: Mutex::new(PublicationState::default()),
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
		mut listener: LocalTransportListener,
		mut shutdown_receiver: oneshot::Receiver<()>,
	) -> TerminationReceipt {
		let mut tasks = OwnedTasks::new();
		let mut receipt = TerminationReceiptBuilder::default();
		let (command_sender, mut command_receiver) =
			mpsc::channel::<OwnedFuture>(self.inner.config.outbound_queue_capacity);
		let (stop_sender, _) = watch::channel(false);
		let (service_stop_sender, service_stop_receiver) = watch::channel(false);
		let mut service_tasks: FuturesUnordered<_> = self
			.inner
			.application
			.daemon_service_tasks(service_stop_receiver)
			.into_iter()
			.map(|task| AssertUnwindSafe(task).catch_unwind())
			.collect();

		loop {
			tokio::select! {
				biased;

				joined = tasks.join_next_with_id(), if !tasks.is_empty() => {
					match joined {
						Some(joined) => {
							if receipt.record_join(&mut tasks, joined, false) {
								break;
							}
						},
						None => {
							receipt.record_owner_integrity();
							break;
						},
					}
				},
				_ = &mut shutdown_receiver => {
					receipt.requested_shutdown = true;
					break;
				},
				command = command_receiver.recv() => {
					match command {
						Some(command) => {
							if tasks.spawn(OwnedTaskKind::Command, command, &mut receipt).is_err() {
								receipt.record_owner_integrity();
								break;
							}
						},
						None => {
							receipt.record_owner_integrity();
							break;
						},
					}
				},
				_ = service_tasks.next(), if !service_tasks.is_empty() => {
					receipt.record_owner_integrity();
					break;
				},
				accepted = listener.accept() => {
					match accepted {
						Ok(stream) => {
							let server = self.clone();
							let session_commands = command_sender.clone();
							let session_stop = stop_sender.subscribe();
							let session = async move {
								server
									.handle_stream(stream, session_commands, session_stop)
									.await;
							};

							if tasks
								.spawn(OwnedTaskKind::Session, Box::pin(session), &mut receipt)
								.is_err()
							{
								receipt.record_owner_integrity();
								break;
							}
						},
						Err(refusal) if refusal.invalidates_listener() => {
							receipt.record_endpoint_refusal(refusal);
							break;
						},
						Err(_) => {},
					}
				},
			}
		}

		// This is the only deadline construction. All later waits use this exact instant.
		let stopping_started = time::Instant::now();
		let deadline =
			stopping_started.checked_add(self.inner.config.shutdown_timeout).unwrap_or_else(|| {
				receipt.record_owner_integrity();

				stopping_started
			});
		let deadline_sleep = time::sleep_until(deadline);

		tokio::pin!(deadline_sleep);

		command_receiver.close();
		let _ = stop_sender.send(true);
		drop(command_sender);

		let mut forced_abort = false;
		// `recv` returns `None` only after the closed channel has no buffered
		// submission and no outstanding pre-close permit.
		let mut ingress_drained = false;

		loop {
			if !forced_abort && time::Instant::now() >= deadline {
				forced_abort = true;
				tasks.abort_all();
			}
			if tasks.is_empty() && ingress_drained {
				match tasks.join_next_with_id().await {
					None => break,
					Some(joined) => {
						receipt.record_owner_integrity();
						receipt.record_join(&mut tasks, joined, forced_abort);
					},
				}
			}

			if forced_abort {
				tokio::select! {
					biased;

					command = command_receiver.recv(), if !ingress_drained => match command {
						Some(command) => {
							if tasks
								.spawn_forced(OwnedTaskKind::Command, command, &mut receipt)
								.is_err()
							{
								receipt.record_owner_integrity();
							}
						},
						None => ingress_drained = true,
					},
					joined = tasks.join_next_with_id(), if !tasks.is_empty() => match joined {
						Some(joined) => {
							receipt.record_join(&mut tasks, joined, true);
						},
						None => receipt.record_owner_integrity(),
					},
				}
			} else {
				tokio::select! {
					biased;

					_ = &mut deadline_sleep => {
						forced_abort = true;
						tasks.abort_all();
					},
					command = command_receiver.recv(), if !ingress_drained => match command {
						Some(command) => {
							if tasks
								.spawn(OwnedTaskKind::Command, command, &mut receipt)
								.is_err()
							{
								receipt.record_owner_integrity();
							}
						},
						None => ingress_drained = true,
					},
					joined = tasks.join_next_with_id(), if !tasks.is_empty() => match joined {
						Some(joined) => {
							receipt.record_join(&mut tasks, joined, false);
						},
						None => receipt.record_owner_integrity(),
					},
				}
			}
		}

		receipt.finish_task_accounting(&tasks);
		let _ = service_stop_sender.send(true);
		while let Some(completion) = service_tasks.next().await {
			if completion.is_err() {
				receipt.record_owner_integrity();
			}
		}
		self.inner.state.lock().await.subscribers.clear();

		// No session, command, service task, or application owner remains when
		// the listener removes its publication and releases the namespace lock.
		drop(self);

		if let Err(refusal) = listener.cleanup() {
			receipt.record_cleanup_refusal(refusal);
		}

		receipt.finish()
	}

	async fn handle_stream(
		&self,
		stream: LocalTransportStream,
		command_sender: mpsc::Sender<OwnedFuture>,
		mut stop: watch::Receiver<bool>,
	) {
		let config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(0)
			.max_write_buffer_size(self.inner.config.maximum_message_bytes.saturating_add(1))
			.max_message_size(Some(self.inner.config.maximum_message_bytes))
			.max_frame_size(Some(self.inner.config.maximum_message_bytes));
		let callback = |request: &Request, response: Response| {
			if request.uri().path() == WS_PATH && request.uri().query().is_none() {
				return Ok(response);
			}

			let mut refusal = ErrorResponse::new(Some("WebSocket route is unavailable".to_owned()));

			*refusal.status_mut() = StatusCode::NOT_FOUND;

			Err(refusal)
		};
		let handshake = time::timeout(
			self.inner.config.hello_timeout,
			accept_hdr_async_with_config(stream, callback, Some(config)),
		);
		let socket = tokio::select! {
			biased;

			() = stopped(&mut stop) => return,
			result = handshake => match result {
				Ok(Ok(socket)) => socket,
				_ => return,
			},
		};

		self.handle_connection(socket, command_sender, stop).await;
	}

	async fn handle_connection(
		&self,
		mut socket: WebSocket,
		command_sender: mpsc::Sender<OwnedFuture>,
		mut stop: watch::Receiver<bool>,
	) {
		let Some(hello) = self.receive_hello(&mut socket, &mut stop).await else {
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

			return;
		}

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
			if *stop.borrow() || !self.send_direct(&mut socket, message).await {
				self.remove_subscriber(connection_id).await;

				return;
			}
		}

		let (mut socket_sender, mut socket_receiver) = socket.split();
		let close_sent = {
			let reader_stop = stop.clone();
			let writer_stop = stop.clone();
			let reader = self.read_messages(
				&mut socket_receiver,
				connection_id,
				negotiated,
				&command_sender,
				reader_stop,
			);
			let writer = self.write_messages(&mut socket_sender, receiver, writer_stop);

			tokio::pin!(reader, writer);

			tokio::select! {
				close_sent = &mut writer => close_sent,
				backpressure = &mut reader => {
					if backpressure {
						writer.await
					} else {
						false
					}
				},
			}
		};

		if close_sent {
			self.await_peer_close(&mut socket_receiver, &mut stop).await;
		}

		self.remove_subscriber(connection_id).await;
	}

	async fn receive_hello(
		&self,
		socket: &mut WebSocket,
		stop: &mut watch::Receiver<bool>,
	) -> Option<ClientHello> {
		let received = tokio::select! {
			biased;

			() = stopped(stop) => return None,
			received = time::timeout(self.inner.config.hello_timeout, socket.next()) => received,
		};
		let received = received.ok()?;
		let received = received?;
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
			Ok(ClientMessage::Query(_)) => {
				let refusal =
					protocol_refusal(&self.inner.server_id, "hello is required before a query");
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
				instance_id: (negotiated == CURRENT_VERSION)
					.then(|| self.inner.instance_id.clone())
					.flatten(),
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
			version == CURRENT_VERSION
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
		command_sender: &mpsc::Sender<OwnedFuture>,
		mut stop: watch::Receiver<bool>,
	) -> bool {
		loop {
			let received = tokio::select! {
				biased;

				() = stopped(&mut stop) => return false,
				received = receiver.next() => received,
			};
			let Some(Ok(message)) = received else { return false };
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
					if !self
						.submit_command(
							connection_id,
							command,
							negotiated,
							command_sender,
							&mut stop,
						)
						.await
					{
						return true;
					}
				},
				ClientMessage::Query(query) => {
					if negotiated != CURRENT_VERSION || query.version != negotiated {
						if !self
							.enqueue(
								connection_id,
								protocol_refusal(
									&self.inner.server_id,
									"query requires negotiated protocol 1.2",
								),
							)
							.await
						{
							return true;
						}

						continue;
					}
					if !self.execute_query(connection_id, query, negotiated).await {
						return true;
					}
				},
			}
		}
	}

	async fn execute_query(
		&self,
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

		self.enqueue(connection_id, ServerMessage::QueryResult(result)).await
	}

	async fn submit_command(
		&self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
		command_sender: &mpsc::Sender<OwnedFuture>,
		stop: &mut watch::Receiver<bool>,
	) -> bool {
		let server = self.clone();
		let (result_sender, result_receiver) = oneshot::channel();
		let command_task: OwnedFuture = Box::pin(async move {
			let delivered = server.execute_command_owned(connection_id, command, version).await;
			let _ = result_sender.send(delivered);
		});
		let submitted = tokio::select! {
			biased;

			() = stopped(stop) => return false,
			submitted = command_sender.send(command_task) => submitted,
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

	async fn execute_command_owned(
		&self,
		connection_id: u64,
		command: CommandEnvelope,
		version: ProtocolVersion,
	) -> bool {
		let fingerprint =
			serde_json::to_vec(&(version, &command.expected_revision, &command.payload))
				.expect("typed command serialization cannot fail");
		let receipt_key = (version, command.idempotency_key.clone());
		let mut state = self.inner.state.lock().await;

		if let Some(stored) = state.receipts.get(&receipt_key).cloned() {
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
			result.client_command_id = command.client_command_id.clone();

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

		let version_receipt_count =
			state.receipts.keys().filter(|(stored_version, _)| stored_version == &version).count();

		if version_receipt_count >= self.inner.config.receipt_capacity {
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

		state.receipts.insert(receipt_key, stored);

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
		sender: &mut SplitSink<WebSocket, Message>,
		mut receiver: Receiver<ServerMessage>,
		mut stop: watch::Receiver<bool>,
	) -> bool {
		loop {
			let message = tokio::select! {
				biased;

				() = stopped(&mut stop) => {
					return self
						.send_split_close(sender, 1_001, "server is shutting down")
						.await;
				},
				message = receiver.recv() => message,
			};
			let Some(message) = message else {
				return self
					.send_split_close(sender, 1_013, "bounded outbound queue exceeded")
					.await;
			};
			let Ok(encoded) = decodex_protocol::encode_server_message(&message) else {
				return false;
			};

			if encoded.len() > self.inner.config.maximum_message_bytes {
				return self
					.send_split_close(sender, 1_009, "outbound message exceeds bounded size")
					.await;
			}

			let write_result = time::timeout(
				self.inner.config.write_timeout,
				sender.send(Message::Text(encoded.into())),
			)
			.await;

			if !matches!(write_result, Ok(Ok(()))) {
				return false;
			}
		}
	}

	async fn await_peer_close(
		&self,
		receiver: &mut SplitStream<WebSocket>,
		stop: &mut watch::Receiver<bool>,
	) {
		let handshake = async {
			loop {
				let message = tokio::select! {
					biased;

					() = stopped(stop) => return,
					message = receiver.next() => message,
				};

				if matches!(message, Some(Ok(Message::Close(_))) | Some(Err(_)) | None) {
					return;
				}
			}
		};
		let _ = time::timeout(self.inner.config.write_timeout, handshake).await;
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
	) -> bool {
		let close = sender.send(Message::Close(Some(CloseFrame {
			code: CloseCode::from(code),
			reason: reason.into(),
		})));

		time::timeout(self.inner.config.write_timeout, close)
			.await
			.is_ok_and(|result| result.is_ok())
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
	state: Mutex<PublicationState>,
}

#[derive(Default)]
struct PublicationState {
	cursor: Cursor,
	events: VecDeque<EventEnvelope>,
	receipts: HashMap<(ProtocolVersion, IdempotencyKey), StoredCommand>,
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

struct OwnedTaskCompletion {
	identity: OwnedTaskIdentity,
}

struct OwnedTasks {
	set: JoinSet<OwnedTaskCompletion>,
	identities: HashMap<TokioTaskId, OwnedTaskIdentity>,
	next_spawn_id: u64,
}

impl OwnedTasks {
	fn new() -> Self {
		Self { set: JoinSet::new(), identities: HashMap::new(), next_spawn_id: 1 }
	}

	fn spawn(
		&mut self,
		kind: OwnedTaskKind,
		future: OwnedFuture,
		receipt: &mut TerminationReceiptBuilder,
	) -> Result<(), ()> {
		let next = self.next_spawn_id.checked_add(1).ok_or(())?;
		let identity = OwnedTaskIdentity { spawn_id: SpawnId(self.next_spawn_id), kind };

		self.next_spawn_id = next;

		let handle = self.set.spawn(async move {
			future.await;

			OwnedTaskCompletion { identity }
		});
		let prior = self.identities.insert(handle.id(), identity);

		if prior.is_some() {
			return Err(());
		}

		receipt.record_spawn(kind);

		Ok(())
	}

	fn spawn_forced(
		&mut self,
		kind: OwnedTaskKind,
		future: OwnedFuture,
		receipt: &mut TerminationReceiptBuilder,
	) -> Result<(), ()> {
		let next = self.next_spawn_id.checked_add(1).ok_or(())?;
		let identity = OwnedTaskIdentity { spawn_id: SpawnId(self.next_spawn_id), kind };

		self.next_spawn_id = next;

		// A submission that crosses an outstanding pre-close permit after the
		// deadline still receives an owned identity and terminal receipt, but its
		// command future is never polled.
		let handle = self.set.spawn(async move {
			let _unstarted = future;

			std::future::pending::<OwnedTaskCompletion>().await
		});
		let prior = self.identities.insert(handle.id(), identity);

		if prior.is_some() {
			handle.abort();

			return Err(());
		}

		receipt.record_spawn(kind);
		handle.abort();

		Ok(())
	}

	fn is_empty(&self) -> bool {
		self.set.is_empty()
	}

	fn abort_all(&mut self) {
		self.set.abort_all();
	}

	async fn join_next_with_id(
		&mut self,
	) -> Option<Result<(TokioTaskId, OwnedTaskCompletion), JoinError>> {
		self.set.join_next_with_id().await
	}
}

#[derive(Default)]
struct TerminationReceiptBuilder {
	requested_shutdown: bool,
	spawned_sessions: u64,
	spawned_commands: u64,
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
			OwnedTaskKind::Command => {
				self.spawned_commands = self.spawned_commands.saturating_add(1);
			},
		}
	}

	fn record_join(
		&mut self,
		tasks: &mut OwnedTasks,
		joined: Result<(TokioTaskId, OwnedTaskCompletion), JoinError>,
		after_forced_abort: bool,
	) -> bool {
		self.harvested_tasks = self.harvested_tasks.saturating_add(1);

		match joined {
			Ok((task_id, completion)) => {
				let expected = tasks.identities.remove(&task_id);

				self.expected_tasks = self.expected_tasks.saturating_add(1);

				if expected != Some(completion.identity) {
					self.record_owner_integrity();

					true
				} else {
					false
				}
			},
			Err(error) => {
				let identity = tasks.identities.remove(&error.id());
				let Some(identity) = identity else {
					self.record_owner_integrity();

					return true;
				};

				if error.is_panic() {
					self.panicked_tasks = self.panicked_tasks.saturating_add(1);
					record_lowest(&mut self.lowest_panicked, identity);

					true
				} else if error.is_cancelled() && after_forced_abort {
					self.forced_cancelled_tasks = self.forced_cancelled_tasks.saturating_add(1);
					record_lowest(&mut self.lowest_forced, identity);

					false
				} else {
					self.failed_tasks = self.failed_tasks.saturating_add(1);
					record_lowest(&mut self.lowest_failed, identity);

					true
				}
			},
		}
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

	fn finish_task_accounting(&mut self, tasks: &OwnedTasks) {
		let spawned = self.spawned_sessions.saturating_add(self.spawned_commands);
		let classified = self
			.expected_tasks
			.saturating_add(self.panicked_tasks)
			.saturating_add(self.failed_tasks)
			.saturating_add(self.forced_cancelled_tasks);

		if !tasks.is_empty()
			|| !tasks.identities.is_empty()
			|| spawned != self.harvested_tasks
			|| classified != self.harvested_tasks
		{
			self.record_owner_integrity();
		}
	}

	fn finish(mut self) -> TerminationReceipt {
		if !self.requested_shutdown
			&& self.cleanup_refusal.is_none()
			&& self.endpoint_refusal.is_none()
			&& self.owner_integrity_failures == 0
			&& self.panicked_tasks == 0
			&& self.failed_tasks == 0
			&& self.forced_cancelled_tasks == 0
		{
			self.record_owner_integrity();
		}

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
		} else {
			TerminationPrimary::RequestedShutdown
		};

		TerminationReceipt {
			primary,
			spawned_sessions: self.spawned_sessions,
			spawned_commands: self.spawned_commands,
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
			Self::Terminated(receipt) =>
				write!(formatter, "server lifecycle terminated: {:?}", receipt.primary),
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
