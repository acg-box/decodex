use std::{
	collections::BTreeMap,
	fmt::{Display, Formatter},
	sync::OnceLock,
};

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{BlobHash, MAX_BLOB_BYTES};

macro_rules! domain_id {
	($name:ident, $label:literal) => {
		#[doc = concat!("Stable logical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
		#[serde(transparent)]
		pub struct $name(String);

		impl $name {
			#[doc = concat!("Validate a canonical UUID-shaped ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ConversationError> {
				let value = value.into();
				if !is_canonical_uuid(&value) {
					return Err(ConversationError::InvalidIdentity($label));
				}
				Ok(Self(value))
			}

			/// Borrow the canonical identity text.
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}

		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

domain_id!(ConversationId, "conversation");

domain_id!(RuntimeSessionId, "runtime session");

domain_id!(TurnId, "turn");

domain_id!(HistoryItemId, "history item");

domain_id!(ArtifactId, "artifact");

/// Largest domain-owned title accepted before persistence or protocol rendering.
pub const MAX_CONVERSATION_TITLE_BYTES: usize = 512;
/// Largest exact opaque provider-thread identity accepted across app-server and persistence.
pub const MAX_PROVIDER_THREAD_ID_BYTES: usize = 512;
/// Largest payload retained inline in durable-store history rows.
pub const MAX_INLINE_HISTORY_BYTES: usize = 16 * 1_024;
/// Maximum number of fields in one normalized history metadata projection.
pub const MAX_HISTORY_METADATA_FIELDS: usize = 32;
/// Maximum UTF-8 bytes in one normalized history metadata key.
pub const MAX_HISTORY_METADATA_KEY_BYTES: usize = 64;
/// Maximum UTF-8 bytes in one normalized history metadata string value.
pub const MAX_HISTORY_METADATA_VALUE_BYTES: usize = 256;
/// Hard maximum for a compiled Context Pack. Larger source material is deterministically
/// truncated and remains inspectable through its source provenance.
pub const MAX_CONTEXT_PACK_BYTES: usize = 256 * 1_024;
/// Minimum useful Context Pack size. It leaves room for the pinned revision and policy header.
pub const MIN_CONTEXT_PACK_BYTES: usize = 1_024;
/// Maximum number of recent raw history items considered by one compilation.
pub const MAX_CONTEXT_RECENT_ITEMS: usize = 256;
/// Maximum number of provenance-bearing sources in one Context Pack.
pub const MAX_CONTEXT_SOURCES: usize = 512;
/// Aggregate source bytes accepted by one compilation. Context Packs are summaries, so
/// callers must provide bounded relevant material rather than an unbounded corpus.
pub const MAX_CONTEXT_SOURCE_INPUT_BYTES: usize = 2 * 1_024 * 1_024;

const CONTEXT_PACK_MAGIC: &[u8] = b"decodex/context-pack/2\0";

/// Domain-validation error that never includes rejected caller content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationError {
	/// An identity was not a canonical lowercase UUID string.
	InvalidIdentity(&'static str),
	/// A bounded field was empty or too large.
	InvalidBound(&'static str),
	/// A revision or ordinal was outside its positive domain.
	InvalidRevision(&'static str),
	/// An inline payload exceeded the durable-store inline boundary.
	PayloadRequiresBlob,
	/// A blob reference did not describe positive bounded content.
	InvalidBlobReference,
	/// Normalized history metadata contains a credential-bearing key or concrete value.
	CredentialRejected,
	/// Context Pack policy is outside its closed safety limits.
	InvalidContextPolicy,
	/// Context Pack input is missing its mandatory pinned source.
	MissingPinnedSource,
}
impl std::error::Error for ConversationError {}

impl Display for ConversationError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::InvalidIdentity(kind) => write!(formatter, "invalid {kind} identity"),
			Self::InvalidBound(field) => write!(formatter, "invalid {field} bound"),
			Self::InvalidRevision(field) => write!(formatter, "invalid {field} revision"),
			Self::PayloadRequiresBlob => formatter.write_str("payload must use a blob reference"),
			Self::InvalidBlobReference => formatter.write_str("invalid blob reference"),
			Self::CredentialRejected => formatter.write_str("credential-bearing metadata rejected"),
			Self::InvalidContextPolicy => formatter.write_str("invalid Context Pack policy"),
			Self::MissingPinnedSource =>
				formatter.write_str("Context Pack requires a pinned source"),
		}
	}
}

/// Logical conversation lifecycle, independent of account and Codex-thread identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
	/// Conversation accepts new persisted turns.
	Open,
	/// Conversation is retained for inspection but closed to mutation.
	Archived,
}

/// Lifecycle observed for one manually bound runtime segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSessionState {
	/// Binding metadata exists but no active state is claimed.
	Starting,
	/// The manual fixture represents an active segment.
	Active,
	/// The segment ended without a divergence claim.
	Ended,
	/// External activity made the segment unsafe to continue automatically.
	Diverged,
}

/// Normalized author role for a persisted turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRole {
	/// User-authored input.
	User,
	/// Assistant-authored output.
	Assistant,
	/// System-authored state.
	System,
	/// Tool-authored output.
	Tool,
}

/// Explicit side-effect uncertainty carried across persistence boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PossibleSideEffects {
	/// Readback proves no side effect is possible.
	None,
	/// A side effect may have begun or completed.
	Possible,
	/// Available evidence cannot classify side effects.
	Unknown,
}

/// Closed normalized history item classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryItemKind {
	/// Visible conversational message.
	Message,
	/// Model reasoning projection.
	Reasoning,
	/// Tool invocation description.
	ToolCall,
	/// Tool invocation result.
	ToolResult,
	/// Artifact reference.
	Artifact,
	/// Runtime status observation.
	Status,
}

/// Stream lifecycle for a normalized item.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
	/// More correlated updates may arrive.
	Streaming,
	/// Item completed successfully.
	Completed,
	/// Item terminated with failure evidence.
	Failed,
}

/// Lifecycle of one logical turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
	/// The turn accepts normalized item mutations.
	Active,
	/// The turn completed and is immutable.
	Completed,
	/// The turn failed and is immutable.
	Failed,
}

/// Lifecycle of one immutable-content Artifact entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
	/// Artifact is available for new references.
	Active,
	/// Artifact is retained but unavailable for new references.
	Expired,
	/// Artifact is a terminal retained tombstone.
	Deleted,
}

/// Closed scalar values allowed in normalized history metadata.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum HistoryMetadataValue {
	/// Bounded ordinary text.
	Text(String),
	/// Non-secret boolean fact.
	Boolean(bool),
}

/// Bounded inline bytes or an integrity-checked content-addressed reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizedPayload {
	/// Text small enough for direct durable-store persistence.
	Inline {
		/// Exact normalized text.
		text: String,
		/// Canonical media type shared with offloaded payloads.
		media_type: HistoryMediaType,
		/// Canonical normalized metadata shared with offloaded payloads.
		metadata: HistoryMetadata,
	},
	/// Large bytes stored through the BlobStore boundary.
	Blob {
		/// Verified content-addressed bytes.
		reference: ArtifactReference,
		/// Canonical normalized metadata shared with inline payloads.
		metadata: HistoryMetadata,
	},
}
impl NormalizedPayload {
	/// Validate and construct an inline normalized payload.
	pub fn inline(
		value: impl Into<String>,
		media_type: HistoryMediaType,
		metadata: HistoryMetadata,
	) -> Result<Self, ConversationError> {
		let value = value.into();

		if value.len() > MAX_INLINE_HISTORY_BYTES {
			return Err(ConversationError::PayloadRequiresBlob);
		}

		Ok(Self::Inline { text: value, media_type, metadata })
	}
}

