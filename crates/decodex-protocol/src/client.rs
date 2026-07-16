//! Bounded API-only client transport and profile projection.

use std::{
	fmt::{Debug, Display, Formatter},
	io::ErrorKind,
	net::IpAddr,
	path::Path,
	time::Duration,
};

use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use tokio::time;
use tokio_tungstenite::{
	self,
	tungstenite::{Message, protocol::WebSocketConfig},
};

use crate::{
	CURRENT_VERSION, ClientHello, ClientMessage, DoctorReport, ProtocolVersion, QueryEnvelope,
	QueryId, QueryPayload, QueryResultPayload, Refusal, RefusalEnvelope, RetainedSessionConfig,
	RetainedSessionFailure, ServerId, ServerMessage, VersionRefusal,
};
use decodex_core::{
	ConfigError, DecodexClientConfig, DecodexRoot, PathError, ServerIdentity, ServerProfile,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CLIENT_MESSAGE_BYTES: usize = 256 * 1_024;
const MAX_INTERLEAVED_MESSAGES: usize = 64;
const WS_PATH: &str = "/v1/ws";

/// Whether one selected client profile targets the same host or a different host.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
	/// Loopback service on the client host.
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
	kind: ProfileKind,
	url: String,
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
		let (_, profile) = config.selected_profile(selected).map_err(map_config_error)?;

		match profile {
			ServerProfile::Local(profile) => {
				let expected = match profile.expected_server_identity() {
					Some(identity) => identity.clone(),
					None => ServerIdentity::load(&paths).map_err(map_identity_error)?,
				};

				Ok(Self {
					kind: ProfileKind::Local,
					url: format!("ws://{}{WS_PATH}", profile.address()),
					expected_server_id: server_id(&expected)?,
				})
			},
			ServerProfile::Remote(profile) => {
				let host =
					if profile.host().parse::<IpAddr>().is_ok_and(|address| address.is_ipv6()) {
						format!("[{}]", profile.host())
					} else {
						profile.host().into()
					};

				Ok(Self {
					kind: ProfileKind::Remote,
					url: format!("ws://{host}:{}{WS_PATH}", profile.port()),
					expected_server_id: server_id(profile.expected_server_identity())?,
				})
			},
		}
	}

	/// Local or remote profile classification.
	pub const fn kind(&self) -> ProfileKind {
		self.kind
	}

	/// Project this selected typed profile into the retained-session boundary.
	///
	/// Remote profiles remain fail-closed while retained sessions are loopback-only.
	pub fn retained_session_config(&self) -> Result<RetainedSessionConfig, RetainedSessionFailure> {
		RetainedSessionConfig::new(&self.url, self.expected_server_id.clone())
	}

	#[cfg(test)]
	fn fixture(url: String, expected_server_id: ServerId) -> Self {
		Self { kind: ProfileKind::Local, url, expected_server_id }
	}
}

