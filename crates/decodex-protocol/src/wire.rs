//! Structured JSON envelopes for the V1 WebSocket connection.

pub use decodex_core::{
	HistoryMediaType, HistoryMetadata, HistoryMetadataValue, MAX_HISTORY_METADATA_FIELDS,
	MAX_HISTORY_METADATA_KEY_BYTES, MAX_HISTORY_METADATA_VALUE_BYTES, MAX_RESET_CARD_ITEMS,
};

use std::{
	collections::HashSet,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _, ser::Error as _};
use serde_json::Error;

use crate::{DoctorReport, ProtocolVersion, SupportedVersions, VersionRefusal};

/// Maximum UTF-8 size of any human-readable text carried by V1.
pub const MAX_WIRE_TEXT_BYTES: usize = 4_096;
/// Maximum UTF-8 size of one logical-command idempotency key.
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
/// Maximum inline history bytes in one typed page item.
pub const MAX_HISTORY_INLINE_BYTES: usize = 16 * 1_024;
/// Maximum history items returned in one WebSocket query result. This keeps the worst-case
/// encoded result below the default 256-KiB transport frame bound.
pub const MAX_HISTORY_PAGE_SIZE: u16 = 8;
/// Maximum verified payload length representable in a history blob reference.
pub const MAX_HISTORY_BLOB_BYTES: u64 = 64 * 1_024 * 1_024;

/// Bounded human-readable wire text; artifact content cannot inhabit this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WireText(String);
impl WireText {
	/// Validate and construct bounded wire text.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();

		if value.len() > MAX_WIRE_TEXT_BYTES {
			return Err(WireScalarTooLong {
				actual_bytes: value.len(),
				maximum_bytes: MAX_WIRE_TEXT_BYTES,
			});
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

/// Bounded inline normalized history text. Large content is represented only by a blob reference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryText(String);
impl HistoryText {
	/// Validate and construct bounded inline history text.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();

		if value.len() > MAX_HISTORY_INLINE_BYTES {
			return Err(WireScalarTooLong {
				actual_bytes: value.len(),
				maximum_bytes: MAX_HISTORY_INLINE_BYTES,
			});
		}

		Ok(Self(value))
	}

	/// Borrow the validated inline text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for HistoryText {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Opaque, deterministic keyset cursor capped independently of ordinary wire text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryCursorToken(String);
impl HistoryCursorToken {
	/// Validate and construct a bounded opaque cursor.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();

		if value.len() > 128 {
			return Err(WireScalarTooLong { actual_bytes: value.len(), maximum_bytes: 128 });
		}

		Ok(Self(value))
	}

	/// Borrow the validated cursor text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for HistoryCursorToken {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// A string-backed wire scalar exceeded the V1 byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireScalarTooLong {
	actual_bytes: usize,
	maximum_bytes: usize,
}
impl Display for WireScalarTooLong {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"wire scalar is {} bytes; maximum is {}",
			self.actual_bytes, self.maximum_bytes
		)
	}
}

/// Stable server-host identity retained across daemon lifetimes.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServerId(String);
impl ServerId {
	/// Validate and construct a bounded server identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}

	/// Borrow the bounded stable identity text.
	pub fn as_str(&self) -> &str {
		&self.0
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

/// Ephemeral identity for one in-memory publication/replay epoch.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServerInstanceId(String);
impl ServerInstanceId {
	/// Validate and construct a bounded server-instance identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}

impl<'de> Deserialize<'de> for ServerInstanceId {
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

/// Client-generated identity for one live query observation.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct QueryId(String);
impl QueryId {
	/// Validate and construct a bounded query identity.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		WireText::new(value).map(|value| Self(value.0))
	}
}

impl<'de> Deserialize<'de> for QueryId {
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
	pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
		let value = value.into();

		if value.is_empty() {
			return Err(IdempotencyKeyError::Empty);
		}
		if value.len() > MAX_IDEMPOTENCY_KEY_BYTES {
			return Err(IdempotencyKeyError::TooLong);
		}
		if value.trim() != value {
			return Err(IdempotencyKeyError::SurroundingWhitespace);
		}
		if value.chars().any(char::is_control) {
			return Err(IdempotencyKeyError::ControlCharacter);
		}

		Ok(Self(value))
	}

	/// Borrow the validated logical-command key.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for IdempotencyKey {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Closed validation failures for an idempotency key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdempotencyKeyError {
	/// The key was empty.
	Empty,
	/// The UTF-8 byte length exceeded [`MAX_IDEMPOTENCY_KEY_BYTES`].
	TooLong,
	/// The key had leading or trailing Unicode whitespace.
	SurroundingWhitespace,
	/// The key contained a control character.
	ControlCharacter,
}
impl Display for IdempotencyKeyError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Empty => "idempotency key is empty",
			Self::TooLong => "idempotency key exceeds the 256-byte maximum",
			Self::SurroundingWhitespace =>
				"idempotency key contains leading or trailing whitespace",
			Self::ControlCharacter => "idempotency key contains a control character",
		})
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

	/// Borrow the validated entity identity.
	pub fn as_str(&self) -> &str {
		&self.0
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

/// Monotonic cursor in one in-memory publication epoch.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Cursor(pub u64);

/// Optimistic entity revision.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct EntityRevision(pub u64);

/// First client message and optional reconnect position.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ClientHello {
	/// Client protocol revision.
	pub version: ProtocolVersion,
	/// Optional stable server-host identity pin. It is enforced before status or commands.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub expected_server_id: Option<ServerId>,
	/// Previously observed server/cursor pair, when reconnecting.
	pub resume: Option<ResumeCursor>,
}

/// A cursor is meaningful only for the publication epoch that issued it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResumeCursor {
	/// Stable server host that issued the cursor.
	pub server_id: ServerId,
	/// Ephemeral publication epoch that issued the cursor.
	///
	/// V1.1 clients omit this field and therefore receive a snapshot fallback.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub instance_id: Option<ServerInstanceId>,
	/// Last snapshot or event cursor fully applied by the client.
	///
	/// A welcome high-water mark is informational and is not an applied checkpoint.
	pub cursor: Cursor,
}

/// A typed command envelope.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

/// A live read envelope. Queries are observations, not idempotent mutations.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Identity echoed with this observation; it is not a deduplication key.
	pub query_id: QueryId,
	/// Typed live query.
	pub payload: QueryPayload,
}

/// Negotiated session metadata.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ServerWelcome {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Server compatibility window.
	pub supported: SupportedVersions,
	/// Stable identity of this server host.
	pub server_id: ServerId,
	/// Ephemeral identity of the in-memory publication epoch.
	///
	/// This is present for V1.2 and omitted for the compatible V1.1 shape.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub instance_id: Option<ServerInstanceId>,
	/// Informational server high-water mark; never a client resume checkpoint by itself.
	pub cursor: Cursor,
	/// Reconnect strategy selected by the server.
	pub reconnect: ReconnectMode,
}

/// A bounded current-state snapshot. Large artifacts have no representation here.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SnapshotEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Stable identity of the server host producing the snapshot.
	pub server_id: ServerId,
	/// Cursor represented by the snapshot.
	pub cursor: Cursor,
	/// Bounded current-state items.
	pub items: Vec<SnapshotItem>,
}

/// One resumable, ordered publication.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EventEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Stable identity of the publishing server host.
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

/// Publication-epoch receipt returned before deterministic result readback.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CommandReceipt {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Stable identity of the receiving server host.
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
	/// Stable identity of the receiving server host.
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

/// Fresh live-query result. Reusing a query identity performs another observation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QueryResultEnvelope {
	/// Negotiated protocol revision.
	pub version: ProtocolVersion,
	/// Stable identity of the server host producing the observation.
	pub server_id: ServerId,
	/// Client identity for this observation.
	pub query_id: QueryId,
	/// Typed current result.
	pub payload: QueryResultPayload,
}

/// One deterministic bounded page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConversationHistoryPage {
	/// Ordered normalized items.
	pub items: Vec<HistoryItemDto>,
	/// Cursor for the next page only when more rows exist.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub next_cursor: Option<HistoryCursorToken>,
}
impl<'de> Deserialize<'de> for ConversationHistoryPage {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		struct Page {
			items: Vec<HistoryItemDto>,
			next_cursor: Option<HistoryCursorToken>,
		}

		let page = Page::deserialize(deserializer)?;

		if page.items.len() > usize::from(MAX_HISTORY_PAGE_SIZE) {
			return Err(D::Error::custom("history page exceeds item bound"));
		}

		Ok(Self { items: page.items, next_cursor: page.next_cursor })
	}
}

/// Normalized history item served by the daemon contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HistoryItemDto {
	/// Stable item identity.
	pub history_item_id: EntityId,
	/// Parent turn identity.
	pub turn_id: EntityId,
	/// Producing runtime segment identity.
	pub runtime_session_id: EntityId,
	/// Normalized author role.
	pub turn_role: HistoryTurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: HistorySideEffectState,
	/// Normalized item class.
	pub kind: HistoryItemKindDto,
	/// Item stream lifecycle.
	pub status: HistoryItemStatusDto,
	/// Inline text or content-addressed metadata.
	pub payload: HistoryPayloadDto,
	/// Canonical media type for inline and offloaded payloads alike.
	pub media_type: HistoryMediaType,
	/// Bounded credential-negative normalized metadata projection.
	pub metadata: HistoryMetadata,
	/// Exact typed Artifact revision for Artifact items.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub artifact: Option<HistoryArtifactReference>,
	/// Persisted optimistic revision.
	pub revision: EntityRevision,
}
impl<'de> Deserialize<'de> for HistoryItemDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawItem {
			history_item_id: EntityId,
			turn_id: EntityId,
			runtime_session_id: EntityId,
			turn_role: HistoryTurnRole,
			possible_side_effects: HistorySideEffectState,
			kind: HistoryItemKindDto,
			status: HistoryItemStatusDto,
			payload: HistoryPayloadDto,
			media_type: HistoryMediaType,
			metadata: HistoryMetadata,
			artifact: Option<HistoryArtifactReference>,
			revision: EntityRevision,
		}

		let raw = RawItem::deserialize(deserializer)?;

		if (raw.kind == HistoryItemKindDto::Artifact) != raw.artifact.is_some() {
			return Err(D::Error::custom("history Artifact kind/reference is inconsistent"));
		}

		Ok(Self {
			history_item_id: raw.history_item_id,
			turn_id: raw.turn_id,
			runtime_session_id: raw.runtime_session_id,
			turn_role: raw.turn_role,
			possible_side_effects: raw.possible_side_effects,
			kind: raw.kind,
			status: raw.status,
			payload: raw.payload,
			media_type: raw.media_type,
			metadata: raw.metadata,
			artifact: raw.artifact,
			revision: raw.revision,
		})
	}
}

/// Exact client-visible reference to one immutable Artifact revision.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct HistoryArtifactReference {
	/// Canonical Artifact UUID.
	pub artifact_id: HistoryArtifactId,
	/// Positive immutable Artifact revision.
	pub revision: HistoryArtifactRevision,
}

/// Canonical Artifact UUID carried by the history protocol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryArtifactId(String);
impl HistoryArtifactId {
	/// Validate one canonical lowercase UUID.
	pub fn new(value: impl Into<String>) -> Option<Self> {
		let value = value.into();

		is_canonical_uuid(&value).then_some(Self(value))
	}

	/// Borrow canonical UUID text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for HistoryArtifactId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;

		Self::new(value).ok_or_else(|| D::Error::custom("Artifact identity is not canonical"))
	}
}

/// Positive immutable Artifact revision carried by the history protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryArtifactRevision(u64);
impl HistoryArtifactRevision {
	/// Validate a positive Artifact revision.
	pub fn new(value: u64) -> Option<Self> {
		(value > 0).then_some(Self(value))
	}

	/// Return the positive revision.
	pub fn get(self) -> u64 {
		self.0
	}
}

impl<'de> Deserialize<'de> for HistoryArtifactRevision {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(u64::deserialize(deserializer)?)
			.ok_or_else(|| D::Error::custom("Artifact revision must be positive"))
	}
}