/// Inert rollover or fallback proposal classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposedTransitionKind {
	/// Proposed size or latency rollover.
	Rollover,
	/// Proposed recovery or account-failure fallback.
	Fallback,
}

/// Provenance category included in a compiled Context Pack.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSourceKind {
	/// Mandatory pinned durable source revision.
	PinnedRevision,
	/// Repository instruction revision.
	RepositoryInstructions,
	/// OpenWiki source revision.
	OpenWiki,
	/// Relevant decision revision.
	Decision,
	/// Relevant fact revision.
	Fact,
	/// Artifact content-address reference.
	Artifact,
	/// Recent raw normalized history.
	RecentRaw,
}

/// How one manifest source is represented by compiled bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSourceDisposition {
	/// Complete source bytes are included.
	Complete,
	/// A deterministic prefix is included.
	Truncated,
	/// No source bytes are included, but provenance remains bound by the manifest.
	Omitted,
}

/// Legal bounded canonical media type (`type/subtype` with visible ASCII token characters).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryMediaType(String);
impl HistoryMediaType {
	/// Validate one bounded canonical media type.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationError> {
		let value = value.into();

		if !is_canonical_media_type(&value) {
			return Err(ConversationError::InvalidBound("history media type"));
		}

		Ok(Self(value))
	}

	/// Borrow the canonical media type.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for HistoryMediaType {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}

/// Bounded, flat, credential-negative normalized history metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HistoryMetadata(BTreeMap<String, HistoryMetadataValue>);
impl HistoryMetadata {
	/// Validate the canonical normalized metadata projection.
	pub fn new(values: BTreeMap<String, HistoryMetadataValue>) -> Result<Self, ConversationError> {
		if values.len() > MAX_HISTORY_METADATA_FIELDS {
			return Err(ConversationError::InvalidBound("history metadata fields"));
		}

		for (key, value) in &values {
			if key.is_empty() || key.len() > MAX_HISTORY_METADATA_KEY_BYTES {
				return Err(ConversationError::InvalidBound("history metadata key"));
			}
			if is_credential_metadata_key(key) {
				return Err(ConversationError::CredentialRejected);
			}

			if let HistoryMetadataValue::Text(text) = value {
				if text.len() > MAX_HISTORY_METADATA_VALUE_BYTES {
					return Err(ConversationError::InvalidBound("history metadata value"));
				}
				if contains_credential_material(text) {
					return Err(ConversationError::CredentialRejected);
				}
			}
		}

		Ok(Self(values))
	}

	/// Return an empty valid projection.
	pub fn empty() -> Self {
		Self(BTreeMap::new())
	}

	/// Borrow the validated projection.
	pub fn as_map(&self) -> &BTreeMap<String, HistoryMetadataValue> {
		&self.0
	}
}

impl<'de> Deserialize<'de> for HistoryMetadata {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(BTreeMap::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}

/// Durable logical dialogue. It intentionally has no account or Codex thread field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Conversation {
	/// Stable logical identity.
	pub id: ConversationId,
	/// Bounded user-visible title.
	pub title: String,
	/// Current logical lifecycle.
	pub status: ConversationStatus,
	/// Optimistic entity revision.
	pub revision: u64,
}
impl Conversation {
	/// Create revision one of an open logical Conversation.
	pub fn new(id: ConversationId, title: impl Into<String>) -> Result<Self, ConversationError> {
		let title = title.into();

		validate_nonempty(&title, MAX_CONVERSATION_TITLE_BYTES, "conversation title")?;

		Ok(Self { id, title, status: ConversationStatus::Open, revision: 1 })
	}
}

/// Immutable non-secret account facts captured at RuntimeSession creation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AccountSnapshot {
	/// Non-secret source account identity.
	pub account_id: String,
	/// Non-secret display label captured at binding time.
	pub display_label: String,
	/// Observed account state captured at binding time.
	pub observed_state: String,
	/// Source account revision captured immutably.
	pub source_revision: u64,
}
impl AccountSnapshot {
	/// Validate immutable non-secret account facts.
	pub fn new(
		account_id: impl Into<String>,
		display_label: impl Into<String>,
		observed_state: impl Into<String>,
		source_revision: u64,
	) -> Result<Self, ConversationError> {
		let account_id = account_id.into();
		let display_label = display_label.into();
		let observed_state = observed_state.into();

		validate_nonempty(&account_id, 128, "account snapshot identity")?;
		validate_nonempty(&display_label, 128, "account snapshot label")?;
		validate_symbol(&observed_state, 64, "account snapshot state")?;
		validate_revision(source_revision, "account snapshot")?;

		Ok(Self { account_id, display_label, observed_state, source_revision })
	}
}

/// Immutable user-selected RoleProfile facts captured at RuntimeSession creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileSnapshot {
	/// Source RoleProfile identity.
	pub profile_id: String,
	/// Selected role.
	pub role: String,
	/// Selected model.
	pub model: String,
	/// Selected reasoning effort.
	pub reasoning_effort: String,
	/// Selected service tier.
	pub service_tier: String,
	/// Digest of the exact instruction bytes.
	pub instructions_digest: BlobHash,
	/// Source RoleProfile revision.
	pub source_revision: u64,
}
impl ProfileSnapshot {
	/// Validate an immutable RoleProfile snapshot.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		profile_id: impl Into<String>,
		role: impl Into<String>,
		model: impl Into<String>,
		reasoning_effort: impl Into<String>,
		service_tier: impl Into<String>,
		instructions_digest: BlobHash,
		source_revision: u64,
	) -> Result<Self, ConversationError> {
		let profile_id = profile_id.into();
		let role = role.into();
		let model = model.into();
		let reasoning_effort = reasoning_effort.into();
		let service_tier = service_tier.into();

		validate_nonempty(&profile_id, 128, "profile snapshot identity")?;
		validate_symbol(&role, 32, "profile snapshot role")?;
		validate_nonempty(&model, 128, "profile snapshot model")?;
		validate_symbol(&reasoning_effort, 32, "profile snapshot reasoning effort")?;
		validate_symbol(&service_tier, 32, "profile snapshot service tier")?;
		validate_revision(source_revision, "profile snapshot")?;

		Ok(Self {
			profile_id,
			role,
			model,
			reasoning_effort,
			service_tier,
			instructions_digest,
			source_revision,
		})
	}
}

/// A manually or synthetically bound Codex thread segment. Construction cannot select
/// an account, start a process, or dispatch a turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSession {
	/// Stable runtime-segment identity.
	pub id: RuntimeSessionId,
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Explicitly supplied Codex thread identity, if known.
	pub codex_thread_id: Option<String>,
	/// Immutable non-secret account snapshot.
	pub account_snapshot: AccountSnapshot,
	/// Immutable selected profile snapshot.
	pub profile_snapshot: ProfileSnapshot,
	/// Persisted runtime state.
	pub state: RuntimeSessionState,
	/// Last manually observed Codex turn identity.
	pub last_known_turn_id: Option<String>,
	/// Optimistic entity revision.
	pub revision: u64,
}

/// One normalized logical turn correlated to a RuntimeSession.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Turn {
	/// Stable turn identity.
	pub id: TurnId,
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Runtime segment that produced this turn.
	pub runtime_session_id: RuntimeSessionId,
	/// Monotonic position inside the logical Conversation.
	pub sequence: u64,
	/// Normalized author role.
	pub role: TurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: PossibleSideEffects,
	/// Current lifecycle.
	pub status: TurnStatus,
	/// Optimistic entity revision.
	pub revision: u64,
}

