//! Logical Conversation, RuntimeSession, bounded history, blob, and Context Pack persistence.
//!
//! This owner intentionally exposes no account selection, process start, Codex dispatch,
//! rollover executor, fallback executor, or wake scheduler.

use std::time::Duration;
#[cfg(debug_assertions)] use std::{env, fs, path::PathBuf};

use deadpool_postgres::{Client, ClientWrapper};
use serde_json::{self, Value};
#[cfg(debug_assertions)] use tokio::time;
use tokio_postgres::{Row, Transaction, error::SqlState};

use crate::{
	CommandIdentity, PostgresStore, StoreError,
	accounts::{self, CommandClaim, CommandDescriptor},
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_effect_digest, validate_exact_key},
};
use decodex_core::{
	self, ArtifactId, ArtifactStatus, BlobHash, BlobInventoryCursor, BlobStore, ContextPack,
	ContextPackPolicy, ContextSourceDisposition, ContextSourceKind, ConversationId, HistoryItemId,
	HistoryItemKind, HistoryMediaType, HistoryMetadata, ItemStatus, MAX_BLOB_BYTES,
	MAX_INLINE_HISTORY_BYTES, PossibleSideEffects, ProposedTransitionKind, RuntimeSessionId,
	RuntimeSessionState, TurnId, TurnRole, TurnStatus,
};

const MAX_PAGE_SIZE: u16 = 100;
const HIERARCHY_COORDINATION_LOCK: i64 = 1_271;
const CURSOR_COORDINATION_LOCK: i64 = 1_272;
const BLOB_LOCK_NAMESPACE: i32 = 1_273;
const BLOB_SHARD_LOCK_NAMESPACE: i32 = 1_274;
const READ_ORDINARY_TASK_CONVERSATIONS_SQL: &str = "SELECT conversation_id::text,conversation_revision,\
	 runtime_session_id::text,runtime_session_revision,runtime_session_state::text,\
	 codex_thread_id::text,thread_start_request_id,thread_start_request_sha256,\
	 thread_start_response_id,thread_start_response_sha256,has_acknowledged_turn,\
		 active_user_turn_id::text,\
		 active_user_turn_count,has_active_provider_attempt,has_unknown_provider_attempt,\
		 pre_session_state::text,routing_decision_id::text,updated_at_micros,\
		 routing_successor_conversation_id::text,routing_successor_conversation_revision,\
		 has_admitted_user_turn \
	 FROM decodex.read_ordinary_task_conversations_exact(\
	 $1::text::uuid,$2,$3::text::uuid,$4)";
const READ_QUICK_TASK_REQUEST_SQL: &str = "SELECT message,working_directory \
	 FROM decodex.read_quick_task_request_exact($1::text::uuid)";
const CREATE_ROUTING_SUCCESSOR_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.create_quick_task_routing_successor_exact($1,$2,$3::text::uuid,$4)";
const ADMIT_INITIAL_QUICK_TASK_TURN_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.admit_initial_quick_task_turn_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8::text::uuid,\
	 $9::text::uuid,$10,$11,$12,$13)";
const READ_TURN_ADMISSION_SQL: &str = "SELECT conversation_id::text,runtime_session_id::text,turn_id::text,sequence,\
	 role::text,possible_side_effects::text,status::text,revision \
	 FROM decodex.read_turn_admission_exact(\
	 $1::text::uuid,$2::text::uuid,$3::text::uuid)";
const TERMINALIZE_QUICK_TASK_TURN_SQL: &str = "SELECT result_code,conversation_id::text,\
	 conversation_revision,runtime_session_id::text,prior_runtime_session_revision,\
	 runtime_session_revision,user_turn_id::text,user_turn_revision,assistant_turn_id::text,\
	 assistant_turn_revision,provider_attempt_id::text,provider_attempt_revision,\
	 provider_evidence_id::text \
	 FROM decodex.terminalize_quick_task_turn_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8,\
	 $9::text::uuid,$10,$11::text::uuid,$12,$13::text::uuid,\
	 $14::text::decodex.provider_attempt_terminal_outcome,$15::text::uuid,$16)";
const RECONCILE_QUICK_TASK_TERMINALIZATIONS_SQL: &str =
	"SELECT terminalized_count FROM decodex.reconcile_quick_task_terminalizations_exact($1)";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_conversation_admission_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 7] = [
		READ_ORDINARY_TASK_CONVERSATIONS_SQL,
		READ_QUICK_TASK_REQUEST_SQL,
		CREATE_ROUTING_SUCCESSOR_SQL,
		ADMIT_INITIAL_QUICK_TASK_TURN_SQL,
		READ_TURN_ADMISSION_SQL,
		TERMINALIZE_QUICK_TASK_TURN_SQL,
		RECONCILE_QUICK_TASK_TERMINALIZATIONS_SQL,
	];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

/// Create a logical Conversation without any account or Codex-thread identity.
#[derive(Clone, Debug)]
pub struct CreateConversation {
	/// Caller-selected logical identity.
	pub conversation_id: ConversationId,
	/// Bounded display title.
	pub title: String,
}

/// Create one ordinary Quick Task Conversation with immutable initial request coordinates.
#[derive(Clone, Debug)]
pub struct CreateQuickTaskConversation {
	/// Caller-selected logical identity.
	pub conversation_id: ConversationId,
	/// Bounded display title.
	pub title: String,
	/// Original first-Turn message used by route and establishment recovery.
	pub message: String,
	/// Original absolute working directory used by establishment recovery.
	pub working_directory: String,
}

/// Immutable original request coordinates for one open ordinary Quick Task Conversation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskRequest {
	/// Original first-Turn message.
	pub message: String,
	/// Original absolute working directory.
	pub working_directory: String,
}

/// Conversation-owner input for a waiting/no-route routing successor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateQuickTaskRoutingSuccessor {
	/// Waiting/no-route source Conversation.
	pub source_conversation_id: ConversationId,
	/// Exact expected open source revision.
	pub expected_source_revision: i64,
}

/// Exact immutable source-to-successor relation readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskRoutingSuccessor {
	/// Archived waiting/no-route source Conversation.
	pub source_conversation_id: ConversationId,
	/// Archived source revision after the command.
	pub source_revision: i64,
	/// Fresh open routing Conversation.
	pub successor_conversation_id: ConversationId,
	/// Fresh successor revision, always one.
	pub successor_revision: i64,
	/// Initial waiting/no-route decision that authorized the successor.
	pub source_routing_decision_id: String,
}

/// Exact result of the Conversation-owned routing-successor command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskRoutingSuccessorOutcome {
	/// This call created and archived the exact pair.
	Fresh(QuickTaskRoutingSuccessor),
	/// The same exact command returned its committed pair.
	Replayed(QuickTaskRoutingSuccessor),
	/// Stable refusal with no successor created.
	Rejected {
		/// Stable refusal code.
		code: String,
		/// Whether this result was replayed.
		replayed: bool,
	},
}

/// Committed logical Conversation readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredConversation {
	/// Stable logical identity.
	pub conversation_id: ConversationId,
	/// Persisted title.
	pub title: String,
	/// Persisted optimistic revision.
	pub revision: i64,
}

/// Exact current authority for one logical user Turn reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnReservationReadback {
	/// Stable logical Turn identity.
	pub turn_id: TurnId,
	/// Exact persisted sequence within the Conversation.
	pub sequence: i64,
	/// Current durable Turn lifecycle.
	pub status: TurnStatus,
	/// Exact current Turn revision.
	pub revision: i64,
}

/// Whether this call created the Turn or read back the exact completed reservation command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnReservationOutcome {
	/// The history item and active revision-1 Turn committed in this call.
	Fresh(TurnReservationReadback),
	/// The exact history command was already complete; current Turn authority is read back only.
	Replayed(TurnReservationReadback),
}

/// Exact first-Turn admission consumed only by the initial Quick Task plan owner.
#[derive(Clone, Debug)]
pub struct AdmitInitialQuickTaskTurn {
	/// Positive Conversation revision bound by the initial plan.
	pub expected_conversation_revision: i64,
	/// Exact starting RuntimeSession revision; this command accepts only revision one.
	pub expected_runtime_session_revision: i64,
	/// Immutable initial-thread Continuation Plan identity.
	pub continuation_plan_id: String,
	/// Exact initial user Turn and completed message shape.
	pub message: RecordHistoryItem,
}

/// Exact durable readback from atomic initial Quick Task admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialQuickTaskTurnAdmissionReadback {
	/// Selected initial Routing Decision bound through the immutable plan.
	pub routing_decision_id: String,
	/// Exact immutable initial-thread Continuation Plan identity.
	pub continuation_plan_id: String,
	/// Active revision-one user Turn created by the command.
	pub turn: TurnReservationReadback,
	/// Completed revision-one message created with the Turn.
	pub history_item_id: HistoryItemId,
}

/// Closed stable refusal from atomic initial Quick Task admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialQuickTaskTurnAdmissionRejection {
	/// Input failed the command's exact bounded shape.
	InvalidInput,
	/// Conversation, session, plan, or selected route authority is unavailable.
	AuthorityUnavailable,
	/// Another Turn or history identity already occupies the initial admission surface.
	InitialAdmissionConflict,
	/// The referenced content-addressed message blob is absent.
	MessageBlobMissing,
}

/// Closed exact result from atomic initial Quick Task Turn admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitialQuickTaskTurnAdmissionOutcome {
	/// This call committed the Turn, message, activity, outbox, and exact response.
	Fresh(InitialQuickTaskTurnAdmissionReadback),
	/// The same exact command was already complete and returned immutable response bytes.
	Replayed(InitialQuickTaskTurnAdmissionReadback),
	/// The exact command durably refused without creating the Turn or message.
	Rejected {
		/// Stable typed refusal.
		rejection: InitialQuickTaskTurnAdmissionRejection,
		/// True when this call read the already completed refusal receipt.
		replayed: bool,
	},
}

/// Exact positive-evidence coordinates for one crash-convergent ordinary Turn terminalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalizeQuickTaskTurn {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Current bound RuntimeSession.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact pre-acknowledgement RuntimeSession revision.
	pub expected_runtime_session_revision: i64,
	/// Active user Turn supported by positive provider evidence.
	pub user_turn_id: TurnId,
	/// Exact active user Turn revision.
	pub expected_user_turn_revision: i64,
	/// Optional assistant Turn and exact active revision.
	pub assistant_turn: Option<(TurnId, i64)>,
	/// Exact terminal ProviderAttempt.
	pub provider_attempt_id: decodex_core::ProviderAttemptId,
	/// Exact terminal ProviderAttempt revision.
	pub expected_provider_attempt_revision: i64,
	/// Exact positive provider evidence.
	pub provider_evidence_id: decodex_core::ProviderEvidenceId,
	/// Positive terminal outcome shared by the attempt and Turns.
	pub provider_outcome: decodex_core::ProviderTerminalOutcome,
	/// Exact provider thread bound to the RuntimeSession.
	pub provider_thread_id: String,
	/// Exact positive provider Turn identity.
	pub provider_turn_id: String,
}

/// Atomic terminal user/assistant Turn and RuntimeSession acknowledgement readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskTerminalizationReadback {
	/// RuntimeSession revision after terminal acknowledgement.
	pub runtime_session_revision: i64,
	/// Terminal user Turn revision.
	pub user_turn_revision: i64,
	/// Optional terminal assistant Turn revision.
	pub assistant_turn_revision: Option<i64>,
	/// Exact terminal ProviderAttempt revision consumed by the transaction.
	pub provider_attempt_revision: i64,
}

/// Closed result of one bounded idempotent terminalization transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskTerminalizationOutcome {
	/// The complete terminalization committed now.
	Applied(QuickTaskTerminalizationReadback),
	/// The exact complete terminalization was already durable.
	Replayed(QuickTaskTerminalizationReadback),
	/// Positive stable authority rejected the transaction without partial completion.
	Rejected,
	/// Exact receipts cannot prove whether terminalization committed.
	Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryCommandDisposition {
	Fresh,
	Replayed,
}

/// Exact keyset position for ordinary Task-conversation listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryTaskConversationCursor {
	/// Last-seen effective activity timestamp in Unix microseconds.
	pub updated_at_micros: i64,
	/// Last-seen ordinary Conversation identity at that timestamp.
	pub conversation_id: ConversationId,
}

/// Strict credential-negative ordinary Conversation and sole-current-session projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryTaskConversationReadback {
	/// Stable ordinary Conversation identity.
	pub conversation_id: ConversationId,
	/// Exact ordinary Conversation revision.
	pub conversation_revision: i64,
	/// Sole current RuntimeSession identity, absent before first-session planning succeeds.
	pub runtime_session_id: Option<RuntimeSessionId>,
	/// Exact current RuntimeSession revision, jointly absent before first-session planning
	/// succeeds.
	pub runtime_session_revision: Option<i64>,
	/// Current generic RuntimeSession lifecycle, jointly absent before first-session planning.
	pub runtime_session_state: Option<RuntimeSessionState>,
	/// The RuntimeSession owner acknowledged at least one positive terminal provider Turn.
	pub has_acknowledged_turn: bool,
	/// Exact active logical user Turn, when durable state requires reconciliation.
	pub active_turn_id: Option<TurnId>,
	/// At least one logical user Turn has been durably admitted for this session.
	pub has_admitted_user_turn: bool,
	/// A prepared or dispatch-authorized ProviderAttempt remains unresolved.
	pub has_active_provider_attempt: bool,
	/// A ProviderAttempt has terminally unknown submission outcome.
	pub has_unknown_provider_attempt: bool,
	/// Durable pre-session routing projection, absent once a current RuntimeSession exists.
	pub pre_session_state: Option<OrdinaryTaskPreSessionState>,
	/// Latest immutable L0 Routing Decision, absent only before the first decision commits.
	pub routing_decision_id: Option<String>,
	/// Effective activity position used by deterministic pagination.
	pub updated_at_micros: i64,
}

/// Typed ordinary Conversation get/list projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrdinaryTaskConversationProjection {
	/// Current open ordinary Conversation.
	Current(OrdinaryTaskConversationReadback),
	/// Archived waiting/no-route source redirects directly to its sole open successor.
	RoutingSuccessorRedirect {
		/// Archived source identity requested by the caller.
		source_conversation_id: ConversationId,
		/// Exact archived source revision.
		source_revision: i64,
		/// Sole fresh routing successor.
		successor_conversation_id: ConversationId,
		/// Exact open successor revision.
		successor_conversation_revision: i64,
	},
}

/// Credential-negative state derived from the latest immutable L0 Routing Decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryTaskPreSessionState {
	/// The Conversation committed before a corresponding Routing Decision was available.
	RoutingPending,
	/// Initial selection committed, but RuntimeSession and initial plan are absent.
	EstablishmentPending,
	/// Positive current quota facts exhausted every eligible route.
	QuotaExhausted,
	/// The latest explicit decision found no eligible route.
	NoRoute,
}

struct OrdinaryTaskConversationRow {
	conversation_id: ConversationId,
	conversation_revision: i64,
	runtime_session_id: Option<RuntimeSessionId>,
	runtime_session_revision: Option<i64>,
	runtime_session_state: Option<RuntimeSessionState>,
	codex_thread_id: Option<String>,
	thread_start_request_id: Option<i64>,
	thread_start_request_sha256: Option<String>,
	thread_start_response_id: Option<i64>,
	thread_start_response_sha256: Option<String>,
	has_acknowledged_turn: bool,
	active_turn_id: Option<TurnId>,
	active_turn_count: i64,
	has_admitted_user_turn: bool,
	has_active_provider_attempt: bool,
	has_unknown_provider_attempt: bool,
	pre_session_state: Option<OrdinaryTaskPreSessionState>,
	routing_decision_id: Option<String>,
	updated_at_micros: i64,
	routing_successor_conversation_id: Option<ConversationId>,
	routing_successor_conversation_revision: Option<i64>,
}

