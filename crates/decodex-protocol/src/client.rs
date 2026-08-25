//! Bounded API-only client transport and profile projection.

use std::{
	fmt::{Debug, Display, Formatter},
	io::ErrorKind,
	path::Path,
	sync::atomic::{AtomicBool, Ordering},
	time::Duration,
};

use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use tokio::time;
use tokio_tungstenite::{
	self, WebSocketStream,
	tungstenite::{Message, protocol::WebSocketConfig},
};

use crate::{
	AccountInitialSelectionResult, AccountInspectResult, AccountLoginRequest,
	AccountLoginRequestEnvelope, AccountLoginResponseEnvelope, AccountLoginStart,
	AccountLoginStatus, AccountObservationSignal, AccountProfileEmailDto, AccountProfileResult,
	AccountSelectionModeDto, AccountsResult, CURRENT_ARTIFACT_COHORT, CURRENT_VERSION,
	ClientCommandId, ClientHello, ClientMessage, CodexAuthProjectionResult, CommandEnvelope,
	CommandError, CommandOutcome, CommandPayload, CorrelationId, DoctorReport, EntityId,
	EntityRevision, IdempotencyKey, ProtocolVersion, QueryEnvelope, QueryId, QueryPayload,
	QueryResultPayload, ReceiptDisposition, Refusal, RefusalEnvelope, ResetCardDescriptorDto,
	ResetCardInventoryResult, ResetCardOperationResult, ResultPayload, RetainedSessionConfig,
	RetainedSessionFailure, ServerId, ServerMessage, VersionRefusal, WorkItemBoardPageSize,
	WorkItemBoardProjectId, WorkItemBoardResult, WorkItemBoardWorkItemId, WorkItemState,
	local_transport::{LocalTransportAuthority, LocalTransportRefusal, LocalTransportStream},
};
use decodex_core::{
	ConfigError, DecodexClientConfig, DecodexRoot, PathError, ServerIdentity, ServerProfile,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
// Doctor revalidates the complete local database and runtime-authority contract.
// Keep its bounded read budget separate from ordinary cached UI queries.
const DOCTOR_CLIENT_TIMEOUT: Duration = Duration::from_secs(15);
const RESET_CARD_CLIENT_TIMEOUT: Duration = Duration::from_secs(35);
const ONE_SHOT_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CLIENT_MESSAGE_BYTES: usize = 256 * 1_024;
const MAX_INTERLEAVED_MESSAGES: usize = 64;
// This URI is WebSocket handshake metadata only. The client passes an already
// admitted Unix stream, so this value cannot resolve or dial a TCP endpoint.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

type OneShotSocket = WebSocketStream<LocalTransportStream>;

struct CompletedOneShot<T> {
	value: T,
	socket: OneShotSocket,
}

impl<T> CompletedOneShot<T> {
	const fn new(value: T, socket: OneShotSocket) -> Self {
		Self { value, socket }
	}
}

async fn close_one_shot_socket(mut socket: OneShotSocket) {
	let close = async {
		if socket.send(Message::Close(None)).await.is_err() {
			return;
		}

		while let Some(message) = socket.next().await {
			match message {
				Ok(Message::Close(_)) | Err(_) => return,
				Ok(Message::Ping(payload)) => {
					if socket.send(Message::Pong(payload)).await.is_err() {
						return;
					}
				},
				Ok(Message::Pong(_)) => {},
				Ok(Message::Text(_) | Message::Binary(_) | Message::Frame(_)) => return,
			}
		}
	};

	// A completed application response remains authoritative if bounded cleanup fails.
	let _ = time::timeout(ONE_SHOT_CLOSE_TIMEOUT, close).await;
}

/// Whether one selected client profile targets the same host or a different host.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
	/// Same-UID service on the client host.
	Local,
	/// Explicit different-host service with a mandatory identity pin.
	Remote,
}

/// Validated client-only projection of one selected server profile.
///
/// This type cannot carry a repository, database, credential, or other
/// server-host-only value.
#[derive(Clone, Eq, PartialEq)]
pub struct ClientProfile {
	profile_name: String,
	kind: ProfileKind,
	local_transport: Option<LocalTransportAuthority>,
	expected_server_id: ServerId,
}
impl ClientProfile {
	/// Load the active or explicitly named profile from the platform default root.
	pub fn load_default(selected: Option<&str>) -> Result<Self, ClientFailure> {
		let root = DecodexRoot::platform_default().map_err(map_root_error)?;

		Self::load(root.as_path(), selected)
	}

	/// Load the active or explicitly named profile from one typed Decodex root.
	pub fn load(root: &Path, selected: Option<&str>) -> Result<Self, ClientFailure> {
		let root = DecodexRoot::new(root).map_err(map_root_error)?;
		let paths = root.paths();
		let config = DecodexClientConfig::load(&paths).map_err(map_config_error)?;
		let (profile_name, profile) =
			config.selected_profile(selected).map_err(map_config_error)?;
		let profile_name = profile_name.as_str().to_owned();

		match profile {
			ServerProfile::Local(profile) => {
				let local_transport = LocalTransportAuthority::new(
					paths.clone(),
					profile.policy(),
					profile.service_owner_uid(),
				)
				.map_err(map_local_transport_failure)?;
				let expected = match profile.expected_server_identity() {
					Some(identity) => identity.clone(),
					None => ServerIdentity::load(&paths).map_err(map_identity_error)?,
				};

				Ok(Self {
					profile_name,
					kind: ProfileKind::Local,
					local_transport: Some(local_transport),
					expected_server_id: server_id(&expected)?,
				})
			},
			ServerProfile::Remote(profile) => Ok(Self {
				profile_name,
				kind: ProfileKind::Remote,
				local_transport: None,
				expected_server_id: server_id(profile.expected_server_identity())?,
			}),
		}
	}

	/// Local or remote profile classification.
	pub const fn kind(&self) -> ProfileKind {
		self.kind
	}

	/// Selected validated profile name.
	pub fn name(&self) -> &str {
		&self.profile_name
	}

	/// Stable server identity expected by this client profile.
	pub const fn expected_server_id(&self) -> &ServerId {
		&self.expected_server_id
	}

	/// Apply a stricter caller-retained server-identity pin.
	pub fn with_expected_server_id(mut self, expected_server_id: ServerId) -> Self {
		self.expected_server_id = expected_server_id;

		self
	}

	/// Project this selected typed profile into the retained-session boundary.
	///
	/// Remote profiles remain fail-closed while retained sessions are same-UID local only.
	pub fn retained_session_config(&self) -> Result<RetainedSessionConfig, RetainedSessionFailure> {
		let local_transport =
			self.local_transport.clone().ok_or(RetainedSessionFailure::RemoteTransportDisabled)?;

		Ok(RetainedSessionConfig::new(local_transport, self.expected_server_id.clone()))
	}

	#[cfg(test)]
	fn fixture(local_transport: LocalTransportAuthority, expected_server_id: ServerId) -> Self {
		Self {
			profile_name: "fixture".into(),
			kind: ProfileKind::Local,
			local_transport: Some(local_transport),
			expected_server_id,
		}
	}
}

impl Debug for ClientProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ClientProfile")
			.field("profile_selected", &true)
			.field("kind", &self.kind)
			.field("endpoint", &"<redacted>")
			.field("identity_pinned", &true)
			.finish()
	}
}

/// Reusable bounded WebSocket client for authoritative doctor/status queries.
pub struct DoctorClient {
	profile: ClientProfile,
	timeout: Duration,
}
impl DoctorClient {
	/// Build a client with the fixed production timeout and bounded wire limits.
	pub const fn new(profile: ClientProfile) -> Self {
		Self { profile, timeout: DOCTOR_CLIENT_TIMEOUT }
	}

	/// Selected client profile.
	pub const fn profile(&self) -> &ClientProfile {
		&self.profile
	}

	/// Negotiate the current protocol, verify the stable server identity, and
	/// return one fresh authoritative doctor report.
	pub async fn query(&self) -> Result<DoctorReport, ClientFailure> {
		let completed = time::timeout(self.timeout, self.query_inner())
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		Ok(completed.value)
	}

	async fn query_inner(&self) -> Result<CompletedOneShot<DoctorReport>, ClientFailure> {
		let local_transport =
			self.profile.local_transport.as_ref().ok_or(ClientFailure::RemoteTransportDisabled)?;
		let config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(16 * 1_024)
			.max_write_buffer_size(MAX_CLIENT_MESSAGE_BYTES)
			.max_message_size(Some(MAX_CLIENT_MESSAGE_BYTES))
			.max_frame_size(Some(MAX_CLIENT_MESSAGE_BYTES));
		let stream = time::timeout(self.timeout, local_transport.connect())
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)?
			.map_err(map_local_transport_failure)?;
		let (mut socket, _) = time::timeout(
			self.timeout,
			tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, Some(config)),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)?
		.map_err(map_connect_error)?;
		let hello = ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			artifact_cohort: Some(CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(self.profile.expected_server_id.clone()),
			resume: None,
		});

		self.send(&mut socket, hello).await?;

		let welcome = match self.receive(&mut socket).await? {
			ServerMessage::Welcome(welcome) => welcome,
			ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
			_ => return Err(ClientFailure::ProtocolMalformed),
		};

		if welcome.version != CURRENT_VERSION
			|| welcome.supported != crate::SupportedVersions::current()
		{
			return Err(version_failure(welcome.version));
		}
		if welcome.artifact_cohort != Some(CURRENT_ARTIFACT_COHORT) {
			return Err(ClientFailure::ArtifactCohortMismatch);
		}

		self.verify_server(&welcome.server_id)?;

		let snapshot = match self.receive(&mut socket).await? {
			ServerMessage::Snapshot(snapshot) => snapshot,
			ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
			_ => return Err(ClientFailure::ProtocolMalformed),
		};

		if snapshot.version != CURRENT_VERSION {
			return Err(version_failure(snapshot.version));
		}

		self.verify_server(&snapshot.server_id)?;

		let query_id =
			QueryId::new("decodex-cli-doctor").expect("the fixed doctor query identity is bounded");
		let query = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: query_id.clone(),
			payload: QueryPayload::GetDoctorStatus,
		});

		self.send(&mut socket, query).await?;

		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.receive(&mut socket).await? {
				ServerMessage::QueryResult(result) => {
					if result.version != CURRENT_VERSION {
						return Err(version_failure(result.version));
					}

					self.verify_server(&result.server_id)?;

					if result.query_id != query_id {
						return Err(ClientFailure::ProtocolMalformed);
					}

					let QueryResultPayload::DoctorStatus(report) = result.payload else {
						return Err(ClientFailure::ProtocolMalformed);
					};

					if report.version() != CURRENT_VERSION {
						return Err(version_failure(report.version()));
					}

					self.verify_server(report.server_id())?;

					if !report.has_current_component_set() {
						return Err(ClientFailure::ProtocolMalformed);
					}

					return Ok(CompletedOneShot::new(report, socket));
				},
				ServerMessage::Event(event) => {
					if event.version != CURRENT_VERSION {
						return Err(version_failure(event.version));
					}

					self.verify_server(&event.server_id)?;
				},
				ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}

		Err(ClientFailure::ProtocolBackpressure)
	}

	async fn send<S>(&self, socket: &mut S, message: ClientMessage) -> Result<(), ClientFailure>
	where
		S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
	{
		let encoded = serde_json::to_string(&message)
			.expect("typed bounded client message serialization cannot fail");

		time::timeout(self.timeout, socket.send(Message::Text(encoded.into())))
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)?
			.map_err(|_| ClientFailure::ProtocolDisconnected)
	}

	async fn receive<S>(&self, socket: &mut S) -> Result<ServerMessage, ClientFailure>
	where
		S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
	{
		loop {
			let message = time::timeout(self.timeout, socket.next())
				.await
				.map_err(|_| ClientFailure::ProtocolTimeout)?
				.ok_or(ClientFailure::ProtocolDisconnected)?
				.map_err(map_receive_error)?;

			match message {
				Message::Text(text) => {
					return serde_json::from_str(&text)
						.map_err(|_| ClientFailure::ProtocolMalformed);
				},
				Message::Ping(_) | Message::Pong(_) => {},
				Message::Close(_) => return Err(ClientFailure::ProtocolDisconnected),
				Message::Binary(_) | Message::Frame(_) => {
					return Err(ClientFailure::ProtocolMalformed);
				},
			}
		}
	}

	fn verify_server(&self, actual: &ServerId) -> Result<(), ClientFailure> {
		if actual == &self.profile.expected_server_id {
			Ok(())
		} else {
			Err(ClientFailure::ServerIdentityMismatch)
		}
	}

	fn refusal_failure(&self, refusal: RefusalEnvelope) -> ClientFailure {
		match self.verify_server(&refusal.server_id) {
			Ok(()) => map_refusal(refusal.refusal),
			Err(failure) => failure,
		}
	}
}

