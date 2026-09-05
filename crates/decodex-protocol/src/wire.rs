//! Structured JSON envelopes for the exact-current V2.14 WebSocket connection.

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

use crate::{
	AccountLoginRequestEnvelope, AccountLoginResponseEnvelope, ConversationExecutionSettings,
	ConversationListCursor, ConversationListResult, ConversationListSize,
	ConversationRecoveryAction, ConversationResult, ConversationSummary, ConversationTurnOutcome,
	ConversationWorkingDirectory, DoctorReport, ProtocolVersion,
	program_cycle::{
		ProgramContinuationDraftDto, ProgramCycleDraftDto, ProgramCycleDto, ProgramCycleResult,
		ProgramListResult, ProgramReviewDraftDto,
	},
};

/// Maximum UTF-8 size of any human-readable text carried by V2.14.
pub const MAX_WIRE_TEXT_BYTES: usize = 4_096;
/// Maximum UTF-8 size of one logical-command idempotency key.
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
/// Maximum inline history bytes in one typed page item.
pub const MAX_HISTORY_INLINE_BYTES: usize = 16 * 1_024;
/// Maximum history items returned in one WebSocket query result. This keeps the worst-case
/// encoded result below the default 256-KiB transport frame bound.
pub const MAX_HISTORY_PAGE_SIZE: u16 = 8;
/// Maximum daily usage facts retained and returned for one account profile.
pub const MAX_ACCOUNT_PROFILE_DAILY_USAGE: usize = 36;
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

/// A string-backed wire scalar exceeded its V2.14 byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireScalarTooLong {
	actual_bytes: usize,
	maximum_bytes: usize,
}
impl WireScalarTooLong {
	pub(crate) fn new(actual_bytes: usize, maximum_bytes: usize) -> Self {
		Self { actual_bytes, maximum_bytes }
	}
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

	/// Borrow the bounded correlation identity.
	pub fn as_str(&self) -> &str {
		&self.0
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

	/// Borrow the bounded direct-cause identity.
	pub fn as_str(&self) -> &str {
		&self.0
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

/// Stable singleton identity for daemon-owned desktop settings.
pub const DESKTOP_SETTINGS_ENTITY_ID: &str = "desktop-settings";

/// Complete persistent desktop settings projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSettingsDto {
	/// Whether the sole Decodex application exposes its in-process menu-bar item.
	pub show_in_menu_bar: bool,
	/// Positive optimistic revision of this singleton projection.
	pub revision: EntityRevision,
}

impl DesktopSettingsDto {
	/// Construct one valid positive-revision desktop settings projection.
	pub const fn new(
		show_in_menu_bar: bool,
		revision: EntityRevision,
	) -> Result<Self, &'static str> {
		if revision.0 == 0 {
			return Err("desktop settings revision must be positive");
		}
		Ok(Self { show_in_menu_bar, revision })
	}

	/// Validate an untrusted decoded desktop settings projection.
	pub const fn is_valid(self) -> bool {
		self.revision.0 > 0
	}
}

/// Daemon-owned desktop settings readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopSettingsResult {
	/// Complete current persistent settings.
	Available(DesktopSettingsDto),
	/// The daemon's product store is unavailable.
	Unavailable,
}

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
	/// A V2.14 resume requires this field. Older hello envelopes can omit it
	/// only so negotiation can return a typed version refusal.
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
	/// Stable identity of this server host.
	pub server_id: ServerId,
	/// Ephemeral identity of the in-memory publication epoch.
	///
	/// This is present in the exact-current V2.14 welcome.
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

/// Bounded current reset-card observation for one account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResetCardInventoryResult {
	/// One observation whose descriptors are present only when detail completeness is true.
	Available {
		/// Canonical vNext account UUID.
		account_id: EntityId,
		/// Current optimistic account revision.
		account_revision: EntityRevision,
		/// Provider-reported current number of available cards, when reported.
		reported_available_count: Option<u64>,
		/// Whether every reported card has one complete unique public descriptor.
		details_complete: bool,
		/// Complete unique public card observations, present only when details are complete.
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
				reported_available_count: Option<u64>,
				details_complete: bool,
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
				reported_available_count,
				details_complete,
				cards,
				five_hour_quota,
				seven_day_quota,
			} => {
				validate_reset_card_inventory(
					account_id,
					*account_revision,
					*reported_available_count,
					*details_complete,
					cards,
					*five_hour_quota,
					*seven_day_quota,
				)
				.map_err(S::Error::custom)?;
				RawResult::Available {
					account_id,
					account_revision: *account_revision,
					reported_available_count: *reported_available_count,
					details_complete: *details_complete,
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
				reported_available_count: Option<u64>,
				details_complete: bool,
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
				reported_available_count,
				details_complete,
				cards,
				five_hour_quota,
				seven_day_quota,
			} => {
				validate_reset_card_inventory(
					&account_id,
					account_revision,
					reported_available_count,
					details_complete,
					&cards,
					five_hour_quota,
					seven_day_quota,
				)
				.map_err(D::Error::custom)?;

				Ok(Self::Available {
					account_id,
					account_revision,
					reported_available_count,
					details_complete,
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
	/// Compatibility result retained for decoding older durable/public records. The current
	/// direct provider API path never emits an executable or app-server version requirement.
	SchemaUnsupported,
	/// The upstream provider could not establish current state.
	ProviderUnavailable,
	/// The provider inventory was incomplete or ambiguous.
	InventoryIncomplete,
	/// The selected public descriptor no longer identifies the same available card.
	InventoryChanged,
	/// The daemon's bounded provider observation deadline elapsed.
	RequestTimedOut,
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
	/// Execute one memory-only account-login Start, Status, or Cancel exchange.
	AccountLogin(AccountLoginRequestEnvelope),
}

const fn version_supports_current(version: ProtocolVersion) -> bool {
	version.major == crate::CURRENT_VERSION.major && version.minor == crate::CURRENT_VERSION.minor
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
	/// The product store accepted the operation before an external effect.
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

/// Server-owned quota value classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountQuotaStateDto {
	/// No current public quota fact is available.
	Unknown,
	/// The retained quota fact is current.
	Current {
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

/// Email visibility selected explicitly by one account-profile query.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "visibility", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountProfileEmailDto {
	/// The provider email was intentionally omitted from this response.
	Redacted,
	/// The bounded provider email was explicitly requested.
	Visible(WireText),
}

/// One bounded provider daily-usage fact.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountProfileDailyUsageDto {
	/// Canonical provider calendar date in `YYYY-MM-DD` form.
	pub start_date: WireText,
	/// Non-negative tokens attributed to the date.
	pub tokens: u64,
}

/// One persisted account-profile snapshot plus current non-secret credential claims.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileDto {
	/// Canonical vNext account UUID.
	pub account_id: EntityId,
	/// Account revision fenced when this profile observation was persisted.
	pub account_revision: EntityRevision,
	/// Provider observation time in Unix microseconds.
	pub observed_at_unix_micros: i64,
	/// Explicitly redacted or visible current credential email.
	pub email: AccountProfileEmailDto,
	/// Current credential plan claim. This is not live capacity evidence.
	pub plan_type: Option<WireText>,
	/// Provider profile display name.
	pub display_name: Option<WireText>,
	/// Provider profile user name.
	pub username: Option<WireText>,
	/// Provider-reported lifetime token count.
	pub lifetime_tokens: Option<u64>,
	/// Provider-reported or daily-derived peak token count.
	pub peak_daily_tokens: Option<u64>,
	/// Provider-reported longest running task duration.
	pub longest_task_seconds: Option<u64>,
	/// Provider-reported current streak.
	pub current_streak_days: Option<u32>,
	/// Provider-reported longest streak.
	pub longest_streak_days: Option<u32>,
	/// At most 36 unique ascending daily usage facts.
	pub daily_usage: Vec<AccountProfileDailyUsageDto>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawAccountProfileDto {
	account_id: EntityId,
	account_revision: EntityRevision,
	observed_at_unix_micros: i64,
	email: AccountProfileEmailDto,
	#[serde(skip_serializing_if = "Option::is_none")]
	plan_type: Option<WireText>,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_name: Option<WireText>,
	#[serde(skip_serializing_if = "Option::is_none")]
	username: Option<WireText>,
	#[serde(skip_serializing_if = "Option::is_none")]
	lifetime_tokens: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	peak_daily_tokens: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	longest_task_seconds: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	current_streak_days: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	longest_streak_days: Option<u32>,
	daily_usage: Vec<AccountProfileDailyUsageDto>,
}
impl From<&AccountProfileDto> for RawAccountProfileDto {
	fn from(profile: &AccountProfileDto) -> Self {
		Self {
			account_id: profile.account_id.clone(),
			account_revision: profile.account_revision,
			observed_at_unix_micros: profile.observed_at_unix_micros,
			email: profile.email.clone(),
			plan_type: profile.plan_type.clone(),
			display_name: profile.display_name.clone(),
			username: profile.username.clone(),
			lifetime_tokens: profile.lifetime_tokens,
			peak_daily_tokens: profile.peak_daily_tokens,
			longest_task_seconds: profile.longest_task_seconds,
			current_streak_days: profile.current_streak_days,
			longest_streak_days: profile.longest_streak_days,
			daily_usage: profile.daily_usage.clone(),
		}
	}
}
impl From<RawAccountProfileDto> for AccountProfileDto {
	fn from(profile: RawAccountProfileDto) -> Self {
		Self {
			account_id: profile.account_id,
			account_revision: profile.account_revision,
			observed_at_unix_micros: profile.observed_at_unix_micros,
			email: profile.email,
			plan_type: profile.plan_type,
			display_name: profile.display_name,
			username: profile.username,
			lifetime_tokens: profile.lifetime_tokens,
			peak_daily_tokens: profile.peak_daily_tokens,
			longest_task_seconds: profile.longest_task_seconds,
			current_streak_days: profile.current_streak_days,
			longest_streak_days: profile.longest_streak_days,
			daily_usage: profile.daily_usage,
		}
	}
}
impl Serialize for AccountProfileDto {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		validate_account_profile(self).map_err(S::Error::custom)?;
		RawAccountProfileDto::from(self).serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountProfileDto {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let profile = Self::from(RawAccountProfileDto::deserialize(deserializer)?);
		validate_account_profile(&profile).map_err(D::Error::custom)?;
		Ok(profile)
	}
}

/// Closed account-profile observation failure safe for a local client.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProfileErrorDto {
	/// The request did not identify one canonical account.
	InvalidRequest,
	/// The account does not exist or is tombstoned.
	AccountUnavailable,
	/// Authoritative product state was unavailable.
	ProductStateUnavailable,
	/// The exact host credential item was absent, stale, or unavailable.
	CredentialUnavailable,
	/// The provider rejected the exact credential with HTTP 401.
	Unauthorized,
	/// The fixed provider endpoint could not complete successfully.
	ProviderUnavailable,
	/// The bounded provider payload did not satisfy the profile contract.
	ProtocolUnavailable,
	/// The account revision or provider binding changed before persistence.
	AccountChanged,
}

/// Independent per-account profile observation with bounded cached fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountProfileResult {
	/// A fresh provider observation was persisted before this response.
	Current(Box<AccountProfileDto>),
	/// A prior persisted snapshot is available after a typed refresh failure.
	Cached {
		/// The latest persisted profile snapshot.
		profile: Box<AccountProfileDto>,
		/// Stable reason the fresh observation did not complete.
		refresh_error: AccountProfileErrorDto,
	},
	/// No safe profile snapshot is available.
	Unavailable {
		/// Stable row-scoped failure.
		error: AccountProfileErrorDto,
		/// Explicitly redacted or visible current credential email.
		email: AccountProfileEmailDto,
		/// Current credential plan claim. This is not live capacity evidence.
		plan_type: Option<WireText>,
	},
}
impl Serialize for AccountProfileResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum Raw<'a> {
			Current(&'a AccountProfileDto),
			Cached {
				profile: &'a AccountProfileDto,
				refresh_error: AccountProfileErrorDto,
			},
			Unavailable {
				error: AccountProfileErrorDto,
				email: &'a AccountProfileEmailDto,
				#[serde(skip_serializing_if = "Option::is_none")]
				plan_type: Option<&'a WireText>,
			},
		}
		let raw = match self {
			Self::Current(profile) => {
				validate_account_profile(profile).map_err(S::Error::custom)?;
				Raw::Current(profile)
			},
			Self::Cached { profile, refresh_error } => {
				validate_account_profile(profile).map_err(S::Error::custom)?;
				Raw::Cached { profile, refresh_error: *refresh_error }
			},
			Self::Unavailable { error, email, plan_type } => {
				validate_account_profile_claims(email, plan_type.as_ref())
					.map_err(S::Error::custom)?;
				Raw::Unavailable { error: *error, email, plan_type: plan_type.as_ref() }
			},
		};
		raw.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for AccountProfileResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum Raw {
			Current(Box<AccountProfileDto>),
			Cached {
				profile: Box<AccountProfileDto>,
				refresh_error: AccountProfileErrorDto,
			},
			Unavailable {
				error: AccountProfileErrorDto,
				email: AccountProfileEmailDto,
				plan_type: Option<WireText>,
			},
		}
		match Raw::deserialize(deserializer)? {
			Raw::Current(profile) => {
				validate_account_profile(&profile).map_err(D::Error::custom)?;
				Ok(Self::Current(profile))
			},
			Raw::Cached { profile, refresh_error } => {
				validate_account_profile(&profile).map_err(D::Error::custom)?;
				Ok(Self::Cached { profile, refresh_error })
			},
			Raw::Unavailable { error, email, plan_type } => {
				validate_account_profile_claims(&email, plan_type.as_ref())
					.map_err(D::Error::custom)?;
				Ok(Self::Unavailable { error, email, plan_type })
			},
		}
	}
}