/// Create one immutable-content Artifact scoped to a logical Conversation.
#[derive(Clone, Debug)]
pub struct CreateArtifact {
	/// Caller-selected stable Artifact identity.
	pub artifact_id: ArtifactId,
	/// Owning logical Conversation.
	pub conversation_id: ConversationId,
	/// Immutable bounded content bytes.
	pub bytes: Vec<u8>,
	/// Canonical bounded media type.
	pub media_type: String,
	/// Optional bounded display name.
	pub display_name: Option<String>,
}

/// Complete verified Artifact revision read model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredArtifact {
	/// Stable Artifact identity.
	pub artifact_id: ArtifactId,
	/// Owning logical Conversation.
	pub conversation_id: ConversationId,
	/// Exact immutable revision.
	pub revision: i64,
	/// Lifecycle state recorded by this revision.
	pub status: ArtifactStatus,
	/// Verified content address.
	pub blob_hash: BlobHash,
	/// Verified exact content bytes.
	pub bytes: Vec<u8>,
	/// Persisted media type.
	pub media_type: String,
	/// Persisted optional display name.
	pub display_name: Option<String>,
}

/// One normalized streamed or completed history-item mutation.
#[derive(Clone, Debug)]
pub struct RecordHistoryItem {
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Producing runtime segment.
	pub runtime_session_id: RuntimeSessionId,
	/// Parent turn identity.
	pub turn_id: TurnId,
	/// Logical turn ordering key.
	pub turn_sequence: i64,
	/// Normalized turn role.
	pub turn_role: TurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: PossibleSideEffects,
	/// Stable normalized item identity.
	pub history_item_id: HistoryItemId,
	/// Stable position within the turn.
	pub ordinal: i32,
	/// Normalized item class.
	pub kind: HistoryItemKind,
	/// Stream lifecycle.
	pub status: ItemStatus,
	/// Exact normalized UTF-8 payload, offloaded when large.
	pub text: String,
	/// Bounded media type.
	pub media_type: HistoryMediaType,
	/// Bounded credential-negative structured metadata.
	pub metadata: HistoryMetadata,
	/// `None` creates revision 1; `Some` updates only that exact item revision.
	pub expected_revision: Option<i64>,
	/// Exact typed Artifact revision for artifact history items.
	pub artifact: Option<(ArtifactId, i64)>,
}

/// Opaque versioned handle to one PostgreSQL-issued Conversation snapshot boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryCursor {
	/// Random persisted issuance identity; no boundary metadata is client-editable.
	token: String,
}
impl HistoryCursor {
	/// Encode the opaque cursor for the typed protocol.
	pub fn encode(&self) -> String {
		format!("v1:{}", self.token)
	}

	/// Parse only the versioned opaque shape; PostgreSQL validates issuance and binding.
	pub fn parse(value: &str) -> Result<Self, StoreError> {
		let Some(token) = value.strip_prefix("v1:") else {
			return Err(StoreError::InvalidInput("history cursor is malformed"));
		};

		if !is_canonical_uuid(token) {
			return Err(StoreError::InvalidInput("history cursor is malformed"));
		}

		Ok(Self { token: token.to_owned() })
	}

	fn issued(token: String) -> Result<Self, StoreError> {
		if is_canonical_uuid(&token) {
			Ok(Self { token })
		} else {
			Err(StoreError::Incompatible("issued history cursor identity is invalid".into()))
		}
	}
}

/// Verified persisted normalized history read model.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
	/// Stable item identity.
	pub history_item_id: String,
	/// Parent turn identity.
	pub turn_id: String,
	/// Producing runtime segment identity.
	pub runtime_session_id: String,
	/// Normalized author role.
	pub turn_role: TurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: PossibleSideEffects,
	/// Normalized item class.
	pub kind: HistoryItemKind,
	/// Stream lifecycle.
	pub status: ItemStatus,
	/// Inline payload when small.
	pub inline_text: Option<String>,
	/// Content address when offloaded.
	pub blob_hash: Option<BlobHash>,
	/// Verified offloaded length.
	pub blob_byte_length: Option<u64>,
	/// Persisted media type.
	pub media_type: HistoryMediaType,
	/// Persisted bounded metadata.
	pub metadata: HistoryMetadata,
	/// Exact typed Artifact revision for Artifact history items.
	pub artifact: Option<(ArtifactId, u64)>,
	/// Persisted optimistic revision.
	pub revision: i64,
}

/// One strictly bounded deterministic history page.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPage {
	/// Ordered verified entries.
	pub entries: Vec<HistoryEntry>,
	/// Cursor for the next page only when more rows exist.
	pub next_cursor: Option<HistoryCursor>,
}

/// Result of one bounded deterministic blob-inventory reclamation pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobReclaimPage {
	/// Files removed in this pass.
	pub removed: u16,
	/// Continuation required to cover the complete SHA-256 namespace.
	pub next_cursor: Option<BlobInventoryCursor>,
}

/// Persist one already compiled immutable Context Pack and its exact source revisions.
#[derive(Clone, Debug)]
pub struct PersistContextPack {
	/// Caller-selected pack identity.
	pub context_pack_id: String,
	/// Immutable revision within the Conversation.
	pub pack_revision: i64,
}

/// Verified committed Context Pack metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackRecord {
	/// Stable pack identity.
	pub context_pack_id: String,
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Immutable pack revision.
	pub pack_revision: i64,
	/// Digest of exact compiled bytes.
	pub compiled_digest: BlobHash,
	/// Verified compiled length.
	pub byte_length: u64,
	/// Whether deterministic policy shortened any source.
	pub truncated: bool,
	/// Count of source inputs omitted entirely.
	pub omitted_source_count: usize,
	/// Completely reconstructed, byte- and manifest-verified immutable pack.
	pub pack: ContextPack,
}

/// Persist an inert rollover/fallback proposal. `dispatch_enabled` is fixed false in schema.
#[derive(Clone, Debug)]
pub struct ProposeTransition {
	/// Caller-selected proposal identity.
	pub transition_id: String,
	/// Logical Conversation preserved by the proposal.
	pub conversation_id: ConversationId,
	/// Runtime segment the proposal would replace.
	pub from_runtime_session_id: RuntimeSessionId,
	/// Exact persisted Context Pack identity.
	pub context_pack_id: String,
	/// Proposed transition class.
	pub kind: ProposedTransitionKind,
	/// Bounded operator-inspectable rationale.
	pub reason: String,
}

pub(crate) struct BlobSession {
	client: ClientWrapper,
}

fn parse_ordinary_task_conversation_row(
	row: Row,
) -> Result<OrdinaryTaskConversationRow, StoreError> {
	let conversation_id = ConversationId::new(row.get::<_, String>(0)).map_err(|_| {
		StoreError::Incompatible("ordinary Task Conversation identity is invalid".into())
	})?;
	let runtime_session_id =
		row.get::<_, Option<String>>(2).map(RuntimeSessionId::new).transpose().map_err(|_| {
			StoreError::Incompatible("ordinary Task RuntimeSession identity is invalid".into())
		})?;
	let runtime_session_state = row
		.get::<_, Option<String>>(4)
		.map(|state| match state.as_str() {
			"starting" => Ok(RuntimeSessionState::Starting),
			"active" => Ok(RuntimeSessionState::Active),
			"ended" => Ok(RuntimeSessionState::Ended),
			"diverged" => Ok(RuntimeSessionState::Diverged),
			_ => Err(StoreError::Incompatible(
				"ordinary Task RuntimeSession lifecycle is invalid".into(),
			)),
		})
		.transpose()?;
	let active_turn_id =
		row.get::<_, Option<String>>(11).map(TurnId::new).transpose().map_err(|_| {
			StoreError::Incompatible("ordinary Task active Turn identity is invalid".into())
		})?;
	let pre_session_state = row
		.get::<_, Option<String>>(15)
		.map(|state| match state.as_str() {
			"routing_pending" => Ok(OrdinaryTaskPreSessionState::RoutingPending),
			"establishment_pending" => Ok(OrdinaryTaskPreSessionState::EstablishmentPending),
			"quota_exhausted" => Ok(OrdinaryTaskPreSessionState::QuotaExhausted),
			"no_route" => Ok(OrdinaryTaskPreSessionState::NoRoute),
			_ => Err(StoreError::Incompatible("ordinary Task pre-session state is invalid".into())),
		})
		.transpose()?;
	Ok(OrdinaryTaskConversationRow {
		conversation_id,
		conversation_revision: row.get(1),
		runtime_session_id,
		runtime_session_revision: row.get(3),
		runtime_session_state,
		codex_thread_id: row.get(5),
		thread_start_request_id: row.get(6),
		thread_start_request_sha256: row.get(7),
		thread_start_response_id: row.get(8),
		thread_start_response_sha256: row.get(9),
		has_acknowledged_turn: row.get(10),
		active_turn_id,
		active_turn_count: row.get(12),
		has_active_provider_attempt: row.get(13),
		has_unknown_provider_attempt: row.get(14),
		pre_session_state,
		routing_decision_id: row.get(16),
		updated_at_micros: row.get(17),
		routing_successor_conversation_id: row
			.get::<_, Option<String>>(18)
			.map(ConversationId::new)
			.transpose()
			.map_err(|_| {
				StoreError::Incompatible(
					"ordinary Task routing successor identity is invalid".into(),
				)
			})?,
		routing_successor_conversation_revision: row.get(19),
		has_admitted_user_turn: row.get(20),
	})
}

impl OrdinaryTaskConversationRow {
	fn into_projection(self) -> Result<OrdinaryTaskConversationProjection, StoreError> {
		if let Some(successor_conversation_id) = self.routing_successor_conversation_id.clone() {
			let Some(successor_conversation_revision) =
				self.routing_successor_conversation_revision
			else {
				return Err(StoreError::Incompatible(
					"ordinary Task routing redirect successor revision is absent".into(),
				));
			};
			if self.conversation_revision <= 0
				|| successor_conversation_revision <= 0
				|| self.updated_at_micros <= 0
				|| self.runtime_session_id.is_some()
				|| self.runtime_session_revision.is_some()
				|| self.runtime_session_state.is_some()
				|| self.codex_thread_id.is_some()
				|| self.thread_start_request_id.is_some()
				|| self.thread_start_request_sha256.is_some()
				|| self.thread_start_response_id.is_some()
				|| self.thread_start_response_sha256.is_some()
				|| self.has_acknowledged_turn
				|| self.active_turn_id.is_some()
				|| self.active_turn_count != 0
				|| self.has_admitted_user_turn
				|| self.has_active_provider_attempt
				|| self.has_unknown_provider_attempt
				|| self.pre_session_state.is_some()
				|| self.routing_decision_id.is_some()
			{
				return Err(StoreError::Incompatible(
					"ordinary Task routing redirect is inconsistent".into(),
				));
			}
			return Ok(OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
				source_conversation_id: self.conversation_id,
				source_revision: self.conversation_revision,
				successor_conversation_id,
				successor_conversation_revision,
			});
		}
		if self.routing_successor_conversation_revision.is_some() {
			return Err(StoreError::Incompatible(
				"ordinary Task current projection has redirect revision authority".into(),
			));
		}
		self.into_readback().map(OrdinaryTaskConversationProjection::Current)
	}

	fn into_readback(self) -> Result<OrdinaryTaskConversationReadback, StoreError> {
		let starting_thread_shape = || {
			!self.has_acknowledged_turn
				&& self.codex_thread_id.is_none()
				&& self.thread_start_response_id.is_none()
				&& self.thread_start_response_sha256.is_none()
				&& match (self.thread_start_request_id, self.thread_start_request_sha256.as_deref())
				{
					(None, None) => true,
					(Some(id), Some(digest)) => id > 0 && is_lower_sha256(digest),
					_ => false,
				}
		};
		let active_thread_shape = || {
			self.codex_thread_id.as_ref().is_some_and(|id| is_canonical_uuid(id))
				&& self.thread_start_request_id.is_some_and(|id| id > 0)
				&& self.thread_start_response_id.is_some_and(|id| id > 0)
				&& self.thread_start_response_id == self.thread_start_request_id
				&& self
					.thread_start_request_sha256
					.as_ref()
					.is_some_and(|digest| is_lower_sha256(digest))
				&& self
					.thread_start_response_sha256
					.as_ref()
					.is_some_and(|digest| is_lower_sha256(digest))
		};
		let lifecycle_valid = match self.runtime_session_state.as_ref() {
			Some(RuntimeSessionState::Starting) => starting_thread_shape(),
			Some(RuntimeSessionState::Active) => active_thread_shape(),
			Some(RuntimeSessionState::Ended | RuntimeSessionState::Diverged) =>
				starting_thread_shape() || active_thread_shape(),
			None =>
				!self.has_acknowledged_turn
					&& self.codex_thread_id.is_none()
					&& self.thread_start_request_id.is_none()
					&& self.thread_start_request_sha256.is_none()
					&& self.thread_start_response_id.is_none()
					&& self.thread_start_response_sha256.is_none(),
		};
		let has_runtime_session = self.runtime_session_id.is_some()
			&& self.runtime_session_revision.is_some()
			&& self.runtime_session_state.is_some();
		let routing_shape_valid = match self.pre_session_state.as_ref() {
			Some(OrdinaryTaskPreSessionState::RoutingPending) => self.routing_decision_id.is_none(),
			Some(
				OrdinaryTaskPreSessionState::EstablishmentPending
				| OrdinaryTaskPreSessionState::QuotaExhausted
				| OrdinaryTaskPreSessionState::NoRoute,
			) => self.routing_decision_id.as_deref().is_some_and(is_canonical_uuid),
			None =>
				has_runtime_session
					&& self.routing_decision_id.as_deref().is_some_and(is_canonical_uuid),
		};
		if self.conversation_revision <= 0
			|| self.runtime_session_revision.is_some_and(|revision| revision <= 0)
			|| self.updated_at_micros <= 0
			|| self.runtime_session_id.is_some() != self.runtime_session_revision.is_some()
			|| self.runtime_session_id.is_some() != self.runtime_session_state.is_some()
			|| has_runtime_session == self.pre_session_state.is_some()
			|| !(0..=1).contains(&self.active_turn_count)
			|| (self.active_turn_count == 1) != self.active_turn_id.is_some()
			|| self.active_turn_id.is_some() && !self.has_admitted_user_turn
			|| self.has_acknowledged_turn && !self.has_admitted_user_turn
			|| self.has_active_provider_attempt && self.has_unknown_provider_attempt
			|| !has_runtime_session
				&& (self.has_admitted_user_turn
					|| self.active_turn_id.is_some()
					|| self.has_active_provider_attempt
					|| self.has_unknown_provider_attempt)
			|| !routing_shape_valid
			|| !lifecycle_valid
		{
			return Err(StoreError::Incompatible(
				"ordinary Task Conversation projection is inconsistent".into(),
			));
		}
		Ok(OrdinaryTaskConversationReadback {
			conversation_id: self.conversation_id,
			conversation_revision: self.conversation_revision,
			runtime_session_id: self.runtime_session_id,
			runtime_session_revision: self.runtime_session_revision,
			runtime_session_state: self.runtime_session_state,
			has_acknowledged_turn: self.has_acknowledged_turn,
			active_turn_id: self.active_turn_id,
			has_admitted_user_turn: self.has_admitted_user_turn,
			has_active_provider_attempt: self.has_active_provider_attempt,
			has_unknown_provider_attempt: self.has_unknown_provider_attempt,
			pre_session_state: self.pre_session_state,
			routing_decision_id: self.routing_decision_id,
			updated_at_micros: self.updated_at_micros,
		})
	}
}