/// Verified metadata for content-addressed history bytes.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct HistoryBlobReference {
	/// Lowercase SHA-256 content address.
	pub sha256: Sha256Digest,
	/// Verified byte length.
	pub byte_length: HistoryBlobLength,
}

/// Canonical lowercase SHA-256 wire scalar.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);
impl Sha256Digest {
	/// Validate one exact lowercase 64-character digest.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();

		if value.len() != 64
			|| !value.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
		{
			return Err(WireScalarTooLong { actual_bytes: value.len(), maximum_bytes: 64 });
		}

		Ok(Self(value))
	}

	/// Borrow canonical digest text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for Sha256Digest {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Nonzero payload length bounded by the BlobStore authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryBlobLength(u64);
impl HistoryBlobLength {
	/// Validate a nonzero BlobStore-bounded byte length.
	pub fn new(value: u64) -> Result<Self, WireScalarTooLong> {
		if value == 0 || value > MAX_HISTORY_BLOB_BYTES {
			return Err(WireScalarTooLong {
				actual_bytes: usize::try_from(value).unwrap_or(usize::MAX),
				maximum_bytes: usize::try_from(MAX_HISTORY_BLOB_BYTES)
					.expect("history blob maximum fits usize"),
			});
		}

		Ok(Self(value))
	}

	/// Return the verified byte length.
	pub fn get(self) -> u64 {
		self.0
	}
}

impl<'de> Deserialize<'de> for HistoryBlobLength {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(u64::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Public, credential-negative identity of one reset card.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ResetCardDescriptorDto {
	granted_at_unix_seconds: i64,
	expires_at_unix_seconds: i64,
}
impl ResetCardDescriptorDto {
	/// Validate one public reset-card descriptor.
	pub fn new(
		granted_at_unix_seconds: i64,
		expires_at_unix_seconds: i64,
	) -> Result<Self, ResetCardDescriptorError> {
		if granted_at_unix_seconds < 0 {
			return Err(ResetCardDescriptorError::NegativeGrantedAt);
		}
		if expires_at_unix_seconds < 0 {
			return Err(ResetCardDescriptorError::NegativeExpiresAt);
		}
		if expires_at_unix_seconds <= granted_at_unix_seconds {
			return Err(ResetCardDescriptorError::InvalidWindow);
		}

		Ok(Self { granted_at_unix_seconds, expires_at_unix_seconds })
	}

	/// Return the nonnegative grant timestamp.
	pub const fn granted_at_unix_seconds(self) -> i64 {
		self.granted_at_unix_seconds
	}

	/// Return the expiry timestamp, which is later than the grant timestamp.
	pub const fn expires_at_unix_seconds(self) -> i64 {
		self.expires_at_unix_seconds
	}
}
impl<'de> Deserialize<'de> for ResetCardDescriptorDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawDescriptor {
			granted_at_unix_seconds: i64,
			expires_at_unix_seconds: i64,
		}

		let raw = RawDescriptor::deserialize(deserializer)?;

		Self::new(raw.granted_at_unix_seconds, raw.expires_at_unix_seconds)
			.map_err(D::Error::custom)
	}
}

/// Closed validation failures for a public reset-card descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardDescriptorError {
	/// The grant timestamp was before the Unix epoch.
	NegativeGrantedAt,
	/// The expiry timestamp was before the Unix epoch.
	NegativeExpiresAt,
	/// The expiry timestamp was not later than the grant timestamp.
	InvalidWindow,
}
impl Display for ResetCardDescriptorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::NegativeGrantedAt => "reset-card grant timestamp is negative",
			Self::NegativeExpiresAt => "reset-card expiry timestamp is negative",
			Self::InvalidWindow => "reset-card expiry must be later than its grant",
		})
	}
}

/// One public reset-card inventory observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResetCardObservationDto {
	/// Public descriptor used for explicit operator selection.
	pub descriptor: ResetCardDescriptorDto,
}

/// Bounded current reset-card inventory for one account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResetCardInventoryResult {
	/// A complete inventory safe for explicit selection.
	Available {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Current optimistic account revision.
		account_revision: EntityRevision,
		/// Exact current number of available cards.
		available_count: u16,
		/// Complete unique public card observations.
		cards: Vec<ResetCardObservationDto>,
		/// Freshness and value of the exact 300-minute window from this same provider call.
		five_hour_quota: AccountQuotaWindowDto,
		/// Freshness and value of the exact 10,080-minute window from this same provider call.
		seven_day_quota: AccountQuotaWindowDto,
	},
	/// The account row was established, but its bounded provider observation failed.
	ObservationFailed {
		/// Canonical vNext account UUID established before the provider call.
		account_id: EntityId,
		/// Account revision established before the provider call.
		account_revision: EntityRevision,
		/// Persisted 300-minute evidence from this observation attempt.
		five_hour_quota: AccountQuotaWindowDto,
		/// Persisted 10,080-minute evidence from this observation attempt.
		seven_day_quota: AccountQuotaWindowDto,
		/// Stable row-scoped failure class.
		error: ResetCardError,
	},
	/// Inventory could not be established safely.
	Unavailable {
		/// Stable reason class.
		error: ResetCardError,
	},
}
impl Serialize for ResetCardInventoryResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum RawResult<'a> {
			Available {
				account_id: &'a EntityId,
				account_revision: EntityRevision,
				available_count: u16,
				cards: &'a [ResetCardObservationDto],
				five_hour_quota: AccountQuotaWindowDto,
				seven_day_quota: AccountQuotaWindowDto,
			},
			ObservationFailed {
				account_id: &'a EntityId,
				account_revision: EntityRevision,
				five_hour_quota: AccountQuotaWindowDto,
				seven_day_quota: AccountQuotaWindowDto,
				error: ResetCardError,
			},
			Unavailable {
				error: ResetCardError,
			},
		}

		let raw = match self {
			Self::Available {
				account_id,
				account_revision,
				available_count,
				cards,
				five_hour_quota,
				seven_day_quota,
			} => {
				validate_reset_card_inventory(
					account_id,
					*account_revision,
					*available_count,
					cards,
					*five_hour_quota,
					*seven_day_quota,
				)
				.map_err(S::Error::custom)?;
				RawResult::Available {
					account_id,
					account_revision: *account_revision,
					available_count: *available_count,
					cards,
					five_hour_quota: *five_hour_quota,
					seven_day_quota: *seven_day_quota,
				}
			},
			Self::ObservationFailed {
				account_id,
				account_revision,
				five_hour_quota,
				seven_day_quota,
				error,
			} => {
				validate_reset_card_observation_failure(
					account_id,
					*account_revision,
					*five_hour_quota,
					*seven_day_quota,
				)
				.map_err(S::Error::custom)?;
				RawResult::ObservationFailed {
					account_id,
					account_revision: *account_revision,
					five_hour_quota: *five_hour_quota,
					seven_day_quota: *seven_day_quota,
					error: *error,
				}
			},
			Self::Unavailable { error } => RawResult::Unavailable { error: *error },
		};

		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for ResetCardInventoryResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum RawResult {
			Available {
				account_id: EntityId,
				account_revision: EntityRevision,
				available_count: u16,
				cards: Vec<ResetCardObservationDto>,
				five_hour_quota: AccountQuotaWindowDto,
				seven_day_quota: AccountQuotaWindowDto,
			},
			ObservationFailed {
				account_id: EntityId,
				account_revision: EntityRevision,
				five_hour_quota: AccountQuotaWindowDto,
				seven_day_quota: AccountQuotaWindowDto,
				error: ResetCardError,
			},
			Unavailable {
				error: ResetCardError,
			},
		}

		match RawResult::deserialize(deserializer)? {
			RawResult::Available {
				account_id,
				account_revision,
				available_count,
				cards,
				five_hour_quota,
				seven_day_quota,
			} => {
				validate_reset_card_inventory(
					&account_id,
					account_revision,
					available_count,
					&cards,
					five_hour_quota,
					seven_day_quota,
				)
				.map_err(D::Error::custom)?;

				Ok(Self::Available {
					account_id,
					account_revision,
					available_count,
					cards,
					five_hour_quota,
					seven_day_quota,
				})
			},
			RawResult::ObservationFailed {
				account_id,
				account_revision,
				five_hour_quota,
				seven_day_quota,
				error,
			} => {
				validate_reset_card_observation_failure(
					&account_id,
					account_revision,
					five_hour_quota,
					seven_day_quota,
				)
				.map_err(D::Error::custom)?;
				Ok(Self::ObservationFailed {
					account_id,
					account_revision,
					five_hour_quota,
					seven_day_quota,
					error,
				})
			},
			RawResult::Unavailable { error } => Ok(Self::Unavailable { error }),
		}
	}
}

/// Credential-negative reset-card service failure classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetCardError {
	/// The account identity, descriptor, or command guard was invalid.
	InvalidRequest,
	/// The canonical account was not found.
	AccountNotFound,
	/// The current account state does not admit manual reset.
	AccountStateRejected,
	/// The daemon credential vault could not provide an account credential.
	VaultUnavailable,
	/// The selected Codex app-server schema does not advertise reset-card support.
	SchemaUnsupported,
	/// The upstream provider could not establish current state.
	ProviderUnavailable,
	/// The provider inventory was incomplete or ambiguous.
	InventoryIncomplete,
	/// The selected public descriptor no longer identifies the same available card.
	InventoryChanged,
	/// A bounded local resource limit was reached.
	ResourceExhausted,
	/// Authoritative product state was unavailable.
	ProductStateUnavailable,
	/// The external effect may have happened and requires authoritative reconciliation.
	EffectAmbiguous,
}

/// Closed provider outcome after reset-card consumption.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetCardOutcome {
	/// Rate limits were reset.
	Reset,
	/// The account had no active rate-limit exhaustion to reset.
	NothingToReset,
	/// The selected account no longer had an eligible credit.
	NoCredit,
	/// The exact credit was already redeemed.
	AlreadyRedeemed,
}

/// Observation of one reset-card operation or typed authoritative-state unavailability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResetCardOperationResult {
	/// No durable operation exists for the supplied logical-command key.
	NotFound,
	/// The exact public selection and provider operation identity are durable before the effect.
	Prepared,
	/// The provider effect may have happened and requires authoritative reconciliation.
	EffectAmbiguous,
	/// The operation reached one terminal provider outcome.
	Completed {
		/// Closed provider outcome.
		outcome: ResetCardOutcome,
	},
	/// The operation failed before an external effect could happen.
	FailedBeforeEffect {
		/// Stable failure class.
		error: ResetCardError,
	},
	/// Authoritative operation state could not be read. This is not a durable terminal result.
	Unavailable {
		/// Stable unavailable reason.
		error: ResetCardError,
	},
}

/// A refusal that leaves no ambiguous application mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RefusalEnvelope {
	/// Stable identity of the refusing server host.
	pub server_id: ServerId,
	/// Typed refusal detail.
	pub refusal: Refusal,
}

/// A client-to-server WebSocket message.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
	/// Must be the first message on a connection.
	Hello(ClientHello),
	/// Execute one typed application command after negotiation.
	Command(CommandEnvelope),
	/// Observe current typed state after negotiation without creating a receipt.
	Query(QueryEnvelope),
}

const fn version_supports(version: ProtocolVersion, minimum: ProtocolVersion) -> bool {
	version.major == minimum.major && version.minor >= minimum.minor
}

/// Supported credential-negative account provider projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProviderDto {
	/// ChatGPT OAuth credentials consumed by Codex.
	Chatgpt,
}

/// Persisted observed account health, independent of administrative enablement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountObservedStateDto {
	/// The account or a required host boundary is unavailable.
	Unavailable,
	/// No current evidence establishes provider health.
	Unknown,
	/// Fresh provider evidence reports availability.
	Available,
	/// A required quota window is depleted.
	Depleted,
	/// The provider rejected authentication.
	AuthFailed,
	/// A required provider plugin is not ready.
	PluginUnready,
}