/// durable-store metadata for verified content-addressed bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactReference {
	/// SHA-256 content address.
	pub hash: BlobHash,
	/// Verified byte length.
	pub byte_length: u64,
	/// Bounded media type.
	pub media_type: HistoryMediaType,
}
impl ArtifactReference {
	/// Validate verified blob metadata.
	pub fn new(
		hash: BlobHash,
		byte_length: u64,
		media_type: impl Into<String>,
	) -> Result<Self, ConversationError> {
		let media_type = HistoryMediaType::new(media_type)?;

		if byte_length == 0 || byte_length > u64::try_from(MAX_BLOB_BYTES).unwrap_or(u64::MAX) {
			return Err(ConversationError::InvalidBlobReference);
		}

		Ok(Self { hash, byte_length, media_type })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One normalized persisted item within a logical turn.
pub struct HistoryItem {
	/// Stable item identity.
	pub id: HistoryItemId,
	/// Parent turn identity.
	pub turn_id: TurnId,
	/// Stable position inside the turn.
	pub ordinal: u32,
	/// Normalized item class.
	pub kind: HistoryItemKind,
	/// Stream lifecycle.
	pub status: ItemStatus,
	/// Bounded payload representation.
	pub payload: NormalizedPayload,
	/// Optimistic entity revision.
	pub revision: u64,
}

/// Inert representation only. There is deliberately no transition executor or enabled flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposedTransition {
	/// Proposed transition class.
	pub kind: ProposedTransitionKind,
	/// Logical Conversation preserved by the proposal.
	pub conversation_id: ConversationId,
	/// Runtime segment the proposal would replace.
	pub from_session_id: RuntimeSessionId,
	/// Exact proposed Context Pack digest.
	pub context_pack_digest: BlobHash,
	/// Bounded operator-inspectable rationale.
	pub reason: String,
}
impl ProposedTransition {
	/// Construct an inert proposal without any execution capability.
	pub fn new(
		kind: ProposedTransitionKind,
		conversation_id: ConversationId,
		from_session_id: RuntimeSessionId,
		context_pack_digest: BlobHash,
		reason: impl Into<String>,
	) -> Result<Self, ConversationError> {
		let reason = reason.into();

		validate_nonempty(&reason, 512, "transition reason")?;

		Ok(Self { kind, conversation_id, from_session_id, context_pack_digest, reason })
	}
}

/// One immutable provenance-bearing Context Pack input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackSource {
	kind: ContextSourceKind,
	source_id: String,
	revision: u64,
	content: Vec<u8>,
	content_digest: BlobHash,
	artifact: Option<(ArtifactId, u64)>,
}
impl ContextPackSource {
	/// Validate and digest one non-artifact source revision.
	pub fn new(
		kind: ContextSourceKind,
		source_id: impl Into<String>,
		revision: u64,
		content: impl Into<Vec<u8>>,
	) -> Result<Self, ConversationError> {
		if kind == ContextSourceKind::Artifact {
			return Err(ConversationError::InvalidBound("typed artifact source"));
		}

		Self::build(kind, source_id.into(), revision, content.into(), None)
	}

	/// Construct a typed Artifact revision source.
	pub fn artifact(
		artifact_id: ArtifactId,
		artifact_revision: u64,
		content: impl Into<Vec<u8>>,
	) -> Result<Self, ConversationError> {
		Self::build(
			ContextSourceKind::Artifact,
			artifact_id.as_str().to_owned(),
			artifact_revision,
			content.into(),
			Some((artifact_id, artifact_revision)),
		)
	}

	fn build(
		kind: ContextSourceKind,
		source_id: String,
		revision: u64,
		content: Vec<u8>,
		artifact: Option<(ArtifactId, u64)>,
	) -> Result<Self, ConversationError> {
		validate_nonempty(&source_id, 256, "context source identity")?;
		validate_revision(revision, "context source")?;

		if content.len() > MAX_CONTEXT_SOURCE_INPUT_BYTES {
			return Err(ConversationError::InvalidBound("context source content"));
		}

		let content_digest = BlobHash::digest(&content);

		Ok(Self { kind, source_id, revision, content, content_digest, artifact })
	}

	/// Semantic provenance category.
	pub fn kind(&self) -> ContextSourceKind {
		self.kind
	}

	/// Stable source identity.
	pub fn source_id(&self) -> &str {
		&self.source_id
	}

	/// Pinned source revision.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	/// Exact source content digest.
	pub fn content_digest(&self) -> BlobHash {
		self.content_digest
	}

	/// Typed Artifact identity and revision when this is an Artifact source.
	pub fn artifact_reference(&self) -> Option<(&ArtifactId, u64)> {
		self.artifact.as_ref().map(|(id, revision)| (id, *revision))
	}
}

/// Mandatory pinned source modeled separately from optional Context Pack material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedContextSource(ContextPackSource);
impl PinnedContextSource {
	/// Validate one non-empty mandatory pinned revision.
	pub fn new(
		source_id: impl Into<String>,
		revision: u64,
		content: impl Into<Vec<u8>>,
	) -> Result<Self, ConversationError> {
		let source = ContextPackSource::new(
			ContextSourceKind::PinnedRevision,
			source_id,
			revision,
			content,
		)?;

		if source.content.is_empty() {
			return Err(ConversationError::InvalidBound("pinned source content"));
		}

		Ok(Self(source))
	}
}

/// Deterministic Context Pack size and recent-window policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ContextPackPolicy {
	max_bytes: usize,
	recent_item_limit: usize,
}
impl ContextPackPolicy {
	/// Validate a closed Context Pack policy.
	pub fn new(max_bytes: usize, recent_item_limit: usize) -> Result<Self, ConversationError> {
		if !(MIN_CONTEXT_PACK_BYTES..=MAX_CONTEXT_PACK_BYTES).contains(&max_bytes)
			|| recent_item_limit == 0
			|| recent_item_limit > MAX_CONTEXT_RECENT_ITEMS
		{
			return Err(ConversationError::InvalidContextPolicy);
		}

		Ok(Self { max_bytes, recent_item_limit })
	}

	/// Maximum compiled byte length.
	pub fn max_bytes(self) -> usize {
		self.max_bytes
	}

	/// Maximum recent raw items selected from the tail.
	pub fn recent_item_limit(self) -> usize {
		self.recent_item_limit
	}

	fn validate(self) -> Result<(), ConversationError> {
		Self::new(self.max_bytes, self.recent_item_limit).map(|_| ())
	}
}

impl<'de> Deserialize<'de> for ContextPackPolicy {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawPolicy {
			max_bytes: usize,
			recent_item_limit: usize,
		}

		let raw = RawPolicy::deserialize(deserializer)?;

		Self::new(raw.max_bytes, raw.recent_item_limit).map_err(serde::de::Error::custom)
	}
}

/// Caller-pinned inputs for deterministic Context Pack compilation.
#[derive(Clone, Debug)]
pub struct ContextPackInput {
	/// Logical Conversation represented by the pack.
	pub conversation_id: ConversationId,
	/// Side-effect uncertainty carried into a proposed continuation.
	pub possible_side_effects: PossibleSideEffects,
	/// Deterministic compilation policy.
	pub policy: ContextPackPolicy,
	/// Mandatory pinned durable revision.
	pub pinned: PinnedContextSource,
	/// Optional decisions, facts, instructions, artifacts, and recent raw items.
	pub optional_sources: Vec<ContextPackSource>,
}