/// Verified response to one reset-card consume command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResetCardConsumeResponse {
	/// The daemon durably accepted the operation or replayed its durable state.
	Accepted {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Public descriptor selected by the operator.
		descriptor: ResetCardDescriptorDto,
		/// Current durable operation state.
		state: ResetCardOperationResult,
		/// Exact account revision accepted by the command.
		entity_revision: EntityRevision,
	},
	/// Protocol or application guards rejected the command before acceptance.
	Rejected {
		/// Closed typed command rejection.
		error: CommandError,
	},
	/// The command send was attempted, so transport failure cannot prove non-dispatch.
	PotentiallyDispatched {
		/// Closed local transport or protocol failure.
		failure: ClientFailure,
	},
}

/// Bounded reset-card client over the pinned, versioned local daemon protocol.
///
/// Every operation uses one connection and sends each consume command exactly
/// once. Callers can poll [`Self::status`] without replaying a mutation.
pub struct ResetCardClient {
	profile: ClientProfile,
	timeout: Duration,
}
impl ResetCardClient {
	/// Build a reset-card client with a deadline longer than the daemon's typed query deadline.
	pub const fn new(profile: ClientProfile) -> Self {
		Self { profile, timeout: RESET_CARD_CLIENT_TIMEOUT }
	}

	/// Selected client profile.
	pub const fn profile(&self) -> &ClientProfile {
		&self.profile
	}

	/// Read one fresh public reset-card observation with explicit detail completeness.
	pub async fn list(
		&self,
		account_id: EntityId,
	) -> Result<ResetCardInventoryResult, ClientFailure> {
		self.require_local_profile()?;
		let expected_account_id = account_id.clone();
		let completed = time::timeout(
			// This query reads the daemon-owned observation cache. It must not inherit
			// the longer budget used by reset-card effects and recovery.
			CLIENT_TIMEOUT,
			self.query_inner("decodex-reset-card-list", QueryPayload::GetResetCards { account_id }),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;

		match payload {
			QueryResultPayload::ResetCards(result) => {
				let mismatched = match &result {
					ResetCardInventoryResult::Available { account_id, .. }
					| ResetCardInventoryResult::ObservationFailed { account_id, .. } => {
						account_id != &expected_account_id
					},
					ResetCardInventoryResult::Unavailable { .. } => false,
				};
				if mismatched { Err(ClientFailure::ProtocolMalformed) } else { Ok(result) }
			},
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Read the current durable state of one reset-card operation.
	pub async fn status(
		&self,
		idempotency_key: IdempotencyKey,
	) -> Result<ResetCardOperationResult, ClientFailure> {
		self.require_local_profile()?;
		let completed = time::timeout(
			self.timeout,
			self.query_inner(
				"decodex-reset-card-status",
				QueryPayload::GetResetCardOperation { idempotency_key },
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;

		match payload {
			QueryResultPayload::ResetCardOperation(result) => Ok(result),
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Send one consume command exactly once and return its durable acceptance.
	///
	/// `expected_revision` is mandatory at this API boundary. This method does
	/// not reconnect or retry after the command is sent. An error guarantees
	/// that no send was attempted. Failure after a send attempt is returned as
	/// [`ResetCardConsumeResponse::PotentiallyDispatched`].
	pub async fn consume(
		&self,
		account_id: EntityId,
		descriptor: ResetCardDescriptorDto,
		expected_revision: EntityRevision,
		idempotency_key: IdempotencyKey,
	) -> Result<ResetCardConsumeResponse, ClientFailure> {
		self.require_local_profile()?;
		let dispatch_attempted = AtomicBool::new(false);
		let result = time::timeout(
			self.timeout,
			self.consume_inner(
				account_id,
				descriptor,
				expected_revision,
				idempotency_key,
				&dispatch_attempted,
			),
		)
		.await;
		let result = match result {
			Ok(Ok(completed)) => {
				close_one_shot_socket(completed.socket).await;
				Ok(completed.value)
			},
			Ok(Err(failure)) => Err(failure),
			Err(_) => Err(ClientFailure::ProtocolTimeout),
		};

		match result {
			Ok(response) => Ok(response),
			Err(failure) if dispatch_attempted.load(Ordering::Acquire) => {
				Ok(ResetCardConsumeResponse::PotentiallyDispatched { failure })
			},
			Err(failure) => Err(failure),
		}
	}

	fn require_local_profile(&self) -> Result<(), ClientFailure> {
		if self.profile.kind() == ProfileKind::Local {
			Ok(())
		} else {
			Err(ClientFailure::RemoteMutationUnsupported)
		}
	}

	async fn query_inner(
		&self,
		query_identity: &'static str,
		payload: QueryPayload,
	) -> Result<CompletedOneShot<QueryResultPayload>, ClientFailure> {
		let mut socket = self.connect().await?;
		let query_id =
			QueryId::new(query_identity).expect("fixed query identity is bounded and nonempty");

		self.send(
			&mut socket,
			ClientMessage::Query(QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: query_id.clone(),
				payload,
			}),
		)
		.await?;

		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.receive(&mut socket).await? {
				ServerMessage::QueryResult(result) => {
					self.verify_version_and_server(result.version, &result.server_id)?;

					if result.query_id != query_id {
						return Err(ClientFailure::ProtocolMalformed);
					}
					return Ok(CompletedOneShot::new(result.payload, socket));
				},
				ServerMessage::Event(event) => {
					self.verify_version_and_server(event.version, &event.server_id)?
				},
				ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}

		Err(ClientFailure::ProtocolBackpressure)
	}

	async fn consume_inner(
		&self,
		account_id: EntityId,
		descriptor: ResetCardDescriptorDto,
		expected_revision: EntityRevision,
		idempotency_key: IdempotencyKey,
		dispatch_attempted: &AtomicBool,
	) -> Result<CompletedOneShot<ResetCardConsumeResponse>, ClientFailure> {
		let mut socket = self.connect().await?;
		let command_identity = format!("reset-card-use:{}", idempotency_key.as_str());
		let client_command_id = ClientCommandId::new(command_identity.clone())
			.map_err(|_| ClientFailure::ProtocolMalformed)?;
		let correlation_id =
			CorrelationId::new(command_identity).map_err(|_| ClientFailure::ProtocolMalformed)?;
		let command = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: client_command_id.clone(),
			idempotency_key: idempotency_key.clone(),
			expected_revision: Some(expected_revision),
			correlation_id,
			causation_id: None,
			payload: crate::CommandPayload::ConsumeResetCard {
				account_id: account_id.clone(),
				descriptor,
			},
		});

		dispatch_attempted.store(true, Ordering::Release);
		self.send(&mut socket, command).await?;

		let mut receipt_disposition = None;

		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.receive(&mut socket).await? {
				ServerMessage::CommandReceipt(receipt) => {
					self.verify_version_and_server(receipt.version, &receipt.server_id)?;

					if receipt_disposition.is_some()
						|| receipt.client_command_id != client_command_id
						|| receipt.idempotency_key != idempotency_key
						|| (receipt.disposition != ReceiptDisposition::Duplicate
							&& receipt.original_client_command_id != client_command_id)
						|| !matches!(
							receipt.disposition,
							ReceiptDisposition::Executed
								| ReceiptDisposition::Duplicate
								| ReceiptDisposition::Refused
						) {
						return Err(ClientFailure::ProtocolMalformed);
					}

					receipt_disposition = Some(receipt.disposition);
				},
				ServerMessage::CommandResult(result) => {
					self.verify_version_and_server(result.version, &result.server_id)?;

					let Some(receipt_disposition) = receipt_disposition else {
						return Err(ClientFailure::ProtocolMalformed);
					};
					if result.client_command_id != client_command_id
						|| result.idempotency_key != idempotency_key
						|| (receipt_disposition == ReceiptDisposition::Refused
							&& result.outcome != CommandOutcome::Rejected)
					{
						return Err(ClientFailure::ProtocolMalformed);
					}

					let response = match (
						result.outcome,
						result.entity_revision,
						result.payload,
						result.error,
					) {
						(
							CommandOutcome::Succeeded,
							Some(entity_revision),
							Some(ResultPayload::ResetCardOperationAccepted {
								account_id: result_account_id,
								descriptor: result_descriptor,
								state,
							}),
							None,
						) if result_account_id == account_id
							&& result_descriptor == descriptor
							&& entity_revision == expected_revision =>
						{
							Ok(ResetCardConsumeResponse::Accepted {
								account_id,
								descriptor,
								state,
								entity_revision,
							})
						},
						(
							CommandOutcome::Succeeded,
							Some(entity_revision),
							Some(ResultPayload::ResetCardConsumed {
								account_id: result_account_id,
								descriptor: result_descriptor,
								outcome,
							}),
							None,
						) if result_account_id == account_id
							&& result_descriptor == descriptor
							&& entity_revision == expected_revision =>
						{
							Ok(ResetCardConsumeResponse::Accepted {
								account_id,
								descriptor,
								state: ResetCardOperationResult::Completed { outcome },
								entity_revision,
							})
						},
						(CommandOutcome::Rejected, None, None, Some(error))
							if !matches!(&error, CommandError::AcceptanceUnknown) =>
						{
							Ok(ResetCardConsumeResponse::Rejected { error })
						},
						(
							CommandOutcome::AcceptanceUnknown,
							None,
							None,
							Some(CommandError::AcceptanceUnknown),
						) => Ok(ResetCardConsumeResponse::PotentiallyDispatched {
							failure: ClientFailure::ApplicationAcceptanceUnknown,
						}),
						_ => Err(ClientFailure::ProtocolMalformed),
					};

					return response.map(|value| CompletedOneShot::new(value, socket));
				},
				ServerMessage::Event(event) => {
					self.verify_version_and_server(event.version, &event.server_id)?
				},
				ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}

		Err(ClientFailure::ProtocolBackpressure)
	}

	async fn connect(&self) -> Result<OneShotSocket, ClientFailure> {
		let local_transport =
			self.profile.local_transport.as_ref().ok_or(ClientFailure::RemoteTransportDisabled)?;
		let config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(16 * 1_024)
			.max_write_buffer_size(MAX_CLIENT_MESSAGE_BYTES)
			.max_message_size(Some(MAX_CLIENT_MESSAGE_BYTES))
			.max_frame_size(Some(MAX_CLIENT_MESSAGE_BYTES));
		let stream = time::timeout(self.timeout, local_transport.connect())
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)?
			.map_err(map_local_transport_failure)?;
		let (mut socket, _) = time::timeout(
			self.timeout,
			tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, Some(config)),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)?
		.map_err(map_connect_error)?;

		self.send(
			&mut socket,
			ClientMessage::Hello(ClientHello {
				version: CURRENT_VERSION,
				artifact_cohort: Some(CURRENT_ARTIFACT_COHORT),
				expected_server_id: Some(self.profile.expected_server_id.clone()),
				resume: None,
			}),
		)
		.await?;

		let welcome = match self.receive(&mut socket).await? {
			ServerMessage::Welcome(welcome) => welcome,
			ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
			_ => return Err(ClientFailure::ProtocolMalformed),
		};

		if welcome.server_id != self.profile.expected_server_id {
			return Err(ClientFailure::ServerIdentityMismatch);
		}
		if welcome.version != CURRENT_VERSION {
			return Err(version_failure(welcome.version));
		}
		if welcome.supported != crate::SupportedVersions::current() {
			return Err(ClientFailure::ProtocolMinorMismatch);
		}
		if welcome.artifact_cohort != Some(CURRENT_ARTIFACT_COHORT) {
			return Err(ClientFailure::ArtifactCohortMismatch);
		}

		self.verify_version_and_server(welcome.version, &welcome.server_id)?;

		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.receive(&mut socket).await? {
				ServerMessage::Snapshot(snapshot) => {
					self.verify_version_and_server(snapshot.version, &snapshot.server_id)?;

					return Ok(socket);
				},
				ServerMessage::Event(event) => {
					self.verify_version_and_server(event.version, &event.server_id)?
				},
				ServerMessage::Refusal(refusal) => return Err(self.refusal_failure(refusal)),
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}

		Err(ClientFailure::ProtocolBackpressure)
	}

	async fn send<S>(&self, socket: &mut S, message: ClientMessage) -> Result<(), ClientFailure>
	where
		S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
	{
		let encoded = serde_json::to_string(&message)
			.expect("typed bounded client message serialization cannot fail");

		time::timeout(self.timeout, socket.send(Message::Text(encoded.into())))
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)?
			.map_err(|_| ClientFailure::ProtocolDisconnected)
	}

	async fn receive<S>(&self, socket: &mut S) -> Result<ServerMessage, ClientFailure>
	where
		S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
	{
		loop {
			let message = time::timeout(self.timeout, socket.next())
				.await
				.map_err(|_| ClientFailure::ProtocolTimeout)?
				.ok_or(ClientFailure::ProtocolDisconnected)?
				.map_err(map_receive_error)?;

			match message {
				Message::Text(text) => {
					return serde_json::from_str(&text)
						.map_err(|_| ClientFailure::ProtocolMalformed);
				},
				Message::Ping(_) | Message::Pong(_) => {},
				Message::Close(_) => return Err(ClientFailure::ProtocolDisconnected),
				Message::Binary(_) | Message::Frame(_) => {
					return Err(ClientFailure::ProtocolMalformed);
				},
			}
		}
	}

	fn verify_version_and_server(
		&self,
		version: ProtocolVersion,
		server_id: &ServerId,
	) -> Result<(), ClientFailure> {
		if server_id != &self.profile.expected_server_id {
			return Err(ClientFailure::ServerIdentityMismatch);
		}
		if version != CURRENT_VERSION {
			return Err(version_failure(version));
		}

		Ok(())
	}

	fn refusal_failure(&self, refusal: RefusalEnvelope) -> ClientFailure {
		if refusal.server_id != self.profile.expected_server_id {
			ClientFailure::ServerIdentityMismatch
		} else {
			map_refusal(refusal.refusal)
		}
	}
}

/// Read-only V2.10 client for bounded canonical WorkItem board pages.
pub struct WorkItemBoardClient {
	transport: ResetCardClient,
}
impl WorkItemBoardClient {
	/// Build a board client over one selected pinned server profile.
	pub const fn new(profile: ClientProfile) -> Self {
		Self { transport: ResetCardClient::new(profile) }
	}