/// Exact Account Lifecycle admission state derived by the daemon.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLifecycleReadinessDto {
	/// All admission boundaries agree.
	Ready,
	/// No current credential binding exists.
	CredentialAbsent,
	/// The host credential store is unavailable.
	StoreUnavailable,
	/// Registry and host-store metadata differ.
	StoreMismatch,
	/// Registry and host-store provider identities differ.
	ProviderMismatch,
	/// A finite credential operation is unsettled.
	OperationUnsettled,
	/// The exact Codex refresh callback is not ready.
	CallbackCapabilityUnready,
	/// The account was logged out.
	Tombstoned,
}

/// Finite credential operation kind shown with an unsettled account row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountOperationKindDto {
	/// Enroll from shared Codex credentials.
	Enroll,
	/// Import from an explicit credential file.
	Import,
	/// Rotate to the next credential version.
	Refresh,
	/// Delete credentials and tombstone the account.
	Logout,
}

/// Finite nonterminal credential operation phase shown with an account row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountOperationPhaseDto {
	/// PostgreSQL accepted the operation before an external effect.
	Prepared,
	/// A provider effect can no longer be proved absent.
	ProviderEffectPending,
	/// The host-store effect is proved and the registry commit is pending.
	StoreApplied,
	/// Explicit reconciliation is required.
	RecoveryRequired,
}

/// Credential-negative operation state sufficient for independent row rendering.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountUnsettledOperationDto {
	/// Stable operation UUID.
	pub operation_id: EntityId,
	/// Finite lifecycle operation kind.
	pub kind: AccountOperationKindDto,
	/// Current nonterminal phase.
	pub phase: AccountOperationPhaseDto,
	/// Stable recovery reason code, only for manual recovery.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub recovery_code: Option<WireText>,
}

/// Credential-negative canonical host-store binding.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountCredentialBindingDto {
	/// HostCredentialStore schema version.
	pub schema_version: u16,
	/// Monotonic per-account credential version.
	pub version: u64,
	/// Canonical fingerprint of the complete secret bundle.
	pub fingerprint_sha256: Sha256Digest,
	/// Provider kind bound to the secret bundle.
	pub provider: AccountProviderDto,
	/// Non-secret provider account identity.
	pub provider_account_id: WireText,
}

/// Closed bounded provider-observation error for one account row and quota duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountQuotaErrorDto {
	/// The provider request could not complete.
	ProviderUnavailable,
	/// The provider response did not satisfy the protocol contract.
	ProtocolUnavailable,
	/// The provider response identified another account.
	AccountMismatch,
	/// One required quota duration was absent.
	UnsupportedWindow,
}

/// Server-owned quota freshness and value classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountQuotaStateDto {
	/// No observation exists.
	Unknown,
	/// The retained quota fact is current.
	Current {
		/// Provider-reported percentage used.
		used_percent: u8,
		/// Provider-reported reset time in Unix microseconds.
		resets_at_unix_micros: i64,
	},
	/// The retained quota fact is no longer current.
	Stale {
		/// Provider-reported percentage used.
		used_percent: u8,
		/// Provider-reported reset time in Unix microseconds.
		resets_at_unix_micros: i64,
	},
	/// The latest observation produced a bounded error.
	Error {
		/// Stable observation failure.
		error: AccountQuotaErrorDto,
	},
}

/// One required independently observed quota duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountQuotaWindowDto {
	/// Exact window duration. V1.4 accepts 300 and 10080 minutes only.
	pub duration_minutes: u32,
	/// Exact observation time, absent only when state is unknown.
	pub observed_at_unix_micros: Option<i64>,
	/// Closed current, unknown, stale, or error result.
	pub result: AccountQuotaStateDto,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawAccountQuotaWindowDto {
	duration_minutes: u32,
	observed_at_unix_micros: RequiredQuotaObservationTime,
	result: AccountQuotaStateDto,
}
#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct RequiredQuotaObservationTime(Option<i64>);
impl From<AccountQuotaWindowDto> for RawAccountQuotaWindowDto {
	fn from(quota: AccountQuotaWindowDto) -> Self {
		Self {
			duration_minutes: quota.duration_minutes,
			observed_at_unix_micros: RequiredQuotaObservationTime(quota.observed_at_unix_micros),
			result: quota.result,
		}
	}
}
impl From<RawAccountQuotaWindowDto> for AccountQuotaWindowDto {
	fn from(quota: RawAccountQuotaWindowDto) -> Self {
		Self {
			duration_minutes: quota.duration_minutes,
			observed_at_unix_micros: quota.observed_at_unix_micros.0,
			result: quota.result,
		}
	}
}
impl Serialize for AccountQuotaWindowDto {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		validate_public_quota_window(*self).map_err(S::Error::custom)?;
		RawAccountQuotaWindowDto::from(*self).serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountQuotaWindowDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let quota = Self::from(RawAccountQuotaWindowDto::deserialize(deserializer)?);
		validate_public_quota_window(quota).map_err(D::Error::custom)?;
		Ok(quota)
	}
}

/// Credential-negative daemon-owned account projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountDto {
	/// Canonical Decodex account identity.
	pub account_id: EntityId,
	/// Non-secret operator label.
	pub display_label: WireText,
	/// Independent administrative admission switch.
	pub enabled: bool,
	/// Optimistic account revision.
	pub account_revision: EntityRevision,
	/// Persisted provider-observed health.
	pub observed_state: AccountObservedStateDto,
	/// Derived lifecycle admission gate.
	pub lifecycle_readiness: AccountLifecycleReadinessDto,
	/// Current credential-negative host-store binding.
	pub credential_binding: Option<AccountCredentialBindingDto>,
	/// Current unsettled credential operation.
	pub unsettled_operation: Option<AccountUnsettledOperationDto>,
	/// Required 300-minute quota observation.
	pub five_hour_quota: AccountQuotaWindowDto,
	/// Required 10,080-minute quota observation.
	pub seven_day_quota: AccountQuotaWindowDto,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawAccountDto {
	account_id: EntityId,
	display_label: WireText,
	enabled: bool,
	account_revision: EntityRevision,
	observed_state: AccountObservedStateDto,
	lifecycle_readiness: AccountLifecycleReadinessDto,
	#[serde(skip_serializing_if = "Option::is_none")]
	credential_binding: Option<AccountCredentialBindingDto>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unsettled_operation: Option<AccountUnsettledOperationDto>,
	five_hour_quota: AccountQuotaWindowDto,
	seven_day_quota: AccountQuotaWindowDto,
}
impl From<&AccountDto> for RawAccountDto {
	fn from(account: &AccountDto) -> Self {
		Self {
			account_id: account.account_id.clone(),
			display_label: account.display_label.clone(),
			enabled: account.enabled,
			account_revision: account.account_revision,
			observed_state: account.observed_state,
			lifecycle_readiness: account.lifecycle_readiness,
			credential_binding: account.credential_binding.clone(),
			unsettled_operation: account.unsettled_operation.clone(),
			five_hour_quota: account.five_hour_quota,
			seven_day_quota: account.seven_day_quota,
		}
	}
}
impl From<RawAccountDto> for AccountDto {
	fn from(account: RawAccountDto) -> Self {
		Self {
			account_id: account.account_id,
			display_label: account.display_label,
			enabled: account.enabled,
			account_revision: account.account_revision,
			observed_state: account.observed_state,
			lifecycle_readiness: account.lifecycle_readiness,
			credential_binding: account.credential_binding,
			unsettled_operation: account.unsettled_operation,
			five_hour_quota: account.five_hour_quota,
			seven_day_quota: account.seven_day_quota,
		}
	}
}
impl Serialize for AccountDto {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		validate_account_dto(self).map_err(S::Error::custom)?;
		RawAccountDto::from(self).serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let account = Self::from(RawAccountDto::deserialize(deserializer)?);
		validate_account_dto(&account).map_err(D::Error::custom)?;
		Ok(account)
	}
}

/// User-owned initial account selection mode.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "mode", content = "account_id", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountSelectionModeDto {
	/// Select only one configured account.
	Fixed(EntityId),
	/// Select the first eligible account in the complete order.
	Balanced,
}

/// Versioned deterministic account routing controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRoutingControlDto {
	/// Optimistic routing-control revision.
	pub revision: EntityRevision,
	/// Current initial-selection mode.
	pub mode: AccountSelectionModeDto,
	/// Complete deterministic order of visible accounts.
	pub order: Vec<EntityId>,
}
impl Serialize for AccountRoutingControlDto {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		struct RawRoutingControl<'a> {
			revision: EntityRevision,
			mode: &'a AccountSelectionModeDto,
			order: &'a [EntityId],
		}

		validate_routing_control(self).map_err(S::Error::custom)?;
		RawRoutingControl { revision: self.revision, mode: &self.mode, order: &self.order }
			.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountRoutingControlDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawRoutingControl {
			revision: EntityRevision,
			mode: AccountSelectionModeDto,
			order: Vec<EntityId>,
		}

		let raw = RawRoutingControl::deserialize(deserializer)?;
		let routing = Self { revision: raw.revision, mode: raw.mode, order: raw.order };
		validate_routing_control(&routing).map_err(D::Error::custom)?;
		Ok(routing)
	}
}

/// Closed recovery action returned when initial selection cannot proceed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSelectionRecoveryDto {
	/// Select an existing account in fixed mode.
	ConfigureFixedAccount,
	/// Enable the selected account.
	EnableAccount,
	/// Install credentials for an account.
	EnrollCredentials,
	/// Reconcile or cancel an unsettled credential operation.
	ResolveCredentialOperation,
	/// Restore registry and host-store agreement.
	RepairCredentialStore,
	/// Restore exact provider identity agreement.
	RestoreProviderAgreement,
	/// Refresh both required quota observations.
	RefreshQuota,
	/// Install a Codex build with the required callback capability.
	UpgradeCodex,
}

/// Deterministic initial-selection result. No fallback or wake is implied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountInitialSelectionResult {
	/// One account is ready for initial work admission.
	Selected {
		/// Canonical selected account identity.
		account_id: EntityId,
		/// Exact selected account revision.
		account_revision: EntityRevision,
	},
	/// No account is ready and one explicit action is required.
	RecoveryRequired {
		/// Account that requires the action, when one can be selected.
		account_id: Option<EntityId>,
		/// Stable operator recovery action.
		action: AccountSelectionRecoveryDto,
	},
	/// Initial selection could not be evaluated safely.
	Unavailable,
}
impl Serialize for AccountInitialSelectionResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum RawResult<'a> {
			Selected {
				account_id: &'a EntityId,
				account_revision: EntityRevision,
			},
			RecoveryRequired {
				#[serde(skip_serializing_if = "Option::is_none")]
				account_id: &'a Option<EntityId>,
				action: AccountSelectionRecoveryDto,
			},
			Unavailable,
		}

		validate_initial_selection_result(self).map_err(S::Error::custom)?;
		let raw = match self {
			Self::Selected { account_id, account_revision } =>
				RawResult::Selected { account_id, account_revision: *account_revision },
			Self::RecoveryRequired { account_id, action } =>
				RawResult::RecoveryRequired { account_id, action: *action },
			Self::Unavailable => RawResult::Unavailable,
		};
		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountInitialSelectionResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum RawResult {
			Selected { account_id: EntityId, account_revision: EntityRevision },
			RecoveryRequired { account_id: Option<EntityId>, action: AccountSelectionRecoveryDto },
			Unavailable,
		}

		let result = match RawResult::deserialize(deserializer)? {
			RawResult::Selected { account_id, account_revision } =>
				Self::Selected { account_id, account_revision },
			RawResult::RecoveryRequired { account_id, action } =>
				Self::RecoveryRequired { account_id, action },
			RawResult::Unavailable => Self::Unavailable,
		};
		validate_initial_selection_result(&result).map_err(D::Error::custom)?;
		Ok(result)
	}
}

/// Narrow, typed manual recovery actions for one unsettled credential operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountManualRecoveryActionDto {
	/// Re-read PostgreSQL and the exact host-store version and settle only a proven state.
	ReconcileExactStoreState,
	/// Cancel an operation only when the daemon proves that no external effect began.
	CancelBeforeEffect,
}