/// Immutable source-manifest entry bound into the compiled digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceManifest {
	kind: ContextSourceKind,
	source_id: String,
	revision: u64,
	content_digest: BlobHash,
	original_byte_length: u64,
	included_byte_length: u64,
	included_digest: BlobHash,
	disposition: ContextSourceDisposition,
	artifact: Option<(ArtifactId, u64)>,
}
impl ContextSourceManifest {
	/// Reconstruct one persisted manifest entry after validating every canonical relation.
	#[allow(clippy::too_many_arguments)]
	pub fn from_persisted(
		kind: ContextSourceKind,
		source_id: impl Into<String>,
		revision: u64,
		content_digest: BlobHash,
		original_byte_length: u64,
		included_byte_length: u64,
		included_digest: BlobHash,
		disposition: ContextSourceDisposition,
		artifact: Option<(ArtifactId, u64)>,
	) -> Result<Self, ConversationError> {
		let manifest = Self {
			kind,
			source_id: source_id.into(),
			revision,
			content_digest,
			original_byte_length,
			included_byte_length,
			included_digest,
			disposition,
			artifact,
		};

		manifest.validate()?;

		Ok(manifest)
	}

	/// Semantic source category.
	pub fn kind(&self) -> ContextSourceKind {
		self.kind
	}

	/// Stable source identity.
	pub fn source_id(&self) -> &str {
		&self.source_id
	}

	/// Exact source revision.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	/// Digest of the complete source.
	pub fn content_digest(&self) -> BlobHash {
		self.content_digest
	}

	/// Complete source byte length.
	pub fn original_byte_length(&self) -> u64 {
		self.original_byte_length
	}

	/// Number of represented bytes.
	pub fn included_byte_length(&self) -> u64 {
		self.included_byte_length
	}

	/// Digest of represented bytes.
	pub fn included_digest(&self) -> BlobHash {
		self.included_digest
	}

	/// Canonical representation decision.
	pub fn disposition(&self) -> ContextSourceDisposition {
		self.disposition
	}

	/// Typed Artifact identity and revision, when applicable.
	pub fn artifact_reference(&self) -> Option<(&ArtifactId, u64)> {
		self.artifact.as_ref().map(|(id, revision)| (id, *revision))
	}

	fn validate(&self) -> Result<(), ConversationError> {
		validate_nonempty(&self.source_id, 256, "context source identity")?;
		validate_revision(self.revision, "context source")?;

		if self.original_byte_length > MAX_CONTEXT_SOURCE_INPUT_BYTES as u64
			|| self.included_byte_length > self.original_byte_length
		{
			return Err(ConversationError::InvalidContextPolicy);
		}

		let empty_digest = BlobHash::digest(&[]);
		let canonical_disposition = match self.disposition {
			ContextSourceDisposition::Complete =>
				self.original_byte_length > 0
					&& self.included_byte_length == self.original_byte_length
					&& self.included_digest == self.content_digest,
			ContextSourceDisposition::Truncated =>
				self.included_byte_length > 0
					&& self.included_byte_length < self.original_byte_length,
			ContextSourceDisposition::Omitted =>
				self.included_byte_length == 0 && self.included_digest == empty_digest,
		};
		let canonical_artifact = match (&self.kind, &self.artifact) {
			(ContextSourceKind::Artifact, Some((id, revision))) =>
				id.as_str() == self.source_id && *revision == self.revision,
			(ContextSourceKind::Artifact, None) | (_, Some(_)) => false,
			(_, None) => true,
		};

		if !canonical_disposition || !canonical_artifact {
			return Err(ConversationError::InvalidContextPolicy);
		}

		Ok(())
	}
}

/// Immutable compiled Context Pack plus inspectable provenance selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPack {
	conversation_id: ConversationId,
	possible_side_effects: PossibleSideEffects,
	policy: ContextPackPolicy,
	source_manifest: Vec<ContextSourceManifest>,
	manifest_digest: BlobHash,
	bytes: Vec<u8>,
	digest: BlobHash,
	omitted_source_count: usize,
	truncated: bool,
}
impl ContextPack {
	/// Logical Conversation bound into the compiled bytes.
	pub fn conversation_id(&self) -> &ConversationId {
		&self.conversation_id
	}

	/// Explicit possible-side-effect state bound into the compiled bytes.
	pub fn possible_side_effects(&self) -> PossibleSideEffects {
		self.possible_side_effects
	}

	/// Deterministic size/window policy bound into the compiled bytes.
	pub fn policy(&self) -> ContextPackPolicy {
		self.policy
	}

	/// Complete canonical source manifest, including omitted optional sources.
	pub fn source_manifest(&self) -> &[ContextSourceManifest] {
		&self.source_manifest
	}

	/// Digest of the complete canonical source manifest.
	pub fn manifest_digest(&self) -> BlobHash {
		self.manifest_digest
	}

	/// Exact compiled bytes.
	pub fn bytes(&self) -> &[u8] {
		&self.bytes
	}

	/// Render the represented UTF-8 sources as one bounded model input.
	///
	/// The length-delimited binary bytes remain the persisted authority. This view removes the
	/// binary header and framing only after full verification; it never reconstructs omitted data.
	pub fn render_model_input(&self) -> Result<String, ConversationError> {
		self.verify()?;
		let mut cursor = encoded_header_length();
		let mut output = String::new();
		for (position, source) in self.source_manifest.iter().enumerate() {
			if source.included_byte_length == 0 {
				continue;
			}
			let encoded_position = read_u16(&self.bytes, &mut cursor)?;
			let length = usize::try_from(read_u32(&self.bytes, &mut cursor)?)
				.map_err(|_| ConversationError::InvalidContextPolicy)?;
			let end = cursor
				.checked_add(length)
				.filter(|end| *end <= self.bytes.len())
				.ok_or(ConversationError::InvalidContextPolicy)?;
			if usize::from(encoded_position) != position
				|| u64::try_from(length).ok() != Some(source.included_byte_length)
			{
				return Err(ConversationError::InvalidContextPolicy);
			}
			let represented = &self.bytes[cursor..end];
			let text = match std::str::from_utf8(represented) {
				Ok(text) => text,
				Err(error) if error.error_len().is_none() =>
					std::str::from_utf8(&represented[..error.valid_up_to()])
						.map_err(|_| ConversationError::InvalidContextPolicy)?,
				Err(_) => return Err(ConversationError::InvalidContextPolicy),
			};
			if text.contains('\0') {
				return Err(ConversationError::InvalidContextPolicy);
			}
			if !output.is_empty() && !text.is_empty() {
				output.push_str("\n\n");
			}
			output.push_str(text);
			cursor = end;
		}
		if cursor != self.bytes.len() || output.is_empty() || output.len() > self.policy.max_bytes {
			return Err(ConversationError::InvalidContextPolicy);
		}
		Ok(output)
	}

	/// Digest of exact compiled bytes.
	pub fn digest(&self) -> BlobHash {
		self.digest
	}

	/// Count of optional sources represented only in the manifest.
	pub fn omitted_source_count(&self) -> usize {
		self.omitted_source_count
	}

	/// Whether any source was shortened or omitted.
	pub fn truncated(&self) -> bool {
		self.truncated
	}