impl OrdinaryTaskConversationProjection {
	fn conversation_id(&self) -> &ConversationId {
		match self {
			Self::Current(readback) => &readback.conversation_id,
			Self::RoutingSuccessorRedirect { source_conversation_id, .. } => source_conversation_id,
		}
	}

	fn updated_at_micros(&self) -> i64 {
		match self {
			Self::Current(readback) => readback.updated_at_micros,
			Self::RoutingSuccessorRedirect { .. } => 0,
		}
	}
}

impl PostgresStore {
	async fn dedicated_session(&self) -> Result<BlobSession, StoreError> {
		let client = self.pool().get().await?;

		Ok(BlobSession { client: Client::take(client) })
	}

	pub(crate) async fn lock_blob_session(
		&self,
		hashes: &[BlobHash],
		capacity_hashes: &[BlobHash],
	) -> Result<BlobSession, StoreError> {
		let session = self.dedicated_session().await?;
		let mut encoded_hashes = hashes.iter().map(|hash| hash.to_hex()).collect::<Vec<_>>();

		encoded_hashes.sort_unstable();
		encoded_hashes.dedup();

		for encoded in &encoded_hashes {
			session
				.client
				.query_one(
					"SELECT pg_catalog.pg_advisory_lock($1, pg_catalog.hashtext($2))",
					&[&BLOB_LOCK_NAMESPACE, encoded],
				)
				.await?;
		}

		let mut shards = capacity_hashes
			.iter()
			.map(|hash| {
				let encoded = hash.to_hex();

				i32::from_str_radix(&encoded[..2], 16)
					.map_err(|_| StoreError::Incompatible("blob shard identity is invalid".into()))
			})
			.collect::<Result<Vec<_>, _>>()?;

		shards.sort_unstable();
		shards.dedup();

		for shard in shards {
			session
				.client
				.query_one(
					"SELECT pg_catalog.pg_advisory_lock($1, $2)",
					&[&BLOB_SHARD_LOCK_NAMESPACE, &shard],
				)
				.await?;
		}

		Ok(session)
	}

	async fn history_artifact_hashes(
		&self,
		mutation: &RecordHistoryItem,
	) -> Result<Vec<BlobHash>, StoreError> {
		let Some((artifact_id, revision)) = &mutation.artifact else {
			return Ok(Vec::new());
		};
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT blob_hash FROM decodex.artifact_revisions \
				 WHERE artifact_id=$1::text::uuid AND conversation_id=$2::text::uuid AND revision=$3",
				&[&artifact_id.as_str(), &mutation.conversation_id.as_str(), revision],
			)
			.await?
			.ok_or(StoreError::InvalidInput("Artifact history reference does not exist"))?;

