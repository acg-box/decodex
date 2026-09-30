//! Typed, fail-closed Codex and account backend adapter foundation.
//!
//! This crate owns protocol decoding, capability evidence, and redaction. Private
//! process supervision belongs to the runtime composition owner. The app-server client
//! supplies a multiplexed transport and explicit process shutdown; the runtime owner
//! supplies dispatch authorization and environment. XY-1304 governs only later automatic
//! cross-account fallback and all-depleted wake.
//!
//! Product runner capacity and durable-store authorization are deliberately absent:
//!
//! ```compile_fail
//! use decodex_codex::AppServerCommand;
//! ```
//!
//! ```compile_fail
//! use decodex_codex::CredentialVault;
//! ```
//!
//! ```compile_fail
//! use decodex_codex::ReadOnlyProbe;
//! ```
//!
//! ```compile_fail
//! use decodex_codex::RunnerCapacity;
//! ```

#[doc(hidden)] pub mod protocol;
#[doc(hidden)] pub mod schema;

pub mod app_server_client;

mod account_api;
mod capability;
mod conversation;
mod event;
pub mod guardian;
mod response_usage;
mod usage;

pub use response_usage::{ResponseUsage, ResponseUsageMetadata, decode_response_usage};
mod account_api_banner;
pub use account_api_banner::{
	AccountApiBanner, AccountApiBannerAction, AccountApiBannerCta, AccountApiBannerState,
};
pub use usage::{ThreadTokenUsage, TokenUsageBreakdown};

pub use self::{
	account_api::{
		AccountApiConsumeOutcome, AccountApiDailyUsage, AccountApiProfile, AccountApiProtocolError,
		AccountApiQuotaWindow, AccountApiRecoveryContext, AccountApiResetCredit,
		AccountApiResetCredits, AccountApiUsage, ExactResetCreditId, MAX_ACCOUNT_API_BODY_BYTES,
		MAX_EXACT_RESET_CREDIT_ID_BYTES, MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES,
		MAX_RESET_CARDS_PER_INVENTORY, ResetCardIdempotencyKey, decode_account_api_consume,
		decode_account_api_profile, decode_account_api_reset_credits, decode_account_api_usage,
	},
	capability::{
		Capability, CapabilityCache, CapabilityContradiction, CapabilityProfile, CapabilityState,
		DegradedReason, LiveMethodOutcome, MethodObservation, NegotiationError, UnavailableReason,
		UnsupportedReason,
	},
	conversation::{
		ConversationContractError, ConversationInstructions, ConversationMethod, ConversationModel,
		ConversationNotification, ConversationReasoningEffort, ConversationText,
		ConversationThreadArchiveRequest, ConversationThreadArchiveResponse,
		ConversationThreadResumeRequest, ConversationThreadResumeResponse,
		ConversationThreadStartRequest, ConversationThreadStartResponse, ConversationTurnInput,
		ConversationTurnInterruptRequest, ConversationTurnInterruptResponse,
		ConversationTurnStartRequest, ConversationTurnStartResponse, ConversationTurnStatus,
		ExactTurnId, MAX_CONVERSATION_INPUT_BYTES, MAX_CONVERSATION_INPUT_ITEMS,
		MAX_CONVERSATION_INSTRUCTIONS_BYTES, MAX_CONVERSATION_MODEL_BYTES,
		MAX_CONVERSATION_MODEL_PROVIDER_BYTES, MAX_CONVERSATION_REASONING_EFFORT_BYTES,
		MAX_CONVERSATION_RESPONSE_BYTES, MAX_CONVERSATION_TEXT_BYTES, MAX_EXACT_TURN_ID_BYTES,
		decode_conversation_thread_archive_response, decode_conversation_thread_resume_response,
		decode_conversation_thread_start_response, decode_conversation_turn_interrupt_response,
		decode_conversation_turn_start_response,
	},
	event::{
		CollaborationActivityKind, CollaborationTool, CollaborationToolCall,
		CollaborationToolStatus, ConversationMessageDelta, ConversationMessageDeltaError,
		EventDecodeError, MAX_CONVERSATION_MESSAGE_DELTA_BYTES, NormalizedEvent,
		NormalizedItemKind, OpaqueId, RunLocalActor, ThreadStatus, TurnStatus, normalize_event,
		project_conversation_message_delta,
	},
	protocol::{
		ArchiveReconciliationOutcome, ArchiveUnverifiedReason, BuildId, DecodexThreadSearchTerm,
		ExactSubmittedTurnReadback, ExactThreadFacts, ExactThreadId, ExactThreadListFilter,
		ExactThreadListResult, ExactThreadReadResult, LossyThreadHistory,
		MAX_EXACT_THREAD_ID_BYTES, MAX_EXACT_THREAD_LIST_RESULTS, MAX_EXACT_THREAD_READ_ITEMS,
		MAX_EXACT_THREAD_READ_TURNS, MAX_EXACT_TURN_ASSISTANT_BYTES, MAX_THREAD_CWD_BYTES,
		MAX_THREAD_PROVENANCE_BYTES, MAX_THREAD_SEARCH_TERM_BYTES, MAX_THREAD_TITLE_BYTES,
		ThreadArchivedFilter, ThreadCreatedAt, ThreadCwd, ThreadId, ThreadProvenance,
		ThreadSummary, ThreadTitle,
	},
	schema::{
		ACCEPTED_SCHEMA_RECEIPT, ConversationSchemaError, ConversationSchemaRequirement,
		REQUIRED_NOTIFICATION_METHODS, REQUIRED_REQUEST_METHODS, SchemaContract, SchemaMarker,
	},
};