/// Closed account read result without credential material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountsResult {
	/// Complete visible account rows and matching routing controls.
	Available {
		/// Bounded visible account projections.
		accounts: Vec<AccountDto>,
		/// Routing controls with an exact account permutation.
		routing: AccountRoutingControlDto,
	},
	/// The account authority could not return a safe snapshot.
	Unavailable,
}

/// Closed account inspection result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountInspectResult {
	/// The requested account exists.
	Available(Box<AccountDto>),
	/// No visible account has the requested identity.
	NotFound,
	/// The account authority could not return a safe result.
	Unavailable,
}

impl Serialize for AccountsResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum Raw<'a> {
			Available { accounts: &'a [AccountDto], routing: &'a AccountRoutingControlDto },
			Unavailable,
		}
		let raw = match self {
			Self::Available { accounts, routing } => {
				validate_accounts_result(accounts, routing).map_err(S::Error::custom)?;
				Raw::Available { accounts, routing }
			},
			Self::Unavailable => Raw::Unavailable,
		};
		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountsResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum Raw {
			Available { accounts: Vec<AccountDto>, routing: AccountRoutingControlDto },
			Unavailable,
		}
		match Raw::deserialize(deserializer)? {
			Raw::Available { accounts, routing } => {
				validate_accounts_result(&accounts, &routing).map_err(D::Error::custom)?;
				Ok(Self::Available { accounts, routing })
			},
			Raw::Unavailable => Ok(Self::Unavailable),
		}
	}
}

impl Serialize for AccountInspectResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum Raw<'a> {
			Available(&'a AccountDto),
			NotFound,
			Unavailable,
		}
		let raw = match self {
			Self::Available(account) => {
				validate_account_dto(account).map_err(S::Error::custom)?;
				Raw::Available(account)
			},
			Self::NotFound => Raw::NotFound,
			Self::Unavailable => Raw::Unavailable,
		};
		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountInspectResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum Raw {
			Available(Box<AccountDto>),
			NotFound,
			Unavailable,
		}
		match Raw::deserialize(deserializer)? {
			Raw::Available(account) => {
				validate_account_dto(&account).map_err(D::Error::custom)?;
				Ok(Self::Available(account))
			},
			Raw::NotFound => Ok(Self::NotFound),
			Raw::Unavailable => Ok(Self::Unavailable),
		}
	}
}

/// Successful typed manual recovery disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountManualRecoveryOutcomeDto {
	/// The exact host-store effect was proved and committed.
	Committed,
	/// The operation was proved effect-free and cancelled.
	Cancelled,
	/// Exact state could not be reconciled automatically.
	StillRequiresRecovery,
}

/// Live queries available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryPayload {
	/// Revalidate and return the bounded authoritative doctor/status report.
	GetDoctorStatus,
	/// Read one immutable V16 execution-route decision without acquiring execution authority.
	GetExecutionDecision {
		/// Stable V16 decision identity.
		decision_id: EntityId,
	},
	/// Read one bounded deterministic logical-conversation history page.
	GetConversationHistory {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
		/// Last fully applied keyset cursor, when continuing a page sequence.
		#[serde(skip_serializing_if = "Option::is_none")]
		after: Option<HistoryCursorToken>,
		/// Requested item count, bounded again by the daemon.
		page_size: u16,
	},
	/// Read one complete reset-card inventory.
	GetResetCards {
		/// Canonical vNext account UUID.
		account_id: EntityId,
	},
	/// Read one durable reset-card operation by its logical-command key.
	GetResetCardOperation {
		/// Stable key supplied to the original consume command.
		idempotency_key: IdempotencyKey,
	},
	/// List daemon-owned accounts and deterministic routing controls.
	ListAccounts,
	/// Inspect one daemon-owned account and exact lifecycle readiness.
	InspectAccount {
		/// Canonical account identity to inspect.
		account_id: EntityId,
	},
	/// Evaluate initial account selection without creating fallback or wake work.
	GetInitialAccountSelection,
}
impl QueryPayload {
	/// Whether this query existed in the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::GetDoctorStatus
			| Self::GetExecutionDecision { .. }
			| Self::GetConversationHistory { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 2 }),
			Self::GetResetCards { .. } | Self::GetResetCardOperation { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
			Self::ListAccounts | Self::InspectAccount { .. } | Self::GetInitialAccountSelection =>
				version_supports(version, ProtocolVersion { major: 1, minor: 4 }),
		}
	}
}

/// Commands available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandPayload {
	/// Refresh a bounded system-health observation through the common application boundary.
	RefreshSystemObservation {
		/// Foundation entity to observe.
		entity_id: EntityId,
	},
	/// Accept one explicit public reset-card selection for durable execution.
	ConsumeResetCard {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Exact public descriptor selected from a fresh complete inventory.
		descriptor: ResetCardDescriptorDto,
	},
	/// Enroll the explicit account from the normal Codex-owned shared auth file.
	EnrollAccountFromSharedCodex {
		/// Stable finite lifecycle operation identity.
		operation_id: EntityId,
		/// Canonical new account identity.
		account_id: EntityId,
		/// Non-secret operator label.
		display_label: WireText,
		/// Initial administrative admission switch.
		enabled: bool,
	},
	/// Import one owner-private daemon-opened credential file without carrying secret bytes.
	ImportAccountCredentialFile {
		/// Stable finite lifecycle operation identity.
		operation_id: EntityId,
		/// Canonical account identity.
		account_id: EntityId,
		/// Non-secret operator label.
		display_label: WireText,
		/// Initial administrative admission switch.
		enabled: bool,
		/// Owner-private path descriptor opened by the daemon.
		source_descriptor: WireText,
	},
	/// Rename one account under optimistic account revision.
	RenameAccount {
		/// Canonical account identity.
		account_id: EntityId,
		/// Replacement non-secret operator label.
		display_label: WireText,
	},
	/// Change administrative enablement under optimistic account revision.
	SetAccountEnabled {
		/// Canonical account identity.
		account_id: EntityId,
		/// Replacement admission switch.
		enabled: bool,
	},
	/// Delete one exact host bundle and tombstone its registry projection.
	LogoutAccount {
		/// Stable finite lifecycle operation identity.
		operation_id: EntityId,
		/// Canonical account identity.
		account_id: EntityId,
	},
	/// Select one fixed account under routing-control and account revision guards.
	SetFixedAccountSelection {
		/// Canonical fixed account identity.
		account_id: EntityId,
		/// Exact optimistic revision of the fixed account.
		expected_account_revision: EntityRevision,
	},
	/// Select balanced initial account routing under the routing-control revision guard.
	SetBalancedAccountSelection,
	/// Replace the complete deterministic user-owned account order.
	SetAccountOrder {
		/// Complete replacement visible-account order.
		order: Vec<EntityId>,
	},
	/// Proactively refresh one exact account through the serialized Account Service path.
	RefreshAccount {
		/// Stable finite lifecycle operation identity.
		operation_id: EntityId,
		/// Canonical account identity.
		account_id: EntityId,
	},
	/// Apply one narrow explicit recovery action to a nonterminal credential operation.
	RecoverAccountOperation {
		/// Stable lifecycle operation identity.
		operation_id: EntityId,
		/// Explicit bounded reconciliation action.
		action: AccountManualRecoveryActionDto,
	},
}
impl CommandPayload {
	/// Whether this command existed in the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::RefreshSystemObservation { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 1 }),
			Self::ConsumeResetCard { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
			Self::EnrollAccountFromSharedCodex { .. }
			| Self::ImportAccountCredentialFile { .. }
			| Self::RenameAccount { .. }
			| Self::SetAccountEnabled { .. }
			| Self::LogoutAccount { .. }
			| Self::SetFixedAccountSelection { .. }
			| Self::SetBalancedAccountSelection
			| Self::SetAccountOrder { .. }
			| Self::RefreshAccount { .. }
			| Self::RecoverAccountOperation { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 4 }),
		}
	}
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
	/// Fresh result of one live query observation.
	QueryResult(QueryResultEnvelope),
	/// Explicit protocol refusal.
	Refusal(RefusalEnvelope),
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

/// Event payloads available in this foundation slice.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventPayload {
	/// A foundation system observation was refreshed.
	SystemObservationRefreshed {
		/// Small human-readable foundation status.
		status: WireText,
	},
	/// A durable reset-card operation changed state.
	ResetCardOperationAccepted {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Public descriptor selected by the operator.
		descriptor: ResetCardDescriptorDto,
		/// Current durable operation state.
		state: ResetCardOperationResult,
	},
	/// A reset-card operation reached a terminal provider outcome.
	ResetCardConsumed {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Public descriptor selected by the operator.
		descriptor: ResetCardDescriptorDto,
		/// Closed terminal provider outcome.
		outcome: ResetCardOutcome,
	},
	/// One account registry projection changed.
	AccountChanged {
		/// Complete credential-negative account projection.
		account: Box<AccountDto>,
	},
	/// One account was logged out and removed from the non-tombstoned account universe.
	AccountLoggedOut {
		/// Canonical logged-out account identity.
		account_id: EntityId,
		/// Exact committed tombstone revision.
		tombstone_revision: EntityRevision,
	},
	/// User-owned account routing controls changed.
	AccountRoutingChanged {
		/// Complete updated routing controls.
		routing: AccountRoutingControlDto,
	},
	/// One explicit account-operation recovery action completed.
	AccountOperationRecovered {
		/// Stable recovered operation identity.
		operation_id: EntityId,
		/// Finite recovery disposition.
		outcome: AccountManualRecoveryOutcomeDto,
	},
}
impl EventPayload {
	/// Whether this event can be decoded by the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::SystemObservationRefreshed { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 1 }),
			Self::ResetCardOperationAccepted { .. } | Self::ResetCardConsumed { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
			Self::AccountChanged { .. }
			| Self::AccountLoggedOut { .. }
			| Self::AccountRoutingChanged { .. }
			| Self::AccountOperationRecovered { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 4 }),
		}
	}
}

/// Publication-epoch receipt disposition for one command attempt.
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

/// High-level command outcome classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
	/// Application execution completed successfully.
	Succeeded,
	/// Protocol or application guards rejected execution.
	Rejected,
	/// Application execution could not establish whether durable acceptance committed.
	AcceptanceUnknown,
}

/// Typed successful command results.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultPayload {
	/// A foundation system observation was refreshed.
	SystemObservationRefreshed {
		/// Small human-readable foundation status.
		status: WireText,
	},
	/// A reset-card operation was accepted into the durable daemon-owned worker.
	ResetCardOperationAccepted {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Public descriptor selected by the operator.
		descriptor: ResetCardDescriptorDto,
		/// Current durable state, normally prepared or a replayed terminal state.
		state: ResetCardOperationResult,
	},
	/// A reset-card operation reached a terminal provider outcome.
	ResetCardConsumed {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Public descriptor selected by the operator.
		descriptor: ResetCardDescriptorDto,
		/// Closed terminal provider outcome.
		outcome: ResetCardOutcome,
	},
	/// One account lifecycle or administration mutation completed.
	AccountChanged {
		/// Complete credential-negative account projection.
		account: Box<AccountDto>,
	},
	/// One account logout completed with an exact tombstone revision.
	AccountLoggedOut {
		/// Canonical logged-out account identity.
		account_id: EntityId,
		/// Exact committed tombstone revision.
		tombstone_revision: EntityRevision,
	},
	/// User-owned initial-selection controls were replaced atomically.
	AccountRoutingChanged {
		/// Complete updated routing controls.
		routing: AccountRoutingControlDto,
	},
	/// One explicit credential-operation recovery action completed.
	AccountOperationRecovered {
		/// Stable recovered operation identity.
		operation_id: EntityId,
		/// Finite recovery disposition.
		outcome: AccountManualRecoveryOutcomeDto,
	},
}