		Ok(vec![BlobHash::parse(row.get(0))?])
	}

	/// Atomically create one logical Conversation with activity, outbox, and receipt evidence.
	pub async fn create_conversation(
		&self,
		command: &CommandIdentity,
		create: &CreateConversation,
	) -> Result<StoredConversation, StoreError> {
		if create.title.is_empty() || create.title.len() > 512 {
			return Err(StoreError::InvalidInput("conversation title must contain 1..=512 bytes"));
		}

		crate::ensure_credential_negative_text(&create.title)?;

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"create_conversation",
			("global", "conversations", create.conversation_id.as_str()),
			None,
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				return conversation_from_response(&response);
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let inserted = transaction
			.query_opt(
				"INSERT INTO decodex.conversations (conversation_id, title) \
				 VALUES ($1::text::uuid, $2) ON CONFLICT DO NOTHING RETURNING revision",
				&[&create.conversation_id.as_str(), &create.title],
			)
			.await?;

		if inserted.is_none() {
			return Err(StoreError::RevisionConflict {
				entity: format!("conversation/{}", create.conversation_id),
				expected: None,
				actual: conversation_revision(&transaction, &create.conversation_id).await?,
			});
		}

		let payload = serde_json::json!({
			"conversation_id": create.conversation_id.as_str(),
			"revision": 1,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"conversation",
			create.conversation_id.as_str(),
			1,
			"conversation_created",
			&command.key,
			&payload,
		)
		.await?;

		let response = serde_json::json!({
			"kind": "conversation",
			"conversation_id": create.conversation_id.as_str(),
			"title": create.title,
			"revision": 1,
		});

		accounts::finish_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		conversation_from_response(&response)
	}

	/// Atomically create one ordinary Quick Task Conversation and retain its original request.
	pub async fn create_quick_task_conversation(
		&self,
		command: &CommandIdentity,
		create: &CreateQuickTaskConversation,
	) -> Result<StoredConversation, StoreError> {
		if create.title.is_empty()
			|| create.title.len() > 512
			|| create.message.is_empty()
			|| create.message.len() > 16_384
			|| create.working_directory.is_empty()
			|| create.working_directory.len() > 4_096
			|| !create.working_directory.starts_with('/')
			|| create.working_directory.chars().any(char::is_control)
		{
			return Err(StoreError::InvalidInput(
				"initial Quick Task Conversation request is invalid",
			));
		}
		crate::ensure_credential_negative_text(&create.title)?;
		crate::ensure_credential_negative_text(&create.message)?;

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"create_quick_task_conversation",
			("global", "conversations", create.conversation_id.as_str()),
			None,
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				return conversation_from_response(&response);
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let inserted = transaction
			.query_opt(
				"INSERT INTO decodex.conversations (conversation_id,title,\
				 initial_quick_task_message,initial_quick_task_working_directory) \
				 VALUES ($1::text::uuid,$2,$3,$4) ON CONFLICT DO NOTHING \
				 RETURNING revision",
				&[
					&create.conversation_id.as_str(),
					&create.title,
					&create.message,
					&create.working_directory,
				],
			)
			.await?;
		if inserted.is_none() {
			return Err(StoreError::RevisionConflict {
				entity: format!("conversation/{}", create.conversation_id),
				expected: None,
				actual: conversation_revision(&transaction, &create.conversation_id).await?,
			});
		}
		let payload = serde_json::json!({
			"conversation_id": create.conversation_id.as_str(),
			"revision": 1,
			"request_kind": "quick_task",
		});
		accounts::append_activity_and_outbox(
			&transaction,
			"conversation",
			create.conversation_id.as_str(),
			1,
			"quick_task_conversation_created",
			&command.key,
			&payload,
		)
		.await?;
		let response = serde_json::json!({
			"kind": "conversation",
			"conversation_id": create.conversation_id.as_str(),
			"title": create.title,
			"revision": 1,
		});
		accounts::finish_command(&transaction, &reservation, &response).await?;
		transaction.commit().await?;
		conversation_from_response(&response)
	}

	/// Read immutable original request coordinates for routing or establishment recovery.
	pub async fn read_quick_task_request(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<QuickTaskRequest>, StoreError> {
		let rows = self
			.pool()
			.get()
			.await?
			.query(READ_QUICK_TASK_REQUEST_SQL, &[&conversation_id.as_str()])
			.await?;
		if rows.len() > 1 {
			return Err(StoreError::Incompatible(
				"Quick Task request readback is not unique".into(),
			));
		}
		Ok(rows
			.first()
			.map(|row| QuickTaskRequest { message: row.get(0), working_directory: row.get(1) }))
	}

	/// Create one fresh routing Conversation and archive its waiting/no-route source.
	pub async fn create_quick_task_routing_successor(
		&self,
		idempotency_key: &str,
		request: &CreateQuickTaskRoutingSuccessor,
	) -> Result<QuickTaskRoutingSuccessorOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		if request.expected_source_revision <= 0 {
			return Err(StoreError::InvalidInput(
				"routing successor source revision must be positive",
			));
		}
		let (response, replayed) = self
			.execute_exact_with_replay_status(
				CREATE_ROUTING_SUCCESSOR_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.source_conversation_id.as_str(),
					&request.expected_source_revision,
				],
			)
			.await?;
		parse_routing_successor_response(&response, replayed, request)
	}

	/// Atomically admit the first Quick Task Turn and its completed user message.
	pub async fn admit_initial_quick_task_turn(
		&self,
		blob_store: &BlobStore,
		idempotency_key: &str,
		request: &AdmitInitialQuickTaskTurn,
	) -> Result<InitialQuickTaskTurnAdmissionOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		let message = &request.message;
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision != 1
			|| !is_canonical_uuid(&request.continuation_plan_id)
			|| message.turn_sequence != 1
			|| message.turn_role != TurnRole::User
			|| message.possible_side_effects != PossibleSideEffects::Unknown
			|| message.ordinal != 0
			|| message.kind != HistoryItemKind::Message
			|| message.status != ItemStatus::Completed
			|| message.expected_revision.is_some()
			|| message.artifact.is_some()
		{
			return Err(StoreError::InvalidInput("initial Quick Task admission shape is invalid"));
		}
		validate_history_item(message)?;

		let blob = prepare_payload(&message.text)?;
		let metadata = history_metadata_json(&message.metadata)?;
		let mut publication = if let Some((hash, _)) = blob {
			let publication = self.lock_blob_session(&[hash], &[hash]).await?;
			publish_verified_blob(blob_store, hash, message.text.as_bytes())?;
			publication
		} else {
			self.dedicated_session().await?
		};
		let transaction = publication.client.transaction().await?;
		if let Some((hash, byte_length)) = blob {
			insert_verified_blob(&transaction, hash, byte_length).await?;
		}
		let inline_text = blob.is_none().then_some(message.text.as_str());
		let blob_hash = blob.map(|(hash, _)| hash.to_hex());
		let row = transaction
			.query_one(
				ADMIT_INITIAL_QUICK_TASK_TURN_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&message.conversation_id.as_str(),
					&request.expected_conversation_revision,
					&message.runtime_session_id.as_str(),
					&request.expected_runtime_session_revision,
					&request.continuation_plan_id,
					&message.turn_id.as_str(),
					&message.history_item_id.as_str(),
					&inline_text,
					&blob_hash,
					&message.media_type.as_str(),
					&metadata,
				],
			)
			.await?;
		let response: Vec<u8> = row.get(0);
		let replayed: bool = row.get(1);
		let outcome = parse_initial_quick_task_admission_response(
			&response,
			replayed,
			request,
			inline_text,
			blob_hash.as_deref(),
			&metadata,
		)?;
		transaction.commit().await?;
		Ok(outcome)
	}

	/// Read one bounded function-only page of ordinary Task-role Conversations.
	pub async fn read_ordinary_task_conversations(
		&self,
		conversation_id: Option<&ConversationId>,
		after: Option<&OrdinaryTaskConversationCursor>,
		limit: usize,
	) -> Result<Vec<OrdinaryTaskConversationProjection>, StoreError> {
		if limit == 0 || limit > 65 || conversation_id.is_some() && after.is_some() {
			return Err(StoreError::InvalidInput(
				"ordinary Task Conversation read bound is invalid",
			));
		}
		if after.is_some_and(|cursor| cursor.updated_at_micros <= 0) {
			return Err(StoreError::InvalidInput("ordinary Task Conversation cursor is invalid"));
		}
		let conversation_id = conversation_id.map(ConversationId::as_str);
		let after_updated_at_micros = after.map(|cursor| cursor.updated_at_micros);
		let after_conversation_id = after.map(|cursor| cursor.conversation_id.as_str());
		let limit = i64::try_from(limit)
			.map_err(|_| StoreError::InvalidInput("ordinary Task Conversation limit is invalid"))?;
		let rows = self
			.pool()
			.get()
			.await?
			.query(
				READ_ORDINARY_TASK_CONVERSATIONS_SQL,
				&[&conversation_id, &after_updated_at_micros, &after_conversation_id, &limit],
			)
			.await?;
		if rows.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
			return Err(StoreError::Incompatible(
				"ordinary Task Conversation function exceeded its bound".into(),
			));
		}

		let readbacks = rows
			.into_iter()
			.map(|row| parse_ordinary_task_conversation_row(row)?.into_projection())
			.collect::<Result<Vec<_>, StoreError>>()?;

		if readbacks.windows(2).any(|pair| {
			pair[0].updated_at_micros() < pair[1].updated_at_micros()
				|| pair[0].updated_at_micros() == pair[1].updated_at_micros()
					&& pair[0].conversation_id().as_str() <= pair[1].conversation_id().as_str()
		}) || after.is_some_and(|cursor| {
			readbacks.first().is_some_and(|first| {
				first.updated_at_micros() > cursor.updated_at_micros
					|| first.updated_at_micros() == cursor.updated_at_micros
						&& first.conversation_id().as_str() >= cursor.conversation_id.as_str()
			})
		}) {
			return Err(StoreError::Incompatible(
				"ordinary Task Conversation ordering is invalid".into(),
			));
		}

		Ok(readbacks)
	}

	/// Atomically terminalize positive ProviderAttempt evidence, both logical Turns, and session
	/// ack.
	pub async fn terminalize_quick_task_turn(
		&self,
		idempotency_key: &str,
		request: &TerminalizeQuickTaskTurn,
	) -> Result<QuickTaskTerminalizationOutcome, StoreError> {
		crate::exact_commands::validate_exact_key(idempotency_key)?;
		let assistant_turn_id = request.assistant_turn.as_ref().map(|(id, _)| id.as_str());
		let assistant_turn_revision =
			request.assistant_turn.as_ref().map(|(_, revision)| *revision);
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision <= 0
			|| request.expected_user_turn_revision <= 0
			|| request.expected_provider_attempt_revision <= 0
			|| assistant_turn_revision.is_some_and(|revision| revision <= 0)
			|| !is_canonical_uuid(&request.provider_thread_id)
			|| !is_canonical_uuid(&request.provider_turn_id)
		{
			return Err(StoreError::InvalidInput(
				"Quick Task terminalization coordinates are invalid",
			));
		}
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				TERMINALIZE_QUICK_TASK_TURN_SQL,
				&[
					&crate::exact_commands::EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.conversation_id.as_str(),
					&request.expected_conversation_revision,
					&request.runtime_session_id.as_str(),
					&request.expected_runtime_session_revision,
					&request.user_turn_id.as_str(),
					&request.expected_user_turn_revision,
					&assistant_turn_id,
					&assistant_turn_revision,
					&request.provider_attempt_id.as_str(),
					&request.expected_provider_attempt_revision,
					&request.provider_evidence_id.as_str(),
					&quick_task_terminal_outcome_sql(request.provider_outcome),
					&request.provider_thread_id,
					&request.provider_turn_id,
				],
			)
			.await?;
		let result_code: &str = row.get(0);
		if result_code == "rejected" {
			return Ok(QuickTaskTerminalizationOutcome::Rejected);
		}
		if result_code == "unknown" {
			return Ok(QuickTaskTerminalizationOutcome::Unknown);
		}
		if !matches!(result_code, "applied" | "replayed") {
			return Err(StoreError::Incompatible(
				"Quick Task terminalization result is unknown".into(),
			));
		}
		let assistant_id: Option<String> = row.get(8);
		let assistant_revision: Option<i64> = row.get(9);
		if row.get::<_, String>(1) != request.conversation_id.as_str()
			|| row.get::<_, i64>(2) != request.expected_conversation_revision
			|| row.get::<_, String>(3) != request.runtime_session_id.as_str()
			|| row.get::<_, i64>(4) != request.expected_runtime_session_revision
			|| row.get::<_, String>(6) != request.user_turn_id.as_str()
			|| row.get::<_, i64>(7) != request.expected_user_turn_revision.saturating_add(1)
			|| assistant_id.as_deref() != assistant_turn_id
			|| assistant_revision
				!= assistant_turn_revision.map(|revision| revision.saturating_add(1))
			|| row.get::<_, String>(10) != request.provider_attempt_id.as_str()
			|| row.get::<_, i64>(11) != request.expected_provider_attempt_revision
			|| row.get::<_, String>(12) != request.provider_evidence_id.as_str()
		{
			return Err(StoreError::Incompatible(
				"Quick Task terminalization readback is cross-linked".into(),
			));
		}
		let readback = QuickTaskTerminalizationReadback {
			runtime_session_revision: row.get(5),
			user_turn_revision: row.get(7),
			assistant_turn_revision: assistant_revision,
			provider_attempt_revision: row.get(11),
		};
		if readback.runtime_session_revision
			!= request.expected_runtime_session_revision.saturating_add(1)
		{
			return Err(StoreError::Incompatible(
				"Quick Task terminalization session revision is invalid".into(),
			));
		}
		Ok(if result_code == "applied" {
			QuickTaskTerminalizationOutcome::Applied(readback)
		} else {
			QuickTaskTerminalizationOutcome::Replayed(readback)
		})
	}

	/// Converge a bounded page whose positive terminal evidence already has exact receipts.
	pub async fn reconcile_quick_task_terminalizations(
		&self,
		limit: u16,
	) -> Result<u16, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput("Quick Task terminalization bound is invalid"));
		}
		let count: i64 = self
			.pool()
			.get()
			.await?
			.query_one(RECONCILE_QUICK_TASK_TERMINALIZATIONS_SQL, &[&i32::from(limit)])
			.await?
			.get(0);
		u16::try_from(count).ok().filter(|count| *count <= limit).ok_or_else(|| {
			StoreError::Incompatible("Quick Task terminalization exceeded its bound".into())
		})
	}

	/// Complete or fail one active normalized Turn after its items are terminal.
	pub async fn transition_turn(
		&self,
		command: &CommandIdentity,
		turn_id: &TurnId,
		expected_revision: i64,
		status: decodex_core::TurnStatus,
	) -> Result<i64, StoreError> {
		if expected_revision < 1 || status == decodex_core::TurnStatus::Active {
			return Err(StoreError::InvalidInput("Turn transition is invalid"));
		}

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"transition_turn",
			("turn", turn_id.as_str(), turn_id.as_str()),
			Some(expected_revision),
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				return response_revision(&response, "turn");
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let state = match status {
			decodex_core::TurnStatus::Completed => "completed",
			decodex_core::TurnStatus::Failed => "failed",
			decodex_core::TurnStatus::Active => unreachable!(),
		};
		let row=transaction.query_opt("UPDATE decodex.turns SET status=$3::text::decodex.turn_status, revision=revision+1, updated_at=clock_timestamp(), completed_at=clock_timestamp() WHERE turn_id=$1::text::uuid AND revision=$2 AND NOT EXISTS (SELECT 1 FROM decodex.history_items WHERE turn_id=$1::text::uuid AND status='streaming') RETURNING revision, conversation_id::text",&[&turn_id.as_str(),&expected_revision,&state]).await?.ok_or(StoreError::RevisionConflict{entity:format!("turn/{turn_id}"),expected:Some(expected_revision),actual:None})?;
		let revision: i64 = row.get(0);
		let conversation_id: String = row.get(1);

		accounts::append_activity_and_outbox(
			&transaction,
			"turn",
			turn_id.as_str(),
			revision,
			"turn_transitioned",
			&command.key,
			&serde_json::json!({"conversation_id":conversation_id,"status":state,"revision":revision}),
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"turn","turn_id":turn_id.as_str(),"revision":revision}),
		)
		.await?;

		transaction.commit().await?;

		Ok(revision)
	}

	/// Transactionally publish and reference one immutable Artifact revision.
	pub async fn create_artifact(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		create: &CreateArtifact,
	) -> Result<StoredArtifact, StoreError> {
		validate_artifact(create)?;

		let hash = BlobHash::digest(&create.bytes);
		let byte_length = i64::try_from(create.bytes.len())
			.map_err(|_| StoreError::InvalidInput("Artifact blob length is invalid"))?;
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"create_artifact",
			("conversation", create.conversation_id.as_str(), create.artifact_id.as_str()),
			None,
			Some((hash, byte_length)),
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let revision = response_revision(&response, "artifact")?;

				return self.artifact(blob_store, &create.artifact_id, Some(revision)).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let mut publication = self.lock_blob_session(&[hash], &[hash]).await?;

		publish_verified_blob(blob_store, hash, &create.bytes)?;
		blob_publish_test_barrier().await?;

		let transaction = publication.client.transaction().await?;

		insert_verified_blob(&transaction, hash, byte_length).await?;

		transaction.execute(
			"INSERT INTO decodex.artifacts (artifact_id, conversation_id) VALUES ($1::text::uuid, $2::text::uuid)",
			&[&create.artifact_id.as_str(), &create.conversation_id.as_str()],
		).await?;
		transaction.execute(
			"INSERT INTO decodex.artifact_revisions (artifact_id, conversation_id, revision, blob_hash, media_type, display_name, status) VALUES ($1::text::uuid, $2::text::uuid, 1, $3, $4, $5, 'active')",
			&[&create.artifact_id.as_str(), &create.conversation_id.as_str(), &hash.to_hex(), &create.media_type, &create.display_name],
		).await?;

		accounts::append_activity_and_outbox(&transaction, "artifact", create.artifact_id.as_str(), 1, "artifact_created", &command.key,
			&serde_json::json!({"conversation_id": create.conversation_id.as_str(), "blob_hash": hash.to_hex(), "revision": 1})).await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"artifact","artifact_id":create.artifact_id.as_str(),"revision":1}),
		)
		.await?;

		transaction.commit().await?;

		self.artifact(blob_store, &create.artifact_id, Some(1)).await
	}

	/// Apply one legal Artifact lifecycle transition and retain an immutable revision row.
	pub async fn transition_artifact(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		artifact_id: &ArtifactId,
		expected_revision: i64,
		status: ArtifactStatus,
	) -> Result<StoredArtifact, StoreError> {
		if expected_revision < 1 || status == ArtifactStatus::Active {
			return Err(StoreError::InvalidInput("Artifact transition is invalid"));
		}

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"transition_artifact",
			("artifact", artifact_id.as_str(), artifact_id.as_str()),
			Some(expected_revision),
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let revision = response_revision(&response, "artifact")?;

				return self.artifact(blob_store, artifact_id, Some(revision)).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let row = transaction.query_opt(
			"UPDATE decodex.artifacts SET status=$3::text::decodex.artifact_status, revision=revision+1, updated_at=clock_timestamp() WHERE artifact_id=$1::text::uuid AND revision=$2 RETURNING conversation_id::text, revision",
			&[&artifact_id.as_str(), &expected_revision, &artifact_status_sql(status)],
		).await?.ok_or(StoreError::RevisionConflict { entity: format!("artifact/{artifact_id}"), expected: Some(expected_revision), actual: None })?;
		let conversation_id: String = row.get(0);
		let revision: i64 = row.get(1);

		transaction.execute(
			"INSERT INTO decodex.artifact_revisions (artifact_id, conversation_id, revision, blob_hash, media_type, display_name, status) SELECT artifact_id, conversation_id, $2, blob_hash, media_type, display_name, $3::text::decodex.artifact_status FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=$4",
			&[&artifact_id.as_str(), &revision, &artifact_status_sql(status), &expected_revision],
		).await?;

		accounts::append_activity_and_outbox(&transaction, "artifact", artifact_id.as_str(), revision, "artifact_transitioned", &command.key,
			&serde_json::json!({"conversation_id":conversation_id,"status":artifact_status_sql(status),"revision":revision})).await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"artifact","artifact_id":artifact_id.as_str(),"revision":revision}),
		)
		.await?;

		transaction.commit().await?;

		self.artifact(blob_store, artifact_id, Some(revision)).await
	}

	/// Read and verify one exact or current Artifact revision.
	pub async fn artifact(
		&self,
		blob_store: &BlobStore,
		artifact_id: &ArtifactId,
		revision: Option<i64>,
	) -> Result<StoredArtifact, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT ar.artifact_id::text, ar.conversation_id::text, ar.revision, ar.status::text, \
			 ar.blob_hash, ar.media_type, ar.display_name, bo.byte_length \
			 FROM decodex.artifact_revisions ar JOIN decodex.artifacts a ON a.artifact_id=ar.artifact_id \
			 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
			 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=COALESCE($2,a.revision)",
				&[&artifact_id.as_str(), &revision],
			)
			.await?
			.ok_or(StoreError::InvalidInput("Artifact revision does not exist"))?;
		let hash = BlobHash::parse(row.get(4))?;
		let bytes = blob_store.read(hash)?;

		if i64::try_from(bytes.len()).ok() != Some(row.get(7)) {
			return Err(StoreError::Incompatible(
				"Artifact blob length differs from committed metadata".into(),
			));
		}

		let media_type: String = row.get(5);

		if !decodex_core::is_canonical_media_type(&media_type) {
			return Err(StoreError::Incompatible("stored Artifact media type is invalid".into()));
		}

		Ok(StoredArtifact {
			artifact_id: ArtifactId::new(row.get::<_, String>(0)).map_err(|_| {
				StoreError::Incompatible("stored Artifact identity is invalid".into())
			})?,
			conversation_id: ConversationId::new(row.get::<_, String>(1)).map_err(|_| {
				StoreError::Incompatible("stored Conversation identity is invalid".into())
			})?,
			revision: row.get(2),
			status: artifact_status_from_sql(row.get(3))?,
			blob_hash: hash,
			bytes,
			media_type,
			display_name: row.get(6),
		})
	}

	/// Persist a normalized history item. Blob bytes are atomically published and fully verified
	/// before PostgreSQL metadata/reference commit. A crash can therefore leave only an
	/// unreferenced content-addressed blob; it cannot commit a reference before publication.
	pub async fn record_history_item(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<HistoryEntry, StoreError> {
		self.record_history_item_command(blob_store, command, mutation)
			.await
			.map(|(entry, _)| entry)
	}

	/// Reserve one user Turn through its first completed history item and return exact admission.
	///
	/// A receipt replay reads the current Turn status and revision. It never recreates or
	/// reactivates a terminal Turn.
	pub async fn reserve_user_turn_with_history_item(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<TurnReservationOutcome, StoreError> {
		if mutation.turn_role != TurnRole::User
			|| mutation.possible_side_effects != PossibleSideEffects::Unknown
			|| mutation.expected_revision.is_some()
			|| mutation.status != ItemStatus::Completed
		{
			return Err(StoreError::InvalidInput("user Turn reservation history item is invalid"));
		}

		let existing = self.read_turn_reservation(mutation).await?;
		let mut exact_mutation = mutation.clone();
		if let Some(existing) = existing.as_ref() {
			exact_mutation.turn_sequence = existing.sequence;
		}
		let (_, disposition) =
			self.record_history_item_command(blob_store, command, &exact_mutation).await?;
		let readback = self.read_turn_reservation(&exact_mutation).await?.ok_or_else(|| {
			StoreError::Incompatible("reserved user Turn readback is missing".into())
		})?;

		match disposition {
			HistoryCommandDisposition::Fresh => {
				if existing.is_some()
					|| readback.sequence != mutation.turn_sequence
					|| readback.status != TurnStatus::Active
					|| readback.revision != 1
				{
					return Err(StoreError::Incompatible(
						"fresh user Turn reservation is not active revision 1".into(),
					));
				}
				Ok(TurnReservationOutcome::Fresh(readback))
			},
			HistoryCommandDisposition::Replayed => Ok(TurnReservationOutcome::Replayed(readback)),
		}
	}

	async fn read_turn_reservation(
		&self,
		mutation: &RecordHistoryItem,
	) -> Result<Option<TurnReservationReadback>, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				READ_TURN_ADMISSION_SQL,
				&[
					&mutation.conversation_id.as_str(),
					&mutation.runtime_session_id.as_str(),
					&mutation.turn_id.as_str(),
				],
			)
			.await?;
		let Some(row) = row else {
			return Ok(None);
		};
		let conversation_id: String = row.get(0);
		let runtime_session_id: String = row.get(1);
		let turn_id = TurnId::new(row.get::<_, String>(2))
			.map_err(|_| StoreError::Incompatible("Turn admission identity is invalid".into()))?;
		let sequence: i64 = row.get(3);
		let role = turn_role_from_sql(row.get(4))?;
		let side_effects = side_effect_from_sql(row.get(5))?;
		let status = match row.get::<_, String>(6).as_str() {
			"active" => TurnStatus::Active,
			"completed" => TurnStatus::Completed,
			"failed" => TurnStatus::Failed,
			_ => return Err(StoreError::Incompatible("Turn admission status is invalid".into())),
		};
		let revision: i64 = row.get(7);
		if conversation_id != mutation.conversation_id.as_str()
			|| runtime_session_id != mutation.runtime_session_id.as_str()
			|| turn_id != mutation.turn_id
			|| sequence <= 0
			|| role != TurnRole::User
			|| side_effects != PossibleSideEffects::Unknown
			|| revision <= 0
		{
			return Err(StoreError::Incompatible("Turn admission readback is cross-linked".into()));
		}

		Ok(Some(TurnReservationReadback { turn_id, sequence, status, revision }))
	}

	async fn record_history_item_command(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<(HistoryEntry, HistoryCommandDisposition), StoreError> {
		validate_history_item(mutation)?;

		let blob = prepare_payload(&mutation.text)?;
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"record_history_item",
			("conversation", mutation.conversation_id.as_str(), mutation.history_item_id.as_str()),
			mutation.expected_revision,
			blob,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let entry = history_entry_from_response(&response)?;

				self.verify_history_entry(blob_store, &entry).await?;

				return Ok((entry, HistoryCommandDisposition::Replayed));
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let capacity_hashes = blob.map(|(hash, _)| vec![hash]).unwrap_or_default();
		let mut referenced_hashes = self.history_artifact_hashes(mutation).await?;

		if let Some((hash, _)) = blob {
			referenced_hashes.push(hash);
		}

		let mut publication = if !referenced_hashes.is_empty() {
			let publication = self.lock_blob_session(&referenced_hashes, &capacity_hashes).await?;

			if let Some((hash, _)) = blob {
				publish_verified_blob(blob_store, hash, mutation.text.as_bytes())?;
			}

			publication
		} else {
			self.dedicated_session().await?
		};
		let transaction = publication.client.transaction().await?;

		if let Some((hash, byte_length)) = blob {
			insert_verified_blob(&transaction, hash, byte_length).await?;
		}

		ensure_turn(&transaction, mutation).await?;

		let revision = match mutation.expected_revision {
			None => insert_history_item(&transaction, mutation, blob).await?,
			Some(expected) => update_history_item(&transaction, mutation, blob, expected).await?,
		};
		let payload = serde_json::json!({
			"conversation_id": mutation.conversation_id.as_str(),
			"runtime_session_id": mutation.runtime_session_id.as_str(),
			"turn_id": mutation.turn_id.as_str(),
			"history_item_id": mutation.history_item_id.as_str(),
			"status": item_status_sql(mutation.status),
			"blob_hash": blob.map(|(hash, _)| hash.to_hex()),
			"revision": revision,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"history_item",
			mutation.history_item_id.as_str(),
			revision,
			if revision == 1 { "history_item_recorded" } else { "history_item_updated" },
			&command.key,
			&payload,
		)
		.await?;

		let response = history_entry_response(mutation, blob, revision)?;

		accounts::finish_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		let entry = history_entry_from_response(&response)?;

		self.verify_history_entry(blob_store, &entry).await?;

		Ok((entry, HistoryCommandDisposition::Fresh))
	}

	/// Remove a bounded inventory of grace-aged filesystem blobs that PostgreSQL proves have
	/// no committed reference. Metadata deletion commits before byte removal; collection then
	/// reacquires the writer lock and rechecks references, so every crash residue remains an
	/// inventory-visible content-addressed orphan rather than metadata whose bytes disappeared.
	pub async fn reclaim_orphan_blobs(
		&self,
		blob_store: &BlobStore,
		grace: Duration,
		limit: u16,
		after: Option<BlobInventoryCursor>,
	) -> Result<BlobReclaimPage, StoreError> {
		if limit == 0 || limit > 256 || grace.is_zero() {
			return Err(StoreError::InvalidInput("blob reclamation bounds are invalid"));
		}

		self.pool().get().await?.query_one("SELECT decodex.prune_history_snapshots()", &[]).await?;

		let page = blob_store.old_inventory(grace, usize::from(limit), after)?;
		let mut removed = 0_u16;

		for candidate in page.entries {
			let mut session = self.lock_blob_session(&[candidate.hash], &[candidate.hash]).await?;
			let transaction = session.client.transaction().await?;
			let referenced: bool = transaction
				.query_one(
					"SELECT EXISTS (SELECT 1 FROM decodex.history_items WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.history_item_versions WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.artifact_revisions WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.context_packs WHERE blob_hash=$1)",
					&[&candidate.hash.to_hex()],
				)
				.await?
				.get(0);
			let eligible_for_unlink = !referenced;

			if !referenced {
				let deletion = transaction
					.execute(
						"DELETE FROM decodex.blob_objects WHERE blob_hash=$1",
						&[&candidate.hash.to_hex()],
					)
					.await;

				match deletion {
					Ok(_) => {},
					Err(error)
						if error.code().is_some_and(|code| {
							code == &SqlState::FOREIGN_KEY_VIOLATION
								|| code == &SqlState::RESTRICT_VIOLATION
						}) =>
					{
						transaction.rollback().await?;

						continue;
					},
					Err(error) => return Err(error.into()),
				}
			}

			transaction.commit().await?;

			if eligible_for_unlink && blob_store.remove_orphan_if_old(candidate.hash, grace)? {
				removed += 1;
			}
		}

		Ok(BlobReclaimPage { removed, next_cursor: page.next_cursor })
	}

	/// Read one strictly bounded keyset page and verify every referenced blob before return.
	pub async fn conversation_history(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		after: Option<&HistoryCursor>,
		page_size: u16,
	) -> Result<HistoryPage, StoreError> {
		if page_size == 0 || page_size > MAX_PAGE_SIZE {
			return Err(StoreError::InvalidInput("history page size must be within 1..=100"));
		}

		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock($1)",
				&[&HIERARCHY_COORDINATION_LOCK],
			)
			.await?;
		transaction
			.query_one("SELECT pg_catalog.pg_advisory_xact_lock($1)", &[&CURSOR_COORDINATION_LOCK])
			.await?;

		if transaction
			.query_opt(
				"SELECT 1 FROM decodex.conversations \
				 WHERE conversation_id = $1::text::uuid FOR UPDATE",
				&[&conversation_id.as_str()],
			)
			.await?
			.is_none()
		{
			return Err(StoreError::InvalidInput("Conversation does not exist"));
		}

		let current_high_water: i64 = transaction
			.query_one(
				"SELECT COALESCE(max(history_position), 0) FROM decodex.history_items \
				 WHERE conversation_id = $1::text::uuid",
				&[&conversation_id.as_str()],
			)
			.await?
			.get(0);
		let current_snapshot_version: i64 = transaction
			.query_one(
				"SELECT COALESCE(max(version_sequence), 0) FROM decodex.history_item_versions \
				 WHERE conversation_id = $1::text::uuid",
				&[&conversation_id.as_str()],
			)
			.await?
			.get(0);
		let (snapshot_high_water, snapshot_version, last_position): (i64, i64, i64) =
			if let Some(cursor) = after {
				let row = transaction
					.query_opt(
						"SELECT snapshot_high_water, snapshot_version_sequence, last_position, page_size \
					 FROM decodex.history_cursors \
					 WHERE cursor_id=$1::text::uuid AND conversation_id=$2::text::uuid \
					 AND expires_at > clock_timestamp()",
						&[&cursor.token, &conversation_id.as_str()],
					)
					.await?
					.ok_or(StoreError::InvalidInput(
						"history cursor was not issued or has expired for this Conversation",
					))?;

				if row.get::<_, i32>(3) != i32::from(page_size) {
					return Err(StoreError::InvalidInput(
						"history page size must match the issued cursor",
					));
				}

				(row.get(0), row.get(1), row.get(2))
			} else {
				(current_high_water, current_snapshot_version, 0)
			};
		let fetch_limit = i64::from(page_size) + 1;
		let rows = transaction
			.query(
				"SELECT hi.history_position, hi.history_item_id::text, t.turn_id::text, \
				 t.runtime_session_id::text, t.role::text, t.possible_side_effects::text, \
				 hi.kind::text, hi.status::text, hi.inline_text, hi.blob_hash, bo.byte_length, \
				 hi.media_type, hi.metadata, hi.artifact_id::text, hi.artifact_revision, hi.revision \
				 FROM (SELECT DISTINCT ON (history_position) * \
				       FROM decodex.history_item_versions \
				       WHERE conversation_id=$1::text::uuid AND version_sequence <= $4 \
				       ORDER BY history_position, version_sequence DESC) AS hi \
				 JOIN decodex.turns AS t ON t.turn_id = hi.turn_id \
				 LEFT JOIN decodex.blob_objects AS bo ON bo.blob_hash = hi.blob_hash \
				 WHERE hi.history_position > $2 AND hi.history_position <= $3 \
				 ORDER BY hi.history_position LIMIT $5",
				&[
					&conversation_id.as_str(),
					&last_position,
					&snapshot_high_water,
					&snapshot_version,
					&fetch_limit,
				],
			)
			.await?;
		let has_more = rows.len() > usize::from(page_size);
		let mut entries = Vec::with_capacity(rows.len().min(usize::from(page_size)));

		for row in rows.into_iter().take(usize::from(page_size)) {
			entries.push(history_entry_from_row(row)?);
		}

		let next_cursor = if has_more {
			Some(
				issue_history_cursor(&transaction, conversation_id, after, i32::from(page_size))
					.await?,
			)
		} else {
			None
		};

		transaction.commit().await?;

		for entry in &entries {
			self.verify_history_entry(blob_store, entry).await?;
		}

		Ok(HistoryPage { entries, next_cursor })
	}

	/// Read the newest bounded history window for deterministic Context-Pack compilation.
	pub async fn recent_conversation_history(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		limit: u16,
	) -> Result<Vec<HistoryEntry>, StoreError> {
		if limit == 0 || usize::from(limit) > decodex_core::MAX_CONTEXT_RECENT_ITEMS {
			return Err(StoreError::InvalidInput("recent history bound is invalid"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock($1)",
				&[&HIERARCHY_COORDINATION_LOCK],
			)
			.await?;
		transaction
			.query_one("SELECT pg_catalog.pg_advisory_xact_lock($1)", &[&CURSOR_COORDINATION_LOCK])
			.await?;
		if transaction
			.query_opt(
				"SELECT 1 FROM decodex.conversations \
					 WHERE conversation_id=$1::text::uuid FOR UPDATE",
				&[&conversation_id.as_str()],
			)
			.await?
			.is_none()
		{
			return Err(StoreError::InvalidInput("Conversation does not exist"));
		}
		let rows = transaction
			.query(
				"SELECT hi.history_position,hi.history_item_id::text,t.turn_id::text,\
					 t.runtime_session_id::text,t.role::text,t.possible_side_effects::text,\
					 hi.kind::text,hi.status::text,hi.inline_text,hi.blob_hash,bo.byte_length,\
					 hi.media_type,hi.metadata,hi.artifact_id::text,hi.artifact_revision,hi.revision \
					 FROM decodex.history_items hi \
					 JOIN decodex.turns t ON t.turn_id=hi.turn_id \
					 LEFT JOIN decodex.blob_objects bo ON bo.blob_hash=hi.blob_hash \
					 WHERE hi.conversation_id=$1::text::uuid \
					 ORDER BY hi.history_position DESC LIMIT $2",
				&[&conversation_id.as_str(), &i64::from(limit)],
			)
			.await?;
		transaction.commit().await?;

		let entries =
			rows.into_iter().rev().map(history_entry_from_row).collect::<Result<Vec<_>, _>>()?;
		for entry in &entries {
			self.verify_history_entry(blob_store, entry).await?;
		}
		Ok(entries)
	}

	/// Persist an immutable compiled Context Pack and its exact provenance revisions.
	pub async fn persist_context_pack(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		request: &PersistContextPack,
		pack: &ContextPack,
	) -> Result<ContextPackRecord, StoreError> {
		validate_context_pack(request, pack)?;

		let blob = if pack.bytes().len() > MAX_INLINE_HISTORY_BYTES {
			Some((
				pack.digest(),
				i64::try_from(pack.bytes().len()).map_err(|_| {
					StoreError::InvalidInput("Context Pack byte length is out of range")
				})?,
			))
		} else {
			None
		};
		let inline_bytes = blob.is_none().then(|| pack.bytes().to_vec());
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"persist_context_pack",
			("conversation", pack.conversation_id().as_str(), &request.context_pack_id),
			Some(request.pack_revision),
			blob,
		)
		.await?
		{
			accounts::CommandClaim::Completed(_) => {
				return self.required_context_pack(blob_store, &request.context_pack_id).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let capacity_hashes = blob.map(|(hash, _)| vec![hash]).unwrap_or_default();
		let referenced_hashes = context_pack_referenced_hashes(pack, blob);
		let mut publication = if !referenced_hashes.is_empty() {
			let publication = self.lock_blob_session(&referenced_hashes, &capacity_hashes).await?;

			if let Some((hash, _)) = blob {
				publish_verified_blob(blob_store, hash, pack.bytes())?;
			}

			publication
		} else {
			self.dedicated_session().await?
		};
		let transaction = publication.client.transaction().await?;

		if let Some((hash, byte_length)) = blob {
			insert_verified_blob(&transaction, hash, byte_length).await?;
		}

		insert_context_pack_sources(&transaction, request, pack).await?;

		transaction
			.execute(
				"INSERT INTO decodex.context_packs \
				 (context_pack_id, conversation_id, pack_revision, compiled_digest, manifest_digest, inline_bytes, \
				  blob_hash, byte_length, max_bytes, recent_item_limit, possible_side_effects, \
				  truncated, omitted_source_count, source_count) \
				 VALUES ($1::text::uuid, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10, \
				 $11::text::decodex.side_effect_state, $12, $13, $14)",
				&[
					&request.context_pack_id,
					&pack.conversation_id().as_str(),
					&request.pack_revision,
					&pack.digest().to_hex(),
					&pack.manifest_digest().to_hex(),
					&inline_bytes,
					&blob.map(|(hash, _)| hash.to_hex()),
					&i64::try_from(pack.bytes().len()).unwrap_or(i64::MAX),
					&i32::try_from(pack.policy().max_bytes()).unwrap_or(i32::MAX),
					&i32::try_from(pack.policy().recent_item_limit()).unwrap_or(i32::MAX),
					&side_effect_sql(pack.possible_side_effects()),
					&pack.truncated(),
					&i32::try_from(pack.omitted_source_count()).unwrap_or(i32::MAX),
					&i32::try_from(pack.source_manifest().len()).unwrap_or(i32::MAX),
				],
			)
			.await?;

		let payload = serde_json::json!({
			"context_pack_id": request.context_pack_id,
			"conversation_id": pack.conversation_id().as_str(),
			"pack_revision": request.pack_revision,
			"compiled_digest": pack.digest().to_hex(),
			"dispatch_enabled": false,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"context_pack",
			&request.context_pack_id,
			request.pack_revision,
			"context_pack_compiled",
			&command.key,
			&payload,
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({
				"kind": "context_pack",
				"context_pack_id": request.context_pack_id,
			}),
		)
		.await?;

		transaction.commit().await?;

		self.required_context_pack(blob_store, &request.context_pack_id).await
	}

	/// Persist an inert transition proposal whose dispatch flag is schema-forced false.
	pub async fn propose_transition(
		&self,
		command: &CommandIdentity,
		proposal: &ProposeTransition,
	) -> Result<(), StoreError> {
		if !is_canonical_uuid(&proposal.transition_id)
			|| !is_canonical_uuid(&proposal.context_pack_id)
			|| proposal.reason.is_empty()
			|| proposal.reason.len() > 512
		{
			return Err(StoreError::InvalidInput("transition proposal is malformed"));
		}

		crate::ensure_credential_negative_text(&proposal.reason)?;

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"propose_transition",
			("conversation", proposal.conversation_id.as_str(), &proposal.transition_id),
			None,
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(_) => return Ok(()),
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;

		transaction
			.execute(
				"INSERT INTO decodex.transition_proposals \
				 (transition_id, conversation_id, from_runtime_session_id, context_pack_id, kind, reason) \
				 VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4::text::uuid, \
				 $5::text::decodex.transition_kind, $6)",
				&[
					&proposal.transition_id,
					&proposal.conversation_id.as_str(),
					&proposal.from_runtime_session_id.as_str(),
					&proposal.context_pack_id,
					&transition_sql(proposal.kind),
					&proposal.reason,
				],
			)
			.await?;

		accounts::append_activity_and_outbox(
			&transaction,
			"transition_proposal",
			&proposal.transition_id,
			1,
			"transition_proposed",
			&command.key,
			&serde_json::json!({
				"conversation_id": proposal.conversation_id.as_str(),
				"kind": transition_sql(proposal.kind),
				"dispatch_enabled": false,
			}),
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({
				"kind": "transition_proposal",
				"transition_id": proposal.transition_id,
				"dispatch_enabled": false,
			}),
		)
		.await?;

		transaction.commit().await?;

		Ok(())
	}

	async fn verify_history_entry(
		&self,
		blob_store: &BlobStore,
		entry: &HistoryEntry,
	) -> Result<(), StoreError> {
		verify_entry_blob(blob_store, entry)?;

		if let Some((artifact_id, revision)) = &entry.artifact {
			let revision = i64::try_from(*revision).map_err(|_| {
				StoreError::Incompatible("history Artifact revision is invalid".into())
			})?;
			let row = self
				.pool()
				.get()
				.await?
				.query_opt(
					"SELECT ar.blob_hash, bo.byte_length FROM decodex.artifact_revisions ar \
					 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
					 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=$2",
					&[&artifact_id.as_str(), &revision],
				)
				.await?
				.ok_or_else(|| {
					StoreError::Incompatible("history Artifact revision metadata is absent".into())
				})?;
			let hash = BlobHash::parse(row.get(0))?;
			let bytes = blob_store.read(hash)?;

			if i64::try_from(bytes.len()).ok() != Some(row.get(1)) {
				return Err(StoreError::Incompatible(
					"history Artifact blob length differs from metadata".into(),
				));
			}
		}

		Ok(())
	}

	/// Inspect Context Pack metadata only after re-verifying its inline or blob bytes.
	pub async fn context_pack(
		&self,
		blob_store: &BlobStore,
		context_pack_id: &str,
	) -> Result<ContextPackRecord, StoreError> {
		if !is_canonical_uuid(context_pack_id) {
			return Err(StoreError::InvalidInput("Context Pack identity is malformed"));
		}

		self.required_context_pack(blob_store, context_pack_id).await
	}

	async fn verify_context_pack_artifacts(
		&self,
		blob_store: &BlobStore,
		sources: &[decodex_core::ContextSourceManifest],
	) -> Result<(), StoreError> {
		for source in sources {
			let Some((artifact_id, revision)) = source.artifact_reference() else {
				continue;
			};
			let revision = i64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("Context Pack Artifact revision is invalid".into())
			})?;
			let row = self
				.pool()
				.get()
				.await?
				.query_opt(
					"SELECT ar.blob_hash, bo.byte_length FROM decodex.artifact_revisions ar \
					 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
					 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=$2",
					&[&artifact_id.as_str(), &revision],
				)
				.await?
				.ok_or_else(|| {
					StoreError::Incompatible(
						"Context Pack Artifact revision metadata is absent".into(),
					)
				})?;
			let hash = BlobHash::parse(row.get(0))?;
			let bytes = blob_store.read(hash)?;

			if hash != source.content_digest()
				|| u64::try_from(bytes.len()).ok() != Some(source.original_byte_length())
				|| i64::try_from(bytes.len()).ok() != Some(row.get(1))
			{
				return Err(StoreError::Incompatible(
					"Context Pack Artifact bytes differ from provenance".into(),
				));
			}
		}

		Ok(())
	}

	pub(crate) async fn required_context_pack(
		&self,
		blob_store: &BlobStore,
		context_pack_id: &str,
	) -> Result<ContextPackRecord, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT context_pack_id::text, conversation_id::text, pack_revision, compiled_digest, \
				 manifest_digest, byte_length, max_bytes, recent_item_limit, possible_side_effects::text, \
				 truncated, omitted_source_count, inline_bytes, blob_hash, source_count \
			 FROM decodex.context_packs \
			 WHERE context_pack_id = $1::text::uuid",
				&[&context_pack_id],
			)
			.await?
			.ok_or_else(|| {
				StoreError::Incompatible("Context Pack command receipt lost entity".into())
			})?;
		let digest = BlobHash::parse(row.get::<_, &str>(3))?;
		let stored_manifest_digest = BlobHash::parse(row.get::<_, &str>(4))?;
		let byte_length = u64::try_from(row.get::<_, i64>(5)).map_err(|_| {
			StoreError::Incompatible("stored Context Pack length is invalid".into())
		})?;
		let inline: Option<Vec<u8>> = row.get(11);
		let blob: Option<String> = row.get(12);
		let bytes = match (inline, blob) {
			(Some(inline), None) => inline,
			(None, Some(blob)) => blob_store.read(BlobHash::parse(&blob)?)?,
			_ => {
				return Err(StoreError::Incompatible(
					"stored Context Pack payload is invalid".into(),
				));
			},
		};

		if BlobHash::digest(&bytes) != digest
			|| u64::try_from(bytes.len()).ok() != Some(byte_length)
		{
			return Err(StoreError::Incompatible(
				"stored Context Pack bytes failed verification".into(),
			));
		}

		let conversation_id = ConversationId::new(row.get::<_, String>(1)).map_err(|_| {
			StoreError::Incompatible("stored conversation identity is invalid".into())
		})?;
		let source_rows = self
			.pool()
			.get()
			.await?
			.query(
				"SELECT kind::text, source_id, source_revision, content_digest, original_byte_length, \
			 included_byte_length, included_digest, disposition::text, artifact_id::text, artifact_revision \
			 FROM decodex.context_pack_sources WHERE context_pack_id = $1::text::uuid ORDER BY position",
				&[&context_pack_id],
			)
			.await?;
		let sources =
			source_rows.into_iter().map(context_source_from_row).collect::<Result<Vec<_>, _>>()?;

		self.verify_context_pack_artifacts(blob_store, &sources).await?;

		if i32::try_from(sources.len()).ok() != Some(row.get(13)) {
			return Err(StoreError::Incompatible(
				"stored Context Pack source count is inconsistent".into(),
			));
		}

		let policy = ContextPackPolicy::new(
			usize::try_from(row.get::<_, i32>(6))
				.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?,
			usize::try_from(row.get::<_, i32>(7))
				.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?,
		)
		.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?;
		let pack = ContextPack::from_persisted(
			conversation_id.clone(),
			side_effect_from_sql(row.get::<_, &str>(8))?,
			policy,
			sources,
			bytes,
			digest,
		)
		.map_err(|_| {
			StoreError::Incompatible(
				"stored Context Pack record failed canonical verification".into(),
			)
		})?;

		if pack.manifest_digest() != stored_manifest_digest
			|| pack.truncated() != row.get::<_, bool>(9)
			|| pack.omitted_source_count()
				!= usize::try_from(row.get::<_, i32>(10)).unwrap_or(usize::MAX)
		{
			return Err(StoreError::Incompatible(
				"stored Context Pack metadata is inconsistent".into(),
			));
		}

		Ok(ContextPackRecord {
			context_pack_id: row.get(0),
			conversation_id,
			pack_revision: row.get(2),
			compiled_digest: digest,
			byte_length,
			truncated: row.get(9),
			omitted_source_count: usize::try_from(row.get::<_, i32>(10))
				.map_err(|_| StoreError::Incompatible("stored source count is invalid".into()))?,
			pack,
		})
	}
}