/// One required independently observed quota duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountQuotaWindowDto {
	/// Exact window duration. The V2.14 account contract accepts 300 and 10080 minutes only.
	pub duration_minutes: u32,
	/// Exact observation time, absent only when state is unknown.
	pub observed_at_unix_micros: Option<i64>,
	/// Closed current, unknown, or error result.
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
	/// Stable daemon-derived credential-negative account alias.
	pub alias: WireText,
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
	alias: WireText,
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
			alias: account.alias.clone(),
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
			alias: account.alias,
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
	/// Re-read durable account and credential state and settle only a proven state.
	ReconcileExactStoreState,
	/// Cancel an operation only when the daemon proves that no external effect began.
	CancelBeforeEffect,
}

/// Closed account read result without credential material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountsResult {
	/// Visible account rows with an independently readable routing capability.
	Available {
		/// Bounded visible account projections.
		accounts: Vec<AccountDto>,
		/// Routing controls with an exact account permutation, when that capability read
		/// succeeded.
		routing: Option<AccountRoutingControlDto>,
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

/// Read-only state of the normal shared Codex authentication projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexAuthProjectionResult {
	/// One daemon-owned account exactly matches the current shared Codex projection.
	Current {
		/// Canonical projected account identity.
		account_id: EntityId,
		/// Exact account revision represented by the projection.
		account_revision: EntityRevision,
		/// Credential-negative digest of the matched account binding.
		projection_digest: Sha256Digest,
	},
	/// The safe shared auth file is not managed by any current daemon account.
	Unmanaged,
	/// The shared auth state could not be read or matched safely.
	Unavailable,
}

impl Serialize for CodexAuthProjectionResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum Raw<'a> {
			Current {
				account_id: &'a EntityId,
				account_revision: EntityRevision,
				projection_digest: &'a Sha256Digest,
			},
			Unmanaged,
			Unavailable,
		}
		let raw = match self {
			Self::Current { account_id, account_revision, projection_digest } => {
				if !is_canonical_uuid(account_id.as_str()) || account_revision.0 == 0 {
					return Err(S::Error::custom("Codex auth projection is invalid"));
				}
				Raw::Current { account_id, account_revision: *account_revision, projection_digest }
			},
			Self::Unmanaged => Raw::Unmanaged,
			Self::Unavailable => Raw::Unavailable,
		};
		raw.serialize(serializer)
	}
}

impl<'de> Deserialize<'de> for CodexAuthProjectionResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
		enum Raw {
			Current {
				account_id: EntityId,
				account_revision: EntityRevision,
				projection_digest: Sha256Digest,
			},
			Unmanaged,
			Unavailable,
		}
		match Raw::deserialize(deserializer)? {
			Raw::Current { account_id, account_revision, projection_digest } => {
				if !is_canonical_uuid(account_id.as_str()) || account_revision.0 == 0 {
					return Err(D::Error::custom("Codex auth projection is invalid"));
				}
				Ok(Self::Current { account_id, account_revision, projection_digest })
			},
			Raw::Unmanaged => Ok(Self::Unmanaged),
			Raw::Unavailable => Ok(Self::Unavailable),
		}
	}
}