/// Typed live-query results available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryResultPayload {
	/// Bounded authoritative doctor/status readback.
	DoctorStatus(DoctorReport),
	/// Immutable execution-consumer and exact route-cause projection.
	ExecutionDecision(ExecutionDecisionResult),
	/// Bounded daemon-owned logical-conversation history result.
	ConversationHistory(ConversationHistoryResult),
	/// Complete reset-card inventory or a closed unavailable reason.
	ResetCards(ResetCardInventoryResult),
	/// Durable reset-card operation state.
	ResetCardOperation(ResetCardOperationResult),
	/// Daemon-owned accounts and user-owned routing controls.
	Accounts(AccountsResult),
	/// One account and exact lifecycle readiness.
	Account(AccountInspectResult),
	/// Deterministic initial account choice or typed recovery.
	InitialAccountSelection(AccountInitialSelectionResult),
}

/// Result of an immutable V16 route-decision observation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum ExecutionDecisionResult {
	/// Complete verified decision projection.
	Decision(ExecutionDecisionDto),
	/// Closed unavailable result without infrastructure detail.
	Unavailable {
		/// Stable reason class.
		error: ExecutionDecisionQueryError,
	},
}

/// Closed execution-decision query failure classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionDecisionQueryError {
	/// Decision identity was invalid or no exact decision exists.
	InvalidRequest,
	/// Authoritative PostgreSQL state was unavailable.
	ProductStateUnavailable,
	/// Persisted decision evidence failed integrity verification.
	IntegrityUnavailable,
}

/// Immutable V16 decision plus its exact ordinary or managed consumer.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ExecutionDecisionDto {
	/// Stable immutable decision identity.
	pub decision_id: EntityId,
	/// Exact consumer whose account decision was persisted.
	pub consumer: ExecutionConsumerDto,
	/// Cause-preserving route projection.
	pub route: ExecutionRouteDto,
}

/// Closed execution-consumer union. Ordinary Conversation work never implies a ManagedRun.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "identity", rename_all = "snake_case")]
pub enum ExecutionConsumerDto {
	/// One ordinary Conversation Turn with exact source RuntimeSession lineage.
	ConversationTurn {
		/// Conversation identity.
		conversation_id: EntityId,
		/// Positive Conversation revision.
		conversation_revision: i64,
		/// Source RuntimeSession identity.
		source_runtime_session_id: EntityId,
		/// Positive source RuntimeSession revision.
		source_runtime_session_revision: i64,
		/// Conversation-owned Turn identity.
		turn_id: EntityId,
	},
	/// One exact ManagedRun execution intent.
	ManagedRunExecution {
		/// ManagedRun identity.
		managed_run_id: EntityId,
		/// Positive ManagedRun revision.
		managed_run_revision: i64,
		/// Exact execution intent identity.
		managed_execution_id: EntityId,
	},
}

/// Cause-preserving V16 route projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ExecutionRouteDto {
	/// One independently eligible account was selected.
	Selected {
		/// Persisted selected account identity.
		account_id: EntityId,
		/// Exact positive quota exclusions that were skipped before selection.
		quota_exclusions: Vec<ExecutionQuotaExclusionDto>,
	},
	/// Every otherwise eligible account is blocked only by positive quota depletion.
	WaitingUsage {
		/// Earliest exact quota reset instant in Unix microseconds.
		ready_at_micros: i64,
		/// Complete exact account-scoped causes.
		causes: Vec<ExecutionRouteCauseDto>,
		/// Independent 300-minute and 10,080-minute depletion facts.
		quota_exclusions: Vec<ExecutionQuotaExclusionDto>,
	},
	/// Every otherwise eligible path is blocked only by unresolved execution authority.
	WaitingReconciliation {
		/// Complete exact account-scoped process or attempt causes.
		causes: Vec<ExecutionRouteCauseDto>,
	},
	/// No route exists and no wake or task failure is implied.
	NoRoute {
		/// Complete causes from the persisted policy-member universe.
		#[serde(deserialize_with = "deserialize_nonempty_route_causes")]
		causes: Vec<ExecutionRouteCauseDto>,
	},
}

fn deserialize_nonempty_route_causes<'de, D>(
	deserializer: D,
) -> Result<Vec<ExecutionRouteCauseDto>, D::Error>
where
	D: Deserializer<'de>,
{
	let causes = Vec::<ExecutionRouteCauseDto>::deserialize(deserializer)?;
	if causes.is_empty() {
		Err(D::Error::custom("NoRoute requires at least one exact cause"))
	} else {
		Ok(causes)
	}
}

/// One exact account-scoped blocker retained without category collapse.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ExecutionRouteCauseDto {
	/// Account path affected by this cause.
	pub account_id: EntityId,
	/// Exact typed blocker.
	pub blocker: ExecutionRouteBlockerDto,
}

/// Exact typed blockers that V16 can persist.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRouteBlockerDto {
	/// Persisted policy excludes the account.
	ExcludedByPolicy,
	/// Account observation is later than the decision clock.
	AccountFromFuture,
	/// Account observation or revision is stale.
	AccountStale,
	/// Account is known to be unavailable.
	AccountUnavailable,
	/// Account state is unknown.
	AccountUnknown,
	/// Account state reports depletion independently of quota facts.
	AccountDepleted,
	/// Account authentication failed.
	AccountAuthFailed,
	/// Account-owned plugin readiness is absent.
	AccountPluginUnready,
	/// Account is administratively disabled.
	AccountDisabled,
	/// Compatibility evidence is absent.
	EvidenceMissing,
	/// Compatibility evidence is later than the decision clock.
	EvidenceFromFuture,
	/// Compatibility evidence is stale.
	EvidenceStale,
	/// Evidence names a different account identity or revision.
	EvidenceAccountMismatch,
	/// Evidence names a different role or role-profile revision.
	EvidenceProfileMismatch,
	/// Evidence names a different exact provider build.
	EvidenceBuildMismatch,
	/// The exact 300-minute quota fact is absent.
	QuotaFiveHourMissing,
	/// The 300-minute quota observation is from the future.
	QuotaFiveHourFromFuture,
	/// The 300-minute quota observation is stale.
	QuotaFiveHourStale,
	/// The 300-minute quota value or confidence is unknown.
	QuotaFiveHourUnknown,
	/// The 300-minute quota reset is not in the future.
	QuotaFiveHourResetElapsed,
	/// Positive 300-minute quota evidence reports depletion.
	QuotaFiveHourDepleted,
	/// The exact 10,080-minute quota fact is absent.
	QuotaSevenDayMissing,
	/// The 10,080-minute quota observation is from the future.
	QuotaSevenDayFromFuture,
	/// The 10,080-minute quota observation is stale.
	QuotaSevenDayStale,
	/// The 10,080-minute quota value or confidence is unknown.
	QuotaSevenDayUnknown,
	/// The 10,080-minute quota reset is not in the future.
	QuotaSevenDayResetElapsed,
	/// Positive 10,080-minute quota evidence reports depletion.
	QuotaSevenDayDepleted,
	/// A required capability lacks positive applicable evidence.
	RequiredCapabilityUnsatisfied,
	/// Authentication is required or unresolved.
	AuthenticationRequired,
	/// Required plugin readiness is unresolved.
	PluginUnready,
	/// An exact dependency blocks this path.
	DependencyBlocked,
	/// Required approval is absent.
	ApprovalRequired,
	/// Explicit user input is required.
	UserRequired,
	/// External authority blocks this path.
	ExternalBlocked,
	/// Usage state lacks pure positive depletion evidence.
	UsageUnproven,
	/// ManagedRun reconciliation lacks exact unresolved process or attempt authority.
	ReconciliationUnproven,
	/// No execution-scoped independent Reviewer is available.
	ReviewerUnavailable,
	/// Independent review rejected the result.
	ReviewerFailed,
	/// Reviewer output is missing or ambiguous.
	ReviewerAmbiguous,
	/// ProcessGeneration authority is unresolved.
	ProcessGenerationUnresolved,
	/// No live fenced ProcessGeneration exists.
	ProcessGenerationUnavailable,
	/// The exact ProviderAttempt is unresolved.
	ProviderAttemptUnresolved,
	/// The exact consumer intent already has a terminal ProviderAttempt.
	ProviderAttemptCompleted,
}

/// One independently typed positive quota-depletion exclusion.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ExecutionQuotaExclusionDto {
	/// Account identity excluded by this exact fact.
	pub account_id: EntityId,
	/// Exact quota-window class.
	pub window: ExecutionQuotaWindowDto,
	/// Exact duration: 300 or 10,080 minutes.
	pub duration_minutes: u16,
	/// Positive source observation revision.
	pub observation_revision: i64,
	/// Exact future reset instant in Unix microseconds.
	pub resets_at_micros: i64,
}

/// Independent quota-window identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionQuotaWindowDto {
	/// Exact 300-minute pool.
	FiveHour,
	/// Exact 10,080-minute pool.
	SevenDay,
}

/// Result of a bounded Conversation-history observation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum ConversationHistoryResult {
	/// Successfully verified page.
	Page(ConversationHistoryPage),
	/// Closed unavailable result without infrastructure detail.
	Unavailable {
		/// Stable reason class.
		error: HistoryQueryError,
	},
}

/// Closed history query failure classes safe for remote clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryQueryError {
	/// Request identity, cursor, or bound was invalid.
	InvalidRequest,
	/// The bounded continuation inventory is temporarily exhausted.
	ResourceExhausted,
	/// Authoritative PostgreSQL product state was unavailable.
	ProductStateUnavailable,
	/// Referenced bytes or persisted metadata failed integrity verification.
	IntegrityUnavailable,
}

/// Bounded item payload projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum HistoryPayloadDto {
	/// Small inline normalized text.
	Inline {
		/// Validated text bytes.
		text: HistoryText,
	},
	/// Large content-addressed bytes, accessed through a future authenticated artifact route.
	Blob(HistoryBlobReference),
}

/// Wire projection of a normalized turn role.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryTurnRole {
	/// User-authored turn.
	User,
	/// Assistant-authored turn.
	Assistant,
	/// System-authored turn.
	System,
	/// Tool-authored turn.
	Tool,
}

/// Wire projection of side-effect uncertainty.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistorySideEffectState {
	/// No possible side effect.
	None,
	/// Possible side effect.
	Possible,
	/// Unknown side-effect state.
	Unknown,
}

/// Wire projection of normalized item classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryItemKindDto {
	/// Visible message.
	Message,
	/// Reasoning projection.
	Reasoning,
	/// Tool invocation.
	ToolCall,
	/// Tool result.
	ToolResult,
	/// Artifact reference.
	Artifact,
	/// Runtime status.
	Status,
}

/// Wire projection of item stream lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryItemStatusDto {
	/// More correlated updates may arrive.
	Streaming,
	/// Item completed.
	Completed,
	/// Item failed.
	Failed,
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
	/// The application could not establish whether durable acceptance committed.
	AcceptanceUnknown,
	/// A stable account-domain guard rejected the logical command before a new effect.
	AccountCommandRejected {
		/// Stable account-domain rejection.
		rejection: AccountCommandRejectionDto,
		/// Current entity revision, when the guard established one.
		#[serde(skip_serializing_if = "Option::is_none")]
		actual_revision: Option<EntityRevision>,
	},
}

/// Closed account command rejection classification. Clients do not infer lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountCommandRejectionDto {
	/// The command shape or bounded input is invalid.
	InvalidRequest,
	/// The requested account does not exist.
	AccountNotFound,
	/// The expected account revision does not match current state.
	StaleAccount,
	/// The expected routing-control revision does not match current state.
	StaleRoutingControl,
	/// Existing work prevents logout.
	AccountInUse,
	/// Another lifecycle operation is unsettled.
	OperationUnsettled,
	/// The requested lifecycle operation does not exist.
	OperationNotFound,
	/// The account has no current credential binding.
	CredentialAbsent,
	/// The host credential store could not complete the request.
	CredentialStoreUnavailable,
	/// Provider identities do not agree.
	ProviderMismatch,
	/// Another lifecycle gate prevents the request.
	LifecycleUnready,
	/// Routing order is not an exact visible-account permutation.
	RoutingOrderInvalid,
	/// An explicit reconciliation command is required.
	ManualRecoveryRequired,
}