	/// Reconstruct and verify a complete immutable record read from persistence.
	pub fn from_persisted(
		conversation_id: ConversationId,
		possible_side_effects: PossibleSideEffects,
		policy: ContextPackPolicy,
		source_manifest: Vec<ContextSourceManifest>,
		bytes: Vec<u8>,
		digest: BlobHash,
	) -> Result<Self, ConversationError> {
		let manifest_digest = digest_manifest(&source_manifest)?;
		let omitted_source_count = source_manifest
			.iter()
			.filter(|source| source.disposition == ContextSourceDisposition::Omitted)
			.count();
		let truncated = source_manifest
			.iter()
			.any(|source| source.disposition != ContextSourceDisposition::Complete);
		let pack = Self {
			conversation_id,
			possible_side_effects,
			policy,
			source_manifest,
			manifest_digest,
			bytes,
			digest,
			omitted_source_count,
			truncated,
		};

		pack.verify()?;

		Ok(pack)
	}

	/// Revalidate every bound field, manifest entry, represented prefix, and digest.
	pub fn verify(&self) -> Result<(), ConversationError> {
		self.policy.validate()?;

		let canonical_manifest =
			self.source_manifest.iter().all(|source| source.validate().is_ok());
		let pinned_is_complete = self.source_manifest.first().is_some_and(|source| {
			source.kind == ContextSourceKind::PinnedRevision
				&& source.disposition == ContextSourceDisposition::Complete
				&& source.included_byte_length > 0
				&& source.artifact.is_none()
		});
		let source_order_is_canonical = self.source_manifest.windows(2).all(|pair| {
			context_kind_tag(pair[0].kind) <= context_kind_tag(pair[1].kind)
				&& pair[1].kind != ContextSourceKind::PinnedRevision
		});
		let omitted_source_count = self
			.source_manifest
			.iter()
			.filter(|source| source.disposition == ContextSourceDisposition::Omitted)
			.count();
		let truncated = self
			.source_manifest
			.iter()
			.any(|source| source.disposition != ContextSourceDisposition::Complete);

		if self.source_manifest.is_empty()
			|| self.source_manifest.len() > MAX_CONTEXT_SOURCES
			|| !canonical_manifest
			|| !pinned_is_complete
			|| !source_order_is_canonical
			|| self.bytes.is_empty()
			|| self.bytes.len() > self.policy.max_bytes
			|| self.digest != BlobHash::digest(&self.bytes)
			|| self.manifest_digest != digest_manifest(&self.source_manifest)?
			|| self.omitted_source_count != omitted_source_count
			|| self.truncated != truncated
		{
			return Err(ConversationError::InvalidContextPolicy);
		}

		verify_pack_encoding(self)
	}
}

/// Return whether text is one bounded canonical `type/subtype` media type.
///
/// Parameters are deliberately unavailable in this persistence slice. Both components use
/// the visible RFC token subset shared by durable-store and the typed wire contract.
pub fn is_canonical_media_type(value: &str) -> bool {
	value.len() <= 128
		&& value.split_once('/').is_some_and(|(type_name, subtype)| {
			!type_name.is_empty()
				&& !subtype.is_empty()
				&& type_name.bytes().chain(subtype.bytes()).all(|byte| {
					byte.is_ascii_alphanumeric()
						|| matches!(
							byte,
							b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
						)
				})
		})
}

/// Return whether a normalized metadata key is reserved for credential material.
///
/// Only ASCII letters and digits participate in classification. Separators are ignored and
/// matching is suffix-based so namespaced forms such as `auth_token` fail closed.
pub fn is_credential_metadata_key(key: &str) -> bool {
	let normalized: String = key
		.chars()
		.filter(char::is_ascii_alphanumeric)
		.map(|character| character.to_ascii_lowercase())
		.collect();

	[
		"credential",
		"credentials",
		"password",
		"passphrase",
		"privatekey",
		"secret",
		"authorization",
		"bearer",
		"apikey",
		"cookie",
		"token",
		"session",
	]
	.iter()
	.any(|suffix| normalized.ends_with(suffix))
}

/// Return whether ordinary text contains a concrete credential representation.
///
/// Ordinary words such as `secret sauce`, `token budget`, and `session summary` are not
/// credentials. The closed patterns match authorization schemes, known token formats, explicit
/// credential assignments, embedded URL passwords, private-key headers, and AWS access keys.
pub fn contains_credential_material(value: &str) -> bool {
	credential_value_pattern().is_match(&normalize_credential_scan_text(value))
}

/// Compile immutable, inspectable context from caller-pinned source revisions. Sources are
/// ordered by semantic class and caller order; recent raw input keeps the newest bounded tail.
/// The same validated input always produces byte-identical output.
pub fn compile_context_pack(input: ContextPackInput) -> Result<ContextPack, ConversationError> {
	input.policy.validate()?;

	if input.optional_sources.len() >= MAX_CONTEXT_SOURCES
		|| input
			.optional_sources
			.iter()
			.any(|source| source.kind == ContextSourceKind::PinnedRevision)
	{
		return Err(ConversationError::InvalidContextPolicy);
	}

	let aggregate_source_bytes = input
		.optional_sources
		.iter()
		.try_fold(input.pinned.0.content.len(), |total, source| {
			total.checked_add(source.content.len())
		})
		.ok_or(ConversationError::InvalidContextPolicy)?;

	if aggregate_source_bytes > MAX_CONTEXT_SOURCE_INPUT_BYTES {
		return Err(ConversationError::InvalidContextPolicy);
	}

	let mut sources = Vec::with_capacity(input.optional_sources.len() + 1);

	sources.push(input.pinned.0);
	sources.extend(input.optional_sources);
	sources[1..].sort_by_key(|source| context_kind_tag(source.kind));

	let recent_count =
		sources.iter().filter(|source| source.kind == ContextSourceKind::RecentRaw).count();
	let mut recent_seen = 0_usize;
	let eligible = sources
		.iter()
		.map(|source| {
			if source.kind != ContextSourceKind::RecentRaw {
				true
			} else {
				recent_seen += 1;

				recent_seen > recent_count.saturating_sub(input.policy.recent_item_limit)
			}
		})
		.collect::<Vec<_>>();
	let fixed_length = encoded_header_length();
	let pinned_length = sources[0].content.len();
	let mandatory_length =
		fixed_length.checked_add(6).and_then(|size| size.checked_add(pinned_length));

	if mandatory_length.is_none_or(|length| length > input.policy.max_bytes) {
		return Err(ConversationError::InvalidContextPolicy);
	}

	let mut remaining = input.policy.max_bytes - fixed_length - 6 - pinned_length;
	let mut represented = Vec::with_capacity(sources.len());

	represented.push(pinned_length);

	for (position, source) in sources.iter().enumerate().skip(1) {
		let included_length = if !eligible[position] || remaining <= 6 {
			0
		} else {
			let remaining_selected =
				eligible[position..].iter().filter(|selected| **selected).count();
			let fair_share = (remaining - 6) / remaining_selected.max(1);
			let selected = source.content.len().min(fair_share);

			remaining = remaining.saturating_sub(6 + selected);

			selected
		};

		represented.push(included_length);
	}

	let source_manifest = sources
		.iter()
		.zip(&represented)
		.map(|(source, included)| source_manifest(source, *included))
		.collect::<Result<Vec<_>, _>>()?;
	let manifest_digest = digest_manifest(&source_manifest)?;
	let truncated = source_manifest
		.iter()
		.any(|source| source.disposition != ContextSourceDisposition::Complete);
	let omitted_source_count = source_manifest
		.iter()
		.filter(|source| source.disposition == ContextSourceDisposition::Omitted)
		.count();
	let bytes = encode_pack(
		&input.conversation_id,
		input.possible_side_effects,
		input.policy,
		manifest_digest,
		truncated,
		&sources,
		&represented,
	)?;
	let pack = ContextPack {
		conversation_id: input.conversation_id,
		possible_side_effects: input.possible_side_effects,
		policy: input.policy,
		source_manifest,
		manifest_digest,
		digest: BlobHash::digest(&bytes),
		bytes,
		omitted_source_count,
		truncated,
	};

	pack.verify()?;

	Ok(pack)
}