impl Serialize for AccountsResult {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Serialize)]
		#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
		enum Raw<'a> {
			Available { accounts: &'a [AccountDto], routing: Option<&'a AccountRoutingControlDto> },
			Unavailable,
		}
		let raw = match self {
			Self::Available { accounts, routing } => {
				validate_accounts_result(accounts, routing.as_ref()).map_err(S::Error::custom)?;
				Raw::Available { accounts, routing: routing.as_ref() }
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
			Available { accounts: Vec<AccountDto>, routing: Option<AccountRoutingControlDto> },
			Unavailable,
		}
		match Raw::deserialize(deserializer)? {
			Raw::Available { accounts, routing } => {
				validate_accounts_result(&accounts, routing.as_ref()).map_err(D::Error::custom)?;
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

/// One daemon account-observation change or bounded heartbeat.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountObservationSignal {
	/// Opaque daemon-lifetime generation of the observation cache.
	pub generation: u64,
}
impl AccountObservationSignal {
	/// Construct one bounded account-observation signal.
	pub const fn new(generation: u64) -> Self {
		Self { generation }
	}
}

/// Live queries available through the exact-current V2.14 protocol.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryPayload {
	/// Read the complete daemon-owned desktop settings projection.
	GetDesktopSettings,
	/// List bounded current Programs for the Factory selector.
	ListPrograms,
	/// Read one complete current Program causal projection.
	GetProgramCycle {
		/// Stable Program identity.
		program_id: EntityId,
	},
	/// List one bounded deterministic page of ordinary Task conversations.
	ListConversations {
		/// Last fully applied most-recent-first keyset position.
		#[serde(skip_serializing_if = "Option::is_none")]
		after: Option<ConversationListCursor>,
		/// Positive requested Conversation count inside the public bound.
		page_size: ConversationListSize,
	},
	/// Read one exact ordinary Conversation.
	GetConversation {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
	},
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
	/// Read one bounded current reset-card observation.
	GetResetCards {
		/// Canonical vNext account UUID.
		account_id: EntityId,
	},
	/// Read one durable reset-card operation by its logical-command key.
	GetResetCardOperation {
		/// Stable key supplied to the original consume command.
		idempotency_key: IdempotencyKey,
	},
	/// List daemon-owned accounts and independently readable routing controls.
	ListAccounts,
	/// Inspect one daemon-owned account and exact lifecycle readiness.
	InspectAccount {
		/// Canonical account identity to inspect.
		account_id: EntityId,
	},
	/// Observe one account's bounded provider profile independently from Reset Card inventory.
	GetAccountProfile {
		/// Canonical account identity to observe.
		account_id: EntityId,
		/// Whether the response may include the bounded current credential email.
		include_email: bool,
	},
	/// Evaluate initial account selection without creating fallback or wake work.
	GetInitialAccountSelection,
	/// Read the current shared Codex authentication projection without exposing credentials.
	GetCodexAuthProjection,
	/// Wait for daemon-owned account observations to advance or emit one bounded heartbeat.
	WaitForAccountObservation {
		/// Last daemon-lifetime generation applied by the caller.
		after_generation: u64,
		/// Optionally ask the daemon to schedule one coalesced observation before waiting.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		request_refresh: Option<bool>,
	},
}
impl QueryPayload {
	/// Whether this query is available in the exact-current protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		let _ = self;
		version_supports_current(version)
	}
}