	/// Selected client profile.
	pub const fn profile(&self) -> &ClientProfile {
		self.transport.profile()
	}

	/// Read one exact Project/filter/cursor page without mutation or execution authority.
	pub async fn page(
		&self,
		project_id: WorkItemBoardProjectId,
		state: Option<WorkItemState>,
		after: Option<WorkItemBoardWorkItemId>,
		page_size: WorkItemBoardPageSize,
	) -> Result<WorkItemBoardResult, ClientFailure> {
		let expected_project_id = project_id.clone();
		let expected_after = after.clone();
		let completed = time::timeout(
			CLIENT_TIMEOUT,
			self.transport.query_inner(
				"decodex-work-item-board-page",
				QueryPayload::GetWorkItemBoardPage { project_id, state, after, page_size },
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;

		match payload {
			QueryResultPayload::WorkItemBoard(result) => {
				if matches!(
					&result,
					WorkItemBoardResult::Page(page)
						if !page.matches_request(
							&expected_project_id,
							state,
							expected_after.as_ref(),
							page_size,
						)
				) {
					Err(ClientFailure::ProtocolMalformed)
				} else {
					Ok(result)
				}
			},
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}
}

/// Short-lived local client for the daemon-owned memory-only account-login service.
pub struct AccountLoginClient {
	transport: ResetCardClient,
}

impl AccountLoginClient {
	/// Build a client that never uses the retained-session or client-cache path.
	pub const fn new(profile: ClientProfile) -> Self {
		Self { transport: ResetCardClient::new(profile) }
	}

	/// Start or idempotently read one exact daemon-owned login session.
	pub async fn start(
		&self,
		start: AccountLoginStart,
	) -> Result<AccountLoginStatus, ClientFailure> {
		let expected_session_id = start.session_id.clone();
		self.exchange(
			"decodex-account-login-start",
			AccountLoginRequest::Start { start: Box::new(start) },
			&expected_session_id,
		)
		.await
	}

	/// Read one daemon-lifetime login status without retaining it in a client cache.
	pub async fn status(&self, session_id: EntityId) -> Result<AccountLoginStatus, ClientFailure> {
		let expected_session_id = session_id.clone();
		self.exchange(
			"decodex-account-login-status",
			AccountLoginRequest::Status { session_id },
			&expected_session_id,
		)
		.await
	}

	/// Cancel one session and wait for daemon-owned terminal cleanup.
	pub async fn cancel(&self, session_id: EntityId) -> Result<AccountLoginStatus, ClientFailure> {
		let expected_session_id = session_id.clone();
		self.exchange(
			"decodex-account-login-cancel",
			AccountLoginRequest::Cancel { session_id },
			&expected_session_id,
		)
		.await
	}

	async fn exchange(
		&self,
		request_identity: &'static str,
		request: AccountLoginRequest,
		expected_session_id: &EntityId,
	) -> Result<AccountLoginStatus, ClientFailure> {
		self.transport.require_local_profile()?;
		if request.validate().is_err() {
			return Err(ClientFailure::ProtocolMalformed);
		}
		let completed =
			time::timeout(self.transport.timeout, self.exchange_inner(request_identity, request))
				.await
				.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let status = completed.value.status;
		if status.session_id != *expected_session_id || status.validate().is_err() {
			return Err(ClientFailure::ProtocolMalformed);
		}
		Ok(status)
	}

	async fn exchange_inner(
		&self,
		request_identity: &'static str,
		request: AccountLoginRequest,
	) -> Result<CompletedOneShot<AccountLoginResponseEnvelope>, ClientFailure> {
		let mut socket = self.transport.connect().await?;
		let request_id = QueryId::new(request_identity)
			.expect("fixed account-login request identity is bounded and nonempty");
		self.transport
			.send(
				&mut socket,
				ClientMessage::AccountLogin(AccountLoginRequestEnvelope {
					version: CURRENT_VERSION,
					request_id: request_id.clone(),
					request,
				}),
			)
			.await?;

		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.transport.receive(&mut socket).await? {
				ServerMessage::AccountLogin(response) => {
					self.transport
						.verify_version_and_server(response.version, &response.server_id)?;
					if response.request_id != request_id || response.status.validate().is_err() {
						return Err(ClientFailure::ProtocolMalformed);
					}
					return Ok(CompletedOneShot::new(response, socket));
				},
				ServerMessage::Event(event) => {
					self.transport.verify_version_and_server(event.version, &event.server_id)?
				},
				ServerMessage::Refusal(refusal) => {
					return Err(self.transport.refusal_failure(refusal));
				},
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}
		Err(ClientFailure::ProtocolBackpressure)
	}
}

/// Verified response to one versioned daemon-owned account command.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum AccountCommandResponse {
	/// The exact logical command completed or replayed its durable public result.
	Applied {
		/// Entity revision committed by the command.
		entity_revision: EntityRevision,
		/// Typed exact command result.
		result: Box<ResultPayload>,
	},
	/// A typed deterministic guard rejected the logical command.
	Rejected {
		/// Stable deterministic rejection.
		error: CommandError,
	},
	/// A send was attempted, so local transport failure cannot prove non-dispatch.
	PotentiallyDispatched {
		/// Sanitized local transport failure.
		failure: ClientFailure,
	},
}

/// Same-UID V2.10 client for daemon-owned account queries and lifecycle commands.
pub struct AccountClient {
	transport: ResetCardClient,
}
impl AccountClient {
	/// Build a local account client with the bounded account-operation deadline.
	pub const fn new(profile: ClientProfile) -> Self {
		Self { transport: ResetCardClient::new(profile) }
	}

	/// Read the canonical account skeleton and routing controls.
	pub async fn list(&self) -> Result<AccountsResult, ClientFailure> {
		self.transport.require_local_profile()?;
		let completed = time::timeout(
			CLIENT_TIMEOUT,
			self.transport.query_inner("decodex-accounts-list", QueryPayload::ListAccounts),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::Accounts(result) => Ok(result),
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Inspect one exact account row without provider inventory work.
	pub async fn inspect(
		&self,
		account_id: EntityId,
	) -> Result<AccountInspectResult, ClientFailure> {
		self.transport.require_local_profile()?;
		let expected = account_id.clone();
		let completed = time::timeout(
			CLIENT_TIMEOUT,
			self.transport.query_inner(
				"decodex-account-inspect",
				QueryPayload::InspectAccount { account_id },
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::Account(result) => {
				if matches!(&result, AccountInspectResult::Available(account) if account.account_id != expected)
				{
					Err(ClientFailure::ProtocolMalformed)
				} else {
					Ok(result)
				}
			},
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Observe one bounded provider profile independently from Reset Card inventory.
	pub async fn profile(
		&self,
		account_id: EntityId,
		include_email: bool,
	) -> Result<AccountProfileResult, ClientFailure> {
		self.transport.require_local_profile()?;
		let expected = account_id.clone();
		let completed = time::timeout(
			CLIENT_TIMEOUT,
			self.transport.query_inner(
				"decodex-account-profile",
				QueryPayload::GetAccountProfile { account_id, include_email },
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::AccountProfile(result) => {
				let matches = match &result {
					AccountProfileResult::Current(profile)
					| AccountProfileResult::Cached { profile, .. } => {
						profile.account_id == expected
							&& (include_email
								|| matches!(profile.email, AccountProfileEmailDto::Redacted))
					},
					AccountProfileResult::Unavailable { email, .. } => {
						include_email || matches!(email, AccountProfileEmailDto::Redacted)
					},
				};
				if matches { Ok(result) } else { Err(ClientFailure::ProtocolMalformed) }
			},
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Evaluate fixed or balanced initial selection without creating work.
	pub async fn initial_selection(&self) -> Result<AccountInitialSelectionResult, ClientFailure> {
		self.transport.require_local_profile()?;
		let completed = time::timeout(
			self.transport.timeout,
			self.transport.query_inner(
				"decodex-account-initial-selection",
				QueryPayload::GetInitialAccountSelection,
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::InitialAccountSelection(result) => Ok(result),
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Read the normal shared Codex auth projection without exposing credentials.
	pub async fn codex_auth_projection(&self) -> Result<CodexAuthProjectionResult, ClientFailure> {
		self.transport.require_local_profile()?;
		let completed = time::timeout(
			CLIENT_TIMEOUT,
			self.transport
				.query_inner("decodex-codex-auth-projection", QueryPayload::GetCodexAuthProjection),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::CodexAuthProjection(result) => Ok(result),
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Wait until daemon-owned account observations advance or return one bounded heartbeat.
	pub async fn wait_for_observation(
		&self,
		after_generation: u64,
	) -> Result<AccountObservationSignal, ClientFailure> {
		self.wait_for_observation_inner(after_generation, false).await
	}

	/// Ask the daemon to coalesce one observation round, then wait for its semantic generation.
	pub async fn request_observation_refresh(
		&self,
		after_generation: u64,
	) -> Result<AccountObservationSignal, ClientFailure> {
		self.wait_for_observation_inner(after_generation, true).await
	}

	async fn wait_for_observation_inner(
		&self,
		after_generation: u64,
		request_refresh: bool,
	) -> Result<AccountObservationSignal, ClientFailure> {
		self.transport.require_local_profile()?;
		let completed = time::timeout(
			self.transport.timeout,
			self.transport.query_inner(
				"decodex-account-observation-wait",
				QueryPayload::WaitForAccountObservation {
					after_generation,
					request_refresh: request_refresh.then_some(true),
				},
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let payload = completed.value;
		match payload {
			QueryResultPayload::AccountObservation(signal) => Ok(signal),
			_ => Err(ClientFailure::ProtocolMalformed),
		}
	}

	/// Refresh, project, and select one account through one daemon-owned Route command.
	pub async fn route_account(
		&self,
		operation_id: EntityId,
		account_id: EntityId,
		expected_account_revision: EntityRevision,
		expected_routing_revision: EntityRevision,
		idempotency_key: IdempotencyKey,
	) -> Result<AccountCommandResponse, ClientFailure> {
		self.execute(
			CommandPayload::RouteAccount { operation_id, account_id, expected_account_revision },
			Some(expected_routing_revision),
			idempotency_key,
		)
		.await
	}

	/// Select balanced initial routing under one routing-control revision.
	pub async fn set_balanced_account_selection(
		&self,
		expected_routing_revision: EntityRevision,
		idempotency_key: IdempotencyKey,
	) -> Result<AccountCommandResponse, ClientFailure> {
		self.execute(
			CommandPayload::SetBalancedAccountSelection,
			Some(expected_routing_revision),
			idempotency_key,
		)
		.await
	}

	/// Replace the complete deterministic account order under one routing-control revision.
	pub async fn set_account_order(
		&self,
		order: Vec<EntityId>,
		expected_routing_revision: EntityRevision,
		idempotency_key: IdempotencyKey,
	) -> Result<AccountCommandResponse, ClientFailure> {
		self.execute(
			CommandPayload::SetAccountOrder { order },
			Some(expected_routing_revision),
			idempotency_key,
		)
		.await
	}

	/// Execute one exact-current lifecycle command exactly once on one connection.
	pub async fn execute(
		&self,
		payload: CommandPayload,
		expected_revision: Option<EntityRevision>,
		idempotency_key: IdempotencyKey,
	) -> Result<AccountCommandResponse, ClientFailure> {
		self.transport.require_local_profile()?;
		if !matches!(
			&payload,
			CommandPayload::EnrollAccountFromSharedCodex { .. }
				| CommandPayload::ImportAccountCredentialFile { .. }
				| CommandPayload::SetAccountEnabled { .. }
				| CommandPayload::LogoutAccount { .. }
				| CommandPayload::RouteAccount { .. }
				| CommandPayload::SetBalancedAccountSelection
				| CommandPayload::SetAccountOrder { .. }
				| CommandPayload::RefreshAccount { .. }
				| CommandPayload::RecoverAccountOperation { .. }
		) {
			return Err(ClientFailure::ProtocolMalformed);
		}
		let dispatch_attempted = AtomicBool::new(false);
		let result = time::timeout(
			self.transport.timeout,
			self.execute_inner(payload, expected_revision, idempotency_key, &dispatch_attempted),
		)
		.await;
		let result = match result {
			Ok(Ok(completed)) => {
				close_one_shot_socket(completed.socket).await;
				Ok(completed.value)
			},
			Ok(Err(failure)) => Err(failure),
			Err(_) => Err(ClientFailure::ProtocolTimeout),
		};
		match result {
			Ok(response) => Ok(response),
			Err(failure) if dispatch_attempted.load(Ordering::Acquire) => {
				Ok(AccountCommandResponse::PotentiallyDispatched { failure })
			},
			Err(failure) => Err(failure),
		}
	}

	async fn execute_inner(
		&self,
		payload: CommandPayload,
		expected_revision: Option<EntityRevision>,
		idempotency_key: IdempotencyKey,
		dispatch_attempted: &AtomicBool,
	) -> Result<CompletedOneShot<AccountCommandResponse>, ClientFailure> {
		let mut socket = self.transport.connect().await?;
		let client_command_id = ClientCommandId::new(idempotency_key.as_str().to_owned())
			.map_err(|_| ClientFailure::ProtocolMalformed)?;
		let correlation_id = CorrelationId::new(idempotency_key.as_str().to_owned())
			.map_err(|_| ClientFailure::ProtocolMalformed)?;
		let command = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: client_command_id.clone(),
			idempotency_key: idempotency_key.clone(),
			expected_revision,
			correlation_id,
			causation_id: None,
			payload: payload.clone(),
		});
		dispatch_attempted.store(true, Ordering::Release);
		self.transport.send(&mut socket, command).await?;
		let mut receipt_disposition = None;
		for _ in 0..MAX_INTERLEAVED_MESSAGES {
			match self.transport.receive(&mut socket).await? {
				ServerMessage::CommandReceipt(receipt) => {
					self.transport
						.verify_version_and_server(receipt.version, &receipt.server_id)?;
					if receipt_disposition.is_some()
						|| receipt.client_command_id != client_command_id
						|| receipt.idempotency_key != idempotency_key
						|| (receipt.disposition != ReceiptDisposition::Duplicate
							&& receipt.original_client_command_id != client_command_id)
					{
						return Err(ClientFailure::ProtocolMalformed);
					}
					receipt_disposition = Some(receipt.disposition);
				},
				ServerMessage::CommandResult(result) => {
					self.transport.verify_version_and_server(result.version, &result.server_id)?;
					let Some(disposition) = receipt_disposition else {
						return Err(ClientFailure::ProtocolMalformed);
					};
					if result.client_command_id != client_command_id
						|| result.idempotency_key != idempotency_key
						|| (disposition == ReceiptDisposition::Refused
							&& result.outcome != CommandOutcome::Rejected)
					{
						return Err(ClientFailure::ProtocolMalformed);
					}
					let response = match (
						result.outcome,
						result.entity_revision,
						result.payload,
						result.error,
					) {
						(CommandOutcome::Succeeded, Some(entity_revision), Some(result), None)
							if account_result_matches(&payload, entity_revision, &result) =>
						{
							Ok(AccountCommandResponse::Applied {
								entity_revision,
								result: Box::new(result),
							})
						},
						(CommandOutcome::Rejected, None, None, Some(error))
							if !matches!(error, CommandError::AcceptanceUnknown) =>
						{
							Ok(AccountCommandResponse::Rejected { error })
						},
						(
							CommandOutcome::AcceptanceUnknown,
							None,
							None,
							Some(CommandError::AcceptanceUnknown),
						) => Ok(AccountCommandResponse::PotentiallyDispatched {
							failure: ClientFailure::ApplicationAcceptanceUnknown,
						}),
						_ => Err(ClientFailure::ProtocolMalformed),
					};

					return response.map(|value| CompletedOneShot::new(value, socket));
				},
				ServerMessage::Event(event) => {
					self.transport.verify_version_and_server(event.version, &event.server_id)?
				},
				ServerMessage::Refusal(refusal) => {
					return Err(self.transport.refusal_failure(refusal));
				},
				_ => return Err(ClientFailure::ProtocolMalformed),
			}
		}
		Err(ClientFailure::ProtocolBackpressure)
	}
}

fn account_result_matches(
	command: &CommandPayload,
	entity_revision: EntityRevision,
	result: &ResultPayload,
) -> bool {
	if entity_revision.0 == 0 {
		return false;
	}
	match (command, result) {
		(
			CommandPayload::EnrollAccountFromSharedCodex { account_id, .. },
			ResultPayload::AccountChanged { account },
		)
		| (
			CommandPayload::ImportAccountCredentialFile { account_id, .. },
			ResultPayload::AccountChanged { account },
		)
		| (
			CommandPayload::SetAccountEnabled { account_id, .. },
			ResultPayload::AccountChanged { account },
		)
		| (
			CommandPayload::RefreshAccount { account_id, .. },
			ResultPayload::AccountChanged { account },
		) => account_id == &account.account_id && entity_revision == account.account_revision,
		(
			CommandPayload::EnrollAccountFromSharedCodex { account_id, .. }
			| CommandPayload::ImportAccountCredentialFile { account_id, .. },
			ResultPayload::AccountRestored { requested_account_id, account },
		) => {
			account_id == requested_account_id
				&& account.account_id != *requested_account_id
				&& entity_revision == account.account_revision
		},
		(
			CommandPayload::LogoutAccount { account_id, .. },
			ResultPayload::AccountLoggedOut { account_id: result_id, tombstone_revision },
		) => account_id == result_id && *tombstone_revision == entity_revision,
		(
			CommandPayload::RouteAccount { account_id, .. },
			ResultPayload::AccountRouted { account, routing, projection_digest: _ },
		) => {
			account.account_id == *account_id
				&& account.account_revision.0 > 0
				&& routing.revision == entity_revision
				&& routing.mode == AccountSelectionModeDto::Fixed(account_id.clone())
		},
		(
			CommandPayload::RouteAccount { operation_id, account_id, .. },
			ResultPayload::AccountRoutePending { pending },
		) => {
			pending.operation_id == *operation_id
				&& pending.account_id == *account_id
				&& pending.routing_revision == entity_revision
		},
		(
			CommandPayload::SetBalancedAccountSelection,
			ResultPayload::AccountRoutingChanged { routing },
		) => {
			routing.revision == entity_revision && routing.mode == AccountSelectionModeDto::Balanced
		},
		(
			CommandPayload::SetAccountOrder { order },
			ResultPayload::AccountRoutingChanged { routing },
		) => routing.revision == entity_revision && routing.order.as_slice() == order.as_slice(),
		(
			CommandPayload::RecoverAccountOperation { operation_id, .. },
			ResultPayload::AccountOperationRecovered { operation_id: result_id, .. },
		) => operation_id == result_id,
		_ => false,
	}
}

/// Closed client-side failures. External parser, socket, host, database, user,
/// and server-provided text cannot inhabit this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientFailure {
	/// The typed configuration file was absent.
	ConfigurationMissing,
	/// The client-visible configuration or profile was malformed.
	ConfigurationMalformed,
	/// The configuration schema version was unsupported.
	ConfigurationVersion,
	/// An explicitly selected profile was absent.
	ProfileMissing,
	/// The configuration root or private file boundary was unsafe.
	UnsafeHostPath,
	/// A local profile had neither a pin nor a readable stable host identity.
	ServerIdentityUnavailable,
	/// Reset-card operations are unavailable for remote profiles.
	RemoteMutationUnsupported,
	/// Durable local policy disables the local endpoint.
	LocalTransportDisabled,
	/// The selected remote transport remains outside this implementation.
	RemoteTransportDisabled,
	/// The platform has no accepted kernel peer-identity implementation.
	LocalTransportUnsupported,
	/// The local directory, lock, socket, or captured identity is unsafe.
	UnsafeLocalEndpoint,
	/// The kernel did not provide an unambiguous local peer identity.
	LocalPeerIdentityUnavailable,
	/// The process or connected peer does not match the configured service UID.
	LocalPeerUidMismatch,
	/// No usable daemon WebSocket connection was established or retained.
	ProtocolDisconnected,
	/// A bounded connection or response deadline elapsed.
	ProtocolTimeout,
	/// The server used a different protocol generation.
	ProtocolMajorMismatch,
	/// The server did not support the requested current minor.
	ProtocolMinorMismatch,
	/// The daemon and client artifacts are not from the same build/protocol cohort.
	ArtifactCohortMismatch,
	/// The server did not match the selected stable identity pin.
	ServerIdentityMismatch,
	/// A server response was not a valid expected typed envelope.
	ProtocolMalformed,
	/// The server refused message ordering or query availability.
	ProtocolViolation,
	/// The bounded message allowance was exhausted.
	ProtocolBackpressure,
	/// The daemon could not establish whether application acceptance committed.
	ApplicationAcceptanceUnknown,
}
impl Display for ClientFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::ConfigurationMissing => "configuration is missing",
			Self::ConfigurationMalformed => "configuration is malformed",
			Self::ConfigurationVersion => "configuration version is unsupported",
			Self::ProfileMissing => "selected profile is missing",
			Self::UnsafeHostPath => "client configuration path is unsafe",
			Self::ServerIdentityUnavailable => "stable server identity is unavailable",
			Self::RemoteMutationUnsupported => {
				"reset-card operations require a local pinned profile"
			},
			Self::LocalTransportDisabled => "local daemon transport is disabled",
			Self::RemoteTransportDisabled => "remote daemon transport is disabled",
			Self::LocalTransportUnsupported => {
				"local daemon transport is unsupported on this platform"
			},
			Self::UnsafeLocalEndpoint => "local daemon endpoint is unsafe",
			Self::LocalPeerIdentityUnavailable => "local daemon peer identity is unavailable",
			Self::LocalPeerUidMismatch => "local daemon peer UID does not match",
			Self::ProtocolDisconnected => "daemon protocol is disconnected",
			Self::ProtocolTimeout => "daemon protocol timed out",
			Self::ProtocolMajorMismatch => "daemon protocol major version does not match",
			Self::ProtocolMinorMismatch => "daemon protocol minor version is unsupported",
			Self::ArtifactCohortMismatch => "daemon and client artifact cohorts do not match",
			Self::ServerIdentityMismatch => "stable server identity does not match",
			Self::ProtocolMalformed => "daemon protocol response is malformed",
			Self::ProtocolViolation => "daemon refused the protocol operation",
			Self::ProtocolBackpressure => "daemon protocol backpressure limit was reached",
			Self::ApplicationAcceptanceUnknown => {
				"daemon could not establish whether application acceptance committed"
			},
		})
	}
}
impl std::error::Error for ClientFailure {}

fn server_id(identity: &ServerIdentity) -> Result<ServerId, ClientFailure> {
	ServerId::new(identity.as_str()).map_err(|_| ClientFailure::ServerIdentityUnavailable)
}

fn map_root_error(error: PathError) -> ClientFailure {
	match error {
		PathError::HomeUnavailable => ClientFailure::ConfigurationMissing,
		_ => ClientFailure::UnsafeHostPath,
	}
}

fn map_config_error(error: ConfigError) -> ClientFailure {
	match error {
		ConfigError::UnsupportedVersion => ClientFailure::ConfigurationVersion,
		ConfigError::MissingProfile => ClientFailure::ProfileMissing,
		ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. }) => {
			ClientFailure::ConfigurationMissing
		},
		ConfigError::Path(_) => ClientFailure::UnsafeHostPath,
		_ => ClientFailure::ConfigurationMalformed,
	}
}

fn map_identity_error(error: ConfigError) -> ClientFailure {
	match error {
		ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. })
		| ConfigError::InvalidServerIdentity => ClientFailure::ServerIdentityUnavailable,
		ConfigError::Path(_) => ClientFailure::UnsafeHostPath,
		_ => ClientFailure::ServerIdentityUnavailable,
	}
}

fn map_local_transport_failure(failure: LocalTransportRefusal) -> ClientFailure {
	match failure {
		LocalTransportRefusal::Disabled => ClientFailure::LocalTransportDisabled,
		LocalTransportRefusal::InvalidPolicy | LocalTransportRefusal::ConfigurationUnavailable => {
			ClientFailure::ConfigurationMalformed
		},
		LocalTransportRefusal::UnsupportedPlatform => ClientFailure::LocalTransportUnsupported,
		LocalTransportRefusal::EffectiveUidMismatch | LocalTransportRefusal::PeerUidMismatch => {
			ClientFailure::LocalPeerUidMismatch
		},
		LocalTransportRefusal::UnsafeDirectory
		| LocalTransportRefusal::UnsafeEndpoint
		| LocalTransportRefusal::EndpointReplaced => ClientFailure::UnsafeLocalEndpoint,
		LocalTransportRefusal::PeerCredentialsUnavailable => {
			ClientFailure::LocalPeerIdentityUnavailable
		},
		LocalTransportRefusal::EndpointUnavailable | LocalTransportRefusal::EndpointInUse => {
			ClientFailure::ProtocolDisconnected
		},
	}
}

fn map_connect_error(error: tokio_tungstenite::tungstenite::Error) -> ClientFailure {
	match error {
		tokio_tungstenite::tungstenite::Error::Capacity(_) => ClientFailure::ProtocolBackpressure,
		tokio_tungstenite::tungstenite::Error::Protocol(_)
		| tokio_tungstenite::tungstenite::Error::Utf8(_)
		| tokio_tungstenite::tungstenite::Error::Http(_) => ClientFailure::ProtocolMalformed,
		_ => ClientFailure::ProtocolDisconnected,
	}
}

fn map_receive_error(error: tokio_tungstenite::tungstenite::Error) -> ClientFailure {
	match error {
		tokio_tungstenite::tungstenite::Error::Capacity(_) => ClientFailure::ProtocolBackpressure,
		tokio_tungstenite::tungstenite::Error::Protocol(_)
		| tokio_tungstenite::tungstenite::Error::Utf8(_) => ClientFailure::ProtocolMalformed,
		_ => ClientFailure::ProtocolDisconnected,
	}
}

fn map_refusal(refusal: Refusal) -> ClientFailure {
	match refusal {
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. }) => {
			ClientFailure::ProtocolMajorMismatch
		},
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. }) => {
			ClientFailure::ProtocolMinorMismatch
		},
		Refusal::ArtifactCohortMismatch { .. } => ClientFailure::ArtifactCohortMismatch,
		Refusal::ServerIdentityMismatch { .. } => ClientFailure::ServerIdentityMismatch,
		Refusal::ProtocolViolation { .. } => ClientFailure::ProtocolViolation,
		Refusal::Backpressure { .. } => ClientFailure::ProtocolBackpressure,
	}
}

fn version_failure(version: ProtocolVersion) -> ClientFailure {
	if version.major != CURRENT_VERSION.major {
		ClientFailure::ProtocolMajorMismatch
	} else {
		ClientFailure::ProtocolMinorMismatch
	}
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
	#[cfg(unix)]
	use std::os::unix::fs::PermissionsExt as _;
	use std::{fs, time::Duration};

	use futures_util::{SinkExt as _, StreamExt as _};
	use tempfile::TempDir;
	use tokio::{task::JoinHandle, time};
	use tokio_tungstenite::{self, tungstenite::Message};

	use crate::{
		AccountClient, AccountCommandResponse, AccountProfileDto, AccountProfileEmailDto,
		AccountProfileErrorDto,
		AccountProfileResult, AccountRoutePendingDto, CURRENT_ARTIFACT_COHORT, CURRENT_VERSION,
		Channel, ClientCommandId,
		ClientFailure, ClientMessage, ClientProfile, CommandError, CommandOutcome, CommandPayload,
		CommandReceipt, CommandResultEnvelope, CorrelationId, Cursor, DoctorCheck, DoctorClient,
		DoctorComponent,
		DoctorIssue, DoctorReport, DoctorStatus, EntityId, EntityRevision, EventEnvelope,
		EventPayload, IdempotencyKey, LocalTransportAuthority, PREVIOUS_MINOR_VERSION, ProfileKind,
		ProtocolVersion, QueryId, QueryResultEnvelope, QueryResultPayload, ReceiptDisposition,
		ReconnectMode, Refusal, RefusalEnvelope, ResetCardClient, ResetCardConsumeResponse,
		ResetCardDescriptorDto, ResetCardOperationResult, ResultPayload, RetainedSessionFailure,
		ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope, SupportedVersions,
		VersionRefusal, WireText, WorkItemBoardClient, WorkItemBoardPage, WorkItemBoardPageSize,
		WorkItemBoardProjectId, WorkItemBoardResult, WorkItemState,
	};
	use decodex_core::{DecodexRoot, LocalTrustPolicy, ServerIdentity};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	fn typed(message: ServerMessage) -> Message {
		Message::Text(serde_json::to_string(&message).expect("test operation must succeed").into())
	}

	fn initial(server_id: &str) -> Vec<Message> {
		let server_id = ServerId::new(server_id).expect("test operation must succeed");

		vec![
			typed(ServerMessage::Welcome(ServerWelcome {
				version: CURRENT_VERSION,
				artifact_cohort: Some(CURRENT_ARTIFACT_COHORT),
				supported: SupportedVersions::current(),
				server_id: server_id.clone(),
				instance_id: None,
				cursor: Cursor(0),
				reconnect: ReconnectMode::Snapshot,
			})),
			typed(ServerMessage::Snapshot(SnapshotEnvelope {
				version: CURRENT_VERSION,
				server_id,
				cursor: Cursor(0),
				items: Vec::new(),
			})),
		]
	}

	fn report() -> DoctorReport {
		DoctorReport::new(
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.map(|component| {
					DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
				})
				.collect(),
		)
		.expect("test operation must succeed")
	}

	#[test]
	fn account_route_pending_is_a_typed_success_for_the_exact_route() {
		let operation_id =
			EntityId::new("30000000-0000-4000-8000-000000000001").expect("operation ID");
		let account_id =
			EntityId::new("40000000-0000-4000-8000-000000000001").expect("account ID");
		let command = CommandPayload::RouteAccount {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			expected_account_revision: EntityRevision(7),
		};
		let result = ResultPayload::AccountRoutePending {
			pending: AccountRoutePendingDto {
				operation_id,
				account_id,
				routing_revision: EntityRevision(9),
			},
		};
		assert!(super::account_result_matches(&command, EntityRevision(9), &result));
		assert!(!super::account_result_matches(&command, EntityRevision(10), &result));
	}

	#[tokio::test]
	async fn account_client_returns_pending_route_as_confirmed_applied_state() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test listener must bind");
		let operation_id =
			EntityId::new("30000000-0000-4000-8000-000000000001").expect("operation ID");
		let account_id =
			EntityId::new("40000000-0000-4000-8000-000000000001").expect("account ID");
		let expected_operation_id = operation_id.clone();
		let expected_account_id = account_id.clone();
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test connection must arrive");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test socket must upgrade");
			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("initial response must send");
			}
			let Message::Text(request) = socket
				.next()
				.await
				.expect("command must arrive")
				.expect("command must decode")
			else {
				panic!("expected text command")
			};
			let ClientMessage::Command(command) =
				serde_json::from_str::<ClientMessage>(&request).expect("typed command")
			else {
				panic!("expected command")
			};
			assert!(matches!(
				command.payload,
				CommandPayload::RouteAccount {
					ref operation_id,
					ref account_id,
					expected_account_revision: EntityRevision(7),
				} if operation_id == &expected_operation_id && account_id == &expected_account_id
			));
			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).unwrap(),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				})))
				.await
				.unwrap();
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).unwrap(),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(9)),
					payload: Some(ResultPayload::AccountRoutePending {
						pending: AccountRoutePendingDto {
							operation_id: expected_operation_id,
							account_id: expected_account_id,
							routing_revision: EntityRevision(9),
						},
					}),
					error: None,
				})))
				.await
				.unwrap();
			drop(socket);
			listener.cleanup().unwrap();
		});
		let response = AccountClient::new(ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).unwrap(),
		))
		.route_account(
			operation_id.clone(),
			account_id.clone(),
			EntityRevision(7),
			EntityRevision(9),
			IdempotencyKey::new("pending-route-key").unwrap(),
		)
		.await
		.unwrap();
		assert!(matches!(
			response,
			AccountCommandResponse::Applied {
				entity_revision: EntityRevision(9),
				result,
			} if matches!(
				&*result,
				ResultPayload::AccountRoutePending { pending }
					if pending.operation_id == operation_id && pending.account_id == account_id
			)
		));
		task.await.unwrap();
	}

	fn profile_result(account_id: &str, email: AccountProfileEmailDto) -> AccountProfileResult {
		AccountProfileResult::Current(Box::new(AccountProfileDto {
			account_id: EntityId::new(account_id).expect("fixture account identity is bounded"),
			account_revision: EntityRevision(3),
			observed_at_unix_micros: 1_700_000_000_000_000,
			email,
			plan_type: Some(WireText::new("pro").expect("fixture plan is bounded")),
			display_name: Some(WireText::new("Iris").expect("fixture name is bounded")),
			username: None,
			lifetime_tokens: Some(10_000),
			peak_daily_tokens: None,
			longest_task_seconds: None,
			current_streak_days: None,
			longest_streak_days: None,
			daily_usage: Vec::new(),
		}))
	}

	async fn account_profile_query(
		result: AccountProfileResult,
		include_email: bool,
	) -> Result<AccountProfileResult, ClientFailure> {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test listener must bind");
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test connection must arrive");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test socket must upgrade");
			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("initial response must send");
			}
			let request =
				socket.next().await.expect("query must arrive").expect("query must decode");
			let Message::Text(request) = request else { panic!("expected text query") };
			let ClientMessage::Query(query) =
				serde_json::from_str::<ClientMessage>(&request).expect("typed query must decode")
			else {
				panic!("expected typed query")
			};
			socket
				.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("fixture server ID is bounded"),
					query_id: query.query_id,
					payload: QueryResultPayload::AccountProfile(result),
				})))
				.await
				.expect("profile result must send");
			drop(socket);
			listener.cleanup().expect("test listener must clean up");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("fixture server ID is bounded"),
		);
		let response = AccountClient::new(profile)
			.profile(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("fixture account ID is bounded"),
				include_email,
			)
			.await;
		task.await.expect("test server must settle");
		response
	}

	fn result(report: DoctorReport) -> Message {
		typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			query_id: crate::QueryId::new("decodex-cli-doctor")
				.expect("test operation must succeed"),
			payload: QueryResultPayload::DoctorStatus(report),
		}))
	}

	fn event(version: ProtocolVersion, server_id: &str) -> Message {
		typed(ServerMessage::Event(EventEnvelope {
			version,
			server_id: ServerId::new(server_id).expect("test operation must succeed"),
			cursor: Cursor(1),
			channel: Channel::SystemHealth,
			entity_id: EntityId::new("system").expect("test operation must succeed"),
			entity_revision: EntityRevision(1),
			correlation_id: CorrelationId::new("doctor-correlation")
				.expect("test operation must succeed"),
			causation_id: None,
			payload: EventPayload::SystemObservationRefreshed {
				status: WireText::new("bounded").expect("test operation must succeed"),
			},
		}))
	}

	fn refusal(server_id: &str, refusal: Refusal) -> Message {
		typed(ServerMessage::Refusal(RefusalEnvelope {
			server_id: ServerId::new(server_id).expect("test operation must succeed"),
			refusal,
		}))
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
	fn active_local_profile_uses_stable_identity_and_remote_uses_only_profile_data() {
		let temp = TempDir::new().expect("test operation must succeed");
		let root = DecodexRoot::new(
			temp.path().canonicalize().expect("test operation must succeed").join(".decodex"),
		)
		.expect("test operation must succeed");
		let paths = root.paths();

		paths.ensure_layout().expect("test operation must succeed");

		let identity = ServerIdentity::load_or_create(&paths).expect("test operation must succeed");
		// SAFETY: `geteuid` has no arguments or failure return.
		let service_owner_uid = unsafe { libc::geteuid() };
		let config = format!(
			r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}

[profiles.remote]
kind = "remote"
host = "server.example.test"
port = 49152
expected_server_identity = "{SERVER_ID}"

[cache]
max_entries = 0
max_bytes = 0
max_entry_bytes = 0
"#,
		);

		fs::write(paths.config_file(), config).expect("test operation must succeed");

		#[cfg(unix)]
		{
			fs::set_permissions(paths.config_file(), std::fs::Permissions::from_mode(0o600))
				.expect("test operation must succeed");
		}

		let local = ClientProfile::load(root.as_path(), None).expect("test operation must succeed");
		let remote = ClientProfile::load(root.as_path(), Some("remote"))
			.expect("test operation must succeed");

		assert_eq!(local.name(), "local");
		assert_eq!(local.kind(), ProfileKind::Local);
		assert_eq!(local.expected_server_id().as_str(), identity.as_str());
		assert_eq!(
			local
				.retained_session_config()
				.expect("the selected local profile projects into a retained session")
				.expected_server_id()
				.as_str(),
			identity.as_str()
		);
		assert_eq!(remote.name(), "remote");
		assert_eq!(remote.kind(), ProfileKind::Remote);
		assert_eq!(remote.expected_server_id().as_str(), SERVER_ID);
		let stricter = remote.clone().with_expected_server_id(
			ServerId::new("retained-authority").expect("test operation must succeed"),
		);
		assert_eq!(stricter.name(), "remote");
		assert_eq!(stricter.expected_server_id().as_str(), "retained-authority");
		assert_eq!(
			remote.retained_session_config(),
			Err(RetainedSessionFailure::RemoteTransportDisabled),
		);
		assert!(!format!("{remote:?}").contains("server.example.test"));
	}

	#[test]
	fn protocol_constants_expose_only_the_exact_v2_10_window() {
		assert_eq!(CURRENT_VERSION, ProtocolVersion { major: 2, minor: 10 });
		assert_eq!(PREVIOUS_MINOR_VERSION, CURRENT_VERSION);
		assert_eq!(
			SupportedVersions::current(),
			SupportedVersions { major: 2, minimum_minor: 10, maximum_minor: 10 },
		);
		assert!(WireText::new("bounded").is_ok());
	}

	#[tokio::test]
	async fn work_item_board_client_rejects_an_exact_page_echo_mismatch() {
		let requested_project =
			WorkItemBoardProjectId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let echoed_project =
			WorkItemBoardProjectId::new("10000000-0000-4000-8000-000000000002").unwrap();
		let page_size = WorkItemBoardPageSize::new(2).unwrap();
		let page = WorkItemBoardPage::new(
			echoed_project,
			Some(WorkItemState::Planned),
			None,
			page_size,
			Vec::new(),
			None,
		)
		.expect("mismatched echo must remain internally valid");
		let response = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER_ID).unwrap(),
			query_id: QueryId::new("decodex-work-item-board-page").unwrap(),
			payload: QueryResultPayload::WorkItemBoard(WorkItemBoardResult::Page(page)),
		}));
		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![response]).await;

		assert_eq!(
			WorkItemBoardClient::new(profile)
				.page(requested_project, Some(WorkItemState::Planned), None, page_size,)
				.await,
			Err(ClientFailure::ProtocolMalformed),
		);
		task.await.expect("test server must settle");
	}

	#[tokio::test]
	async fn account_profile_client_rejects_identity_mismatch_and_default_email_leakage() {
		let mismatch = account_profile_query(
			profile_result(
				"40000000-0000-4000-8000-000000000002",
				AccountProfileEmailDto::Redacted,
			),
			false,
		)
		.await;
		assert_eq!(mismatch, Err(ClientFailure::ProtocolMalformed));

		let leaked = account_profile_query(
			profile_result(
				"40000000-0000-4000-8000-000000000001",
				AccountProfileEmailDto::Visible(
					WireText::new("iris@example.test").expect("fixture email is bounded"),
				),
			),
			false,
		)
		.await;
		assert_eq!(leaked, Err(ClientFailure::ProtocolMalformed));

		let unavailable_leak = account_profile_query(
			AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Visible(
					WireText::new("iris@example.test").expect("fixture email is bounded"),
				),
				plan_type: Some(WireText::new("pro").expect("fixture plan is bounded")),
			},
			false,
		)
		.await;
		assert_eq!(unavailable_leak, Err(ClientFailure::ProtocolMalformed));
	}

	#[tokio::test]
	async fn reset_card_client_rejects_every_remote_profile_before_connect_or_send() {
		let profile = ClientProfile {
			profile_name: "remote".into(),
			kind: ProfileKind::Remote,
			local_transport: None,
			expected_server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
		};
		let accounts = AccountClient::new(profile.clone());
		let client = ResetCardClient::new(profile);
		let account = EntityId::new("40000000-0000-4000-8000-000000000001")
			.expect("test operation must succeed");
		let key = IdempotencyKey::new("remote-reset-key").expect("test operation must succeed");
		let descriptor = ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed");

		assert_eq!(accounts.list().await.unwrap_err(), ClientFailure::RemoteMutationUnsupported);
		assert_eq!(
			client.list(account.clone()).await.unwrap_err(),
			ClientFailure::RemoteMutationUnsupported
		);
		assert_eq!(
			client.status(key.clone()).await.unwrap_err(),
			ClientFailure::RemoteMutationUnsupported
		);
		assert_eq!(
			client.consume(account, descriptor, EntityRevision(1), key).await.unwrap_err(),
			ClientFailure::RemoteMutationUnsupported
		);
	}

	async fn fixture(
		initial: Vec<Message>,
		query: Vec<Message>,
	) -> (ClientProfile, JoinHandle<()>, TempDir) {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let task = tokio::spawn(async move {
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");
			let hello = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(hello) = hello else { panic!("expected text hello") };
			let ClientMessage::Hello(hello) =
				serde_json::from_str(&hello).expect("test operation must succeed")
			else {
				panic!("expected typed hello")
			};

			assert_eq!(hello.version, CURRENT_VERSION);
			assert_eq!(
				hello.expected_server_id.expect("test operation must succeed").as_str(),
				SERVER_ID
			);

			for response in initial {
				// A fail-closed client can drop the stream while the server flushes
				// the response that caused the protocol failure.
				if socket.send(response).await.is_err() {
					break;
				}
			}

			if !query.is_empty() {
				let request = socket
					.next()
					.await
					.expect("test operation must succeed")
					.expect("test operation must succeed");
				let Message::Text(request) = request else { panic!("expected text query") };

				assert!(matches!(
					serde_json::from_str::<ClientMessage>(&request)
						.expect("test operation must succeed"),
					ClientMessage::Query(_)
				));

				for response in query {
					socket.send(response).await.expect("test operation must succeed");
				}
			}

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);

		(profile, task, temp)
	}

	#[test]
	fn doctor_timeout_is_bounded_and_does_not_widen_ordinary_queries() {
		let profile = ClientProfile {
			profile_name: "remote".into(),
			kind: ProfileKind::Remote,
			local_transport: None,
			expected_server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
		};
		let client = DoctorClient::new(profile);

		assert_eq!(client.timeout, Duration::from_secs(15));
		assert_eq!(super::CLIENT_TIMEOUT, Duration::from_secs(5));
	}

	#[tokio::test]
	async fn client_accepts_only_a_fully_verified_typed_report() {
		let expected = report();
		let (profile, task, _temp) =
			fixture(initial(SERVER_ID), vec![result(expected.clone())]).await;
		let actual = DoctorClient::new(profile).query().await.expect("test operation must succeed");

		assert_eq!(actual, expected);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn one_shot_client_closes_after_a_complete_response() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let expected = report();
		let response = expected.clone();
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			socket.next().await.expect("hello must arrive").expect("hello must decode");
			for message in initial(SERVER_ID) {
				socket.send(message).await.expect("initial response must send");
			}
			socket.next().await.expect("query must arrive").expect("query must decode");
			socket.send(result(response)).await.expect("query response must send");

			let close = time::timeout(Duration::from_secs(1), socket.next())
				.await
				.expect("client close must be bounded")
				.expect("client close must arrive")
				.expect("client close must decode");

			assert!(matches!(close, Message::Close(_)));

			socket.flush().await.expect("close response must flush");
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);

		assert_eq!(
			DoctorClient::new(profile).query().await.expect("test operation must succeed"),
			expected
		);

		task.await.expect("client lifecycle must settle before runtime shutdown");
	}

	#[tokio::test]
	async fn completed_command_survives_close_acknowledgement_past_operation_deadline() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let account_id = EntityId::new("40000000-0000-4000-8000-000000000001")
			.expect("test operation must succeed");
		let descriptor = ResetCardDescriptorDto::new(1_700_000_000, 1_700_003_600)
			.expect("test operation must succeed");
		let expected_account_id = account_id.clone();
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			socket.next().await.expect("hello must arrive").expect("hello must decode");
			for message in initial(SERVER_ID) {
				socket.send(message).await.expect("initial response must send");
			}
			let request =
				socket.next().await.expect("command must arrive").expect("command must decode");
			let Message::Text(request) = request else { panic!("expected text command") };
			let ClientMessage::Command(command) =
				serde_json::from_str::<ClientMessage>(&request).expect("command must be typed")
			else {
				panic!("expected typed command")
			};

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				})))
				.await
				.expect("command receipt must send");
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(7)),
					payload: Some(ResultPayload::ResetCardOperationAccepted {
						account_id: expected_account_id,
						descriptor,
						state: ResetCardOperationResult::Prepared,
					}),
					error: None,
				})))
				.await
				.expect("command result must send");

			let close = socket.next().await.expect("close must arrive").expect("close must decode");
			assert!(matches!(close, Message::Close(_)));
			time::sleep(Duration::from_millis(300)).await;
			socket.flush().await.expect("delayed close acknowledgement must flush");
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let client = ResetCardClient { profile, timeout: Duration::from_millis(200) };

		assert_eq!(
			client
				.consume(
					account_id.clone(),
					descriptor,
					EntityRevision(7),
					IdempotencyKey::new("delayed-close-key").expect("test operation must succeed"),
				)
				.await
				.expect("completed response must remain authoritative"),
			ResetCardConsumeResponse::Accepted {
				account_id,
				descriptor,
				state: ResetCardOperationResult::Prepared,
				entity_revision: EntityRevision(7),
			}
		);

		task.await.expect("client lifecycle must settle before runtime shutdown");
	}

	#[tokio::test]
	async fn client_rejects_every_incomplete_current_report_but_accepts_arbitrary_order() {
		let complete = report();
		let incomplete = [
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				Vec::new(),
			)
			.expect("test operation must succeed"),
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				vec![DoctorCheck::new(DoctorComponent::Configuration, DoctorStatus::Ready)],
			)
			.expect("test operation must succeed"),
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				complete.checks()[..complete.checks().len() - 1].to_vec(),
			)
			.expect("test operation must succeed"),
		];

		for report in incomplete {
			let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![result(report)]).await;

			assert_eq!(
				DoctorClient::new(profile).query().await.unwrap_err(),
				ClientFailure::ProtocolMalformed,
			);

			task.await.expect("test operation must succeed");
		}

		let mut reversed = complete.checks().to_vec();

		reversed.reverse();

		let reversed = DoctorReport::new(
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
			CURRENT_VERSION,
			reversed,
		)
		.expect("test operation must succeed");
		let (profile, task, _temp) =
			fixture(initial(SERVER_ID), vec![result(reversed.clone())]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.expect("test operation must succeed"),
			reversed
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn major_minor_and_server_refusals_remain_distinct() {
		let cases = [
			(
				Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch {
					requested: ProtocolVersion { major: 1, minor: 5 },
					supported: SupportedVersions::current(),
				}),
				ClientFailure::ProtocolMajorMismatch,
			),
			(
				Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor {
					requested: ProtocolVersion { major: 2, minor: 0 },
					supported: SupportedVersions::current(),
				}),
				ClientFailure::ProtocolMinorMismatch,
			),
			(
				Refusal::ArtifactCohortMismatch { expected: CURRENT_ARTIFACT_COHORT, actual: None },
				ClientFailure::ArtifactCohortMismatch,
			),
			(
				Refusal::ServerIdentityMismatch {
					expected: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					actual: ServerId::new("wrong-server").expect("test operation must succeed"),
				},
				ClientFailure::ServerIdentityMismatch,
			),
		];

		for (refusal, expected) in cases {
			let response = typed(ServerMessage::Refusal(RefusalEnvelope {
				server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
				refusal,
			}));
			let (profile, task, _temp) = fixture(vec![response], Vec::new()).await;

			assert_eq!(DoctorClient::new(profile).query().await.unwrap_err(), expected);

			task.await.expect("test operation must succeed");
		}
	}

	#[tokio::test]
	async fn every_refusal_phase_verifies_envelope_identity_before_classification() {
		let wrong_version = refusal(
			"wrong-server",
			Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor {
				requested: ProtocolVersion { major: 2, minor: 0 },
				supported: SupportedVersions::current(),
			}),
		);
		let (profile, task, _temp) = fixture(vec![wrong_version], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let mut wrong_protocol = initial(SERVER_ID);

		wrong_protocol[1] = refusal(
			"wrong-server",
			Refusal::ProtocolViolation {
				message: WireText::new("untrusted-order").expect("test operation must succeed"),
			},
		);

		let (profile, task, _temp) = fixture(wrong_protocol, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_backpressure =
			refusal("wrong-server", Refusal::Backpressure { queue_capacity: 1 });
		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![wrong_backpressure]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn every_envelope_identity_and_version_is_verified_before_status() {
		let mut wrong_welcome = initial("wrong-server");

		wrong_welcome.truncate(1);

		let (profile, task, _temp) = fixture(wrong_welcome, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_welcome_version = typed(ServerMessage::Welcome(ServerWelcome {
			version: ProtocolVersion { major: 2, minor: 0 },
			artifact_cohort: Some(CURRENT_ARTIFACT_COHORT),
			supported: SupportedVersions::current(),
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			instance_id: None,
			cursor: Cursor(0),
			reconnect: ReconnectMode::Snapshot,
		}));
		let (profile, task, _temp) = fixture(vec![wrong_welcome_version], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.expect("test operation must succeed");

		let missing_cohort = typed(ServerMessage::Welcome(ServerWelcome {
			version: CURRENT_VERSION,
			artifact_cohort: None,
			supported: SupportedVersions::current(),
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			instance_id: None,
			cursor: Cursor(0),
			reconnect: ReconnectMode::Snapshot,
		}));
		let (profile, task, _temp) = fixture(vec![missing_cohort], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ArtifactCohortMismatch,
		);

		task.await.expect("test operation must succeed");

		let widened_welcome = typed(ServerMessage::Welcome(ServerWelcome {
			version: CURRENT_VERSION,
			artifact_cohort: Some(CURRENT_ARTIFACT_COHORT),
			supported: SupportedVersions { major: 2, minimum_minor: 0, maximum_minor: 2 },
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			instance_id: None,
			cursor: Cursor(0),
			reconnect: ReconnectMode::Snapshot,
		}));
		let (profile, task, _temp) = fixture(vec![widened_welcome], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.expect("test operation must succeed");

		let mut wrong_snapshot = initial(SERVER_ID);

		wrong_snapshot[1] = typed(ServerMessage::Snapshot(SnapshotEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new("wrong-server").expect("test operation must succeed"),
			cursor: Cursor(0),
			items: Vec::new(),
		}));

		let (profile, task, _temp) = fixture(wrong_snapshot, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_result_identity = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new("wrong-server").expect("test operation must succeed"),
			query_id: QueryId::new("decodex-cli-doctor").expect("test operation must succeed"),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![wrong_result_identity]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_report_identity = DoctorReport::new(
			ServerId::new("wrong-server").expect("test operation must succeed"),
			CURRENT_VERSION,
			report().checks().to_vec(),
		)
		.expect("test operation must succeed");
		let (profile, task, _temp) =
			fixture(initial(SERVER_ID), vec![result(wrong_report_identity)]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.expect("test operation must succeed");

		let mut wrong_report = report();
		let encoded = serde_json::to_value(&wrong_report).expect("test operation must succeed");
		let mut encoded = encoded.as_object().expect("test operation must succeed").clone();

		encoded.insert("version".into(), serde_json::json!({"major": 2, "minor": 0}));

		wrong_report = serde_json::from_value(encoded.into()).expect("test operation must succeed");

		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![result(wrong_report)]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn snapshot_result_and_query_identity_fail_closed_before_report_acceptance() {
		let mut wrong_snapshot_version = initial(SERVER_ID);

		wrong_snapshot_version[1] = typed(ServerMessage::Snapshot(SnapshotEnvelope {
			version: ProtocolVersion { major: 2, minor: 0 },
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			cursor: Cursor(0),
			items: Vec::new(),
		}));

		let (profile, task, _temp) = fixture(wrong_snapshot_version, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_result_version = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: ProtocolVersion { major: 2, minor: 0 },
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			query_id: QueryId::new("decodex-cli-doctor").expect("test operation must succeed"),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![wrong_result_version]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.expect("test operation must succeed");

		let wrong_query_id = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
			query_id: QueryId::new("wrong-query").expect("test operation must succeed"),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![wrong_query_id]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMalformed,
		);

		task.await.expect("test operation must succeed");

		let mut wrong_order = initial(SERVER_ID);

		wrong_order[1] = result(report());

		let (profile, task, _temp) = fixture(wrong_order, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMalformed,
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn interleaved_events_verify_version_and_identity_and_preserve_valid_order() {
		for (event, expected) in [
			(
				event(ProtocolVersion { major: 2, minor: 0 }, SERVER_ID),
				ClientFailure::ProtocolMinorMismatch,
			),
			(event(CURRENT_VERSION, "wrong-server"), ClientFailure::ServerIdentityMismatch),
		] {
			let (profile, task, _temp) = fixture(initial(SERVER_ID), vec![event]).await;

			assert_eq!(DoctorClient::new(profile).query().await.unwrap_err(), expected);

			task.await.expect("test operation must succeed");
		}

		let expected = report();
		let responses = vec![event(CURRENT_VERSION, SERVER_ID), result(expected.clone())];
		let (profile, task, _temp) = fixture(initial(SERVER_ID), responses).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.expect("test operation must succeed"),
			expected
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_sends_once_and_verifies_receipt_and_result() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let descriptor = ResetCardDescriptorDto::new(1_700_000_000, 1_700_003_600)
			.expect("test operation must succeed");
		let expected_descriptor = descriptor;
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");
			let hello = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(hello) = hello else { panic!("expected text hello") };

			assert!(matches!(
				serde_json::from_str::<ClientMessage>(&hello).expect("test operation must succeed"),
				ClientMessage::Hello(_)
			));

			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("test operation must succeed");
			}

			let request = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(request) = request else { panic!("expected text command") };
			let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&request)
				.expect("test operation must succeed")
			else {
				panic!("expected typed command")
			};
			let client_command_id = ClientCommandId::new("reset-card-use:operator-key")
				.expect("test operation must succeed");
			let idempotency_key =
				IdempotencyKey::new("operator-key").expect("test operation must succeed");

			assert_eq!(command.client_command_id, client_command_id);
			assert_eq!(command.idempotency_key, idempotency_key);
			assert_eq!(command.expected_revision, Some(EntityRevision(7)));

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: client_command_id.clone(),
					idempotency_key: idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: client_command_id.clone(),
				})))
				.await
				.expect("test operation must succeed");
			socket
				.send(event(CURRENT_VERSION, SERVER_ID))
				.await
				.expect("test operation must succeed");
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id,
					idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(7)),
					payload: Some(ResultPayload::ResetCardOperationAccepted {
						account_id: EntityId::new("40000000-0000-4000-8000-000000000001")
							.expect("test operation must succeed"),
						descriptor: expected_descriptor,
						state: ResetCardOperationResult::Prepared,
					}),
					error: None,
				})))
				.await
				.expect("test operation must succeed");

			if let Ok(Some(Ok(Message::Text(message)))) =
				time::timeout(Duration::from_millis(50), socket.next()).await
			{
				assert!(
					!matches!(
						serde_json::from_str::<ClientMessage>(&message),
						Ok(ClientMessage::Command(_))
					),
					"consume was retried automatically",
				);
			}

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let response = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				descriptor,
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.expect("test operation must succeed");

		assert_eq!(
			response,
			ResetCardConsumeResponse::Accepted {
				account_id: EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				descriptor,
				state: ResetCardOperationResult::Prepared,
				entity_revision: EntityRevision(7),
			}
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_preserves_key_when_application_acceptance_is_unknown() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("test operation must succeed");
			}
			let request = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(request) = request else { panic!("expected text command") };
			let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&request)
				.expect("test operation must succeed")
			else {
				panic!("expected typed command")
			};

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				})))
				.await
				.expect("test operation must succeed");
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::AcceptanceUnknown,
					entity_revision: None,
					payload: None,
					error: Some(CommandError::AcceptanceUnknown),
				})))
				.await
				.expect("test operation must succeed");

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let response = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed"),
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.expect("test operation must succeed");

		assert_eq!(
			response,
			ResetCardConsumeResponse::PotentiallyDispatched {
				failure: ClientFailure::ApplicationAcceptanceUnknown,
			},
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_rejects_a_mismatched_receipt_key() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("test operation must succeed");
			}
			let _ = socket.next().await;

			let client_command_id = ClientCommandId::new("reset-card-use:operator-key")
				.expect("test operation must succeed");

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: client_command_id.clone(),
					idempotency_key: IdempotencyKey::new("wrong-key")
						.expect("test operation must succeed"),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: client_command_id,
				})))
				.await
				.expect("test operation must succeed");

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let response = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed"),
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.expect("test operation must succeed");

		assert_eq!(
			response,
			ResetCardConsumeResponse::PotentiallyDispatched {
				failure: ClientFailure::ProtocolMalformed
			}
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_rejects_success_after_a_refused_receipt() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let descriptor = ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed");
		let result_descriptor = descriptor;
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("test operation must succeed");
			}
			let request = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(request) = request else { panic!("expected text command") };
			let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&request)
				.expect("test operation must succeed")
			else {
				panic!("expected typed command")
			};

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Refused,
					original_client_command_id: command.client_command_id.clone(),
				})))
				.await
				.expect("test operation must succeed");
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(7)),
					payload: Some(ResultPayload::ResetCardOperationAccepted {
						account_id: EntityId::new("40000000-0000-4000-8000-000000000001")
							.expect("test operation must succeed"),
						descriptor: result_descriptor,
						state: ResetCardOperationResult::Prepared,
					}),
					error: None,
				})))
				.await
				.expect("test operation must succeed");

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let response = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				descriptor,
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.expect("test operation must succeed");

		assert_eq!(
			response,
			ResetCardConsumeResponse::PotentiallyDispatched {
				failure: ClientFailure::ProtocolMalformed
			}
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_rejects_a_mismatched_returned_revision() {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let descriptor = ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed");
		let result_descriptor = descriptor;
		let task = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");

			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("test operation must succeed");
			}
			let request = socket
				.next()
				.await
				.expect("test operation must succeed")
				.expect("test operation must succeed");
			let Message::Text(request) = request else { panic!("expected text command") };
			let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&request)
				.expect("test operation must succeed")
			else {
				panic!("expected typed command")
			};

			socket
				.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				})))
				.await
				.expect("test operation must succeed");
			socket
				.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("test operation must succeed"),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(8)),
					payload: Some(ResultPayload::ResetCardOperationAccepted {
						account_id: EntityId::new("40000000-0000-4000-8000-000000000001")
							.expect("test operation must succeed"),
						descriptor: result_descriptor,
						state: ResetCardOperationResult::Prepared,
					}),
					error: None,
				})))
				.await
				.expect("test operation must succeed");

			drop(socket);
			listener.cleanup().expect("test operation must succeed");
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let response = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				descriptor,
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.expect("test operation must succeed");

		assert_eq!(
			response,
			ResetCardConsumeResponse::PotentiallyDispatched {
				failure: ClientFailure::ProtocolMalformed
			}
		);

		task.await.expect("test operation must succeed");
	}

	#[tokio::test]
	async fn reset_card_consume_error_before_send_guarantees_no_dispatch() {
		let (_temp, authority) = local_transport();
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let failure = ResetCardClient::new(profile)
			.consume(
				EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed"),
				EntityRevision(7),
				IdempotencyKey::new("operator-key").expect("test operation must succeed"),
			)
			.await
			.unwrap_err();

		assert_eq!(failure, ClientFailure::ProtocolDisconnected);
	}

	#[tokio::test]
	async fn disconnected_malformed_oversized_and_timeout_fail_closed() {
		let (_temp, authority) = local_transport();
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolDisconnected,
		);

		let (profile, task, _temp) =
			fixture(vec![Message::Text("untrusted-parser-marker".into())], Vec::new()).await;
		let error = DoctorClient::new(profile).query().await.unwrap_err();

		assert_eq!(error, ClientFailure::ProtocolMalformed);
		assert!(!format!("{error:?} {error}").contains("untrusted-parser-marker"));

		task.await.expect("test operation must succeed");

		let oversized = Message::Text("x".repeat(super::MAX_CLIENT_MESSAGE_BYTES + 1).into());
		let (profile, task, _temp) = fixture(vec![oversized], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolBackpressure,
		);

		task.await.expect("test operation must succeed");

		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("test operation must succeed");
		let task = tokio::spawn(async move {
			let stream = listener.accept().await.expect("test operation must succeed");
			let mut socket =
				tokio_tungstenite::accept_async(stream).await.expect("test operation must succeed");
			let _ = socket.next().await;

			time::sleep(Duration::from_secs(1)).await;
		});
		let profile = ClientProfile::fixture(
			authority,
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
		);
		let client = DoctorClient { profile, timeout: Duration::from_millis(20) };

		assert_eq!(client.query().await.unwrap_err(), ClientFailure::ProtocolTimeout);

		task.abort();
		let _ = task.await;
		drop(temp);
	}
}
