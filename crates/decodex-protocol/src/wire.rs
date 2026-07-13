//! Structured JSON envelopes for the V1 WebSocket connection.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Error;

use crate::{ProtocolVersion, SupportedVersions, VersionRefusal};

/// Maximum UTF-8 size of any human-readable text carried by V1.
pub const MAX_WIRE_TEXT_BYTES: usize = 4_096;

/// Bounded human-readable wire text; artifact content cannot inhabit this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WireText(String);
impl WireText {
	/// Validate and construct bounded wire text.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();

		if value.len() > MAX_WIRE_TEXT_BYTES {
			return Err(WireScalarTooLong { actual_bytes: value.len() });
		}

		Ok(Self(value))
	}

	/// Borrow the validated UTF-8 text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl<'de> Deserialize<'de> for WireText {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;

		Self::new(value).map_err(D::Error::custom)
	}
}

/// A string-backed wire scalar exceeded the V1 byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireScalarTooLong {
	actual_bytes: usize,
}
impl Display for WireScalarTooLong {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"wire scalar is {} bytes; maximum is {MAX_WIRE_TEXT_BYTES}",
			self.actual_bytes
		)
	}
}

/// Identity generated for one server lifetime.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServerId(String);
impl ServerId {
	/// Validate and construct a bounded server identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for ServerId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Client-generated identity for one command attempt.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ClientCommandId(String);
impl ClientCommandId {
	/// Validate and construct a bounded client command identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for ClientCommandId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Stable key used to deduplicate a logical command.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);
impl IdempotencyKey {
	/// Validate and construct a bounded idempotency key.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for IdempotencyKey {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Stable identity of the entity observed or changed.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EntityId(String);
impl EntityId {
	/// Validate and construct a bounded entity identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for EntityId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Identity shared by related protocol activity.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CorrelationId(String);
impl CorrelationId {
	/// Validate and construct a bounded correlation identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for CorrelationId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Identity of the direct causal predecessor.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CausationId(String);
impl CausationId {
	/// Validate and construct a bounded causation identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}
impl<'de> Deserialize<'de> for CausationId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		WireText::deserialize(deserializer).map(|value| Self(value.0))
	}
}

/// Monotonic cursor in one server lifetime.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Cursor(pub u64);

/// Optimistic entity revision.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct EntityRevision(pub u64);

/// A client-to-server WebSocket message.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum ClientMessage {
	/// Must be the first message on a connection.
	Hello(ClientHello),
	/// Execute one typed application command after negotiation.
	Command(CommandEnvelope),
}

/// First client message and optional reconnect position.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ClientHello {
	/// Client protocol revision.
	pub version: ProtocolVersion,
	/// Previously observed server/cursor pair, when reconnecting.
	pub resume: Option<ResumeCursor>,
}

/// A cursor is meaningful only for the server lifetime that issued it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResumeCursor {
	/// Server lifetime that issued the cursor.
	pub server_id: ServerId,
	/// Last snapshot or event cursor fully applied by the client.
	///
	/// A welcome high-water mark is informational and is not an applied checkpoint.
	pub cursor: Cursor,
}

/// A typed command envelope.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CommandEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity of this command attempt.
	pub client_command_id: ClientCommandId,
	/// Stable logical-command deduplication key.
	pub idempotency_key: IdempotencyKey,
	/// Optional optimistic concurrency guard.
	pub expected_revision: Option<EntityRevision>,
	/// Identity shared by related activity.
	pub correlation_id: CorrelationId,
	/// Optional identity of the direct causal predecessor.
	pub causation_id: Option<CausationId>,
	/// Typed application command.
	pub payload: CommandPayload,
}

/// Commands available before product-specific application services land.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
pub enum CommandPayload {
	/// Refresh a bounded system-health observation through the common application boundary.
	RefreshSystemObservation {
		/// Foundation entity to observe.
		entity_id: EntityId,
	},
}

/// A server-to-client WebSocket message.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum ServerMessage {
	/// Successful session negotiation.
	Welcome(ServerWelcome),
	/// Bounded current-state projection.
	Snapshot(SnapshotEnvelope),
	/// Ordered resumable publication.
	Event(EventEnvelope),
	/// Server-lifetime command-attempt receipt.
	CommandReceipt(CommandReceipt),
	/// Deterministic command result.
	CommandResult(CommandResultEnvelope),
	/// Explicit protocol refusal.
	Refusal(RefusalEnvelope),
}