/// Commands available through the exact-current V2.14 protocol.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandPayload {
	/// Replace the persistent menu-bar preference for the sole Decodex application.
	SetDesktopSettings {
		/// Whether Decodex.app shows its same-process status item.
		show_in_menu_bar: bool,
	},
	/// Atomically create one bounded pre-execution Program semantic chain.
	CreateProgramCycle {
		/// Complete V1 Program charter and finite causal chain.
		draft: Box<ProgramCycleDraftDto>,
	},
	/// Bind one built-in Domain Pack to an existing legacy Program exactly once.
	BindProgramDomainPack {
		/// Existing Program that currently has no Pack binding.
		program_id: EntityId,
		/// Daemon-owned built-in Pack selected by stable identifier.
		domain_pack_id: WireText,
	},
	/// Append one manually accepted next cycle to an exact reviewed Program revision.
	ContinueProgram {
		/// Complete next-cycle input with an exact predecessor Review.
		continuation: Box<ProgramContinuationDraftDto>,
	},
	/// Atomically attach required Evidence and one classified Program Review.
	RecordProgramReview {
		/// Complete terminal review input.
		review: Box<ProgramReviewDraftDto>,
	},
	/// Create one ordinary conversation and submit its first turn.
	CreateConversation {
		/// Caller-generated stable logical Conversation identity.
		conversation_id: EntityId,
		/// Optional exact causal WorkItem binding for a Factory execution.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		work_item_id: Option<EntityId>,
		/// Bounded user-authored message.
		message: HistoryText,
		/// Untrusted server-host working directory selected for this process lineage.
		working_directory: ConversationWorkingDirectory,
		/// Explicit execution settings for this user send.
		execution: ConversationExecutionSettings,
	},
	/// Resume the sole initial route for one routing-pending Conversation.
	ResumeConversationRouting {
		/// Stable routing-pending Conversation identity.
		conversation_id: EntityId,
	},
	/// Create and route one fresh successor for a waiting/no-route Conversation.
	CreateConversationRoutingSuccessor {
		/// Stable waiting/no-route source Conversation identity.
		conversation_id: EntityId,
	},
	/// Resume only initial session establishment from one selected decision.
	ResumeConversationEstablishment {
		/// Stable establishment-pending Conversation identity.
		conversation_id: EntityId,
	},
	/// Submit one subsequent turn on the exact existing Codex thread.
	SubmitConversationTurn {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
		/// Caller-generated stable logical Turn identity.
		turn_id: EntityId,
		/// Bounded user-authored message.
		message: HistoryText,
		/// Untrusted server-host working directory used if the thread must be re-established.
		working_directory: ConversationWorkingDirectory,
		/// Explicit execution settings for this user send.
		execution: ConversationExecutionSettings,
	},
	/// Reconcile one selected Decodex task with its exact Codex archive state.
	RefreshConversation {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
	},
	/// Archive one selected exact Codex thread and its Decodex projection.
	ArchiveConversation {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
	},
	/// Interrupt one exact active Conversation turn.
	InterruptConversation {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
		/// Exact active logical Turn identity.
		turn_id: EntityId,
	},
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
		/// Initial administrative admission switch.
		enabled: bool,
	},
	/// Import one owner-private daemon-opened credential file without carrying secret bytes.
	ImportAccountCredentialFile {
		/// Stable finite lifecycle operation identity.
		operation_id: EntityId,
		/// Canonical account identity.
		account_id: EntityId,
		/// Initial administrative admission switch.
		enabled: bool,
		/// Owner-private path descriptor opened by the daemon.
		source_descriptor: WireText,
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
	/// Refresh, project, and select one account as one daemon-owned Route command.
	RouteAccount {
		/// Canonical routed account identity.
		account_id: EntityId,
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
	/// Whether this command is available in the exact-current protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		let _ = self;
		version_supports_current(version)
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
	/// Current memory-only account-login status for one one-shot exchange.
	AccountLogin(AccountLoginResponseEnvelope),
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
	/// Persistent desktop settings changed.
	DesktopSettingsChanged {
		/// Complete current settings after the committed change.
		settings: DesktopSettingsDto,
	},
	/// One complete Program causal projection changed.
	ProgramCycleChanged {
		/// Current authoritative projection.
		cycle: Box<ProgramCycleDto>,
	},
	/// The bounded presentation projection of one ordinary Task conversation changed.
	ConversationChanged {
		/// Complete credential-negative ordinary projection plus local overlay.
		conversation: ConversationSummary,
	},
	/// One bounded user-visible assistant-message delta arrived for an active turn.
	ConversationMessageDelta {
		/// Exact logical Conversation identity.
		conversation_id: EntityId,
		/// Exact logical Turn identity.
		turn_id: EntityId,
		/// Bounded normalized assistant text delta.
		delta: HistoryText,
	},
	/// One ordinary Turn reached a definite evidence-backed terminal outcome.
	ConversationTurnFinished {
		/// Complete ordinary Conversation projection after the Turn settled.
		conversation: ConversationSummary,
		/// Exact logical Turn that settled.
		turn_id: EntityId,
		/// Definite positive or failed outcome.
		outcome: ConversationTurnOutcome,
	},
	/// One ordinary Conversation was verified archived and left the active projection.
	ConversationArchived {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
		/// Exact committed archived revision.
		conversation_revision: EntityRevision,
	},
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
	/// One daemon-owned Route command completed as one coherent projection.
	AccountRouted {
		/// Complete credential-negative routed account projection.
		account: Box<AccountDto>,
		/// Complete updated routing controls.
		routing: AccountRoutingControlDto,
		/// Credential-negative digest of the projected account binding.
		projection_digest: Sha256Digest,
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
	/// Whether this event is available in the exact-current protocol revision.
	pub const fn is_supported_in(&self, version: ProtocolVersion) -> bool {
		let _ = self;
		version_supports_current(version)
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
	/// Persistent desktop settings changed.
	DesktopSettingsChanged {
		/// Complete current settings after the committed change.
		settings: DesktopSettingsDto,
	},
	/// One Program semantic command committed.
	ProgramCycleChanged {
		/// Current authoritative projection.
		cycle: Box<ProgramCycleDto>,
	},
	/// An ordinary Conversation create or later-Turn command reached a closed accepted state.
	ConversationAccepted {
		/// Complete current ordinary projection after acceptance.
		conversation: ConversationSummary,
	},
	/// A waiting/no-route source was archived in favor of one routed successor.
	ConversationRoutingSuccessorAccepted {
		/// Archived source Conversation identity.
		source_conversation_id: EntityId,
		/// Exact archived source revision.
		source_conversation_revision: EntityRevision,
		/// Complete current projection of the direct successor.
		successor: ConversationSummary,
	},
	/// An interrupt request reached the exact daemon-local active Turn handle.
	ConversationInterruptAccepted {
		/// Complete current ordinary projection; completion later returns it to `ready`.
		conversation: ConversationSummary,
	},
	/// One explicit refresh or archive verified that the Codex thread is archived.
	ConversationArchived {
		/// Stable logical Conversation identity.
		conversation_id: EntityId,
		/// Exact committed archived revision.
		conversation_revision: EntityRevision,
	},
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
	/// A new enrollment request restored the one tombstoned account with the same provider binding.
	AccountRestored {
		/// New account identity proposed by the client before the provider binding was known.
		requested_account_id: EntityId,
		/// Restored canonical account projection and original stable identity.
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
	/// One daemon-owned Route command completed as one coherent projection.
	AccountRouted {
		/// Complete credential-negative routed account projection.
		account: Box<AccountDto>,
		/// Complete updated routing controls.
		routing: AccountRoutingControlDto,
		/// Credential-negative digest of the projected account binding.
		projection_digest: Sha256Digest,
	},
	/// One explicit credential-operation recovery action completed.
	AccountOperationRecovered {
		/// Stable recovered operation identity.
		operation_id: EntityId,
		/// Finite recovery disposition.
		outcome: AccountManualRecoveryOutcomeDto,
	},
}

impl ResultPayload {
	/// Whether this terminal result can evolve after publication.
	pub const fn is_evolving_receipt(&self) -> bool {
		false
	}
}

/// Typed live-query results available through the exact-current V2.14 protocol.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryResultPayload {
	/// Complete daemon-owned desktop settings projection.
	DesktopSettings(DesktopSettingsResult),
	/// Bounded current Program selector projection.
	Programs(ProgramListResult),
	/// One exact complete Program causal projection.
	ProgramCycle(ProgramCycleResult),
	/// Bounded current Conversation destination projection.
	Conversations(ConversationListResult),
	/// One exact ordinary Conversation readback.
	Conversation(ConversationResult),
	/// Bounded authoritative doctor/status readback.
	DoctorStatus(DoctorReport),
	/// Bounded daemon-owned logical-conversation history result.
	ConversationHistory(ConversationHistoryResult),
	/// Bounded current reset-card observation or a closed unavailable reason.
	ResetCards(ResetCardInventoryResult),
	/// Durable reset-card operation state.
	ResetCardOperation(ResetCardOperationResult),
	/// Daemon-owned accounts and user-owned routing controls.
	Accounts(AccountsResult),
	/// One account and exact lifecycle readiness.
	Account(AccountInspectResult),
	/// One independent bounded account-profile observation.
	AccountProfile(AccountProfileResult),
	/// Deterministic initial account choice or typed recovery.
	InitialAccountSelection(AccountInitialSelectionResult),
	/// Current shared Codex authentication projection.
	CodexAuthProjection(CodexAuthProjectionResult),
	/// One daemon account-observation change or bounded heartbeat.
	AccountObservation(AccountObservationSignal),
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
	/// Authoritative product state was unavailable.
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
impl HistoryPayloadDto {
	/// Borrow inline normalized text when this payload is not a blob reference.
	pub fn inline_text(&self) -> Option<&HistoryText> {
		match self {
			Self::Inline { text } => Some(text),
			Self::Blob(_) => None,
		}
	}
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
	/// Conversation execution was unavailable when this daemon process assembled its owners.
	ConversationUnavailable {
		/// Closed startup reason. No credential, path, account, or provider text is representable.
		unavailable_reason: crate::ConversationUnavailableReason,
	},
	/// The application could not establish whether durable acceptance committed.
	AcceptanceUnknown,
	/// Account selection or immutable account affinity requires an explicit user action.
	ConversationRecoveryRequired {
		/// Closed recovery action safe for direct presentation.
		action: ConversationRecoveryAction,
	},
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
	/// Another Decodex account already owns the current shared provider login.
	ProviderAlreadyEnrolled,
	/// Provider identities do not agree.
	ProviderMismatch,
	/// Another lifecycle gate prevents the request.
	LifecycleUnready,
	/// Official Codex or ChatGPT is running; the caller must quit it and retry.
	CodexIsRunning,
	/// The target account is administratively disabled.
	AccountDisabled,
	/// The target account has no stored credential.
	CredentialMissing,
	/// A prior refresh may have rotated the token without safely storing its successor.
	CredentialNeedsLogin,
	/// The provider rejected the target credential refresh.
	CredentialRefreshRejected,
	/// The provider refresh could not start or complete before any known effect.
	CredentialRefreshUnavailable,
	/// The shared auth file could not be read safely.
	AuthFileUnreadable,
	/// The shared auth source changed before projection.
	AuthFileChanged,
	/// Atomic auth-file replacement failed.
	AuthWriteFailed,
	/// Exact auth-file readback did not match the projected credential.
	AuthReadbackMismatch,
	/// Routing order is not an exact visible-account permutation.
	RoutingOrderInvalid,
	/// An explicit reconciliation command is required.
	ManualRecoveryRequired,
}

/// Protocol-level refusal that guarantees no application mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Refusal {
	/// The requested version differs from the one exact service version.
	ServiceVersionMismatch {
		/// Version requested by the client.
		requested: ProtocolVersion,
		/// Exact version supported by the service.
		supported: ProtocolVersion,
	},
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

/// Serialize a message using the only V2.14 wire encoding.
pub fn encode_server_message(message: &ServerMessage) -> Result<String, Error> {
	serde_json::to_string(message)
}

/// Parse a client message using the only V2.14 wire encoding.
pub fn decode_client_message(message: &str) -> Result<ClientMessage, Error> {
	let decoded = serde_json::from_str(message)?;
	validate_client_message(&decoded).map_err(|reason| {
		serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
	})?;
	Ok(decoded)
}

fn validate_client_message(message: &ClientMessage) -> Result<(), &'static str> {
	match message {
		ClientMessage::Hello(hello)
			if hello.version == crate::CURRENT_VERSION
				&& hello.resume.as_ref().is_some_and(|resume| resume.instance_id.is_none()) =>
			Err("current protocol resume requires a publication instance"),
		ClientMessage::Hello(_) => Ok(()),
		ClientMessage::Query(query) => match &query.payload {
			QueryPayload::GetDesktopSettings => Ok(()),
			QueryPayload::GetProgramCycle { program_id }
				if !is_canonical_uuid(program_id.as_str()) =>
				Err("Program query identity is not canonical"),
			QueryPayload::ListConversations { after, .. }
				if after.as_ref().is_some_and(|cursor| {
					!is_canonical_uuid(cursor.conversation_id().as_str())
				}) =>
				Err("Conversation list cursor identity is not canonical"),
			QueryPayload::GetConversation { conversation_id }
				if !is_canonical_uuid(conversation_id.as_str()) =>
				Err("Conversation conversation identity is not canonical"),
			QueryPayload::GetResetCards { account_id }
			| QueryPayload::InspectAccount { account_id }
			| QueryPayload::GetAccountProfile { account_id, .. }
				if !is_canonical_uuid(account_id.as_str()) =>
				Err("account query identity is not canonical"),
			_ => Ok(()),
		},
		ClientMessage::Command(command) => validate_account_command(command),
		ClientMessage::AccountLogin(request) => {
			if request.version != crate::CURRENT_VERSION || request.request.validate().is_err() {
				Err("account login request contract is invalid")
			} else {
				Ok(())
			}
		},
	}
}

fn validate_account_command(command: &CommandEnvelope) -> Result<(), &'static str> {
	let positive_expected = command.expected_revision.is_some_and(|revision| revision.0 > 0);
	match &command.payload {
		CommandPayload::SetDesktopSettings { .. } =>
			positive_expected.then_some(()).ok_or("desktop settings revision is required"),
		CommandPayload::CreateProgramCycle { draft } => {
			if command.expected_revision.is_some() || draft.validate().is_err() {
				Err("Program cycle create contract is invalid")
			} else {
				Ok(())
			}
		},
		CommandPayload::BindProgramDomainPack { program_id, domain_pack_id } =>
			if !positive_expected
				|| !is_canonical_uuid(program_id.as_str())
				|| !crate::domain_pack::is_namespaced_symbol(domain_pack_id.as_str())
			{
				Err("Program Domain Pack binding contract is invalid")
			} else {
				Ok(())
			},
		CommandPayload::ContinueProgram { continuation } => {
			if !positive_expected || continuation.validate().is_err() {
				Err("Program continuation contract is invalid")
			} else {
				Ok(())
			}
		},
		CommandPayload::RecordProgramReview { review } => {
			if command.expected_revision.is_some() || review.validate().is_err() {
				Err("Program Review contract is invalid")
			} else {
				Ok(())
			}
		},
		CommandPayload::CreateConversation { .. }
		| CommandPayload::ResumeConversationRouting { .. }
		| CommandPayload::CreateConversationRoutingSuccessor { .. }
		| CommandPayload::ResumeConversationEstablishment { .. }
		| CommandPayload::SubmitConversationTurn { .. }
		| CommandPayload::RefreshConversation { .. }
		| CommandPayload::ArchiveConversation { .. }
		| CommandPayload::InterruptConversation { .. } => validate_conversation_command(command),
		CommandPayload::RefreshSystemObservation { .. } => Ok(()),
		CommandPayload::ConsumeResetCard { account_id, .. } => {
			if is_canonical_uuid(account_id.as_str()) && positive_expected {
				Ok(())
			} else {
				Err("reset-card account identity or revision is invalid")
			}
		},
		CommandPayload::EnrollAccountFromSharedCodex { operation_id, account_id, .. } =>
			validate_account_install_command(
				operation_id,
				account_id,
				command.expected_revision.is_none(),
			),
		CommandPayload::ImportAccountCredentialFile {
			operation_id,
			account_id,
			source_descriptor,
			..
		} => {
			validate_account_install_command(
				operation_id,
				account_id,
				command.expected_revision.is_none(),
			)?;
			let source = source_descriptor.as_str();
			if source.is_empty() || source.len() > 4096 || source.chars().any(char::is_control) {
				Err("account credential source descriptor is invalid")
			} else {
				Ok(())
			}
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
		CommandPayload::RouteAccount { account_id } => {
			validate_canonical_account(account_id)?;
			command
				.expected_revision
				.is_none()
				.then_some(())
				.ok_or("account Route has no revision fence")
		},
		CommandPayload::SetBalancedAccountSelection =>
			positive_expected.then_some(()).ok_or("account routing revision is required"),
		CommandPayload::SetAccountOrder { order } =>
			validate_account_order_command(order, positive_expected),
		CommandPayload::RecoverAccountOperation { operation_id, .. } => {
			validate_canonical_operation(operation_id)?;
			positive_expected.then_some(()).ok_or("account revision is required")
		},
	}
}

fn validate_account_order_command(
	order: &[EntityId],
	positive_expected: bool,
) -> Result<(), &'static str> {
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
}

fn validate_conversation_command(command: &CommandEnvelope) -> Result<(), &'static str> {
	let positive_expected = command.expected_revision.is_some_and(|revision| revision.0 > 0);
	match &command.payload {
		CommandPayload::CreateConversation { conversation_id, work_item_id, message, .. } =>
			if command.expected_revision.is_some()
				|| !is_canonical_uuid(conversation_id.as_str())
				|| work_item_id
					.as_ref()
					.is_some_and(|work_item_id| !is_canonical_uuid(work_item_id.as_str()))
				|| message.as_str().trim().is_empty()
			{
				Err("Conversation create identity, revision, or message is invalid")
			} else {
				Ok(())
			},
		CommandPayload::ResumeConversationRouting { conversation_id }
		| CommandPayload::CreateConversationRoutingSuccessor { conversation_id }
		| CommandPayload::ResumeConversationEstablishment { conversation_id } => {
			if positive_expected && is_canonical_uuid(conversation_id.as_str()) {
				Ok(())
			} else {
				Err("Conversation recovery identity or revision is invalid")
			}
		},
		CommandPayload::SubmitConversationTurn { conversation_id, turn_id, message, .. } =>
			if positive_expected
				&& is_canonical_uuid(conversation_id.as_str())
				&& is_canonical_uuid(turn_id.as_str())
				&& !message.as_str().trim().is_empty()
			{
				Ok(())
			} else {
				Err("Conversation turn identity, revision, or message is invalid")
			},
		CommandPayload::InterruptConversation { conversation_id, turn_id } => {
			if positive_expected
				&& is_canonical_uuid(conversation_id.as_str())
				&& is_canonical_uuid(turn_id.as_str())
			{
				Ok(())
			} else {
				Err("Conversation interrupt identity or revision is invalid")
			}
		},
		CommandPayload::RefreshConversation { conversation_id }
		| CommandPayload::ArchiveConversation { conversation_id } => {
			if positive_expected && is_canonical_uuid(conversation_id.as_str()) {
				Ok(())
			} else {
				Err("Conversation control identity or revision is invalid")
			}
		},
		_ => unreachable!("Conversation validation requires a Conversation command"),
	}
}

fn validate_account_install_command(
	operation_id: &EntityId,
	account_id: &EntityId,
	expected_revision_absent: bool,
) -> Result<(), &'static str> {
	validate_canonical_operation(operation_id)?;
	validate_canonical_account(account_id)?;
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
	routing: Option<&AccountRoutingControlDto>,
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
	if let Some(routing) = routing {
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
	}
	Ok(())
}