fn normalize_credential_scan_text(value: &str) -> String {
	value
		.chars()
		.map(|character| match character {
			'\u{0009}'
			| '\u{000a}'
			| '\u{000b}'
			| '\u{000c}'
			| '\u{000d}'
			| '\u{0020}'
			| '\u{0085}'
			| '\u{00a0}'
			| '\u{1680}'
			| '\u{2000}'..='\u{200a}'
			| '\u{2028}'
			| '\u{2029}'
			| '\u{202f}'
			| '\u{205f}'
			| '\u{3000}' => ' ',
			'A'..='Z' => character.to_ascii_lowercase(),
			_ => character,
		})
		.collect()
}

fn credential_value_pattern() -> &'static Regex {
	static PATTERN: OnceLock<Regex> = OnceLock::new();

	PATTERN.get_or_init(|| {
		Regex::new(
			r"(?x)
			(?:^|[[:space:][:punct:]])(?:bearer[[:space:]]+[[:alnum:]_.~+/-]{8,}|basic[[:space:]]+[[:alnum:]+/]{8,}={0,2})
			|(?:^|[^[:alnum:]])(?:sk-[[:alnum:]_-]{8,}|(?:sk|pk|rk)_(?:live|test|proj)?[[:alnum:]_-]{8,}|xox[baprs]-[[:alnum:]-]{8,}|glpat-[[:alnum:]_-]{8,}|npm_[[:alnum:]]{8,})
			|gh[pousr]_[[:alnum:]]{20,}
			|eyj[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}
			|-----begin[^-]*private[[:space:]]+key-----
			|(?:password|passphrase|secret|token|authorization)[[:space:]]*[:=][[:space:]]*[^[:space:]]{4,}
			|[a-z][a-z0-9+.-]*://[^/:[:space:]]+:[^@[:space:]]+@
			|akia[0-9a-z]{16}",
		)
		.expect("credential material regex is valid")
	})
}

fn source_manifest(
	source: &ContextPackSource,
	included_length: usize,
) -> Result<ContextSourceManifest, ConversationError> {
	let original_byte_length = u64::try_from(source.content.len())
		.map_err(|_| ConversationError::InvalidBound("context source content"))?;
	let included_byte_length = u64::try_from(included_length)
		.map_err(|_| ConversationError::InvalidBound("context source content"))?;
	let disposition = if included_length == 0 {
		ContextSourceDisposition::Omitted
	} else if included_length == source.content.len() {
		ContextSourceDisposition::Complete
	} else {
		ContextSourceDisposition::Truncated
	};

	ContextSourceManifest::from_persisted(
		source.kind,
		source.source_id.clone(),
		source.revision,
		source.content_digest,
		original_byte_length,
		included_byte_length,
		BlobHash::digest(&source.content[..included_length]),
		disposition,
		source.artifact.clone(),
	)
}

fn digest_manifest(manifest: &[ContextSourceManifest]) -> Result<BlobHash, ConversationError> {
	let mut bytes = Vec::new();

	push_u16(&mut bytes, manifest.len())?;

	for source in manifest {
		bytes.push(context_kind_tag(source.kind));

		push_bytes_u16(&mut bytes, source.source_id.as_bytes())?;

		bytes.extend_from_slice(&source.revision.to_be_bytes());
		bytes.extend_from_slice(source.content_digest.to_hex().as_bytes());
		bytes.extend_from_slice(&source.original_byte_length.to_be_bytes());
		bytes.extend_from_slice(&source.included_byte_length.to_be_bytes());
		bytes.extend_from_slice(source.included_digest.to_hex().as_bytes());
		bytes.push(disposition_tag(source.disposition));

		match &source.artifact {
			Some((id, revision)) => {
				bytes.push(1);
				bytes.extend_from_slice(id.as_str().as_bytes());
				bytes.extend_from_slice(&revision.to_be_bytes());
			},
			None => bytes.push(0),
		}
	}

	Ok(BlobHash::digest(&bytes))
}

fn encode_pack(
	conversation_id: &ConversationId,
	possible_side_effects: PossibleSideEffects,
	policy: ContextPackPolicy,
	manifest_digest: BlobHash,
	truncated: bool,
	sources: &[ContextPackSource],
	represented: &[usize],
) -> Result<Vec<u8>, ConversationError> {
	policy.validate()?;

	let mut bytes = Vec::with_capacity(policy.max_bytes);

	bytes.extend_from_slice(CONTEXT_PACK_MAGIC);
	bytes.extend_from_slice(conversation_id.as_str().as_bytes());
	bytes.push(side_effect_tag(possible_side_effects));

	push_u32(&mut bytes, policy.max_bytes)?;
	push_u16(&mut bytes, policy.recent_item_limit)?;
	push_u16(&mut bytes, sources.len())?;

	bytes.extend_from_slice(manifest_digest.to_hex().as_bytes());
	bytes.push(u8::from(truncated));

	for (position, (source, included)) in sources.iter().zip(represented).enumerate() {
		if *included == 0 {
			continue;
		}

		push_u16(&mut bytes, position)?;
		push_u32(&mut bytes, *included)?;

		bytes.extend_from_slice(&source.content[..*included]);
	}

	if bytes.len() > policy.max_bytes {
		return Err(ConversationError::InvalidContextPolicy);
	}

	Ok(bytes)
}

fn verify_pack_encoding(pack: &ContextPack) -> Result<(), ConversationError> {
	let mut expected_prefix = Vec::new();

	expected_prefix.extend_from_slice(CONTEXT_PACK_MAGIC);
	expected_prefix.extend_from_slice(pack.conversation_id.as_str().as_bytes());
	expected_prefix.push(side_effect_tag(pack.possible_side_effects));

	push_u32(&mut expected_prefix, pack.policy.max_bytes)?;
	push_u16(&mut expected_prefix, pack.policy.recent_item_limit)?;
	push_u16(&mut expected_prefix, pack.source_manifest.len())?;

	expected_prefix.extend_from_slice(pack.manifest_digest.to_hex().as_bytes());
	expected_prefix.push(u8::from(pack.truncated));

	if !pack.bytes.starts_with(&expected_prefix) {
		return Err(ConversationError::InvalidContextPolicy);
	}

	let mut cursor = expected_prefix.len();

	for (position, source) in pack.source_manifest.iter().enumerate() {
		if source.included_byte_length == 0 {
			continue;
		}

		let encoded_position = read_u16(&pack.bytes, &mut cursor)?;
		let length = usize::try_from(read_u32(&pack.bytes, &mut cursor)?)
			.map_err(|_| ConversationError::InvalidContextPolicy)?;

		if usize::from(encoded_position) != position
			|| u64::try_from(length).ok() != Some(source.included_byte_length)
			|| cursor.checked_add(length).is_none_or(|end| end > pack.bytes.len())
		{
			return Err(ConversationError::InvalidContextPolicy);
		}

		let end = cursor + length;

		if BlobHash::digest(&pack.bytes[cursor..end]) != source.included_digest {
			return Err(ConversationError::InvalidContextPolicy);
		}

		cursor = end;
	}

	if cursor != pack.bytes.len()
		|| pack.omitted_source_count
			!= pack
				.source_manifest
				.iter()
				.filter(|source| source.disposition == ContextSourceDisposition::Omitted)
				.count()
	{
		return Err(ConversationError::InvalidContextPolicy);
	}

	Ok(())
}