pub(crate) fn context_pack_referenced_hashes(
	pack: &ContextPack,
	blob: Option<(BlobHash, i64)>,
) -> Vec<BlobHash> {
	let mut hashes = pack
		.source_manifest()
		.iter()
		.filter(|source| source.artifact_reference().is_some())
		.map(decodex_core::ContextSourceManifest::content_digest)
		.collect::<Vec<_>>();

	if let Some((hash, _)) = blob {
		hashes.push(hash);
	}

	hashes
}

fn validate_artifact(create: &CreateArtifact) -> Result<(), StoreError> {
	if create.bytes.is_empty()
		|| create.bytes.len() > MAX_BLOB_BYTES
		|| !decodex_core::is_canonical_media_type(&create.media_type)
		|| create.display_name.as_ref().is_some_and(|name| name.is_empty() || name.len() > 256)
	{
		return Err(StoreError::InvalidInput("Artifact violates a field or payload bound"));
	}

	crate::ensure_credential_negative_text(&create.media_type)?;

	if let Some(name) = &create.display_name {
		crate::ensure_credential_negative_text(name)?;
	}

	Ok(())
}

fn validate_history_item(mutation: &RecordHistoryItem) -> Result<(), StoreError> {
	if mutation.turn_sequence <= 0
		|| !(0..=1_000_000).contains(&mutation.ordinal)
		|| mutation.text.len() > MAX_BLOB_BYTES
		|| mutation.expected_revision.is_some_and(|revision| revision < 1)
		|| (mutation.kind == HistoryItemKind::Artifact) != mutation.artifact.is_some()
	{
		return Err(StoreError::InvalidInput("history item violates a field or payload bound"));
	}

	crate::ensure_credential_negative_text(&mutation.text)?;
	crate::ensure_credential_negative_text(mutation.media_type.as_str())?;

	Ok(())
}