/// Negotiated session metadata.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ServerWelcome {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Server compatibility window.
	pub supported: SupportedVersions,
	/// Identity of this server lifetime.
	pub server_id: ServerId,
	/// Informational server high-water mark; never a client resume checkpoint by itself.
	pub cursor: Cursor,
	/// Reconnect strategy selected by the server.
	pub reconnect: ReconnectMode,
}

/// How the server fulfilled a reconnect request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconnectMode {
	/// A new session receives a current snapshot.
	Snapshot,
	/// A known cursor receives retained deltas.
	Resume,
	/// An unknown or stale cursor receives a current snapshot.
	SnapshotFallback,
}

/// A bounded current-state snapshot. Large artifacts have no representation here.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SnapshotEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity of the server lifetime producing the snapshot.
	pub server_id: ServerId,
	/// Cursor represented by the snapshot.
	pub cursor: Cursor,
	/// Bounded current-state items.
	pub items: Vec<SnapshotItem>,
}

/// Small state shapes allowed in a WebSocket snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "state", rename_all = "snake_case")]
pub enum SnapshotItem {
	/// Bounded system-health state.
	SystemState {
		/// Stable identity of the observed system.
		entity_id: EntityId,
		/// Revision represented by this state.
		revision: EntityRevision,
		/// Small human-readable foundation status.
		status: WireText,
	},
}

/// Logical channels multiplexed on the single connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
	/// Connection and session control.
	Control,
	/// Streaming conversation content.
	ConversationStream,
	/// Project and work-item state.
	ProjectWork,
	/// Run and process activity.
	RunActivity,
	/// Agent-authored messages.
	AgentMessage,
	/// Automation schedule activity.
	AutomationFiring,
	/// Account and credential health.
	AccountsHealth,
	/// Server and system health.
	SystemHealth,
}

/// One resumable, ordered publication.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EventEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity of the publishing server lifetime.
	pub server_id: ServerId,
	/// Monotonic publication cursor.
	pub cursor: Cursor,
	/// Logical publication channel.
	pub channel: Channel,
	/// Stable identity of the affected entity.
	pub entity_id: EntityId,
	/// Revision of the affected entity after publication.
	pub entity_revision: EntityRevision,
	/// Identity shared by related activity.
	pub correlation_id: CorrelationId,
	/// Optional identity of the direct causal predecessor.
	pub causation_id: Option<CausationId>,
	/// Typed event data.
	pub payload: EventPayload,
}

/// Event payloads available in this foundation slice.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
	/// A foundation system observation was refreshed.
	SystemObservationRefreshed {
		/// Small human-readable foundation status.
		status: WireText,
	},
}

/// Server-lifetime receipt disposition for one command attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptDisposition {
	/// The command was executed for the first time.
	Executed,
	/// The command reused a previously recorded logical command.
	Duplicate,
	/// The command was refused before application execution.
	Refused,
}

/// Server-lifetime receipt returned before deterministic result readback.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CommandReceipt {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity of the receiving server lifetime.
	pub server_id: ServerId,
	/// Identity of this command attempt.
	pub client_command_id: ClientCommandId,
	/// Stable logical-command deduplication key.
	pub idempotency_key: IdempotencyKey,
	/// Whether execution was new or deduplicated.
	pub disposition: ReceiptDisposition,
	/// Identity of the command attempt that first used the key.
	pub original_client_command_id: ClientCommandId,
}

/// Deterministic command outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CommandResultEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity of the receiving server lifetime.
	pub server_id: ServerId,
	/// Identity of this command attempt.
	pub client_command_id: ClientCommandId,
	/// Stable logical-command deduplication key.
	pub idempotency_key: IdempotencyKey,
	/// Success or rejection classification.
	pub outcome: CommandOutcome,
	/// Resulting entity revision, when execution succeeded.
	pub entity_revision: Option<EntityRevision>,
	/// Typed success result, when execution succeeded.
	pub payload: Option<ResultPayload>,
	/// Typed rejection, when execution was rejected.
	pub error: Option<CommandError>,
}

