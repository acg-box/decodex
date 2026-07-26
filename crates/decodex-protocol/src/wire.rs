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

/// Manual reset-card account admission state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetCardAdmissionState {
	/// The account can be observed and can consume a reset card.
	Available,
	/// The account has exhausted a quota but remains eligible for manual reset.
	Depleted,
}

/// One bounded account projection available to reset-card clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResetCardAccountDto {
	/// Canonical vNext account UUID.
	pub account_id: EntityId,
	/// Non-secret operator label.
	pub display_label: WireText,
	/// Current optimistic account revision.
	pub account_revision: EntityRevision,
	/// Closed manual reset-card admission state.
	pub admission_state: ResetCardAdmissionState,
}
impl<'de> Deserialize<'de> for ResetCardAccountDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawAccount {
			account_id: EntityId,
			display_label: WireText,
			account_revision: EntityRevision,
			admission_state: ResetCardAdmissionState,
		}

		let raw = RawAccount::deserialize(deserializer)?;

		require_canonical_account_id::<D::Error>(&raw.account_id)?;

		Ok(Self {
			account_id: raw.account_id,
			display_label: raw.display_label,
			account_revision: raw.account_revision,
			admission_state: raw.admission_state,
		})
	}
}

/// Bounded reset-card account discovery result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResetCardAccountsResult {
	/// Current manually admitted account projections.
	Available {
		/// At most [`MAX_RESET_CARD_ITEMS`] unique accounts.
		accounts: Vec<ResetCardAccountDto>,
	},
	/// Account discovery was unavailable without exposing infrastructure detail.
	Unavailable {
		/// Stable reason class.
		error: ResetCardError,
	},
}
impl ResetCardAccountsResult {
	/// Borrow account records when discovery succeeded.
	pub fn accounts(&self) -> Option<&[ResetCardAccountDto]> {
		match self {
			Self::Available { accounts } => Some(accounts),
			Self::Unavailable { .. } => None,
		}
	}
}
impl Serialize for ResetCardAccountsResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum RawResult<'a> {
			Available { accounts: &'a [ResetCardAccountDto] },
			Unavailable { error: ResetCardError },
		}

		let raw = match self {
			Self::Available { accounts } => {
				validate_reset_card_accounts(accounts).map_err(S::Error::custom)?;
				RawResult::Available { accounts }
			},
			Self::Unavailable { error } => RawResult::Unavailable { error: *error },
		};

		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for ResetCardAccountsResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum RawResult {
			Available { accounts: Vec<ResetCardAccountDto> },
			Unavailable { error: ResetCardError },
		}

		match RawResult::deserialize(deserializer)? {
			RawResult::Available { accounts } => {
				validate_reset_card_accounts(&accounts).map_err(D::Error::custom)?;

				Ok(Self::Available { accounts })
			},
			RawResult::Unavailable { error } => Ok(Self::Unavailable { error }),
		}
	}
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
			},
			Unavailable {
				error: ResetCardError,
			},
		}

		let raw = match self {
			Self::Available { account_id, account_revision, available_count, cards } => {
				validate_reset_card_inventory(
					account_id,
					*account_revision,
					*available_count,
					cards,
				)
				.map_err(S::Error::custom)?;
				RawResult::Available {
					account_id,
					account_revision: *account_revision,
					available_count: *available_count,
					cards,
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
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum RawResult {
			Available {
				account_id: EntityId,
				account_revision: EntityRevision,
				available_count: u16,
				cards: Vec<ResetCardObservationDto>,
			},
			Unavailable {
				error: ResetCardError,
			},
		}

		match RawResult::deserialize(deserializer)? {
			RawResult::Available { account_id, account_revision, available_count, cards } => {
				validate_reset_card_inventory(
					&account_id,
					account_revision,
					available_count,
					&cards,
				)
				.map_err(D::Error::custom)?;

				Ok(Self::Available { account_id, account_revision, available_count, cards })
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
#[serde(tag = "state", content = "data", rename_all = "snake_case")]
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
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
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

/// Live queries available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
pub enum QueryPayload {
	/// Revalidate and return the bounded authoritative doctor/status report.
	GetDoctorStatus,
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
	/// Discover vNext accounts admitted for manual reset-card use.
	ListResetCardAccounts,
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
}
impl QueryPayload {
	/// Whether this query existed in the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::GetDoctorStatus | Self::GetConversationHistory { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 2 }),
			Self::ListResetCardAccounts
			| Self::GetResetCards { .. }
			| Self::GetResetCardOperation { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
		}
	}
}

/// Commands available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
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
}
impl CommandPayload {
	/// Whether this command existed in the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::RefreshSystemObservation { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 1 }),
			Self::ConsumeResetCard { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
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
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
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
}
impl EventPayload {
	/// Whether this event can be decoded by the negotiated minor protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		match self {
			Self::SystemObservationRefreshed { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 1 }),
			Self::ResetCardOperationAccepted { .. } | Self::ResetCardConsumed { .. } =>
				version_supports(version, ProtocolVersion { major: 1, minor: 3 }),
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
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
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
}

/// Typed live-query results available through the current V1 compatibility window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
pub enum QueryResultPayload {
	/// Bounded authoritative doctor/status readback.
	DoctorStatus(DoctorReport),
	/// Bounded daemon-owned logical-conversation history result.
	ConversationHistory(ConversationHistoryResult),
	/// Bounded vNext accounts admitted for manual reset-card use.
	ResetCardAccounts(ResetCardAccountsResult),
	/// Complete reset-card inventory or a closed unavailable reason.
	ResetCards(ResetCardInventoryResult),
	/// Durable reset-card operation state.
	ResetCardOperation(ResetCardOperationResult),
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
	serde_json::from_str(message)
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

fn require_canonical_account_id<E>(account_id: &EntityId) -> Result<(), E>
where
	E: serde::de::Error,
{
	if is_canonical_uuid(account_id.as_str()) {
		Ok(())
	} else {
		Err(E::custom("reset-card account identity is not canonical"))
	}
}

fn validate_reset_card_accounts(accounts: &[ResetCardAccountDto]) -> Result<(), &'static str> {
	if accounts.len() > MAX_RESET_CARD_ITEMS {
		return Err("reset-card account result exceeds item bound");
	}
	if accounts.iter().any(|account| !is_canonical_uuid(account.account_id.as_str())) {
		return Err("reset-card account identity is not canonical");
	}
	if accounts.iter().any(|account| account.account_revision.0 == 0) {
		return Err("reset-card account revision is not positive");
	}

	let unique = accounts.iter().map(|account| account.account_id.as_str()).collect::<HashSet<_>>();

	if unique.len() != accounts.len() {
		return Err("reset-card account result contains duplicates");
	}

	Ok(())
}

fn validate_reset_card_inventory(
	account_id: &EntityId,
	account_revision: EntityRevision,
	available_count: u16,
	cards: &[ResetCardObservationDto],
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

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::{
		CURRENT_VERSION, CausationId, ClientCommandId, CorrelationId, EntityId, EventPayload,
		HistoryCursorToken, HistoryText, IdempotencyKey, MAX_HISTORY_INLINE_BYTES,
		MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE, MAX_IDEMPOTENCY_KEY_BYTES,
		MAX_RESET_CARD_ITEMS, MAX_WIRE_TEXT_BYTES, PREVIOUS_MINOR_VERSION, QueryId,
		ResetCardDescriptorDto, ResetCardOutcome, ServerId, ServerInstanceId, WireText,
		wire::{
			ClientHello, ClientMessage, CommandEnvelope, CommandPayload, Cursor, EntityRevision,
			QueryEnvelope, QueryPayload, ResetCardInventoryResult, ResumeCursor,
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
				r#"{"type":"hello","body":{"version":{"major":1,"minor":3},"#,
				r#""resume":{"server_id":"server-a","instance_id":"instance-a","cursor":42}}}"#,
			)
		);
	}

	#[test]
	fn previous_minor_hello_without_publication_epoch_decodes_compatibly() {
		let encoded = concat!(
			r#"{"type":"hello","body":{"version":{"major":1,"minor":2},"#,
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
				r#"{"type":"command","body":{"version":{"major":1,"minor":3},"#,
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
				"cards":[card.clone(),card.clone()]
			}
		});
		let incomplete = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":2,
				"cards":[card.clone()]
			}
		});
		let zero_revision = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":0,
				"available_count":1,
				"cards":[card.clone()]
			}
		});
		let oversized = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"available_count":u16::try_from(MAX_RESET_CARD_ITEMS + 1).unwrap(),
				"cards":vec![card; MAX_RESET_CARD_ITEMS + 1]
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
				"cards":bounded_cards
			}
		});

		assert_eq!(MAX_RESET_CARD_ITEMS, 64);
		assert!(serde_json::from_value::<ResetCardInventoryResult>(bounded).is_ok());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(duplicate).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(incomplete).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(zero_revision).is_err());
		assert!(serde_json::from_value::<ResetCardInventoryResult>(oversized).is_err());

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
		};
		let zero_revision_outbound = ResetCardInventoryResult::Available {
			account_id: EntityId::new(account_id).unwrap(),
			account_revision: EntityRevision(0),
			available_count: 0,
			cards: Vec::new(),
		};

		assert!(serde_json::to_value(bounded_outbound).is_ok());
		assert!(serde_json::to_value(oversized_outbound).is_err());
		assert!(serde_json::to_value(zero_revision_outbound).is_err());
	}

	#[test]
	fn reset_card_account_inventories_are_strictly_bounded() {
		let account_id = "40000000-0000-4000-8000-000000000001";
		let account = serde_json::json!({
			"account_id":account_id,
			"display_label":"Primary",
			"account_revision":1,
			"admission_state":"depleted"
		});
		let duplicate_accounts = serde_json::json!({
			"outcome":"available",
			"data":{"accounts":[account.clone(),account.clone()]}
		});
		let oversized_accounts = serde_json::json!({
			"outcome":"available",
			"data":{"accounts":vec![account; MAX_RESET_CARD_ITEMS + 1]}
		});
		let zero_revision_accounts = serde_json::json!({
			"outcome":"available",
			"data":{"accounts":[{
				"account_id":account_id,
				"display_label":"Primary",
				"account_revision":0,
				"admission_state":"depleted"
			}]}
		});
		let bounded_accounts = (0..MAX_RESET_CARD_ITEMS)
			.map(|index| {
				serde_json::json!({
					"account_id":format!("40000000-0000-4000-8000-{index:012x}"),
					"display_label":format!("Account {index}"),
					"account_revision":1,
					"admission_state":"available"
				})
			})
			.collect::<Vec<_>>();
		let bounded_accounts = serde_json::json!({
			"outcome":"available",
			"data":{"accounts":bounded_accounts}
		});

		assert!(serde_json::from_value::<super::ResetCardAccountsResult>(bounded_accounts).is_ok());
		assert!(
			serde_json::from_value::<super::ResetCardAccountsResult>(duplicate_accounts).is_err()
		);
		assert!(
			serde_json::from_value::<super::ResetCardAccountsResult>(oversized_accounts).is_err()
		);
		assert!(
			serde_json::from_value::<super::ResetCardAccountsResult>(zero_revision_accounts)
				.is_err()
		);

		let outbound_accounts = (0..MAX_RESET_CARD_ITEMS)
			.map(|index| super::ResetCardAccountDto {
				account_id: EntityId::new(format!("40000000-0000-4000-8000-{index:012x}")).unwrap(),
				display_label: WireText::new(format!("Account {index}")).unwrap(),
				account_revision: EntityRevision(1),
				admission_state: super::ResetCardAdmissionState::Available,
			})
			.collect::<Vec<_>>();
		let bounded_outbound =
			super::ResetCardAccountsResult::Available { accounts: outbound_accounts.clone() };
		let oversized_outbound = super::ResetCardAccountsResult::Available {
			accounts: outbound_accounts
				.into_iter()
				.chain([super::ResetCardAccountDto {
					account_id: EntityId::new("40000000-0000-4000-8000-000000000100").unwrap(),
					display_label: WireText::new("Account 64").unwrap(),
					account_revision: EntityRevision(1),
					admission_state: super::ResetCardAdmissionState::Available,
				}])
				.collect(),
		};
		let zero_revision_outbound = super::ResetCardAccountsResult::Available {
			accounts: vec![super::ResetCardAccountDto {
				account_id: EntityId::new(account_id).unwrap(),
				display_label: WireText::new("Primary").unwrap(),
				account_revision: EntityRevision(0),
				admission_state: super::ResetCardAdmissionState::Depleted,
			}],
		};

		assert!(serde_json::to_value(bounded_outbound).is_ok());
		assert!(serde_json::to_value(oversized_outbound).is_err());
		assert!(serde_json::to_value(zero_revision_outbound).is_err());
	}

	#[test]
	fn v1_2_messages_keep_decoding_during_the_rolling_window() {
		let encoded = concat!(
			r#"{"type":"query","body":{"version":{"major":1,"minor":2},"query_id":"legacy","#,
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
	fn minor_feature_gates_keep_v1_2_free_of_reset_card_shapes() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let descriptor = ResetCardDescriptorDto::new(100, 200).expect("valid descriptor");

		assert!(QueryPayload::GetDoctorStatus.is_supported_in(PREVIOUS_MINOR_VERSION));
		assert!(
			QueryPayload::GetConversationHistory {
				conversation_id: EntityId::new("conversation").unwrap(),
				after: None,
				page_size: 1,
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(!QueryPayload::ListResetCardAccounts.is_supported_in(PREVIOUS_MINOR_VERSION));
		assert!(
			!CommandPayload::ConsumeResetCard { account_id: account_id.clone(), descriptor }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			CommandPayload::RefreshSystemObservation { entity_id: account_id.clone() }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			!EventPayload::ResetCardConsumed {
				account_id,
				descriptor,
				outcome: ResetCardOutcome::Reset,
			}
			.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
		assert!(
			EventPayload::SystemObservationRefreshed { status: WireText::new("ready").unwrap() }
				.is_supported_in(PREVIOUS_MINOR_VERSION)
		);
	}
}