fn encoded_header_length() -> usize {
	CONTEXT_PACK_MAGIC.len() + 36 + 1 + 4 + 2 + 2 + 64 + 1
}

fn context_kind_tag(kind: ContextSourceKind) -> u8 {
	match kind {
		ContextSourceKind::PinnedRevision => 0,
		ContextSourceKind::RepositoryInstructions => 1,
		ContextSourceKind::OpenWiki => 2,
		ContextSourceKind::Decision => 3,
		ContextSourceKind::Fact => 4,
		ContextSourceKind::Artifact => 5,
		ContextSourceKind::RecentRaw => 6,
	}
}

fn side_effect_tag(state: PossibleSideEffects) -> u8 {
	match state {
		PossibleSideEffects::None => 0,
		PossibleSideEffects::Possible => 1,
		PossibleSideEffects::Unknown => 2,
	}
}

fn disposition_tag(disposition: ContextSourceDisposition) -> u8 {
	match disposition {
		ContextSourceDisposition::Complete => 0,
		ContextSourceDisposition::Truncated => 1,
		ContextSourceDisposition::Omitted => 2,
	}
}

fn push_u16(bytes: &mut Vec<u8>, value: usize) -> Result<(), ConversationError> {
	let value = u16::try_from(value).map_err(|_| ConversationError::InvalidContextPolicy)?;

	bytes.extend_from_slice(&value.to_be_bytes());

	Ok(())
}

fn push_u32(bytes: &mut Vec<u8>, value: usize) -> Result<(), ConversationError> {
	let value = u32::try_from(value).map_err(|_| ConversationError::InvalidContextPolicy)?;

	bytes.extend_from_slice(&value.to_be_bytes());

	Ok(())
}