fn validate_account_dto(account: &AccountDto) -> Result<(), &'static str> {
	if !is_canonical_uuid(account.account_id.as_str()) || account.account_revision.0 == 0 {
		return Err("account identity or revision is invalid");
	}
	if !is_canonical_account_alias(account.alias.as_str()) {
		return Err("account alias is invalid");
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

fn is_canonical_account_alias(value: &str) -> bool {
	let bytes = value.as_bytes();
	(2..=16).contains(&bytes.len())
		&& bytes[0].is_ascii_uppercase()
		&& bytes[1..].iter().all(u8::is_ascii_lowercase)
}

fn validate_account_profile(profile: &AccountProfileDto) -> Result<(), &'static str> {
	if !is_canonical_uuid(profile.account_id.as_str())
		|| profile.account_revision.0 == 0
		|| profile.account_revision.0 > i64::MAX as u64
		|| profile.observed_at_unix_micros <= 0
		|| profile.observed_at_unix_micros > 253_402_300_799_999_999
	{
		return Err("account profile identity, revision, or observation time is invalid");
	}
	validate_account_profile_claims(&profile.email, profile.plan_type.as_ref())?;
	if profile.display_name.as_ref().is_some_and(|value| !bounded_profile_text(value, 256))
		|| profile.username.as_ref().is_some_and(|value| !bounded_profile_text(value, 256))
	{
		return Err("account profile text is invalid");
	}
	if profile.lifetime_tokens.is_some_and(|value| value > i64::MAX as u64)
		|| profile.peak_daily_tokens.is_some_and(|value| value > i64::MAX as u64)
		|| profile.longest_task_seconds.is_some_and(|value| value > i64::MAX as u64)
		|| profile.current_streak_days.is_some_and(|value| value > i32::MAX as u32)
		|| profile.longest_streak_days.is_some_and(|value| value > i32::MAX as u32)
	{
		return Err("account profile metric exceeds the storage contract");
	}
	if profile.daily_usage.len() > MAX_ACCOUNT_PROFILE_DAILY_USAGE {
		return Err("account profile daily usage exceeds the cardinality bound");
	}
	let mut previous = None;
	for daily in &profile.daily_usage {
		if daily.tokens > i64::MAX as u64 || !canonical_calendar_date(daily.start_date.as_str()) {
			return Err("account profile daily usage is invalid");
		}
		if previous.is_some_and(|value| value >= daily.start_date.as_str()) {
			return Err("account profile daily usage is not unique and ascending");
		}
		previous = Some(daily.start_date.as_str());
	}
	if profile.display_name.is_none()
		&& profile.username.is_none()
		&& profile.lifetime_tokens.is_none()
		&& profile.peak_daily_tokens.is_none()
		&& profile.longest_task_seconds.is_none()
		&& profile.current_streak_days.is_none()
		&& profile.longest_streak_days.is_none()
		&& profile.daily_usage.is_empty()
	{
		return Err("account profile snapshot is empty");
	}
	Ok(())
}

fn validate_account_profile_claims(
	email: &AccountProfileEmailDto,
	plan_type: Option<&WireText>,
) -> Result<(), &'static str> {
	if let AccountProfileEmailDto::Visible(email) = email
		&& !bounded_profile_text(email, 320)
	{
		return Err("account profile email is invalid");
	}
	if plan_type.is_some_and(|value| !bounded_profile_text(value, 128)) {
		return Err("account profile plan type is invalid");
	}
	Ok(())
}

fn bounded_profile_text(value: &WireText, maximum: usize) -> bool {
	let value = value.as_str();
	!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn canonical_calendar_date(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 10
		|| bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| bytes
			.iter()
			.enumerate()
			.any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
	{
		return false;
	}
	let number = |start: usize, end: usize| {
		value[start..end].parse::<u32>().expect("validated date bytes are decimal")
	};
	let year = number(0, 4);
	let month = number(5, 7);
	let day = number(8, 10);
	let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
	let maximum_day = match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		2 if leap => 29,
		2 => 28,
		_ => return false,
	};
	year > 0 && (1..=maximum_day).contains(&day)
}