impl Debug for ClientProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ClientProfile")
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
		Self { profile, timeout: CLIENT_TIMEOUT }
	}

	/// Selected client profile.
	pub const fn profile(&self) -> &ClientProfile {
		&self.profile
	}

	/// Negotiate the current protocol, verify the stable server identity, and
	/// return one fresh authoritative doctor report.
	pub async fn query(&self) -> Result<DoctorReport, ClientFailure> {
		time::timeout(self.timeout, self.query_inner())
			.await
			.map_err(|_| ClientFailure::ProtocolTimeout)?
	}

	async fn query_inner(&self) -> Result<DoctorReport, ClientFailure> {
		let config = WebSocketConfig::default()
			.read_buffer_size(16 * 1_024)
			.write_buffer_size(16 * 1_024)
			.max_write_buffer_size(MAX_CLIENT_MESSAGE_BYTES)
			.max_message_size(Some(MAX_CLIENT_MESSAGE_BYTES))
			.max_frame_size(Some(MAX_CLIENT_MESSAGE_BYTES));
		let (mut socket, _) = time::timeout(
			self.timeout,
			tokio_tungstenite::connect_async_with_config(&self.profile.url, Some(config), false),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)?
		.map_err(map_connect_error)?;
		let hello = ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
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
			|| welcome.supported.major != CURRENT_VERSION.major
			|| !(welcome.supported.minimum_minor..=welcome.supported.maximum_minor)
				.contains(&CURRENT_VERSION.minor)
		{
			return Err(version_failure(welcome.version));
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

					return Ok(report);
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
	/// No usable daemon WebSocket connection was established or retained.
	ProtocolDisconnected,
	/// A bounded connection or response deadline elapsed.
	ProtocolTimeout,
	/// The server used a different protocol generation.
	ProtocolMajorMismatch,
	/// The server did not support the requested current minor.
	ProtocolMinorMismatch,
	/// The server did not match the selected stable identity pin.
	ServerIdentityMismatch,
	/// A server response was not a valid expected typed envelope.
	ProtocolMalformed,
	/// The server refused message ordering or query availability.
	ProtocolViolation,
	/// The bounded message allowance was exhausted.
	ProtocolBackpressure,
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
			Self::ProtocolDisconnected => "daemon protocol is disconnected",
			Self::ProtocolTimeout => "daemon protocol timed out",
			Self::ProtocolMajorMismatch => "daemon protocol major version does not match",
			Self::ProtocolMinorMismatch => "daemon protocol minor version is unsupported",
			Self::ServerIdentityMismatch => "stable server identity does not match",
			Self::ProtocolMalformed => "daemon protocol response is malformed",
			Self::ProtocolViolation => "daemon refused the protocol operation",
			Self::ProtocolBackpressure => "daemon protocol backpressure limit was reached",
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
		ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. }) =>
			ClientFailure::ConfigurationMissing,
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
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. }) =>
			ClientFailure::ProtocolMajorMismatch,
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. }) =>
			ClientFailure::ProtocolMinorMismatch,
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

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
	use std::{fs, net::Ipv4Addr, time::Duration};

	use futures_util::{SinkExt as _, StreamExt as _};
	use tempfile::TempDir;
	use tokio::{net::TcpListener, task::JoinHandle, time};
	use tokio_tungstenite::{self, tungstenite::Message};

	use crate::{
		CURRENT_VERSION, Channel, ClientFailure, ClientMessage, ClientProfile, CorrelationId,
		Cursor, DoctorCheck, DoctorClient, DoctorComponent, DoctorIssue, DoctorReport,
		DoctorStatus, EntityId, EntityRevision, EventEnvelope, EventPayload,
		PREVIOUS_MINOR_VERSION, ProfileKind, ProtocolVersion, QueryId, QueryResultEnvelope,
		QueryResultPayload, ReconnectMode, Refusal, RefusalEnvelope, RetainedSessionFailure,
		ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope, SupportedVersions,
		VersionRefusal, WireText,
	};
	use decodex_core::{DecodexRoot, ServerIdentity};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	fn typed(message: ServerMessage) -> Message {
		Message::Text(serde_json::to_string(&message).unwrap().into())
	}

	fn initial(server_id: &str) -> Vec<Message> {
		let server_id = ServerId::new(server_id).unwrap();

		vec![
			typed(ServerMessage::Welcome(ServerWelcome {
				version: CURRENT_VERSION,
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
			ServerId::new(SERVER_ID).unwrap(),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.zip(DoctorIssue::ALL)
				.map(|(component, issue)| {
					DoctorCheck::new(component, DoctorStatus::Unavailable(issue))
				})
				.collect(),
		)
		.unwrap()
	}

	fn result(report: DoctorReport) -> Message {
		typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER_ID).unwrap(),
			query_id: crate::QueryId::new("decodex-cli-doctor").unwrap(),
			payload: QueryResultPayload::DoctorStatus(report),
		}))
	}

	fn event(version: ProtocolVersion, server_id: &str) -> Message {
		typed(ServerMessage::Event(EventEnvelope {
			version,
			server_id: ServerId::new(server_id).unwrap(),
			cursor: Cursor(1),
			channel: Channel::SystemHealth,
			entity_id: EntityId::new("system").unwrap(),
			entity_revision: EntityRevision(1),
			correlation_id: CorrelationId::new("doctor-correlation").unwrap(),
			causation_id: None,
			payload: EventPayload::SystemObservationRefreshed {
				status: WireText::new("bounded").unwrap(),
			},
		}))
	}

	fn refusal(server_id: &str, refusal: Refusal) -> Message {
		typed(ServerMessage::Refusal(RefusalEnvelope {
			server_id: ServerId::new(server_id).unwrap(),
			refusal,
		}))
	}

	#[test]
	fn active_local_profile_uses_stable_identity_and_remote_uses_only_profile_data() {
		let temp = TempDir::new().unwrap();
		let root = DecodexRoot::new(temp.path().canonicalize().unwrap().join(".decodex")).unwrap();
		let paths = root.paths();

		paths.ensure_layout().unwrap();

		let identity = ServerIdentity::load_or_create(&paths).unwrap();
		let config = format!(
			r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
address = "127.0.0.1:49152"

[profiles.remote]
kind = "remote"
host = "server.example.test"
port = 49152
expected_server_identity = "{SERVER_ID}"

[server_host.repositories.fixture]
host_path = "../must-not-be-client-validated"

[postgres]
socket_directory = "../must-not-be-client-validated"
expected_peer_uid = 70
database = "ignored"

[postgres.migration]
user = "ignored_migration"

[postgres.runtime]
user = "ignored_runtime"

[cache]
max_entries = 0
max_bytes = 0
max_entry_bytes = 0
"#,
		);

		fs::write(paths.config_file(), config).unwrap();

		#[cfg(unix)]
		{
			fs::set_permissions(paths.config_file(), std::fs::Permissions::from_mode(0o600))
				.unwrap();
		}

		let local = ClientProfile::load(root.as_path(), None).unwrap();
		let remote = ClientProfile::load(root.as_path(), Some("remote")).unwrap();

		assert_eq!(local.kind(), ProfileKind::Local);
		assert_eq!(local.expected_server_id.as_str(), identity.as_str());
		assert_eq!(
			local
				.retained_session_config()
				.expect("the selected local profile projects into a retained session")
				.expected_server_id()
				.as_str(),
			identity.as_str()
		);
		assert_eq!(remote.kind(), ProfileKind::Remote);
		assert_eq!(remote.expected_server_id.as_str(), SERVER_ID);
		assert_eq!(remote.retained_session_config(), Err(RetainedSessionFailure::InvalidEndpoint));
		assert!(!remote.url.contains("must-not-be-client-validated"));
		assert!(!format!("{remote:?}").contains("server.example.test"));
	}

	#[test]
	fn protocol_constants_retain_the_v1_2_v1_1_window() {
		assert_eq!(CURRENT_VERSION, ProtocolVersion { major: 1, minor: 2 });
		assert_eq!(PREVIOUS_MINOR_VERSION, ProtocolVersion { major: 1, minor: 1 });
		assert!(WireText::new("bounded").is_ok());
	}

	async fn fixture(
		initial: Vec<Message>,
		query: Vec<Message>,
	) -> (ClientProfile, JoinHandle<()>) {
		let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
		let address = listener.local_addr().unwrap();
		let task = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let hello = socket.next().await.unwrap().unwrap();
			let Message::Text(hello) = hello else { panic!("expected text hello") };
			let ClientMessage::Hello(hello) = serde_json::from_str(&hello).unwrap() else {
				panic!("expected typed hello")
			};

			assert_eq!(hello.version, CURRENT_VERSION);
			assert_eq!(hello.expected_server_id.unwrap().as_str(), SERVER_ID);

			for response in initial {
				socket.send(response).await.unwrap();
			}

			if !query.is_empty() {
				let request = socket.next().await.unwrap().unwrap();
				let Message::Text(request) = request else { panic!("expected text query") };

				assert!(matches!(
					serde_json::from_str::<ClientMessage>(&request).unwrap(),
					ClientMessage::Query(_)
				));

				for response in query {
					socket.send(response).await.unwrap();
				}
			}
		});
		let profile = ClientProfile::fixture(
			format!("ws://{address}/v1/ws"),
			ServerId::new(SERVER_ID).unwrap(),
		);

		(profile, task)
	}

	#[tokio::test]
	async fn client_accepts_only_a_fully_verified_typed_report() {
		let expected = report();
		let (profile, task) = fixture(initial(SERVER_ID), vec![result(expected.clone())]).await;
		let actual = DoctorClient::new(profile).query().await.unwrap();

		assert_eq!(actual, expected);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn client_rejects_every_incomplete_current_report_but_accepts_arbitrary_order() {
		let complete = report();
		let incomplete = [
			DoctorReport::new(ServerId::new(SERVER_ID).unwrap(), CURRENT_VERSION, Vec::new())
				.unwrap(),
			DoctorReport::new(
				ServerId::new(SERVER_ID).unwrap(),
				CURRENT_VERSION,
				vec![DoctorCheck::new(DoctorComponent::Configuration, DoctorStatus::Ready)],
			)
			.unwrap(),
			DoctorReport::new(
				ServerId::new(SERVER_ID).unwrap(),
				CURRENT_VERSION,
				complete.checks()[..complete.checks().len() - 1].to_vec(),
			)
			.unwrap(),
		];

		for report in incomplete {
			let (profile, task) = fixture(initial(SERVER_ID), vec![result(report)]).await;

			assert_eq!(
				DoctorClient::new(profile).query().await.unwrap_err(),
				ClientFailure::ProtocolMalformed,
			);

			task.await.unwrap();
		}

		let mut reversed = complete.checks().to_vec();

		reversed.reverse();

		let reversed =
			DoctorReport::new(ServerId::new(SERVER_ID).unwrap(), CURRENT_VERSION, reversed)
				.unwrap();
		let (profile, task) = fixture(initial(SERVER_ID), vec![result(reversed.clone())]).await;

		assert_eq!(DoctorClient::new(profile).query().await.unwrap(), reversed);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn major_minor_and_server_refusals_remain_distinct() {
		let cases = [
			(
				Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch {
					requested: ProtocolVersion { major: 2, minor: 0 },
					supported: SupportedVersions::current(),
				}),
				ClientFailure::ProtocolMajorMismatch,
			),
			(
				Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor {
					requested: ProtocolVersion { major: 1, minor: 0 },
					supported: SupportedVersions::current(),
				}),
				ClientFailure::ProtocolMinorMismatch,
			),
			(
				Refusal::ServerIdentityMismatch {
					expected: ServerId::new(SERVER_ID).unwrap(),
					actual: ServerId::new("wrong-server").unwrap(),
				},
				ClientFailure::ServerIdentityMismatch,
			),
		];

		for (refusal, expected) in cases {
			let response = typed(ServerMessage::Refusal(RefusalEnvelope {
				server_id: ServerId::new(SERVER_ID).unwrap(),
				refusal,
			}));
			let (profile, task) = fixture(vec![response], Vec::new()).await;

			assert_eq!(DoctorClient::new(profile).query().await.unwrap_err(), expected);

			task.await.unwrap();
		}
	}

	#[tokio::test]
	async fn every_refusal_phase_verifies_envelope_identity_before_classification() {
		let wrong_version = refusal(
			"wrong-server",
			Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor {
				requested: ProtocolVersion { major: 1, minor: 0 },
				supported: SupportedVersions::current(),
			}),
		);
		let (profile, task) = fixture(vec![wrong_version], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let mut wrong_protocol = initial(SERVER_ID);

		wrong_protocol[1] = refusal(
			"wrong-server",
			Refusal::ProtocolViolation { message: WireText::new("untrusted-order").unwrap() },
		);

		let (profile, task) = fixture(wrong_protocol, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let wrong_backpressure =
			refusal("wrong-server", Refusal::Backpressure { queue_capacity: 1 });
		let (profile, task) = fixture(initial(SERVER_ID), vec![wrong_backpressure]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn every_envelope_identity_and_version_is_verified_before_status() {
		let mut wrong_welcome = initial("wrong-server");

		wrong_welcome.truncate(1);

		let (profile, task) = fixture(wrong_welcome, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let wrong_welcome_version = typed(ServerMessage::Welcome(ServerWelcome {
			version: PREVIOUS_MINOR_VERSION,
			supported: SupportedVersions::current(),
			server_id: ServerId::new(SERVER_ID).unwrap(),
			instance_id: None,
			cursor: Cursor(0),
			reconnect: ReconnectMode::Snapshot,
		}));
		let (profile, task) = fixture(vec![wrong_welcome_version], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.unwrap();

		let mut wrong_snapshot = initial(SERVER_ID);

		wrong_snapshot[1] = typed(ServerMessage::Snapshot(SnapshotEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new("wrong-server").unwrap(),
			cursor: Cursor(0),
			items: Vec::new(),
		}));

		let (profile, task) = fixture(wrong_snapshot, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let wrong_result_identity = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new("wrong-server").unwrap(),
			query_id: QueryId::new("decodex-cli-doctor").unwrap(),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task) = fixture(initial(SERVER_ID), vec![wrong_result_identity]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let wrong_report_identity = DoctorReport::new(
			ServerId::new("wrong-server").unwrap(),
			CURRENT_VERSION,
			report().checks().to_vec(),
		)
		.unwrap();
		let (profile, task) =
			fixture(initial(SERVER_ID), vec![result(wrong_report_identity)]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ServerIdentityMismatch,
		);

		task.await.unwrap();

		let mut wrong_report = report();
		let encoded = serde_json::to_value(&wrong_report).unwrap();
		let mut encoded = encoded.as_object().unwrap().clone();

		encoded.insert("version".into(), serde_json::json!({"major": 1, "minor": 1}));

		wrong_report = serde_json::from_value(encoded.into()).unwrap();

		let (profile, task) = fixture(initial(SERVER_ID), vec![result(wrong_report)]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn snapshot_result_and_query_identity_fail_closed_before_report_acceptance() {
		let mut wrong_snapshot_version = initial(SERVER_ID);

		wrong_snapshot_version[1] = typed(ServerMessage::Snapshot(SnapshotEnvelope {
			version: PREVIOUS_MINOR_VERSION,
			server_id: ServerId::new(SERVER_ID).unwrap(),
			cursor: Cursor(0),
			items: Vec::new(),
		}));

		let (profile, task) = fixture(wrong_snapshot_version, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.unwrap();

		let wrong_result_version = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: PREVIOUS_MINOR_VERSION,
			server_id: ServerId::new(SERVER_ID).unwrap(),
			query_id: QueryId::new("decodex-cli-doctor").unwrap(),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task) = fixture(initial(SERVER_ID), vec![wrong_result_version]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMinorMismatch,
		);

		task.await.unwrap();

		let wrong_query_id = typed(ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER_ID).unwrap(),
			query_id: QueryId::new("wrong-query").unwrap(),
			payload: QueryResultPayload::DoctorStatus(report()),
		}));
		let (profile, task) = fixture(initial(SERVER_ID), vec![wrong_query_id]).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMalformed,
		);

		task.await.unwrap();

		let mut wrong_order = initial(SERVER_ID);

		wrong_order[1] = result(report());

		let (profile, task) = fixture(wrong_order, Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolMalformed,
		);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn interleaved_events_verify_version_and_identity_and_preserve_valid_order() {
		for (event, expected) in [
			(event(PREVIOUS_MINOR_VERSION, SERVER_ID), ClientFailure::ProtocolMinorMismatch),
			(event(CURRENT_VERSION, "wrong-server"), ClientFailure::ServerIdentityMismatch),
		] {
			let (profile, task) = fixture(initial(SERVER_ID), vec![event]).await;

			assert_eq!(DoctorClient::new(profile).query().await.unwrap_err(), expected);

			task.await.unwrap();
		}

		let expected = report();
		let responses = vec![event(CURRENT_VERSION, SERVER_ID), result(expected.clone())];
		let (profile, task) = fixture(initial(SERVER_ID), responses).await;

		assert_eq!(DoctorClient::new(profile).query().await.unwrap(), expected);

		task.await.unwrap();
	}

	#[tokio::test]
	async fn disconnected_malformed_oversized_and_timeout_fail_closed() {
		let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
		let address = listener.local_addr().unwrap();

		drop(listener);

		let profile = ClientProfile::fixture(
			format!("ws://{address}/v1/ws"),
			ServerId::new(SERVER_ID).unwrap(),
		);

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolDisconnected,
		);

		let (profile, task) =
			fixture(vec![Message::Text("untrusted-parser-marker".into())], Vec::new()).await;
		let error = DoctorClient::new(profile).query().await.unwrap_err();

		assert_eq!(error, ClientFailure::ProtocolMalformed);
		assert!(!format!("{error:?} {error}").contains("untrusted-parser-marker"));

		task.await.unwrap();

		let oversized = Message::Text("x".repeat(super::MAX_CLIENT_MESSAGE_BYTES + 1).into());
		let (profile, task) = fixture(vec![oversized], Vec::new()).await;

		assert_eq!(
			DoctorClient::new(profile).query().await.unwrap_err(),
			ClientFailure::ProtocolBackpressure,
		);

		task.await.unwrap();

		let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
		let address = listener.local_addr().unwrap();
		let task = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let _ = socket.next().await;

			time::sleep(Duration::from_secs(1)).await;
		});
		let profile = ClientProfile::fixture(
			format!("ws://{address}/v1/ws"),
			ServerId::new(SERVER_ID).unwrap(),
		);
		let client = DoctorClient { profile, timeout: Duration::from_millis(20) };

		assert_eq!(client.query().await.unwrap_err(), ClientFailure::ProtocolTimeout);

		task.abort();
	}
}