fn history_metadata_json(metadata: &HistoryMetadata) -> Result<Value, StoreError> {
	serde_json::to_value(metadata)
		.map_err(|_| StoreError::Incompatible("history metadata could not be encoded".into()))
}

fn history_metadata_from_json(metadata: Value) -> Result<HistoryMetadata, StoreError> {
	serde_json::from_value(metadata)
		.map_err(|_| StoreError::Incompatible("history metadata is invalid".into()))
}

fn prepare_payload(text: &str) -> Result<Option<(BlobHash, i64)>, StoreError> {
	if text.len() <= MAX_INLINE_HISTORY_BYTES {
		return Ok(None);
	}

	Ok(Some((
		BlobHash::digest(text.as_bytes()),
		i64::try_from(text.len())
			.map_err(|_| StoreError::InvalidInput("blob length is out of range"))?,
	)))
}

fn history_entry_from_row(row: Row) -> Result<HistoryEntry, StoreError> {
	let history_item_id: String = row.get(1);
	let blob_hash =
		row.get::<_, Option<String>>(9).map(|value| BlobHash::parse(&value)).transpose()?;
	let blob_byte_length = row
		.get::<_, Option<i64>>(10)
		.map(|value| {
			u64::try_from(value)
				.map_err(|_| StoreError::Incompatible("blob length is invalid".into()))
		})
		.transpose()?;
	let artifact = match (row.get::<_, Option<String>>(13), row.get::<_, Option<i64>>(14)) {
		(Some(id), Some(revision)) => Some((
			ArtifactId::new(id).map_err(|_| {
				StoreError::Incompatible("history Artifact identity is invalid".into())
			})?,
			u64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("history Artifact revision is invalid".into())
			})?,
		)),
		(None, None) => None,
		_ => {
			return Err(StoreError::Incompatible(
				"history Artifact reference is incomplete".into(),
			));
		},
	};
	let kind = item_kind_from_sql(row.get::<_, &str>(6))?;
	let media_type = HistoryMediaType::new(row.get::<_, String>(11))
		.map_err(|_| StoreError::Incompatible("history media type is invalid".into()))?;
	let metadata = history_metadata_from_json(row.get(12))?;

	if (kind == HistoryItemKind::Artifact) != artifact.is_some() {
		return Err(StoreError::Incompatible(
			"history Artifact kind/reference is inconsistent".into(),
		));
	}

	Ok(HistoryEntry {
		history_item_id,
		turn_id: row.get(2),
		runtime_session_id: row.get(3),
		turn_role: turn_role_from_sql(row.get::<_, &str>(4))?,
		possible_side_effects: side_effect_from_sql(row.get::<_, &str>(5))?,
		kind,
		status: item_status_from_sql(row.get::<_, &str>(7))?,
		inline_text: row.get(8),
		blob_hash,
		blob_byte_length,
		media_type,
		metadata,
		artifact,
		revision: row.get(15),
	})
}

fn verify_entry_blob(blob_store: &BlobStore, entry: &HistoryEntry) -> Result<(), StoreError> {
	match (entry.blob_hash, entry.blob_byte_length) {
		(Some(hash), Some(expected)) => {
			let bytes = blob_store.read(hash)?;

			if u64::try_from(bytes.len()).ok() != Some(expected) {
				return Err(StoreError::Incompatible(
					"verified blob length differs from metadata".into(),
				));
			}

			Ok(())
		},
		(None, None) if entry.inline_text.is_some() => Ok(()),
		_ => Err(StoreError::Incompatible("history payload metadata is incomplete".into())),
	}
}

pub(crate) fn validate_context_pack(
	request: &PersistContextPack,
	pack: &ContextPack,
) -> Result<(), StoreError> {
	if !is_canonical_uuid(&request.context_pack_id)
		|| request.pack_revision < 1
		|| pack.verify().is_err()
	{
		return Err(StoreError::InvalidInput("Context Pack violates its immutable policy"));
	}

	for source in pack.source_manifest() {
		crate::ensure_credential_negative_text(source.source_id())?;
	}

	Ok(())
}

fn context_source_from_row(row: Row) -> Result<decodex_core::ContextSourceManifest, StoreError> {
	let artifact = match (row.get::<_, Option<String>>(8), row.get::<_, Option<i64>>(9)) {
		(Some(id), Some(revision)) => Some((
			ArtifactId::new(id).map_err(|_| {
				StoreError::Incompatible("stored Artifact identity is invalid".into())
			})?,
			u64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("stored Artifact revision is invalid".into())
			})?,
		)),
		(None, None) => None,
		_ => return Err(StoreError::Incompatible("stored Artifact source is incomplete".into())),
	};

	decodex_core::ContextSourceManifest::from_persisted(
		context_source_from_sql(row.get(0))?,
		row.get::<_, String>(1),
		u64::try_from(row.get::<_, i64>(2))
			.map_err(|_| StoreError::Incompatible("stored source revision is invalid".into()))?,
		BlobHash::parse(row.get(3))?,
		u64::try_from(row.get::<_, i64>(4))
			.map_err(|_| StoreError::Incompatible("stored source length is invalid".into()))?,
		u64::try_from(row.get::<_, i64>(5))
			.map_err(|_| StoreError::Incompatible("stored included length is invalid".into()))?,
		BlobHash::parse(row.get(6))?,
		context_disposition_from_sql(row.get(7))?,
		artifact,
	)
	.map_err(|_| StoreError::Incompatible("stored source manifest is invalid".into()))
}

const fn artifact_status_sql(value: ArtifactStatus) -> &'static str {
	match value {
		ArtifactStatus::Active => "active",
		ArtifactStatus::Expired => "expired",
		ArtifactStatus::Deleted => "deleted",
	}
}

fn artifact_status_from_sql(value: &str) -> Result<ArtifactStatus, StoreError> {
	match value {
		"active" => Ok(ArtifactStatus::Active),
		"expired" => Ok(ArtifactStatus::Expired),
		"deleted" => Ok(ArtifactStatus::Deleted),
		_ => Err(StoreError::Incompatible("unknown Artifact status".into())),
	}
}

const fn turn_role_sql(value: TurnRole) -> &'static str {
	match value {
		TurnRole::User => "user",
		TurnRole::Assistant => "assistant",
		TurnRole::System => "system",
		TurnRole::Tool => "tool",
	}
}

fn turn_role_from_sql(value: &str) -> Result<TurnRole, StoreError> {
	match value {
		"user" => Ok(TurnRole::User),
		"assistant" => Ok(TurnRole::Assistant),
		"system" => Ok(TurnRole::System),
		"tool" => Ok(TurnRole::Tool),
		_ => Err(StoreError::Incompatible("unknown turn role".into())),
	}
}

pub(crate) const fn side_effect_sql(value: PossibleSideEffects) -> &'static str {
	match value {
		PossibleSideEffects::None => "none",
		PossibleSideEffects::Possible => "possible",
		PossibleSideEffects::Unknown => "unknown",
	}
}

fn side_effect_from_sql(value: &str) -> Result<PossibleSideEffects, StoreError> {
	match value {
		"none" => Ok(PossibleSideEffects::None),
		"possible" => Ok(PossibleSideEffects::Possible),
		"unknown" => Ok(PossibleSideEffects::Unknown),
		_ => Err(StoreError::Incompatible("unknown side-effect state".into())),
	}
}

const fn item_kind_sql(value: HistoryItemKind) -> &'static str {
	match value {
		HistoryItemKind::Message => "message",
		HistoryItemKind::Reasoning => "reasoning",
		HistoryItemKind::ToolCall => "tool_call",
		HistoryItemKind::ToolResult => "tool_result",
		HistoryItemKind::Artifact => "artifact",
		HistoryItemKind::Status => "status",
	}
}