fn validate_reset_card_inventory(
	account_id: &EntityId,
	account_revision: EntityRevision,
	reported_available_count: Option<u64>,
	details_complete: bool,
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
	if details_complete {
		if reported_available_count != u64::try_from(cards.len()).ok() {
			return Err("complete reset-card inventory does not match its reported count");
		}
	} else {
		if !cards.is_empty() {
			return Err("partial reset-card inventory exposes selectable descriptors");
		}
		if reported_available_count == Some(0) {
			return Err("zero reset-card inventory must be complete");
		}
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
		AccountCommandRejectionDto, AccountDto, AccountInitialSelectionResult,
		AccountLifecycleReadinessDto, AccountObservationSignal, AccountObservedStateDto,
		AccountProfileDailyUsageDto, AccountProfileDto, AccountProfileEmailDto,
		AccountProfileErrorDto, AccountProfileResult, AccountQuotaStateDto, AccountQuotaWindowDto,
		AccountsResult, CURRENT_VERSION, CausationId, ClientCommandId, CodexAuthProjectionResult,
		CommandError, ConversationRecoveryAction, ConversationState, ConversationSummary,
		ConversationWorkingDirectory, CorrelationId, EntityId, EventPayload, HistoryCursorToken,
		HistoryText, IdempotencyKey, MAX_HISTORY_INLINE_BYTES, MAX_HISTORY_METADATA_FIELDS,
		MAX_HISTORY_METADATA_KEY_BYTES, MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE,
		MAX_IDEMPOTENCY_KEY_BYTES, MAX_RESET_CARD_ITEMS, MAX_WIRE_TEXT_BYTES,
		ProgramContinuationDraftDto, ProgramCycleDraftDto, QueryId, QueryResultPayload,
		ResetCardDescriptorDto, ResetCardOutcome, ResultPayload, ServerId, ServerInstanceId,
		Sha256Digest, WireText,
		wire::{
			ClientHello, ClientMessage, CommandEnvelope, CommandPayload, Cursor, EntityRevision,
			QueryEnvelope, QueryPayload, ResetCardInventoryResult, ResetCardOperationResult,
			ResumeCursor, decode_client_message,
		},
	};

	#[test]
	fn retired_stale_quota_state_is_rejected() {
		let stale = serde_json::json!({
			"state": "stale",
			"data": {
				"used_percent": 42,
				"resets_at_unix_micros": 2_000_000,
			},
		});

		assert!(serde_json::from_value::<AccountQuotaStateDto>(stale).is_err());
	}

	#[test]
	fn account_alias_accepts_only_one_canonical_word() {
		assert!(super::is_canonical_account_alias("Iris"));
		assert!(super::is_canonical_account_alias("Val"));
		for invalid in [
			"",
			"A",
			"iris",
			"IRIS",
			"Iris1",
			"Iris Smith",
			"Account DQ6WF-G8BTT",
			"Éden",
			"Abcdefghijklmnopq",
		] {
			assert!(!super::is_canonical_account_alias(invalid), "{invalid}");
		}
	}

	#[test]
	fn account_rows_remain_readable_without_routing_capability() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let unknown_quota = |duration_minutes| AccountQuotaWindowDto {
			duration_minutes,
			observed_at_unix_micros: None,
			result: AccountQuotaStateDto::Unknown,
		};
		let result = AccountsResult::Available {
			accounts: vec![AccountDto {
				account_id,
				alias: WireText::new("Iris").expect("canonical alias"),
				enabled: true,
				account_revision: EntityRevision(1),
				observed_state: AccountObservedStateDto::Unknown,
				lifecycle_readiness: AccountLifecycleReadinessDto::CredentialAbsent,
				credential_binding: None,
				unsettled_operation: None,
				five_hour_quota: unknown_quota(300),
				seven_day_quota: unknown_quota(10_080),
			}],
			routing: None,
		};

		let encoded = serde_json::to_value(&result).expect("account rows should serialize");
		assert!(encoded["data"]["routing"].is_null());
		assert_eq!(serde_json::from_value::<AccountsResult>(encoded).unwrap(), result);
	}

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
	fn retired_credential_file_command_names_do_not_decode() {
		for retired_name in
			["enroll_account_from_credential_file", "reauthenticate_account_from_credential_file"]
		{
			assert!(
				serde_json::from_value::<CommandPayload>(serde_json::json!({
					"name": retired_name,
					"arguments": {},
				}))
				.is_err(),
				"retired public command {retired_name} must not decode",
			);
		}
	}

	#[test]
	fn program_cycle_command_round_trips_and_rejects_duplicate_semantic_identity() {
		let ids = [
			"11000000-0000-4000-8000-000000000001",
			"21000000-0000-4000-8000-000000000001",
			"31000000-0000-4000-8000-000000000001",
			"41000000-0000-4000-8000-000000000001",
			"51000000-0000-4000-8000-000000000001",
			"61000000-0000-4000-8000-000000000001",
		];
		let entity = |value: &str| EntityId::new(value).expect("canonical Program identity");
		let text = |value: &str| WireText::new(value).expect("bounded Program text");
		let draft = ProgramCycleDraftDto {
			program_id: entity(ids[0]),
			domain_pack_id: WireText::new("decodex.dev").unwrap(),
			signal_id: entity(ids[1]),
			claim_id: entity(ids[2]),
			proposal_id: entity(ids[3]),
			objective_id: entity(ids[4]),
			work_item_id: entity(ids[5]),
			name: text("Adaptive Factory V1"),
			purpose: text("Prove one closed coordination cycle"),
			non_goals: vec![text("Do not add multi-agent fan-out")],
			review_policy: text("Review after the bound Codex run settles"),
			signal_source: text("operator observation"),
			signal_summary: text("Unrelated Codex tasks lose causal context"),
			signal_observed_at_micros: 1,
			claim_statement: text("A durable causal spine reduces coordination loss"),
			proposal_summary: text("Run one bounded Program cycle"),
			proposal_expected_effect: text("One restart-safe closed loop"),
			proposal_risk: text("The loop may not close"),
			proposal_evidence_need: text("Deterministic and external evidence"),
			objective_outcome: text("One closed cycle is visible in GPUI"),
			acceptance_criteria: vec![text("The causal cycle reopens after restart")],
			validation_criteria: vec![text("The Conversation request is not replayed")],
			work_item_title: text("Implement the bounded slice"),
			work_item_instructions: text("Return one deterministic result"),
			working_directory: ConversationWorkingDirectory::new("/tmp/decodex")
				.expect("canonical working directory"),
		};
		let message = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("program-create").unwrap(),
			idempotency_key: IdempotencyKey::new("program/create").unwrap(),
			expected_revision: None,
			correlation_id: CorrelationId::new(ids[0]).unwrap(),
			causation_id: None,
			payload: CommandPayload::CreateProgramCycle { draft: Box::new(draft.clone()) },
		});
		let encoded = serde_json::to_string(&message).unwrap();
		assert_eq!(decode_client_message(&encoded).unwrap(), message);

		let continuation = ProgramContinuationDraftDto {
			program_id: draft.program_id.clone(),
			predecessor_review_id: entity("71000000-0000-4000-8000-000000000001"),
			signal_id: entity("81000000-0000-4000-8000-000000000001"),
			claim_id: entity("82000000-0000-4000-8000-000000000001"),
			proposal_id: entity("83000000-0000-4000-8000-000000000001"),
			objective_id: entity("84000000-0000-4000-8000-000000000001"),
			work_item_id: entity("85000000-0000-4000-8000-000000000001"),
			signal_source: text("first cycle Review"),
			signal_summary: text("The first cycle exposed a bounded next gap"),
			signal_observed_at_micros: 2,
			claim_statement: text("One next cycle can close the gap"),
			proposal_summary: text("Append one exact next cycle"),
			proposal_expected_effect: text("The Program retains one identity"),
			proposal_risk: text("A stale append could branch history"),
			proposal_evidence_need: text("Restart and replay evidence"),
			objective_outcome: text("Two cycles remain ordered"),
			acceptance_criteria: vec![text("Prior nodes remain immutable")],
			validation_criteria: vec![text("Replay creates no duplicate")],
			work_item_title: text("Continue the Program"),
			work_item_instructions: text("Execute one finite next step"),
			working_directory: ConversationWorkingDirectory::new("/tmp/decodex").unwrap(),
		};
		let continuation_message = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("program-continue").unwrap(),
			idempotency_key: IdempotencyKey::new("program/continue").unwrap(),
			expected_revision: Some(EntityRevision(2)),
			correlation_id: CorrelationId::new(ids[0]).unwrap(),
			causation_id: None,
			payload: CommandPayload::ContinueProgram {
				continuation: Box::new(continuation.clone()),
			},
		});
		let encoded = serde_json::to_string(&continuation_message).unwrap();
		assert_eq!(decode_client_message(&encoded).unwrap(), continuation_message);
		let missing_revision = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("program-continue-stale").unwrap(),
			idempotency_key: IdempotencyKey::new("program/continue-stale").unwrap(),
			expected_revision: None,
			correlation_id: CorrelationId::new(ids[0]).unwrap(),
			causation_id: None,
			payload: CommandPayload::ContinueProgram { continuation: Box::new(continuation) },
		});
		assert!(decode_client_message(&serde_json::to_string(&missing_revision).unwrap()).is_err());

		let mut invalid = draft;
		invalid.work_item_id = invalid.objective_id.clone();
		let invalid = ClientMessage::Command(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("program-create-invalid").unwrap(),
			idempotency_key: IdempotencyKey::new("program/create-invalid").unwrap(),
			expected_revision: None,
			correlation_id: CorrelationId::new(ids[0]).unwrap(),
			causation_id: None,
			payload: CommandPayload::CreateProgramCycle { draft: Box::new(invalid) },
		});
		assert!(decode_client_message(&serde_json::to_string(&invalid).unwrap()).is_err());
	}
	#[test]
	fn retired_work_item_board_messages_fail_exact_decode() {
		let command_arguments = [
			(
				"register_project",
				serde_json::json!({
					"project_id": "10000000-0000-4000-8000-000000000001",
					"lead_id": "30000000-0000-4000-8000-000000000001",
					"repository_identity": "local/decodex-0123456789ab",
					"repository_root": "/Users/x/code/acg-box/decodex"
				}),
			),
			(
				"create_work_item",
				serde_json::json!({
					"work_item_id": "20000000-0000-4000-8000-000000000001",
					"project_id": "10000000-0000-4000-8000-000000000001",
					"title": "Implement the real conversation",
					"description": "Execute one real Codex conversation."
				}),
			),
			(
				"start_work_item",
				serde_json::json!({
					"work_item_id": "20000000-0000-4000-8000-000000000001",
					"project_id": "10000000-0000-4000-8000-000000000001",
					"conversation_id": "40000000-0000-4000-8000-000000000001"
				}),
			),
			(
				"accept_work_item",
				serde_json::json!({
					"work_item_id": "20000000-0000-4000-8000-000000000001",
					"project_id": "10000000-0000-4000-8000-000000000001",
					"acceptance_id": "50000000-0000-4000-8000-000000000001",
					"evidence_summary": "Verified provider evidence."
				}),
			),
		];

		for (index, (name, arguments)) in command_arguments.into_iter().enumerate() {
			let message = serde_json::json!({
				"type": "command",
				"data": {
					"version": CURRENT_VERSION,
					"client_command_id": format!("retired-board-command-{index}"),
					"idempotency_key": format!("retired/board/{index}"),
					"expected_revision": null,
					"correlation_id": format!("retired-board-correlation-{index}"),
					"causation_id": null,
					"payload": {"name": name, "arguments": arguments}
				}
			});
			assert!(
				decode_client_message(&message.to_string()).is_err(),
				"retired command {name} must not decode",
			);
		}

		for payload in [
			serde_json::json!({"name": "list_projects"}),
			serde_json::json!({
				"name": "get_work_item_board_page",
				"arguments": {
					"project_id": "10000000-0000-4000-8000-000000000001",
					"state": "planned",
					"after": null,
					"page_size": 16
				}
			}),
		] {
			let message = serde_json::json!({
				"type": "query",
				"data": {
					"version": CURRENT_VERSION,
					"query_id": "retired-board-query",
					"payload": payload
				}
			});
			assert!(decode_client_message(&message.to_string()).is_err());
		}
	}

	#[test]
	fn exact_current_queries_round_trip_and_removed_execution_decision_fails_closed() {
		let remaining = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("doctor-query").expect("bounded query ID"),
			payload: QueryPayload::GetDoctorStatus,
		});
		let encoded = serde_json::to_string(&remaining).expect("query serializes");
		assert_eq!(decode_client_message(&encoded).unwrap(), remaining);

		let removed = serde_json::json!({
			"type": "query",
			"body": {
				"version": CURRENT_VERSION,
				"query_id": "execution-decision-query",
				"payload": {
					"name": "get_execution_decision",
					"arguments": {
						"decision_id": "01234567-89ab-4def-8123-456789abcdef"
					}
				}
			}
		});
		assert!(decode_client_message(&removed.to_string()).is_err());
	}

	#[test]
	fn conversation_routing_recovery_has_clean_break_wire_shapes() {
		let source =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical source ID");
		let successor =
			EntityId::new("11234567-89ab-4def-8123-456789abcdef").expect("canonical successor ID");
		let create = CommandPayload::CreateConversation {
			conversation_id: source.clone(),
			work_item_id: None,
			message: HistoryText::new("route this request").expect("bounded message"),
			working_directory: ConversationWorkingDirectory::new("/tmp/work")
				.expect("bounded working directory"),
			execution: crate::ConversationExecutionSettings::new(
				crate::ConversationModel::new("gpt-5.6-sol").expect("bounded model"),
				crate::ConversationReasoningEffort::High,
				false,
			),
		};
		assert_eq!(
			serde_json::to_value(&create).unwrap(),
			serde_json::json!({
				"name": "create_conversation",
				"arguments": {
					"conversation_id": source.as_str(),
					"message": "route this request",
					"working_directory": "/tmp/work",
					"execution": {
						"model": "gpt-5.6-sol",
						"reasoning_effort": "high",
						"fast": false,
					},
				},
			}),
		);
		for (payload, name) in [
			(
				CommandPayload::ResumeConversationRouting { conversation_id: source.clone() },
				"resume_conversation_routing",
			),
			(
				CommandPayload::CreateConversationRoutingSuccessor {
					conversation_id: source.clone(),
				},
				"create_conversation_routing_successor",
			),
			(
				CommandPayload::ResumeConversationEstablishment { conversation_id: source.clone() },
				"resume_conversation_establishment",
			),
		] {
			assert_eq!(
				serde_json::to_value(payload).unwrap(),
				serde_json::json!({
					"name": name,
					"arguments": {"conversation_id": source.as_str()},
				}),
			);
		}
		assert!(
			serde_json::from_value::<CommandPayload>(serde_json::json!({
				"name": "retry_conversation_routing",
				"arguments": {"conversation_id": source.as_str()},
			}))
			.is_err()
		);

		let successor_summary = ConversationSummary::new(
			successor.clone(),
			crate::ConversationTitle::new("Route conversation").unwrap(),
			None,
			None,
			EntityRevision(1),
			1,
			None,
			None,
			ConversationState::RoutingPending,
			None,
			Some(ConversationRecoveryAction::ResumeRouting),
		)
		.expect("routing-pending successor projection is valid");
		let result = ResultPayload::ConversationRoutingSuccessorAccepted {
			source_conversation_id: source.clone(),
			source_conversation_revision: EntityRevision(2),
			successor: successor_summary,
		};
		assert_eq!(
			serde_json::to_value(result).unwrap(),
			serde_json::json!({
				"name": "conversation_routing_successor_accepted",
				"data": {
					"source_conversation_id": source.as_str(),
					"source_conversation_revision": 2,
					"successor": {
						"conversation_id": successor.as_str(),
						"title": "Route conversation",
						"conversation_revision": 1,
						"projection_updated_at_micros": 1,
						"state": "routing_pending",
						"recovery_action": "resume_routing",
					},
				},
			}),
		);
	}

	#[test]
	fn account_routing_commands_have_one_route_and_two_policy_wire_shapes() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let other_id =
			EntityId::new("11234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let route = CommandPayload::RouteAccount { account_id: account_id.clone() };
		let balanced = CommandPayload::SetBalancedAccountSelection;
		let order =
			CommandPayload::SetAccountOrder { order: vec![other_id.clone(), account_id.clone()] };

		assert_eq!(
			serde_json::to_value(&route).unwrap(),
			serde_json::json!({
				"name": "route_account",
				"arguments": {
					"account_id": account_id.as_str(),
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
			CommandPayload::RouteAccount { account_id: account_id.clone() },
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

		let duplicate_provider = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::ProviderAlreadyEnrolled,
			actual_revision: None,
		};
		let encoded = serde_json::to_value(&duplicate_provider).unwrap();
		assert_eq!(
			encoded,
			serde_json::json!({
				"reason": "account_command_rejected",
				"rejection": "provider_already_enrolled",
			}),
		);
		assert_eq!(serde_json::from_value::<CommandError>(encoded).unwrap(), duplicate_provider);
	}

	#[test]
	fn codex_auth_projection_query_is_credential_negative_and_strict() {
		let account_id =
			EntityId::new("21234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let query = QueryPayload::GetCodexAuthProjection;
		let result = CodexAuthProjectionResult::Current {
			account_id: account_id.clone(),
			account_revision: EntityRevision(9),
			projection_digest: Sha256Digest::new("b".repeat(64)).unwrap(),
		};

		assert_eq!(
			serde_json::to_value(query).unwrap(),
			serde_json::json!({"name": "get_codex_auth_projection"}),
		);
		assert_eq!(
			serde_json::to_value(&result).unwrap(),
			serde_json::json!({
				"outcome": "current",
				"data": {
					"account_id": account_id.as_str(),
					"account_revision": 9,
					"projection_digest": "b".repeat(64),
				},
			}),
		);
		assert!(
			serde_json::from_value::<CodexAuthProjectionResult>(serde_json::json!({
				"outcome": "current",
				"data": {
					"account_id": account_id.as_str(),
					"account_revision": 9,
					"projection_digest": "b".repeat(64),
					"access_token": "must-not-be-accepted",
				},
			}))
			.is_err()
		);
	}

	#[test]
	fn account_observation_wait_is_one_strict_opaque_generation() {
		let query =
			QueryPayload::WaitForAccountObservation { after_generation: 17, request_refresh: None };
		let result = QueryResultPayload::AccountObservation(AccountObservationSignal::new(42));

		assert_eq!(
			serde_json::to_value(query).unwrap(),
			serde_json::json!({
				"name": "wait_for_account_observation",
				"arguments": {"after_generation": 17}
			}),
		);
		assert_eq!(
			serde_json::to_value(QueryPayload::WaitForAccountObservation {
				after_generation: 17,
				request_refresh: Some(true),
			})
			.unwrap(),
			serde_json::json!({
				"name": "wait_for_account_observation",
				"arguments": {"after_generation": 17, "request_refresh": true}
			}),
		);
		assert_eq!(
			serde_json::to_value(result).unwrap(),
			serde_json::json!({
				"name": "account_observation",
				"data": {"generation": 42}
			}),
		);
		assert!(
			serde_json::from_value::<AccountObservationSignal>(
				serde_json::json!({"generation": 42, "extra": true}),
			)
			.is_err()
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
				r#"{"type":"hello","body":{"version":{"major":2,"minor":14},"#,
				r#""resume":{"server_id":"server-a","instance_id":"instance-a","cursor":42}}}"#,
			)
		);
	}

	#[test]
	fn exact_current_resume_requires_a_publication_instance() {
		let current_without_instance = concat!(
			r#"{"type":"hello","body":{"version":{"major":2,"minor":14},"#,
			r#""resume":{"server_id":"server-a","cursor":42}}}"#,
		);
		let old_hello = concat!(
			r#"{"type":"hello","body":{"version":{"major":1,"minor":5},"#,
			r#""resume":{"server_id":"server-a","cursor":42}}}"#,
		);

		assert!(decode_client_message(current_without_instance).is_err());

		let ClientMessage::Hello(hello) = decode_client_message(old_hello).unwrap() else {
			panic!("expected hello");
		};

		assert_eq!(hello.version, crate::ProtocolVersion { major: 1, minor: 5 });
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
				r#"{"type":"command","body":{"version":{"major":2,"minor":14},"#,
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
				"reported_available_count":2,
				"details_complete":true,
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
				"reported_available_count":2,
				"details_complete":true,
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
				"reported_available_count":1,
				"details_complete":true,
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
				"reported_available_count":u64::try_from(MAX_RESET_CARD_ITEMS + 1).unwrap(),
				"details_complete":true,
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
				"reported_available_count":u64::try_from(MAX_RESET_CARD_ITEMS).unwrap(),
				"details_complete":true,
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
		let partial = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"reported_available_count":2,
				"details_complete":false,
				"cards":[],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		assert!(serde_json::from_value::<ResetCardInventoryResult>(partial).is_ok());
		let contradictory_empty = serde_json::json!({
			"outcome":"available",
			"data":{
				"account_id":account_id,
				"account_revision":1,
				"reported_available_count":0,
				"details_complete":false,
				"cards":[],
				"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
			}
		});
		assert!(serde_json::from_value::<ResetCardInventoryResult>(contradictory_empty).is_err());
		let timed_out =
			ResetCardInventoryResult::Unavailable { error: super::ResetCardError::RequestTimedOut };
		let encoded_timeout = serde_json::to_value(&timed_out).unwrap();
		assert_eq!(encoded_timeout["outcome"], "unavailable");
		assert_eq!(encoded_timeout["data"]["error"], "request_timed_out");
		assert_eq!(
			serde_json::from_value::<ResetCardInventoryResult>(encoded_timeout).unwrap(),
			timed_out,
		);
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
			reported_available_count: u64::try_from(MAX_RESET_CARD_ITEMS).ok(),
			details_complete: true,
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
			reported_available_count: u64::try_from(MAX_RESET_CARD_ITEMS + 1).ok(),
			details_complete: true,
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
			reported_available_count: Some(0),
			details_complete: true,
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
	fn query_command_and_event_support_gates_are_exact_current() {
		let account_id =
			EntityId::new("01234567-89ab-4def-8123-456789abcdef").expect("canonical account ID");
		let descriptor = ResetCardDescriptorDto::new(100, 200).expect("valid descriptor");
		let legacy = crate::ProtocolVersion { major: 1, minor: 5 };
		let future = crate::ProtocolVersion { major: 2, minor: 15 };
		let query = QueryPayload::GetAccountProfile {
			account_id: account_id.clone(),
			include_email: false,
		};
		let command =
			CommandPayload::ConsumeResetCard { account_id: account_id.clone(), descriptor };
		let event = EventPayload::ResetCardConsumed {
			account_id,
			descriptor,
			outcome: ResetCardOutcome::Reset,
		};

		for version in [legacy, future] {
			assert!(!query.is_supported_in(version));
			assert!(!command.is_supported_in(version));
			assert!(!event.is_supported_in(version));
		}
		assert!(query.is_supported_in(CURRENT_VERSION));
		assert!(command.is_supported_in(CURRENT_VERSION));
		assert!(event.is_supported_in(CURRENT_VERSION));
	}

	#[test]
	fn account_profile_is_bounded_strict_and_email_redacted_explicitly() {
		let profile = AccountProfileDto {
			account_id: EntityId::new("40000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: EntityRevision(7),
			observed_at_unix_micros: 1_700_000_000_000_000,
			email: AccountProfileEmailDto::Redacted,
			plan_type: Some(WireText::new("pro").unwrap()),
			display_name: Some(WireText::new("Iris").unwrap()),
			username: None,
			lifetime_tokens: Some(12_345),
			peak_daily_tokens: Some(900),
			longest_task_seconds: Some(600),
			current_streak_days: Some(3),
			longest_streak_days: Some(8),
			daily_usage: vec![AccountProfileDailyUsageDto {
				start_date: WireText::new("2026-07-28").unwrap(),
				tokens: 900,
			}],
		};
		let encoded =
			serde_json::to_value(AccountProfileResult::Current(Box::new(profile))).unwrap();

		assert_eq!(encoded["outcome"], "current");
		assert_eq!(encoded["data"]["email"]["visibility"], "redacted");
		assert!(encoded["data"]["email"].get("value").is_none());

		let unavailable = serde_json::to_value(AccountProfileResult::Unavailable {
			error: AccountProfileErrorDto::ProviderUnavailable,
			email: AccountProfileEmailDto::Redacted,
			plan_type: Some(WireText::new("pro").unwrap()),
		})
		.unwrap();
		assert_eq!(unavailable["outcome"], "unavailable");
		assert_eq!(unavailable["data"]["error"], "provider_unavailable");
		assert_eq!(unavailable["data"]["email"]["visibility"], "redacted");
		assert_eq!(unavailable["data"]["plan_type"], "pro");
		let mut missing_email = unavailable.clone();
		missing_email["data"].as_object_mut().unwrap().remove("email");
		assert!(serde_json::from_value::<AccountProfileResult>(missing_email).is_err());

		let mut unknown = encoded.clone();
		unknown["data"]["unexpected"] = serde_json::json!(true);
		assert!(serde_json::from_value::<AccountProfileResult>(unknown).is_err());

		let mut overflow = encoded.clone();
		overflow["data"]["account_revision"] = serde_json::json!(i64::MAX as u64 + 1);
		assert!(serde_json::from_value::<AccountProfileResult>(overflow).is_err());

		let mut too_many = encoded.clone();
		too_many["data"]["daily_usage"] = serde_json::Value::Array(
			(1..=37)
				.map(|day| {
					let (month, day) = if day <= 31 { (7, day) } else { (8, day - 31) };
					serde_json::json!({
						"start_date": format!("2026-{month:02}-{day:02}"),
						"tokens": day,
					})
				})
				.collect(),
		);
		assert!(serde_json::from_value::<AccountProfileResult>(too_many).is_err());

		let mut malformed_date = encoded;
		malformed_date["data"]["daily_usage"][0]["start_date"] = serde_json::json!("2026-02-30");
		assert!(serde_json::from_value::<AccountProfileResult>(malformed_date).is_err());
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