/// Protocol-level refusal that guarantees no application mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Refusal {
	/// The requested version falls outside the compatibility window.
	UnsupportedVersion(VersionRefusal),
	/// The client pinned a different stable server host.
	ServerIdentityMismatch {
		/// Identity required by the client profile.
		expected: ServerId,
		/// Identity presented by this server.
		actual: ServerId,
	},
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
	let decoded = serde_json::from_str(message)?;
	validate_client_message(&decoded).map_err(|reason| {
		serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
	})?;
	Ok(decoded)
}

fn validate_client_message(message: &ClientMessage) -> Result<(), &'static str> {
	match message {
		ClientMessage::Hello(_) => Ok(()),
		ClientMessage::Query(query) => match &query.payload {
			QueryPayload::GetResetCards { account_id }
			| QueryPayload::InspectAccount { account_id }
				if !is_canonical_uuid(account_id.as_str()) =>
				Err("account query identity is not canonical"),
			_ => Ok(()),
		},
		ClientMessage::Command(command) => validate_account_command(command),
	}
}

fn validate_account_command(command: &CommandEnvelope) -> Result<(), &'static str> {
	let positive_expected = command.expected_revision.is_some_and(|revision| revision.0 > 0);
	match &command.payload {
		CommandPayload::RefreshSystemObservation { .. } => Ok(()),
		CommandPayload::ConsumeResetCard { account_id, .. } => {
			if is_canonical_uuid(account_id.as_str()) && positive_expected {
				Ok(())
			} else {
				Err("reset-card account identity or revision is invalid")
			}
		},
		CommandPayload::EnrollAccountFromSharedCodex {
			operation_id,
			account_id,
			display_label,
			..
		} => validate_account_install_command(
			operation_id,
			account_id,
			display_label,
			command.expected_revision.is_none(),
		),
		CommandPayload::ImportAccountCredentialFile {
			operation_id,
			account_id,
			display_label,
			source_descriptor,
			..
		} => {
			validate_account_install_command(
				operation_id,
				account_id,
				display_label,
				command.expected_revision.is_none(),
			)?;
			let source = source_descriptor.as_str();
			if source.is_empty() || source.len() > 4096 || source.chars().any(char::is_control) {
				Err("account credential source descriptor is invalid")
			} else {
				Ok(())
			}
		},
		CommandPayload::RenameAccount { account_id, display_label } => {
			validate_canonical_account(account_id)?;
			validate_account_label(display_label)?;
			positive_expected.then_some(()).ok_or("account revision is required")
		},
		CommandPayload::SetAccountEnabled { account_id, .. } => {
			validate_canonical_account(account_id)?;
			positive_expected.then_some(()).ok_or("account revision is required")
		},
		CommandPayload::LogoutAccount { operation_id, account_id }
		| CommandPayload::RefreshAccount { operation_id, account_id } => {
			validate_canonical_operation(operation_id)?;
			validate_canonical_account(account_id)?;
			positive_expected.then_some(()).ok_or("account revision is required")
		},
		CommandPayload::SetFixedAccountSelection { account_id, expected_account_revision } => {
			validate_canonical_account(account_id)?;
			if !positive_expected || expected_account_revision.0 == 0 {
				return Err("account routing or target account revision is invalid");
			}
			Ok(())
		},
		CommandPayload::SetBalancedAccountSelection =>
			positive_expected.then_some(()).ok_or("account routing revision is required"),
		CommandPayload::SetAccountOrder { order } => {
			if !positive_expected
				|| order.len() > 512
				|| order.iter().any(|account_id| !is_canonical_uuid(account_id.as_str()))
			{
				return Err("account routing revision or order is invalid");
			}
			let unique = order.iter().map(EntityId::as_str).collect::<HashSet<_>>();
			if unique.len() != order.len() {
				return Err("account routing order contains duplicates");
			}
			Ok(())
		},
		CommandPayload::RecoverAccountOperation { operation_id, .. } => {
			validate_canonical_operation(operation_id)?;
			positive_expected.then_some(()).ok_or("account revision is required")
		},
	}
}

fn validate_account_install_command(
	operation_id: &EntityId,
	account_id: &EntityId,
	display_label: &WireText,
	expected_revision_absent: bool,
) -> Result<(), &'static str> {
	validate_canonical_operation(operation_id)?;
	validate_canonical_account(account_id)?;
	validate_account_label(display_label)?;
	expected_revision_absent.then_some(()).ok_or("new account command cannot carry a revision")
}

fn validate_canonical_account(account_id: &EntityId) -> Result<(), &'static str> {
	is_canonical_uuid(account_id.as_str()).then_some(()).ok_or("account identity is not canonical")
}

fn validate_canonical_operation(operation_id: &EntityId) -> Result<(), &'static str> {
	is_canonical_uuid(operation_id.as_str())
		.then_some(())
		.ok_or("account operation identity is not canonical")
}

fn validate_account_label(label: &WireText) -> Result<(), &'static str> {
	let label = label.as_str();
	if label.is_empty() || label.len() > 128 || label.chars().any(char::is_control) {
		Err("account display label is invalid")
	} else {
		Ok(())
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

fn validate_routing_control(routing: &AccountRoutingControlDto) -> Result<(), &'static str> {
	if routing.revision.0 == 0 {
		return Err("account routing revision is not positive");
	}
	if routing.order.len() > 512 {
		return Err("account routing order exceeds cardinality bound");
	}
	if routing.order.iter().any(|account_id| !is_canonical_uuid(account_id.as_str())) {
		return Err("account routing identity is not canonical");
	}
	let order = routing.order.iter().map(EntityId::as_str).collect::<HashSet<_>>();
	if order.len() != routing.order.len() {
		return Err("account routing order contains duplicates");
	}
	if let AccountSelectionModeDto::Fixed(account_id) = &routing.mode
		&& (!is_canonical_uuid(account_id.as_str()) || !order.contains(account_id.as_str()))
	{
		return Err("fixed account target is outside the routing universe");
	}
	Ok(())
}

fn validate_initial_selection_result(
	result: &AccountInitialSelectionResult,
) -> Result<(), &'static str> {
	match result {
		AccountInitialSelectionResult::Selected { account_id, account_revision } => {
			validate_canonical_account(account_id)?;
			(account_revision.0 > 0)
				.then_some(())
				.ok_or("selected account revision is not positive")
		},
		AccountInitialSelectionResult::RecoveryRequired { account_id, .. } =>
			account_id.as_ref().map_or(Ok(()), validate_canonical_account),
		AccountInitialSelectionResult::Unavailable => Ok(()),
	}
}

fn validate_accounts_result(
	accounts: &[AccountDto],
	routing: &AccountRoutingControlDto,
) -> Result<(), &'static str> {
	if accounts.len() > 512 {
		return Err("account result exceeds cardinality bound");
	}
	for account in accounts {
		validate_account_dto(account)?;
	}
	let universe =
		accounts.iter().map(|account| account.account_id.as_str()).collect::<HashSet<_>>();
	if universe.len() != accounts.len() {
		return Err("account result contains duplicate identities");
	}
	validate_routing_control(routing)?;
	if routing.order.len() != accounts.len() {
		return Err("account routing control is incomplete");
	}
	let order = routing.order.iter().map(EntityId::as_str).collect::<HashSet<_>>();
	if order.len() != routing.order.len()
		|| order != universe
		|| routing.order.iter().any(|account_id| !is_canonical_uuid(account_id.as_str()))
	{
		return Err("account routing order is not an exact permutation");
	}
	if let AccountSelectionModeDto::Fixed(account_id) = &routing.mode
		&& !universe.contains(account_id.as_str())
	{
		return Err("fixed account target is outside the account universe");
	}
	Ok(())
}

fn validate_account_dto(account: &AccountDto) -> Result<(), &'static str> {
	if !is_canonical_uuid(account.account_id.as_str()) || account.account_revision.0 == 0 {
		return Err("account identity or revision is invalid");
	}
	let label = account.display_label.as_str();
	if label.is_empty() || label.len() > 128 || label.chars().any(char::is_control) {
		return Err("account display label is invalid");
	}
	if matches!(account.lifecycle_readiness, AccountLifecycleReadinessDto::Tombstoned) {
		return Err("tombstoned account is not public");
	}
	if let Some(binding) = &account.credential_binding
		&& (binding.schema_version != 1
			|| binding.version == 0
			|| binding.provider_account_id.as_str().is_empty()
			|| binding.provider_account_id.as_str().len() > 512
			|| binding.provider_account_id.as_str().chars().any(char::is_control))
	{
		return Err("account credential binding is invalid");
	}
	if matches!(account.lifecycle_readiness, AccountLifecycleReadinessDto::Ready)
		&& (account.credential_binding.is_none() || account.unsettled_operation.is_some())
	{
		return Err("ready account has incomplete lifecycle state");
	}
	if matches!(account.lifecycle_readiness, AccountLifecycleReadinessDto::CredentialAbsent)
		&& account.credential_binding.is_some()
	{
		return Err("credential-absent account carries a binding");
	}
	if let Some(operation) = &account.unsettled_operation {
		if !is_canonical_uuid(operation.operation_id.as_str())
			|| operation.recovery_code.as_ref().is_some_and(|code| {
				code.as_str().is_empty()
					|| code.as_str().len() > 128
					|| code.as_str().chars().any(char::is_control)
			}) {
			return Err("account unsettled operation is invalid");
		}
		if (operation.phase == AccountOperationPhaseDto::RecoveryRequired)
			!= operation.recovery_code.is_some()
		{
			return Err("account recovery code does not match operation phase");
		}
	}
	if matches!(account.lifecycle_readiness, AccountLifecycleReadinessDto::OperationUnsettled)
		!= account.unsettled_operation.is_some()
	{
		return Err("account unsettled operation does not match lifecycle readiness");
	}
	validate_quota_window(account.five_hour_quota, 300)?;
	validate_quota_window(account.seven_day_quota, 10_080)?;
	Ok(())
}

fn validate_reset_card_inventory(
	account_id: &EntityId,
	account_revision: EntityRevision,
	available_count: u16,
	cards: &[ResetCardObservationDto],
	five_hour_quota: AccountQuotaWindowDto,
	seven_day_quota: AccountQuotaWindowDto,
) -> Result<(), &'static str> {
	if !is_canonical_uuid(account_id.as_str()) {
		return Err("reset-card account identity is not canonical");
	}
	if account_revision.0 == 0 {
		return Err("reset-card account revision is not positive");
	}
	if cards.len() > MAX_RESET_CARD_ITEMS {
		return Err("reset-card inventory exceeds item bound");
	}
	if usize::from(available_count) != cards.len() {
		return Err("reset-card inventory is incomplete");
	}

	let unique = cards.iter().map(|card| card.descriptor).collect::<HashSet<_>>();

	if unique.len() != cards.len() {
		return Err("reset-card inventory contains duplicates");
	}
	validate_quota_window(five_hour_quota, 300)?;
	validate_quota_window(seven_day_quota, 10_080)?;

	Ok(())
}

fn validate_reset_card_observation_failure(
	account_id: &EntityId,
	account_revision: EntityRevision,
	five_hour_quota: AccountQuotaWindowDto,
	seven_day_quota: AccountQuotaWindowDto,
) -> Result<(), &'static str> {
	if !is_canonical_uuid(account_id.as_str()) {
		return Err("reset-card account identity is not canonical");
	}
	if account_revision.0 == 0 {
		return Err("reset-card account revision is not positive");
	}
	validate_quota_window(five_hour_quota, 300)?;
	validate_quota_window(seven_day_quota, 10_080)?;

	Ok(())
}

fn validate_quota_window(
	quota: AccountQuotaWindowDto,
	expected_duration: u32,
) -> Result<(), &'static str> {
	if quota.duration_minutes != expected_duration {
		return Err("account quota duration is invalid");
	}
	match (quota.observed_at_unix_micros, quota.result) {
		(None, AccountQuotaStateDto::Unknown) => Ok(()),
		(Some(observed), AccountQuotaStateDto::Current { used_percent, resets_at_unix_micros })
			if observed > 0 && used_percent <= 100 && resets_at_unix_micros > observed =>
			Ok(()),
		(Some(observed), AccountQuotaStateDto::Stale { used_percent, resets_at_unix_micros })
			if observed > 0 && used_percent <= 100 && resets_at_unix_micros > observed =>
			Ok(()),
		(Some(observed), AccountQuotaStateDto::Error { .. }) if observed > 0 => Ok(()),
		_ => Err("account quota observation shape is invalid"),
	}
}