fn item_kind_from_sql(value: &str) -> Result<HistoryItemKind, StoreError> {
	match value {
		"message" => Ok(HistoryItemKind::Message),
		"reasoning" => Ok(HistoryItemKind::Reasoning),
		"tool_call" => Ok(HistoryItemKind::ToolCall),
		"tool_result" => Ok(HistoryItemKind::ToolResult),
		"artifact" => Ok(HistoryItemKind::Artifact),
		"status" => Ok(HistoryItemKind::Status),
		_ => Err(StoreError::Incompatible("unknown history-item kind".into())),
	}
}

const fn item_status_sql(value: ItemStatus) -> &'static str {
	match value {
		ItemStatus::Streaming => "streaming",
		ItemStatus::Completed => "completed",
		ItemStatus::Failed => "failed",
	}
}

fn item_status_from_sql(value: &str) -> Result<ItemStatus, StoreError> {
	match value {
		"streaming" => Ok(ItemStatus::Streaming),
		"completed" => Ok(ItemStatus::Completed),
		"failed" => Ok(ItemStatus::Failed),
		_ => Err(StoreError::Incompatible("unknown history-item status".into())),
	}
}

pub(crate) const fn context_source_sql(value: ContextSourceKind) -> &'static str {
	match value {
		ContextSourceKind::PinnedRevision => "pinned_revision",
		ContextSourceKind::RepositoryInstructions => "repository_instructions",
		ContextSourceKind::OpenWiki => "openwiki",
		ContextSourceKind::Decision => "decision",
		ContextSourceKind::Fact => "fact",
		ContextSourceKind::Artifact => "artifact",
		ContextSourceKind::RecentRaw => "recent_raw",
	}
}

fn context_source_from_sql(value: &str) -> Result<ContextSourceKind, StoreError> {
	match value {
		"pinned_revision" => Ok(ContextSourceKind::PinnedRevision),
		"repository_instructions" => Ok(ContextSourceKind::RepositoryInstructions),
		"openwiki" => Ok(ContextSourceKind::OpenWiki),
		"decision" => Ok(ContextSourceKind::Decision),
		"fact" => Ok(ContextSourceKind::Fact),
		"artifact" => Ok(ContextSourceKind::Artifact),
		"recent_raw" => Ok(ContextSourceKind::RecentRaw),
		_ => Err(StoreError::Incompatible("unknown Context Pack source kind".into())),
	}
}

pub(crate) const fn context_disposition_sql(value: ContextSourceDisposition) -> &'static str {
	match value {
		ContextSourceDisposition::Complete => "complete",
		ContextSourceDisposition::Truncated => "truncated",
		ContextSourceDisposition::Omitted => "omitted",
	}
}

fn context_disposition_from_sql(value: &str) -> Result<ContextSourceDisposition, StoreError> {
	match value {
		"complete" => Ok(ContextSourceDisposition::Complete),
		"truncated" => Ok(ContextSourceDisposition::Truncated),
		"omitted" => Ok(ContextSourceDisposition::Omitted),
		_ => Err(StoreError::Incompatible("unknown Context Pack source disposition".into())),
	}
}

const fn transition_sql(value: ProposedTransitionKind) -> &'static str {
	match value {
		ProposedTransitionKind::Rollover => "rollover",
		ProposedTransitionKind::Fallback => "fallback",
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

const fn quick_task_terminal_outcome_sql(
	value: decodex_core::ProviderTerminalOutcome,
) -> &'static str {
	match value {
		decodex_core::ProviderTerminalOutcome::Succeeded => "succeeded",
		decodex_core::ProviderTerminalOutcome::FailedDefinitive => "failed_definitive",
		decodex_core::ProviderTerminalOutcome::NotSubmitted => "not_submitted",
	}
}

fn is_lower_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(crate) fn publish_verified_blob(
	blob_store: &BlobStore,
	hash: BlobHash,
	bytes: &[u8],
) -> Result<(), StoreError> {
	let published = blob_store.put(bytes)?;

	if published != hash || blob_store.read(hash)? != bytes {
		return Err(StoreError::InvalidInput("blob publication verification failed"));
	}

	Ok(())
}

fn response_revision(response: &Value, kind: &str) -> Result<i64, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some(kind) {
		return Err(StoreError::Incompatible("stored command response kind is invalid".into()));
	}

	response.get("revision").and_then(Value::as_i64).filter(|revision| *revision > 0).ok_or_else(
		|| StoreError::Incompatible("stored command response revision is invalid".into()),
	)
}

fn history_entry_response(
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
	revision: i64,
) -> Result<Value, StoreError> {
	let blob_byte_length = blob
		.map(|(_, length)| u64::try_from(length))
		.transpose()
		.map_err(|_| StoreError::Incompatible("history response length is invalid".into()))?;
	let artifact = mutation.artifact.as_ref().map(|(id, artifact_revision)| {
		serde_json::json!({
			"artifact_id": id.as_str(),
			"revision": artifact_revision,
		})
	});

	Ok(serde_json::json!({
		"kind": "history_item",
		"history_item_id": mutation.history_item_id.as_str(),
		"turn_id": mutation.turn_id.as_str(),
		"runtime_session_id": mutation.runtime_session_id.as_str(),
		"turn_role": turn_role_sql(mutation.turn_role),
		"possible_side_effects": side_effect_sql(mutation.possible_side_effects),
		"item_kind": item_kind_sql(mutation.kind),
		"status": item_status_sql(mutation.status),
		"inline_text": blob.is_none().then_some(mutation.text.as_str()),
		"blob_hash": blob.map(|(hash, _)| hash.to_hex()),
		"blob_byte_length": blob_byte_length,
		"media_type": mutation.media_type.as_str(),
		"metadata": mutation.metadata,
		"artifact": artifact,
		"revision": revision,
	}))
}

fn history_entry_from_response(response: &Value) -> Result<HistoryEntry, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("history_item") {
		return Err(StoreError::Incompatible("stored history response kind is invalid".into()));
	}

	let required = |field: &'static str| {
		response
			.get(field)
			.and_then(Value::as_str)
			.ok_or_else(|| StoreError::Incompatible("stored history response is incomplete".into()))
	};
	let inline_text = match response.get("inline_text") {
		Some(Value::String(text)) => Some(text.clone()),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let blob_hash = match response.get("blob_hash") {
		Some(Value::String(hash)) => Some(BlobHash::parse(hash)?),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let blob_byte_length = match response.get("blob_byte_length") {
		Some(Value::Number(length)) => length.as_u64(),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let artifact = match response.get("artifact") {
		Some(Value::Object(reference)) => {
			let id = reference.get("artifact_id").and_then(Value::as_str).ok_or_else(|| {
				StoreError::Incompatible("stored history Artifact response is invalid".into())
			})?;
			let revision = reference
				.get("revision")
				.and_then(Value::as_u64)
				.filter(|v| *v > 0)
				.ok_or_else(|| {
					StoreError::Incompatible("stored history Artifact response is invalid".into())
				})?;

			Some((
				ArtifactId::new(id.to_owned()).map_err(|_| {
					StoreError::Incompatible("stored history Artifact response is invalid".into())
				})?,
				revision,
			))
		},
		Some(Value::Null) => None,
		_ => {
			return Err(StoreError::Incompatible(
				"stored history Artifact response is invalid".into(),
			));
		},
	};
	let metadata = response.get("metadata").cloned().ok_or_else(|| {
		StoreError::Incompatible("stored history metadata response is absent".into())
	})?;
	let media_type = HistoryMediaType::new(required("media_type")?.to_owned())
		.map_err(|_| StoreError::Incompatible("stored history media type is invalid".into()))?;
	let metadata = history_metadata_from_json(metadata)?;
	let kind = item_kind_from_sql(required("item_kind")?)?;

	if (inline_text.is_some() == blob_hash.is_some())
		|| blob_hash.is_some() != blob_byte_length.is_some()
		|| (kind == HistoryItemKind::Artifact) != artifact.is_some()
	{
		return Err(StoreError::Incompatible("stored history response is inconsistent".into()));
	}

	Ok(HistoryEntry {
		history_item_id: required("history_item_id")?.to_owned(),
		turn_id: required("turn_id")?.to_owned(),
		runtime_session_id: required("runtime_session_id")?.to_owned(),
		turn_role: turn_role_from_sql(required("turn_role")?)?,
		possible_side_effects: side_effect_from_sql(required("possible_side_effects")?)?,
		kind,
		status: item_status_from_sql(required("status")?)?,
		inline_text,
		blob_hash,
		blob_byte_length,
		media_type,
		metadata,
		artifact,
		revision: response_revision(response, "history_item")?,
	})
}

fn parse_initial_quick_task_admission_response(
	response: &[u8],
	replayed: bool,
	request: &AdmitInitialQuickTaskTurn,
	inline_text: Option<&str>,
	blob_hash: Option<&str>,
	metadata: &Value,
) -> Result<InitialQuickTaskTurnAdmissionOutcome, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("initial Quick Task admission response is invalid".into())
	})?;
	let classification =
		document.get("classification").and_then(Value::as_str).ok_or_else(|| {
			StoreError::Incompatible("initial Quick Task admission classification is absent".into())
		})?;
	let effect = document.get("effect").filter(|value| value.is_object()).ok_or_else(|| {
		StoreError::Incompatible("initial Quick Task admission effect is invalid".into())
	})?;
	validate_exact_effect_digest(effect)?;
	if classification == "stable_domain_rejection" {
		let rejection = match effect.get("code").and_then(Value::as_str) {
			Some("invalid_input") => InitialQuickTaskTurnAdmissionRejection::InvalidInput,
			Some("authority_unavailable") =>
				InitialQuickTaskTurnAdmissionRejection::AuthorityUnavailable,
			Some("initial_admission_conflict") =>
				InitialQuickTaskTurnAdmissionRejection::InitialAdmissionConflict,
			Some("message_blob_missing") =>
				InitialQuickTaskTurnAdmissionRejection::MessageBlobMissing,
			_ => {
				return Err(StoreError::Incompatible(
					"initial Quick Task admission rejection is unknown".into(),
				));
			},
		};
		if effect.get("operation").and_then(Value::as_str) != Some("admit_initial_quick_task_turn")
		{
			return Err(StoreError::Incompatible(
				"initial Quick Task admission rejection is cross-linked".into(),
			));
		}
		return Ok(InitialQuickTaskTurnAdmissionOutcome::Rejected { rejection, replayed });
	}
	if classification != "success" {
		return Err(StoreError::Incompatible(
			"initial Quick Task admission classification is unknown".into(),
		));
	}

	let message_request = &request.message;
	let turn = effect.get("turn").filter(|value| value.is_object()).ok_or_else(|| {
		StoreError::Incompatible("initial Quick Task Turn effect is absent".into())
	})?;
	let message = effect.get("message").filter(|value| value.is_object()).ok_or_else(|| {
		StoreError::Incompatible("initial Quick Task message effect is absent".into())
	})?;
	let routing_decision_id = effect
		.get("routing_decision_id")
		.and_then(Value::as_str)
		.filter(|value| is_canonical_uuid(value))
		.ok_or_else(|| {
			StoreError::Incompatible("initial Quick Task Routing Decision is invalid".into())
		})?;
	let optional_payload_is_exact = |value: &Value, key: &str, expected: Option<&str>| {
		value.get(key).is_some() && value.get(key).and_then(Value::as_str) == expected
	};
	let exact = effect.get("operation").and_then(Value::as_str)
		== Some("admit_initial_quick_task_turn")
		&& effect.get("kind").and_then(Value::as_str) == Some("initial_quick_task_turn_admission")
		&& effect.get("conversation_id").and_then(Value::as_str)
			== Some(message_request.conversation_id.as_str())
		&& effect.get("conversation_revision").and_then(Value::as_i64)
			== Some(request.expected_conversation_revision)
		&& effect.get("runtime_session_id").and_then(Value::as_str)
			== Some(message_request.runtime_session_id.as_str())
		&& effect.get("runtime_session_revision").and_then(Value::as_i64)
			== Some(request.expected_runtime_session_revision)
		&& effect.get("continuation_plan_id").and_then(Value::as_str)
			== Some(request.continuation_plan_id.as_str())
		&& effect.get("activity_sequence").and_then(Value::as_i64).is_some_and(|v| v > 0)
		&& effect.get("outbox_id").and_then(Value::as_i64).is_some_and(|v| v > 0)
		&& turn.get("turn_id").and_then(Value::as_str) == Some(message_request.turn_id.as_str())
		&& turn.get("conversation_id").and_then(Value::as_str)
			== Some(message_request.conversation_id.as_str())
		&& turn.get("runtime_session_id").and_then(Value::as_str)
			== Some(message_request.runtime_session_id.as_str())
		&& turn.get("sequence").and_then(Value::as_i64) == Some(1)
		&& turn.get("role").and_then(Value::as_str) == Some("user")
		&& turn.get("possible_side_effects").and_then(Value::as_str) == Some("unknown")
		&& turn.get("status").and_then(Value::as_str) == Some("active")
		&& turn.get("revision").and_then(Value::as_i64) == Some(1)
		&& message.get("history_item_id").and_then(Value::as_str)
			== Some(message_request.history_item_id.as_str())
		&& message.get("conversation_id").and_then(Value::as_str)
			== Some(message_request.conversation_id.as_str())
		&& message.get("history_position").and_then(Value::as_i64) == Some(1)
		&& message.get("turn_id").and_then(Value::as_str) == Some(message_request.turn_id.as_str())
		&& message.get("ordinal").and_then(Value::as_i64) == Some(0)
		&& message.get("kind").and_then(Value::as_str) == Some("message")
		&& message.get("status").and_then(Value::as_str) == Some("completed")
		&& optional_payload_is_exact(message, "inline_text", inline_text)
		&& optional_payload_is_exact(message, "blob_hash", blob_hash)
		&& message.get("media_type").and_then(Value::as_str)
			== Some(message_request.media_type.as_str())
		&& message.get("metadata") == Some(metadata)
		&& message.get("revision").and_then(Value::as_i64) == Some(1);
	if !exact {
		return Err(StoreError::Incompatible(
			"initial Quick Task admission success is cross-linked".into(),
		));
	}
	let readback = InitialQuickTaskTurnAdmissionReadback {
		routing_decision_id: routing_decision_id.to_owned(),
		continuation_plan_id: request.continuation_plan_id.clone(),
		turn: TurnReservationReadback {
			turn_id: message_request.turn_id.clone(),
			sequence: 1,
			status: TurnStatus::Active,
			revision: 1,
		},
		history_item_id: message_request.history_item_id.clone(),
	};
	Ok(if replayed {
		InitialQuickTaskTurnAdmissionOutcome::Replayed(readback)
	} else {
		InitialQuickTaskTurnAdmissionOutcome::Fresh(readback)
	})
}