/// High-level command outcome classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
	/// Application execution completed successfully.
	Succeeded,
	/// Protocol or application guards rejected execution.
	Rejected,
}

/// Typed successful command results.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
pub enum ResultPayload {
	/// A foundation system observation was refreshed.
	SystemObservationRefreshed {
		/// Small human-readable foundation status.
		status: WireText,
	},
}

/// Typed command rejection details.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum CommandError {
	/// The optimistic concurrency guard did not match current state.
	ExpectedRevisionMismatch {
		/// Revision supplied by the client.
		expected: EntityRevision,
		/// Current entity revision.
		actual: EntityRevision,
	},
	/// An idempotency key was reused for a different logical command.
	IdempotencyConflict,
	/// The fixed-lifetime idempotency ledger cannot accept a new key.
	IdempotencyCapacityExceeded {
		/// Configured maximum number of retained logical commands.
		capacity: usize,
	},
	/// The application owner required for the command is not enabled.
	ApplicationUnavailable {
		/// Bounded operator-facing explanation.
		message: WireText,
	},
}

/// A refusal that leaves no ambiguous application mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RefusalEnvelope {
	/// Identity of the refusing server lifetime.
	pub server_id: ServerId,
	/// Typed refusal detail.
	pub refusal: Refusal,
}

/// Protocol-level refusal that guarantees no application mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Refusal {
	/// The requested version falls outside the compatibility window.
	UnsupportedVersion(VersionRefusal),
	/// The client violated connection or message ordering rules.
	ProtocolViolation {
		/// Bounded explanation of the violated rule.
		message: WireText,
	},
	/// The bounded outbound queue could not accept more work.
	Backpressure {
		/// Configured outbound message capacity.
		queue_capacity: usize,
	},
}

/// Serialize a message using the only V1 wire encoding.
pub fn encode_server_message(message: &ServerMessage) -> Result<String, Error> {
	serde_json::to_string(message)
}

/// Parse a client message using the only V1 wire encoding.
pub fn decode_client_message(message: &str) -> Result<ClientMessage, Error> {
	serde_json::from_str(message)
}

#[cfg(test)]
mod tests {
	use crate::{
		CURRENT_VERSION, CausationId, ClientCommandId, CorrelationId, EntityId, IdempotencyKey,
		MAX_WIRE_TEXT_BYTES, ServerId, WireText,
		wire::{
			ClientHello, ClientMessage, CommandEnvelope, CommandPayload, Cursor, EntityRevision,
			ResumeCursor,
		},
	};

	#[test]
	fn command_wire_shape_is_structured_and_round_trips() {
		let message = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("command-1").expect("bounded fixture ID"),
			idempotency_key: IdempotencyKey::new("dedupe-1").expect("bounded fixture key"),
			expected_revision: Some(EntityRevision(4)),
			correlation_id: CorrelationId::new("correlation-1").expect("bounded fixture ID"),
			causation_id: Some(CausationId::new("cause-1").expect("bounded fixture ID")),
			payload: CommandPayload::RefreshSystemObservation {
				entity_id: EntityId::new("system").expect("bounded fixture ID"),
			},
		});
		let encoded = serde_json::to_string(&message).unwrap();

		assert!(encoded.contains("\"type\":\"command\""));
		assert!(encoded.contains("\"name\":\"refresh_system_observation\""));
		assert_eq!(serde_json::from_str::<ClientMessage>(&encoded).unwrap(), message);
	}

	#[test]
	fn hello_wire_shape_is_a_stable_json_golden() {
		let message = ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			resume: Some(ResumeCursor {
				server_id: ServerId::new("server-a").expect("bounded fixture ID"),
				cursor: Cursor(42),
			}),
		});

		assert_eq!(
			serde_json::to_string(&message).unwrap(),
			concat!(
				r#"{"type":"hello","body":{"version":{"major":1,"minor":1},"#,
				r#""resume":{"server_id":"server-a","cursor":42}}}"#,
			)
		);
	}

	#[test]
	fn human_readable_wire_text_is_mechanically_bounded() {
		assert!(WireText::new("x".repeat(MAX_WIRE_TEXT_BYTES)).is_ok());
		assert!(WireText::new("x".repeat(MAX_WIRE_TEXT_BYTES + 1)).is_err());
	}
}