fn push_bytes_u16(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), ConversationError> {
	push_u16(bytes, value.len())?;

	bytes.extend_from_slice(value);

	Ok(())
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, ConversationError> {
	let end = cursor.checked_add(2).ok_or(ConversationError::InvalidContextPolicy)?;
	let value = bytes.get(*cursor..end).ok_or(ConversationError::InvalidContextPolicy)?;

	*cursor = end;

	Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, ConversationError> {
	let end = cursor.checked_add(4).ok_or(ConversationError::InvalidContextPolicy)?;
	let value = bytes.get(*cursor..end).ok_or(ConversationError::InvalidContextPolicy)?;

	*cursor = end;

	Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

fn validate_nonempty(
	value: &str,
	max_bytes: usize,
	field: &'static str,
) -> Result<(), ConversationError> {
	if value.is_empty() || value.len() > max_bytes {
		Err(ConversationError::InvalidBound(field))
	} else {
		Ok(())
	}
}

fn validate_symbol(
	value: &str,
	max_bytes: usize,
	field: &'static str,
) -> Result<(), ConversationError> {
	validate_nonempty(value, max_bytes, field)?;

	if value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
	{
		Ok(())
	} else {
		Err(ConversationError::InvalidBound(field))
	}
}

fn validate_revision(value: u64, field: &'static str) -> Result<(), ConversationError> {
	if value == 0 { Err(ConversationError::InvalidRevision(field)) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use crate::{
		ArtifactId, ArtifactReference, BlobHash, ContextPackSource, ContextSourceDisposition,
		ContextSourceKind, ConversationError, ConversationId, ConversationStatus, HistoryMediaType,
		HistoryMetadata, HistoryMetadataValue, MAX_CONTEXT_SOURCE_INPUT_BYTES, MAX_CONTEXT_SOURCES,
		MAX_HISTORY_METADATA_FIELDS, MAX_INLINE_HISTORY_BYTES, NormalizedPayload,
		PinnedContextSource,
		conversation::{
			self, ContextPackInput, ContextPackPolicy, Conversation, MIN_CONTEXT_PACK_BYTES,
			PossibleSideEffects,
		},
	};

	#[test]
	fn media_types_share_one_canonical_domain_invariant() {
		let hash = BlobHash::digest(b"artifact");

		for valid in ["text/plain", "application/octet-stream", "X.Custom/vnd+json"] {
			assert!(super::is_canonical_media_type(valid));
			assert!(ArtifactReference::new(hash, 8, valid).is_ok());
		}
		for invalid in [
			"",
			"not a media type",
			"text/",
			"/plain",
			"text/plain; charset=utf-8",
			"text/plain\n",
			"text/plain/extra",
		] {
			assert!(!super::is_canonical_media_type(invalid));
			assert!(ArtifactReference::new(hash, 8, invalid).is_err());
		}
	}

	fn id() -> ConversationId {
		ConversationId::new("00000000-0000-4000-8000-000000000001").unwrap()
	}

	#[test]
	fn logical_conversation_has_no_runtime_or_account_identity() {
		let conversation = Conversation::new(id(), "A durable dialogue").unwrap();

		assert_eq!(conversation.status, ConversationStatus::Open);
		assert_eq!(conversation.revision, 1);
	}

	#[test]
	fn large_inline_payload_requires_blob_offload() {
		assert_eq!(
			NormalizedPayload::inline(
				"x".repeat(MAX_INLINE_HISTORY_BYTES + 1),
				HistoryMediaType::new("text/plain").unwrap(),
				HistoryMetadata::empty(),
			),
			Err(ConversationError::PayloadRequiresBlob),
		);
	}

	#[test]
	fn history_projection_has_one_bounded_credential_negative_contract() {
		let benign = BTreeMap::from([
			("note".to_owned(), HistoryMetadataValue::Text("secret sauce".to_owned())),
			("summary".to_owned(), HistoryMetadataValue::Text("token budget".to_owned())),
			("visible".to_owned(), HistoryMetadataValue::Boolean(true)),
		]);

		assert!(HistoryMetadata::new(benign).is_ok());
		assert!(HistoryMediaType::new("application/json").is_ok());
		assert!(HistoryMediaType::new("application/json; charset=utf-8").is_err());

		for key in ["token", "auth_session", "service-api-key", "PRIVATE_KEY"] {
			assert_eq!(
				HistoryMetadata::new(BTreeMap::from([(
					key.to_owned(),
					HistoryMetadataValue::Text("ordinary".to_owned()),
				)])),
				Err(ConversationError::CredentialRejected),
			);
		}
		for value in [
			"Bearer abcdefgh",
			"secret=abcd",
			"https://user:password@example.test/path",
			"AKIA1234567890ABCDEF",
		] {
			assert_eq!(
				HistoryMetadata::new(BTreeMap::from([(
					"note".to_owned(),
					HistoryMetadataValue::Text(value.to_owned()),
				)])),
				Err(ConversationError::CredentialRejected),
			);
		}
	}

	#[test]
	fn history_projection_uses_utf8_byte_bounds_and_closed_scalars() {
		let maximum = (0..MAX_HISTORY_METADATA_FIELDS)
			.map(|index| (format!("field-{index}"), HistoryMetadataValue::Boolean(true)))
			.collect();

		assert!(HistoryMetadata::new(maximum).is_ok());
		assert!(
			HistoryMetadata::new(BTreeMap::from([(
				"é".repeat(32),
				HistoryMetadataValue::Text("é".repeat(128)),
			)]))
			.is_ok()
		);
		assert!(
			HistoryMetadata::new(BTreeMap::from([(
				"é".repeat(33),
				HistoryMetadataValue::Boolean(true),
			)]))
			.is_err()
		);
		assert!(
			HistoryMetadata::new(BTreeMap::from([(
				"note".to_owned(),
				HistoryMetadataValue::Text("é".repeat(129)),
			)]))
			.is_err()
		);
	}

	#[test]
	fn context_pack_is_deterministic_and_keeps_recent_tail() {
		let policy = ContextPackPolicy::new(2_048, 2).unwrap();
		let optional_sources = vec![
			ContextPackSource::new(ContextSourceKind::RecentRaw, "turn-1", 1, "old").unwrap(),
			ContextPackSource::new(ContextSourceKind::RecentRaw, "turn-2", 1, "newer").unwrap(),
			ContextPackSource::new(ContextSourceKind::RecentRaw, "turn-3", 1, "newest").unwrap(),
		];
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::Unknown,
			policy,
			pinned: PinnedContextSource::new("project", 7, "pinned").unwrap(),
			optional_sources,
		};
		let first = conversation::compile_context_pack(input.clone()).unwrap();
		let second = conversation::compile_context_pack(input).unwrap();

		assert_eq!(first, second);
		assert_eq!(first.source_manifest()[1].disposition(), ContextSourceDisposition::Omitted);
		assert_ne!(first.source_manifest()[2].disposition(), ContextSourceDisposition::Omitted);
		assert_ne!(first.source_manifest()[3].disposition(), ContextSourceDisposition::Omitted);
		assert_eq!(first.render_model_input().unwrap(), "pinned\n\nnewer\n\nnewest");
	}

	#[test]
	fn context_pack_reserves_pinned_bytes_at_minimum_budget_with_maximum_sources() {
		let policy = ContextPackPolicy::new(MIN_CONTEXT_PACK_BYTES, 8).unwrap();
		let optional_sources = (0..MAX_CONTEXT_SOURCES - 1)
			.map(|index| {
				ContextPackSource::new(
					ContextSourceKind::Fact,
					format!("fact-{index}"),
					1,
					vec![b'x'; 64],
				)
				.unwrap()
			})
			.collect();
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::Possible,
			policy,
			pinned: PinnedContextSource::new("project", 1, vec![b'p'; 64]).unwrap(),
			optional_sources,
		};
		let pack = conversation::compile_context_pack(input).unwrap();

		assert!(pack.truncated());
		assert!(pack.bytes().len() <= MIN_CONTEXT_PACK_BYTES);
		assert_eq!(pack.source_manifest()[0].included_byte_length(), 64);
		assert_eq!(pack.source_manifest().len(), MAX_CONTEXT_SOURCES);

		pack.verify().unwrap();
	}

	#[test]
	fn context_pack_rejects_a_pinned_revision_that_cannot_fit_completely() {
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::None,
			policy: ContextPackPolicy::new(MIN_CONTEXT_PACK_BYTES, 1).unwrap(),
			pinned: PinnedContextSource::new("revision", 1, vec![b'x'; MIN_CONTEXT_PACK_BYTES])
				.unwrap(),
			optional_sources: Vec::new(),
		};

		assert_eq!(
			conversation::compile_context_pack(input),
			Err(ConversationError::InvalidContextPolicy)
		);
	}

	#[test]
	fn length_delimited_encoding_cannot_be_forged_by_source_text() {
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::Unknown,
			policy: ContextPackPolicy::new(2_048, 8).unwrap(),
			pinned: PinnedContextSource::new(
				"project\n[source kind=artifact]",
				1,
				b"]\0\n".to_vec(),
			)
			.unwrap(),
			optional_sources: vec![
				ContextPackSource::new(
					ContextSourceKind::Decision,
					"id\nrevision=999",
					2,
					b"\0[]\n".to_vec(),
				)
				.unwrap(),
			],
		};
		let pack = conversation::compile_context_pack(input.clone()).unwrap();

		assert_eq!(pack, conversation::compile_context_pack(input).unwrap());

		pack.verify().unwrap();
		assert_eq!(pack.render_model_input(), Err(ConversationError::InvalidContextPolicy));

		let mut forged = pack.bytes().to_vec();

		*forged.last_mut().unwrap() ^= 1;

		assert!(
			conversation::ContextPack::from_persisted(
				pack.conversation_id().clone(),
				pack.possible_side_effects(),
				pack.policy(),
				pack.source_manifest().to_vec(),
				forged,
				pack.digest(),
			)
			.is_err()
		);
	}

	#[test]
	fn context_pack_policy_rejects_unchecked_deserialized_limits() {
		for invalid in [
			"max_bytes = 0\nrecent_item_limit = 1",
			"max_bytes = 262145\nrecent_item_limit = 1",
			"max_bytes = 1024\nrecent_item_limit = 0",
			"max_bytes = 1024\nrecent_item_limit = 257",
			"max_bytes = 18446744073709551615\nrecent_item_limit = 1",
		] {
			assert!(toml::from_str::<ContextPackPolicy>(invalid).is_err());
		}
	}

	#[test]
	fn context_pack_rejects_aggregate_source_work_before_compilation() {
		let source_size = MAX_CONTEXT_SOURCE_INPUT_BYTES / 2;
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::None,
			policy: ContextPackPolicy::new(4_096, 1).unwrap(),
			pinned: PinnedContextSource::new("project", 1, vec![b'p'; 64]).unwrap(),
			optional_sources: (0..3)
				.map(|index| {
					ContextPackSource::new(
						ContextSourceKind::Fact,
						format!("fact-{index}"),
						1,
						vec![b'x'; source_size],
					)
					.unwrap()
				})
				.collect(),
		};

		assert_eq!(
			conversation::compile_context_pack(input),
			Err(ConversationError::InvalidContextPolicy)
		);
	}

	#[test]
	fn persisted_manifest_rejects_every_noncanonical_relationship() {
		let input = ContextPackInput {
			conversation_id: id(),
			possible_side_effects: PossibleSideEffects::Unknown,
			policy: ContextPackPolicy::new(2_048, 1).unwrap(),
			pinned: PinnedContextSource::new("project", 1, b"pinned".to_vec()).unwrap(),
			optional_sources: vec![
				ContextPackSource::artifact(
					ArtifactId::new("00000000-0000-4000-8000-000000000099").unwrap(),
					2,
					b"artifact".to_vec(),
				)
				.unwrap(),
			],
		};
		let pack = conversation::compile_context_pack(input).unwrap();
		let assert_rejected = |manifest| {
			assert!(
				conversation::ContextPack::from_persisted(
					pack.conversation_id().clone(),
					pack.possible_side_effects(),
					pack.policy(),
					manifest,
					pack.bytes().to_vec(),
					pack.digest(),
				)
				.is_err()
			);
		};
		let mut mismatched_complete = pack.source_manifest().to_vec();

		mismatched_complete[0].original_byte_length += 1;

		assert_rejected(mismatched_complete);

		let mut mismatched_digest = pack.source_manifest().to_vec();

		mismatched_digest[0].content_digest = crate::BlobHash::digest(b"forged");

		assert_rejected(mismatched_digest);

		let mut invalid_omitted = pack.source_manifest().to_vec();

		invalid_omitted[1].disposition = ContextSourceDisposition::Omitted;
		invalid_omitted[1].included_byte_length = 0;

		assert_rejected(invalid_omitted);

		let mut invalid_artifact = pack.source_manifest().to_vec();

		invalid_artifact[1].artifact = None;

		assert_rejected(invalid_artifact);

		let mut wrong_artifact_revision = pack.source_manifest().to_vec();

		wrong_artifact_revision[1].artifact.as_mut().unwrap().1 = 3;

		assert_rejected(wrong_artifact_revision);

		let mut extra_pinned = pack.source_manifest().to_vec();

		extra_pinned[1].kind = ContextSourceKind::PinnedRevision;

		assert_rejected(extra_pinned);
	}
}