fn parse_routing_successor_response(
	response: &[u8],
	replayed: bool,
	request: &CreateQuickTaskRoutingSuccessor,
) -> Result<QuickTaskRoutingSuccessorOutcome, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("routing Conversation successor response is invalid".into())
	})?;
	let classification =
		document.get("classification").and_then(Value::as_str).ok_or_else(|| {
			StoreError::Incompatible(
				"routing Conversation successor classification is absent".into(),
			)
		})?;
	let effect = document.get("effect").filter(|value| value.is_object()).ok_or_else(|| {
		StoreError::Incompatible("routing Conversation successor effect is absent".into())
	})?;
	validate_exact_effect_digest(effect)?;
	if effect.get("operation").and_then(Value::as_str)
		!= Some("create_quick_task_routing_successor")
	{
		return Err(StoreError::Incompatible(
			"routing Conversation successor response is cross-linked".into(),
		));
	}
	if classification == "stable_domain_rejection" {
		let code = effect.get("rejection").and_then(Value::as_str).ok_or_else(|| {
			StoreError::Incompatible("routing successor rejection is absent".into())
		})?;
		if !matches!(
			code,
			"malformed_input"
				| "source_conversation_mismatch"
				| "routing_successor_already_exists"
				| "routing_successor_forbidden"
		) {
			return Err(StoreError::Incompatible("routing successor rejection is unknown".into()));
		}
		return Ok(QuickTaskRoutingSuccessorOutcome::Rejected { code: code.to_owned(), replayed });
	}
	if classification != "success" {
		return Err(StoreError::Incompatible("routing successor classification is unknown".into()));
	}
	let source_id = effect.get("source_conversation_id").and_then(Value::as_str);
	let successor_id = effect.get("successor_conversation_id").and_then(Value::as_str);
	let source_decision_id = effect.get("source_routing_decision_id").and_then(Value::as_str);
	let source_revision = effect.get("source_conversation_revision").and_then(Value::as_i64);
	let successor_revision = effect.get("successor_conversation_revision").and_then(Value::as_i64);
	if source_id != Some(request.source_conversation_id.as_str())
		|| source_revision != request.expected_source_revision.checked_add(1)
		|| successor_revision != Some(1)
		|| successor_id.is_none_or(|value| !is_canonical_uuid(value))
		|| source_decision_id.is_none_or(|value| !is_canonical_uuid(value))
	{
		return Err(StoreError::Incompatible("routing successor success is cross-linked".into()));
	}
	let successor = QuickTaskRoutingSuccessor {
		source_conversation_id: request.source_conversation_id.clone(),
		source_revision: source_revision.expect("validated source revision is present"),
		successor_conversation_id: ConversationId::new(
			successor_id.expect("validated successor identity is present").to_owned(),
		)
		.map_err(|_| StoreError::Incompatible("routing successor identity is invalid".into()))?,
		successor_revision: 1,
		source_routing_decision_id: source_decision_id
			.expect("validated source decision is present")
			.to_owned(),
	};
	Ok(if replayed {
		QuickTaskRoutingSuccessorOutcome::Replayed(successor)
	} else {
		QuickTaskRoutingSuccessorOutcome::Fresh(successor)
	})
}

fn conversation_from_response(response: &Value) -> Result<StoredConversation, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("conversation") {
		return Err(StoreError::Incompatible(
			"stored Conversation response kind is invalid".into(),
		));
	}

	let conversation_id =
		response.get("conversation_id").and_then(Value::as_str).ok_or_else(|| {
			StoreError::Incompatible("stored Conversation response is incomplete".into())
		})?;
	let title = response.get("title").and_then(Value::as_str).ok_or_else(|| {
		StoreError::Incompatible("stored Conversation response is incomplete".into())
	})?;

	Ok(StoredConversation {
		conversation_id: ConversationId::new(conversation_id.to_owned()).map_err(|_| {
			StoreError::Incompatible("stored Conversation response identity is invalid".into())
		})?,
		title: title.to_owned(),
		revision: response_revision(response, "conversation")?,
	})
}

#[cfg(debug_assertions)]
async fn blob_publish_test_barrier() -> Result<(), StoreError> {
	let Ok(root) = env::var("DECODEX_TEST_BLOB_RESTART_SYNC") else {
		return Ok(());
	};
	let root = PathBuf::from(root);

	fs::write(root.join("published"), b"published")
		.map_err(|_| StoreError::Incompatible("test blob publication barrier failed".into()))?;

	for _ in 0..3_000 {
		if root.join("continue").exists() {
			return Ok(());
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	Err(StoreError::Incompatible("test blob publication barrier timed out".into()))
}

#[cfg(not(debug_assertions))]
async fn blob_publish_test_barrier() -> Result<(), StoreError> {
	Ok(())
}

async fn issue_history_cursor(
	transaction: &Transaction<'_>,
	conversation_id: &ConversationId,
	parent: Option<&HistoryCursor>,
	page_size: i32,
) -> Result<HistoryCursor, StoreError> {
	let parent_token = parent.map(|cursor| cursor.token.as_str());
	let row = transaction
		.query_one(
			"SELECT decodex.issue_history_cursor( \
			 $1::text::uuid,$2::text::uuid,$3)::text",
			&[&conversation_id.as_str(), &parent_token, &page_size],
		)
		.await?;

	HistoryCursor::issued(row.get(0))
}

async fn insert_verified_blob(
	transaction: &Transaction<'_>,
	hash: BlobHash,
	byte_length: i64,
) -> Result<(), StoreError> {
	let row = transaction
		.query_one(
			"WITH write_time AS (SELECT clock_timestamp() AS value), inserted AS ( \
		 INSERT INTO decodex.blob_objects (blob_hash, byte_length, verified_at, created_at) \
		 SELECT $1, $2, value, value FROM write_time ON CONFLICT (blob_hash) DO NOTHING \
		 RETURNING byte_length) \
		 SELECT byte_length FROM inserted UNION ALL \
		 SELECT byte_length FROM decodex.blob_objects WHERE blob_hash = $1 LIMIT 1",
			&[&hash.to_hex(), &byte_length],
		)
		.await?;
	let stored_length: i64 = row.get(0);

	if stored_length != byte_length {
		return Err(StoreError::Incompatible(
			"blob metadata conflicts with content address".into(),
		));
	}

	Ok(())
}

async fn reserve_conversation_command(
	client: &mut Client,
	command: &CommandIdentity,
	operation: &'static str,
	identity: (&'static str, &str, &str),
	expected_revision: Option<i64>,
	payload: Option<(BlobHash, i64)>,
) -> Result<CommandClaim, StoreError> {
	let (project_scope, scope_id, entity_id) = identity;

	accounts::reserve_command(
		client,
		command,
		&CommandDescriptor {
			protocol_version: "decodex/store-command/1",
			operation,
			project_scope,
			scope_id: scope_id.into(),
			entity_id: entity_id.into(),
			expected_revision,
			payload_hash: payload.map(|(hash, _)| hash.to_hex()),
			payload_length: payload.map(|(_, length)| length),
		},
	)
	.await
}

async fn insert_context_pack_sources(
	transaction: &Transaction<'_>,
	request: &PersistContextPack,
	pack: &ContextPack,
) -> Result<(), StoreError> {
	for (position, source) in pack.source_manifest().iter().enumerate() {
		let (artifact_id, artifact_revision) =
			source.artifact_reference().map_or((None, None), |(id, revision)| {
				(Some(id.as_str()), i64::try_from(revision).ok())
			});

		if let (Some(artifact_id), Some(artifact_revision)) = (artifact_id, artifact_revision) {
			let valid: bool = transaction
				.query_one(
					"SELECT EXISTS (SELECT 1 FROM decodex.artifact_revisions ar \
				 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
				 WHERE ar.artifact_id=$1::text::uuid AND ar.conversation_id=$2::text::uuid \
				 AND ar.revision=$3 AND ar.blob_hash=$4 AND bo.byte_length=$5)",
					&[
						&artifact_id,
						&pack.conversation_id().as_str(),
						&artifact_revision,
						&source.content_digest().to_hex(),
						&i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX),
					],
				)
				.await?
				.get(0);

			if !valid {
				return Err(StoreError::InvalidInput(
					"Context Pack Artifact provenance is invalid",
				));
			}
		}

		transaction
			.execute(
				"INSERT INTO decodex.context_pack_sources \
			 (context_pack_id, conversation_id, position, kind, source_id, source_revision, \
			 content_digest, original_byte_length, included_byte_length, included_digest, \
			 disposition, artifact_id, artifact_revision) \
			 VALUES ($1::text::uuid, $2::text::uuid, $3, $4::text::decodex.context_source_kind, \
			 $5, $6, $7, $8, $9, $10, $11::text::decodex.context_source_disposition, $12::text::uuid, $13)",
				&[
					&request.context_pack_id,
					&pack.conversation_id().as_str(),
					&i32::try_from(position).unwrap_or(i32::MAX),
					&context_source_sql(source.kind()),
					&source.source_id(),
					&i64::try_from(source.revision()).unwrap_or(i64::MAX),
					&source.content_digest().to_hex(),
					&i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX),
					&i64::try_from(source.included_byte_length()).unwrap_or(i64::MAX),
					&source.included_digest().to_hex(),
					&context_disposition_sql(source.disposition()),
					&artifact_id,
					&artifact_revision,
				],
			)
			.await?;
	}

	Ok(())
}

async fn ensure_turn(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
) -> Result<(), StoreError> {
	let inserted = transaction
		.execute(
			"INSERT INTO decodex.turns \
		 (turn_id, conversation_id, runtime_session_id, sequence, role, possible_side_effects) \
		 VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4, \
		 $5::text::decodex.turn_role, $6::text::decodex.side_effect_state) \
		 ON CONFLICT (turn_id) DO NOTHING",
			&[
				&mutation.turn_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.runtime_session_id.as_str(),
				&mutation.turn_sequence,
				&turn_role_sql(mutation.turn_role),
				&side_effect_sql(mutation.possible_side_effects),
			],
		)
		.await?;

	if inserted == 1 {
		return Ok(());
	}

	let matches: bool = transaction
		.query_one(
			"SELECT conversation_id = $2::text::uuid AND runtime_session_id = $3::text::uuid \
		 AND sequence = $4 AND role = $5::text::decodex.turn_role \
		 AND possible_side_effects = $6::text::decodex.side_effect_state \
		 FROM decodex.turns WHERE turn_id = $1::text::uuid FOR UPDATE",
			&[
				&mutation.turn_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.runtime_session_id.as_str(),
				&mutation.turn_sequence,
				&turn_role_sql(mutation.turn_role),
				&side_effect_sql(mutation.possible_side_effects),
			],
		)
		.await?
		.get(0);

	if matches { Ok(()) } else { Err(StoreError::IdempotencyConflict) }
}

async fn insert_history_item(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
) -> Result<i64, StoreError> {
	let inline = blob.is_none().then_some(mutation.text.as_str());
	let blob_hash = blob.map(|(hash, _)| hash.to_hex());
	let metadata = history_metadata_json(&mutation.metadata)?;
	let (artifact_id, artifact_revision) = mutation
		.artifact
		.as_ref()
		.map_or((None, None), |(id, revision)| (Some(id.as_str()), Some(*revision)));
	let row = transaction
		.query_opt(
			"INSERT INTO decodex.history_items \
			 (history_item_id, conversation_id, history_position, turn_id, ordinal, kind, status, \
			 inline_text, blob_hash, media_type, metadata, artifact_id, artifact_revision) \
			 VALUES ($1::text::uuid, $2::text::uuid, 1, $3::text::uuid, $4, \
			 $5::text::decodex.history_item_kind, $6::text::decodex.history_item_status, \
			 $7, $8, $9, $10, $11::text::uuid, $12) \
			 ON CONFLICT DO NOTHING RETURNING revision",
			&[
				&mutation.history_item_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.turn_id.as_str(),
				&mutation.ordinal,
				&item_kind_sql(mutation.kind),
				&item_status_sql(mutation.status),
				&inline,
				&blob_hash,
				&mutation.media_type.as_str(),
				&metadata,
				&artifact_id,
				&artifact_revision,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	Err(history_revision_error(transaction, mutation, None).await?)
}

async fn update_history_item(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
	expected: i64,
) -> Result<i64, StoreError> {
	let inline = blob.is_none().then_some(mutation.text.as_str());
	let blob_hash = blob.map(|(hash, _)| hash.to_hex());
	let metadata = history_metadata_json(&mutation.metadata)?;
	let (artifact_id, artifact_revision) = mutation
		.artifact
		.as_ref()
		.map_or((None, None), |(id, revision)| (Some(id.as_str()), Some(*revision)));
	let row = transaction
		.query_opt(
			"UPDATE decodex.history_items SET status = $5::text::decodex.history_item_status, \
		 inline_text = $6, blob_hash = $7, media_type = $8, metadata = $9, \
		 revision = revision + 1, updated_at = clock_timestamp() \
			 WHERE history_item_id = $1::text::uuid AND turn_id = $2::text::uuid AND ordinal = $3 \
			 AND kind = $4::text::decodex.history_item_kind AND revision = $10 \
			 AND artifact_id IS NOT DISTINCT FROM $11::text::uuid \
			 AND artifact_revision IS NOT DISTINCT FROM $12 RETURNING revision",
			&[
				&mutation.history_item_id.as_str(),
				&mutation.turn_id.as_str(),
				&mutation.ordinal,
				&item_kind_sql(mutation.kind),
				&item_status_sql(mutation.status),
				&inline,
				&blob_hash,
				&mutation.media_type.as_str(),
				&metadata,
				&expected,
				&artifact_id,
				&artifact_revision,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	Err(history_revision_error(transaction, mutation, Some(expected)).await?)
}

async fn history_revision_error(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	expected: Option<i64>,
) -> Result<StoreError, StoreError> {
	let actual = transaction
		.query_opt(
			"SELECT revision FROM decodex.history_items WHERE history_item_id = $1::text::uuid",
			&[&mutation.history_item_id.as_str()],
		)
		.await?
		.map(|row| row.get(0));

	Ok(StoreError::RevisionConflict {
		entity: format!("history_item/{}", mutation.history_item_id),
		expected,
		actual,
	})
}

async fn conversation_revision(
	transaction: &Transaction<'_>,
	id: &ConversationId,
) -> Result<Option<i64>, StoreError> {
	Ok(transaction
		.query_opt(
			"SELECT revision FROM decodex.conversations WHERE conversation_id = $1::text::uuid",
			&[&id.as_str()],
		)
		.await?
		.map(|row| row.get(0)))
}

#[cfg(test)]
mod tests {
	use crate::{StoreError, conversations};

	#[test]
	fn history_cursor_round_trips_and_rejects_ambiguous_forms() {
		let cursor =
			conversations::HistoryCursor::issued("00000000-0000-4000-8000-000000000009".into())
				.unwrap();

		assert_eq!(conversations::HistoryCursor::parse(&cursor.encode()).unwrap(), cursor);
		assert!(conversations::HistoryCursor::parse("v1:bad").is_err());
		assert!(
			conversations::HistoryCursor::parse("v1:00000000-0000-4000-8000-000000000009:extra")
				.is_err()
		);
		assert!(
			conversations::HistoryCursor::parse("v2:00000000-0000-4000-8000-000000000009").is_err()
		);
	}

	#[test]
	fn stored_history_response_revalidates_the_core_projection() {
		let valid = serde_json::json!({
			"kind":"history_item",
			"history_item_id":"44000000-0000-4000-8000-000000000009",
			"turn_id":"45000000-0000-4000-8000-000000000009",
			"runtime_session_id":"42000000-0000-4000-8000-000000000009",
			"turn_role":"assistant",
			"possible_side_effects":"none",
			"item_kind":"message",
			"status":"completed",
			"inline_text":"ok",
			"blob_hash":null,
			"blob_byte_length":null,
			"media_type":"application/json",
			"metadata":{"note":"secret sauce","summary":"token budget","visible":true},
			"artifact":null,
			"revision":1
		});

		assert!(conversations::history_entry_from_response(&valid).is_ok());

		for metadata in [
			serde_json::json!({"token":"ordinary"}),
			serde_json::json!({"note":"secret=abcd"}),
			serde_json::json!({"nested":{"unsafe":true}}),
		] {
			let mut malformed = valid.clone();

			malformed["metadata"] = metadata;

			assert!(matches!(
				conversations::history_entry_from_response(&malformed),
				Err(StoreError::Incompatible(_))
			));
		}
	}
}