fn validate_public_quota_window(quota: AccountQuotaWindowDto) -> Result<(), &'static str> {
	if !matches!(quota.duration_minutes, 300 | 10_080) {
		return Err("account quota duration is invalid");
	}
	validate_quota_window(quota, quota.duration_minutes)
}

#[cfg(test)]
mod tests {
	use crate::{
		AccountCommandRejectionDto, AccountInitialSelectionResult, CURRENT_VERSION, CausationId,
		ClientCommandId, CommandError, CorrelationId, EntityId, EventPayload, HistoryCursorToken,
		HistoryText, IdempotencyKey, MAX_HISTORY_INLINE_BYTES, MAX_HISTORY_METADATA_FIELDS,
		MAX_HISTORY_METADATA_KEY_BYTES, MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE,
		MAX_IDEMPOTENCY_KEY_BYTES, MAX_RESET_CARD_ITEMS, MAX_WIRE_TEXT_BYTES,
		PREVIOUS_MINOR_VERSION, QueryId, ResetCardDescriptorDto, ResetCardOutcome, ResultPayload,
		ServerId, ServerInstanceId, WireText,
		wire::{
			ClientHello, ClientMessage, CommandEnvelope, CommandPayload, Cursor, EntityRevision,
			QueryEnvelope, QueryPayload, ResetCardInventoryResult, ResetCardOperationResult,
			ResumeCursor, decode_client_message,
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
	fn account_routing_commands_have_three_clean_break_wire_shapes() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let other_id =
			EntityId::new("11234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let fixed = CommandPayload::SetFixedAccountSelection {
			account_id: account_id.clone(),
			expected_account_revision: EntityRevision(7),
		};
		let balanced = CommandPayload::SetBalancedAccountSelection;
		let order =
			CommandPayload::SetAccountOrder { order: vec![other_id.clone(), account_id.clone()] };

		assert_eq!(
			serde_json::to_value(&fixed).unwrap(),
			serde_json::json!({
				"name": "set_fixed_account_selection",
				"arguments": {
					"account_id": account_id.as_str(),
					"expected_account_revision": 7,
				},
			}),
		);
		assert_eq!(
			serde_json::to_value(&balanced).unwrap(),
			serde_json::json!({"name": "set_balanced_account_selection"}),
		);
		assert_eq!(
			serde_json::to_value(&order).unwrap(),
			serde_json::json!({
				"name": "set_account_order",
				"arguments": {"order": [other_id.as_str(), account_id.as_str()]},
			}),
		);
		assert!(
			serde_json::from_value::<CommandPayload>(serde_json::json!({
				"name": "configure_account_routing",
				"arguments": {
					"expected_routing_revision": 1,
					"mode": {"mode": "balanced"},
					"order": [],
				},
			}))
			.is_err(),
		);

		let command = |payload, expected_revision| {
			ClientMessage::Command(CommandEnvelope {
				version: CURRENT_VERSION,
				client_command_id: ClientCommandId::new("routing-command").unwrap(),
				idempotency_key: IdempotencyKey::new("routing-key").unwrap(),
				expected_revision,
				correlation_id: CorrelationId::new("routing-command").unwrap(),
				causation_id: None,
				payload,
			})
		};
		let is_rejected = |payload, expected_revision| {
			let encoded = serde_json::to_string(&command(payload, expected_revision)).unwrap();
			decode_client_message(&encoded).is_err()
		};
		assert!(is_rejected(
			CommandPayload::SetFixedAccountSelection {
				account_id: account_id.clone(),
				expected_account_revision: EntityRevision(0),
			},
			Some(EntityRevision(1)),
		));
		assert!(is_rejected(CommandPayload::SetBalancedAccountSelection, Some(EntityRevision(0)),));
		assert!(is_rejected(
			CommandPayload::SetAccountOrder { order: vec![account_id.clone(), account_id] },
			Some(EntityRevision(1)),
		));

		let stale_account = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::StaleAccount,
			actual_revision: Some(EntityRevision(7)),
		};
		let stale_routing = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::StaleRoutingControl,
			actual_revision: Some(EntityRevision(3)),
		};
		assert_ne!(
			serde_json::to_value(stale_account).unwrap(),
			serde_json::to_value(stale_routing).unwrap(),
		);
	}

	#[test]
	fn query_wire_shape_is_live_and_round_trips() {
		let message = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("query-1").expect("bounded fixture ID"),
			payload: QueryPayload::GetDoctorStatus,
		});
		let encoded = serde_json::to_string(&message).unwrap();

		assert!(encoded.contains("\"type\":\"query\""));
		assert!(encoded.contains("\"name\":\"get_doctor_status\""));
		assert_eq!(serde_json::from_str::<ClientMessage>(&encoded).unwrap(), message);
	}

	#[test]
	fn public_wire_boundaries_reject_unknown_fields() {
		let command = serde_json::json!({
			"version": CURRENT_VERSION,
			"client_command_id": "command-1",
			"idempotency_key": "dedupe-1",
			"expected_revision": null,
			"correlation_id": "correlation-1",
			"causation_id": null,
			"payload": {"name": "refresh_system_observation", "arguments": {"entity_id": "system"}},
			"unknown": true,
		});
		let query = serde_json::json!({
			"version": CURRENT_VERSION,
			"query_id": "query-1",
			"payload": {"name": "get_doctor_status"},
			"unknown": true,
		});
		let message = serde_json::json!({
			"type": "hello",
			"body": {"version": CURRENT_VERSION, "resume": null},
			"unknown": true,
		});
		let operation = serde_json::json!({
			"state": "completed",
			"data": {"outcome": "reset", "unknown": true},
		});

		assert!(serde_json::from_value::<CommandEnvelope>(command).is_err());
		assert!(serde_json::from_value::<QueryEnvelope>(query).is_err());
		assert!(serde_json::from_value::<ClientMessage>(message).is_err());
		assert!(serde_json::from_value::<ResetCardOperationResult>(operation).is_err());
	}

	#[test]
	fn history_query_shape_and_payload_scalars_are_bounded() {
		let message = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("history-1").expect("bounded fixture ID"),
			payload: QueryPayload::GetConversationHistory {
				conversation_id: EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("bounded Conversation ID"),
				after: Some(
					HistoryCursorToken::new("v1:44000000-0000-4000-8000-000000000001")
						.expect("bounded cursor"),
				),
				page_size: MAX_HISTORY_PAGE_SIZE,
			},
		});
		let encoded = serde_json::to_string(&message).expect("history query serializes");

		assert!(encoded.contains("\"name\":\"get_conversation_history\""));
		assert_eq!(serde_json::from_str::<ClientMessage>(&encoded).unwrap(), message);
		assert!(HistoryText::new("x".repeat(MAX_HISTORY_INLINE_BYTES)).is_ok());
		assert!(HistoryText::new("x".repeat(MAX_HISTORY_INLINE_BYTES + 1)).is_err());
		assert!(HistoryCursorToken::new("x".repeat(128)).is_ok());
		assert!(HistoryCursorToken::new("x".repeat(129)).is_err());
		assert!(
			serde_json::from_str::<super::Sha256Digest>(&format!("\"{}\"", "a".repeat(64))).is_ok()
		);
		assert!(
			serde_json::from_str::<super::Sha256Digest>(&format!("\"{}\"", "g".repeat(64)))
				.is_err()
		);
		assert!(serde_json::from_str::<super::HistoryBlobLength>("0").is_err());
		assert!(serde_json::from_str::<super::HistoryBlobLength>("67108865").is_err());
		assert!(serde_json::from_str::<super::HistoryMediaType>("\"text/plain\"").is_ok());
		assert!(serde_json::from_str::<super::HistoryMediaType>("\"not a media type\"").is_err());

		let item = serde_json::json!({
			"history_item_id":"item","turn_id":"turn","runtime_session_id":"session",
			"turn_role":"user","possible_side_effects":"none","kind":"message","status":"completed",
			"payload":{"kind":"inline","data":{"text":"ok"}},"media_type":"application/json",
			"metadata":{"source":"normalized","safe":true},"revision":1
		});
		let page = serde_json::json!({"items": vec![item; usize::from(MAX_HISTORY_PAGE_SIZE)+1]});

		assert!(serde_json::from_value::<super::ConversationHistoryPage>(page).is_err());
	}

	#[test]
	fn history_artifact_and_metadata_projection_are_closed_and_bounded() {
		let artifact = serde_json::json!({
			"artifact_id":"48000000-0000-4000-8000-000000000001","revision":1
		});
		let artifact_item = serde_json::json!({
			"history_item_id":"item","turn_id":"turn","runtime_session_id":"session",
			"turn_role":"tool","possible_side_effects":"none","kind":"artifact","status":"completed",
			"payload":{"kind":"inline","data":{"text":"artifact"}},"media_type":"text/plain",
			"metadata":{},"artifact":artifact,"revision":1
		});

		assert!(serde_json::from_value::<super::HistoryItemDto>(artifact_item.clone()).is_ok());

		let mut missing_reference = artifact_item.clone();

		missing_reference.as_object_mut().unwrap().remove("artifact");

		assert!(serde_json::from_value::<super::HistoryItemDto>(missing_reference).is_err());

		let mut wrong_kind = artifact_item.clone();

		wrong_kind["kind"] = serde_json::json!("message");

		assert!(serde_json::from_value::<super::HistoryItemDto>(wrong_kind).is_err());

		let mut zero_revision = artifact_item;

		zero_revision["artifact"]["revision"] = serde_json::json!(0);

		assert!(serde_json::from_value::<super::HistoryItemDto>(zero_revision).is_err());

		let unsafe_metadata = serde_json::json!({
			"history_item_id":"item","turn_id":"turn","runtime_session_id":"session",
			"turn_role":"user","possible_side_effects":"none","kind":"message","status":"completed",
			"payload":{"kind":"inline","data":{"text":"ok"}},"media_type":"application/json",
			"metadata":{"api_key":"forbidden"},"revision":1
		});

		assert!(serde_json::from_value::<super::HistoryItemDto>(unsafe_metadata).is_err());
		assert!(
			serde_json::from_value::<super::HistoryMetadata>(serde_json::json!({
				"note": "secret sauce",
				"summary": "token budget",
				"context": "session summary"
			}))
			.is_ok()
		);

		for metadata in [
			serde_json::json!({"token": "ordinary"}),
			serde_json::json!({"auth_session": "ordinary"}),
			serde_json::json!({"note": "Bearer abcdefgh"}),
			serde_json::json!({"note": "secret=abcd"}),
		] {
			assert!(serde_json::from_value::<super::HistoryMetadata>(metadata).is_err());
		}

		let too_many_metadata = serde_json::Value::Object(
			(0..=MAX_HISTORY_METADATA_FIELDS)
				.map(|index| (format!("field-{index}"), serde_json::Value::Bool(true)))
				.collect(),
		);
		let maximum_metadata = serde_json::Value::Object(
			(0..MAX_HISTORY_METADATA_FIELDS)
				.map(|index| (format!("field-{index}"), serde_json::Value::Bool(true)))
				.collect(),
		);

		assert!(serde_json::from_value::<super::HistoryMetadata>(maximum_metadata).is_ok());
		assert!(serde_json::from_value::<super::HistoryMetadata>(too_many_metadata).is_err());
		assert!(
			serde_json::from_value::<super::HistoryMetadata>(serde_json::json!({
				"source": "x".repeat(MAX_HISTORY_METADATA_VALUE_BYTES + 1)
			}))
			.is_err()
		);
		assert!(
			serde_json::from_value::<super::HistoryMetadata>(serde_json::json!({
				"é".repeat(MAX_HISTORY_METADATA_KEY_BYTES / 2): "é".repeat(MAX_HISTORY_METADATA_VALUE_BYTES / 2)
			}))
			.is_ok()
		);
		assert!(
			serde_json::from_value::<super::HistoryMetadata>(serde_json::json!({
				"é".repeat(MAX_HISTORY_METADATA_KEY_BYTES / 2 + 1): true
			}))
			.is_err()
		);
	}

	#[test]
	fn hello_wire_shape_is_a_stable_json_golden() {
		let message = ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: None,
			resume: Some(ResumeCursor {
				server_id: ServerId::new("server-a").expect("bounded fixture ID"),
				instance_id: Some(
					ServerInstanceId::new("instance-a").expect("bounded fixture instance ID"),
				),
				cursor: Cursor(42),
			}),
		});

		assert_eq!(
			serde_json::to_string(&message).unwrap(),
			concat!(
				r#"{"type":"hello","body":{"version":{"major":1,"minor":4},"#,
				r#""resume":{"server_id":"server-a","instance_id":"instance-a","cursor":42}}}"#,
			)
		);
	}

	#[test]
	fn previous_minor_hello_without_publication_epoch_decodes_compatibly() {
		let encoded = concat!(
			r#"{"type":"hello","body":{"version":{"major":1,"minor":3},"#,
			r#""resume":{"server_id":"server-a","cursor":42}}}"#,
		);
		let ClientMessage::Hello(hello) = serde_json::from_str(encoded).unwrap() else {
			panic!("expected hello");
		};
		let resume = hello.resume.expect("expected previous-minor resume cursor");

		assert_eq!(resume.instance_id, None);
	}

	#[test]
	fn human_readable_wire_text_is_mechanically_bounded() {
		assert!(WireText::new("x".repeat(MAX_WIRE_TEXT_BYTES)).is_ok());
		assert!(WireText::new("x".repeat(MAX_WIRE_TEXT_BYTES + 1)).is_err());
	}

	#[test]
	fn reset_card_command_has_a_credential_negative_json_golden() {
		let descriptor = ResetCardDescriptorDto::new(1_700_000_000, 1_700_003_600).unwrap();
		let message = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("reset-card-use:key-1").unwrap(),
			idempotency_key: IdempotencyKey::new("key-1").unwrap(),
			expected_revision: Some(EntityRevision(9)),
			correlation_id: CorrelationId::new("reset-card-use:key-1").unwrap(),
			causation_id: None,
			payload: CommandPayload::ConsumeResetCard {
				account_id: EntityId::new("40000000-0000-4000-8000-000000000001").unwrap(),
				descriptor,
			},
		});

		assert_eq!(
			serde_json::to_string(&message).unwrap(),
			concat!(
				r#"{"type":"command","body":{"version":{"major":1,"minor":4},"#,
				r#""client_command_id":"reset-card-use:key-1","idempotency_key":"key-1","#,
				r#""expected_revision":9,"correlation_id":"reset-card-use:key-1","#,
				r#""causation_id":null,"payload":{"name":"consume_reset_card","arguments":{"#,
				r#""account_id":"40000000-0000-4000-8000-000000000001","descriptor":{"#,
				r#""granted_at_unix_seconds":1700000000,"expires_at_unix_seconds":1700003600}}}}}"#,
			)
		);
		assert_eq!(
			serde_json::from_str::<ClientMessage>(&serde_json::to_string(&message).unwrap())
				.unwrap(),
			message
		);
	}

	#[test]
	fn reset_card_descriptors_and_keys_are_strictly_bounded() {
		assert!(ResetCardDescriptorDto::new(0, 1).is_ok());
		assert!(ResetCardDescriptorDto::new(-1, 1).is_err());
		assert!(ResetCardDescriptorDto::new(1, 1).is_err());
		assert!(ResetCardDescriptorDto::new(2, 1).is_err());
		for invalid in [
			serde_json::json!({"granted_at_unix_seconds":-1,"expires_at_unix_seconds":1}),
			serde_json::json!({"granted_at_unix_seconds":1,"expires_at_unix_seconds":1}),
			serde_json::json!({"granted_at_unix_seconds":1,"expires_at_unix_seconds":2,"credit_id":"forbidden"}),
		] {
			assert!(serde_json::from_value::<ResetCardDescriptorDto>(invalid).is_err());
		}

		assert!(IdempotencyKey::new("x".repeat(MAX_IDEMPOTENCY_KEY_BYTES)).is_ok());
		assert!(IdempotencyKey::new("").is_err());
		assert!(IdempotencyKey::new("x".repeat(MAX_IDEMPOTENCY_KEY_BYTES + 1)).is_err());
		assert_eq!(
			IdempotencyKey::new(" operator-key"),
			Err(super::IdempotencyKeyError::SurroundingWhitespace)
		);
		assert_eq!(
			IdempotencyKey::new("operator-key "),
			Err(super::IdempotencyKeyError::SurroundingWhitespace)
		);
		assert!(IdempotencyKey::new("line\nbreak").is_err());
		assert!(IdempotencyKey::new("control\u{7f}").is_err());
	}

	#[test]
	fn reset_card_inventories_are_strictly_bounded() {
		let account_id = "40000000-0000-4000-8000-000000000001";
		let card = serde_json::json!({
			"descriptor":{"granted_at_unix_seconds":1,"expires_at_unix_seconds":2}
		});
		let duplicate = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":2,
				"cards":[card.clone(),card.clone()],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		let incomplete = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":2,
				"cards":[card.clone()],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		let zero_revision = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":0,
				"available_count":1,
				"cards":[card.clone()],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		let oversized = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":u16::try_from(MAX_RESET_CARD_ITEMS + 1).unwrap(),
				"cards":vec![card; MAX_RESET_CARD_ITEMS + 1],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		let bounded_cards = (0..MAX_RESET_CARD_ITEMS)
			.map(|index| {
				serde_json::json!({
					"descriptor":{
						"granted_at_unix_seconds":index * 2,
						"expires_at_unix_seconds":index * 2 + 1
					}
				})
			})
			.collect::<Vec<_>>();
		let bounded = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":u16::try_from(MAX_RESET_CARD_ITEMS).unwrap(),
				"cards":bounded_cards,
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});

		assert_eq!(MAX_RESET_CARD_ITEMS, 64);
		assert!(serde_json::from_value::<ResetCardInventoryResult>(bounded).is_ok());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(duplicate).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(incomplete).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(zero_revision).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(oversized).is_err());
		assert_reset_card_outbound_bounds(account_id);
	}

	fn assert_reset_card_outbound_bounds(account_id: &str) {
		let outbound_cards = (0..MAX_RESET_CARD_ITEMS)
			.map(|index| super::ResetCardObservationDto {
				descriptor: ResetCardDescriptorDto::new(
					i64::try_from(index * 2).unwrap(),
					i64::try_from(index * 2 + 1).unwrap(),
				)
				.unwrap(),
			})
			.collect::<Vec<_>>();
		let bounded_outbound = ResetCardInventoryResult::Available {
			account_id: EntityId::new(account_id).unwrap(),
			account_revision: EntityRevision(1),
			available_count: u16::try_from(MAX_RESET_CARD_ITEMS).unwrap(),
			cards: outbound_cards.clone(),
			five_hour_quota: super::AccountQuotaWindowDto {
				duration_minutes: 300,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
			seven_day_quota: super::AccountQuotaWindowDto {
				duration_minutes: 10_080,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
		};
		let oversized_outbound = ResetCardInventoryResult::Available {
			account_id: EntityId::new(account_id).unwrap(),
			account_revision: EntityRevision(1),
			available_count: u16::try_from(MAX_RESET_CARD_ITEMS + 1).unwrap(),
			cards: outbound_cards
				.into_iter()
				.chain([super::ResetCardObservationDto {
					descriptor: ResetCardDescriptorDto::new(1_000, 1_001).unwrap(),
				}])
				.collect(),
			five_hour_quota: super::AccountQuotaWindowDto {
				duration_minutes: 300,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
			seven_day_quota: super::AccountQuotaWindowDto {
				duration_minutes: 10_080,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
		};
		let zero_revision_outbound = ResetCardInventoryResult::Available {
			account_id: EntityId::new(account_id).unwrap(),
			account_revision: EntityRevision(0),
			available_count: 0,
			cards: Vec::new(),
			five_hour_quota: super::AccountQuotaWindowDto {
				duration_minutes: 300,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
			seven_day_quota: super::AccountQuotaWindowDto {
				duration_minutes: 10_080,
				observed_at_unix_micros: None,
				result: super::AccountQuotaStateDto::Unknown,
			},
		};

		assert!(serde_json::to_value(bounded_outbound).is_ok());
		assert!(serde_json::to_value(oversized_outbound).is_err());
		assert!(serde_json::to_value(zero_revision_outbound).is_err());
	}

	#[test]
	fn v1_3_messages_keep_decoding_during_the_rolling_window() {
		let encoded = concat!(
			r#"{"type":"query","body":{"version":{"major":1,"minor":3},"query_id":"legacy","#,
			r#""payload":{"name":"get_doctor_status"}}}"#,
		);

		assert_eq!(
			serde_json::from_str::<ClientMessage>(encoded).unwrap(),
			ClientMessage::Query(QueryEnvelope {
				version: crate::PREVIOUS_MINOR_VERSION,
				query_id: QueryId::new("legacy").unwrap(),
				payload: QueryPayload::GetDoctorStatus,
			})
		);
	}

	#[test]
	fn minor_feature_gates_keep_v1_3_free_of_account_lifecycle_shapes() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let descriptor = ResetCardDescriptorDto::new(100, 200).expect("valid descriptor");

		assert!(QueryPayload::GetDoctorStatus.is_supported_in(PREVIOUS_MINOR_VERSION));
		assert!(
			QueryPayload::GetExecutionDecision {
				decision_id: EntityId::new("execution-decision").expect("bounded decision ID"),
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			QueryPayload::GetConversationHistory {
				conversation_id: EntityId::new("conversation").unwrap(),
				after: None,
				page_size: 1,
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			CommandPayload::ConsumeResetCard { account_id: account_id.clone(), descriptor }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			CommandPayload::RefreshSystemObservation { entity_id: account_id.clone() }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			EventPayload::ResetCardConsumed {
				account_id,
				descriptor,
				outcome: ResetCardOutcome::Reset,
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(!QueryPayload::ListAccounts.is_supported_in(PREVIOUS_MINOR_VERSION));
		assert!(
			!CommandPayload::SetAccountEnabled {
				account_id: EntityId::new("01234567-89ab-4def-8123-456789abcdef").unwrap(),
				enabled: false,
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			EventPayload::SystemObservationRefreshed { status: WireText::new("ready").unwrap() }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
	}

	#[test]
	fn account_selection_and_routing_results_reject_noncanonical_shapes() {
		let account_id = "01234567-89ab-4def-8123-456789abcdef";
		let other_id = "11234567-89ab-4def-8123-456789abcdef";
		let invalid_routing = serde_json::json!({
			"name": "account_routing_changed",
			"data": {
				"routing": {
					"revision": 1,
					"mode": {"mode": "fixed", "account_id": other_id},
					"order": [account_id]
				}
			}
		});
		let unknown_routing_field = serde_json::json!({
			"name": "account_routing_changed",
			"data": {
				"routing": {
					"revision": 1,
					"mode": {"mode": "balanced"},
					"order": [account_id]
				},
				"extra": true
			}
		});
		let zero_revision_selection = serde_json::json!({
			"outcome": "selected",
			"data": {"account_id": account_id, "account_revision": 0}
		});
		let unknown_selection_field = serde_json::json!({
			"outcome": "selected",
			"data": {"account_id": account_id, "account_revision": 1, "extra": true}
		});

		assert!(serde_json::from_value::<ResultPayload>(invalid_routing.clone()).is_err());
		assert!(serde_json::from_value::<EventPayload>(invalid_routing).is_err());
		assert!(serde_json::from_value::<ResultPayload>(unknown_routing_field.clone()).is_err());
		assert!(serde_json::from_value::<EventPayload>(unknown_routing_field).is_err());
		assert!(
			serde_json::from_value::<AccountInitialSelectionResult>(zero_revision_selection)
				.is_err()
		);
		assert!(
			serde_json::from_value::<AccountInitialSelectionResult>(unknown_selection_field)
				.is_err()
		);
	}
}
