//! Typed, fail-closed Codex app-server adapter foundation.
//!
//! This crate owns protocol decoding, capability evidence, and redaction. Private
//! process supervision belongs to the runtime composition owner. This crate defines a pure
//! ordinary Quick Task contract but deliberately has no child-launch or production
//! turn-dispatch API. XY-1304 governs only later automatic cross-account fallback and
//! all-depleted wake.
//!
//! Product runner capacity and PostgreSQL authorization are deliberately absent:
//!
//! ```compile_fail
//! use decodex_codex::{AppServerCommand, CredentialVault, ReadOnlyProbe, RunnerCapacity};
//!
//! let _ = RunnerCapacity::daemon();
//! ```

#[doc(hidden)] pub mod protocol;
#[doc(hidden)] pub mod schema;

mod capability;
mod dispatch;
mod event;
mod quick_task;
mod reset_card;

pub use self::{
	capability::{
		Capability, CapabilityCache, CapabilityContradiction, CapabilityProfile, CapabilityState,
		DegradedReason, LiveMethodOutcome, MethodObservation, NegotiationError, UnavailableReason,
		UnsupportedReason,
	},
	dispatch::{
		AUTOMATIC_FALLBACK_WAKE_GATE, DispatchDenied, DispatchGate, DispatchOperation,
		DispatchPath, LIVE_ROUTING_GATE,
	},
	event::{
		CollaborationActivityKind, CollaborationTool, CollaborationToolCall,
		CollaborationToolStatus, EventDecodeError, MAX_QUICK_TASK_MESSAGE_DELTA_BYTES,
		NormalizedEvent, NormalizedItemKind, OpaqueId, QuickTaskMessageDelta,
		QuickTaskMessageDeltaError, RunLocalActor, ThreadStatus, TurnStatus, normalize_event,
		project_quick_task_message_delta,
	},
	protocol::{
		ArchiveReconciliationOutcome, ArchiveUnverifiedReason, BuildId, DecodexThreadSearchTerm,
		ExactThreadFacts, ExactThreadId, ExactThreadListFilter, ExactThreadListResult,
		ExactThreadReadResult, LossyThreadHistory, MAX_EXACT_THREAD_ID_BYTES,
		MAX_EXACT_THREAD_LIST_RESULTS, MAX_THREAD_CWD_BYTES, MAX_THREAD_PROVENANCE_BYTES,
		MAX_THREAD_SEARCH_TERM_BYTES, MAX_THREAD_TITLE_BYTES, ThreadArchivedFilter,
		ThreadCreatedAt, ThreadCwd, ThreadId, ThreadProvenance, ThreadSummary, ThreadTitle,
	},
	quick_task::{
		ExactTurnId, MAX_EXACT_TURN_ID_BYTES, MAX_QUICK_TASK_INPUT_BYTES,
		MAX_QUICK_TASK_INPUT_ITEMS, MAX_QUICK_TASK_INSTRUCTIONS_BYTES, MAX_QUICK_TASK_MODEL_BYTES,
		MAX_QUICK_TASK_REASONING_EFFORT_BYTES, MAX_QUICK_TASK_RESPONSE_BYTES,
		MAX_QUICK_TASK_TEXT_BYTES, QuickTaskContractError, QuickTaskInstructions, QuickTaskMethod,
		QuickTaskModel, QuickTaskNotification, QuickTaskReasoningEffort, QuickTaskText,
		QuickTaskThreadArchiveRequest, QuickTaskThreadArchiveResponse,
		QuickTaskThreadResumeRequest, QuickTaskThreadResumeResponse, QuickTaskThreadStartRequest,
		QuickTaskThreadStartResponse, QuickTaskTurnInput, QuickTaskTurnInterruptRequest,
		QuickTaskTurnInterruptResponse, QuickTaskTurnStartRequest, QuickTaskTurnStartResponse,
		QuickTaskTurnStatus, decode_quick_task_thread_archive_response,
		decode_quick_task_thread_resume_response, decode_quick_task_thread_start_response,
		decode_quick_task_turn_interrupt_response, decode_quick_task_turn_start_response,
	},
	reset_card::{
		AccountRateLimitObservation, AvailableResetCardObservation, ExactResetCreditId,
		MAX_EXACT_RESET_CREDIT_ID_BYTES, MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES,
		MAX_RESET_CARDS_PER_INVENTORY, RESET_CARD_CONSUME_METHOD, RESET_CARD_READ_METHOD,
		ResetCardCapabilityProfile, ResetCardCapabilityState, ResetCardConsumeParams,
		ResetCardConsumeResult, ResetCardIdempotencyKey, ResetCardInventory,
		ResetCardProtocolError, ResetCardResolutionError, decode_reset_card_consume_result,
		decode_reset_card_inventory,
	},
	schema::{
		ACCEPTED_SCHEMA_RECEIPT, QuickTaskSchemaError, QuickTaskSchemaRequirement,
		REQUIRED_NOTIFICATION_METHODS, REQUIRED_REQUEST_METHODS, SchemaContract, SchemaMarker,
	},
};

use decodex_core::{Availability, ConversationRuntime};

/// Production conversation I/O remains unavailable because no composition root owns it.
pub const LIVE_DISPATCH_UNAVAILABLE: &str = "Codex production app-server I/O is not composed";
/// Stable composition-root reason retained while live execution is unavailable.
pub const NOT_IMPLEMENTED: &str = LIVE_DISPATCH_UNAVAILABLE;

/// Continuation-home policy selected by this infrastructure owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexContinuity {
	/// The user's ordinary shared `~/.codex`, never a per-run home.
	SharedNormalHome,
}

/// The bounded Codex foundation selected by the vNext composition root.
#[derive(Clone, Copy, Debug, Default)]
pub struct CodexAdapter;
impl CodexAdapter {
	/// Construct the foundation adapter.
	pub const fn new() -> Self {
		Self
	}

	/// Construct the adapter in its current live-dispatch-unavailable state.
	pub const fn unavailable() -> Self {
		Self
	}

	/// Report the continuation policy owned by this adapter.
	pub const fn continuity(self) -> CodexContinuity {
		CodexContinuity::SharedNormalHome
	}

	/// Return the hard live-dispatch guard.
	pub const fn dispatch_gate(self) -> DispatchGate {
		DispatchGate::production_io_unavailable()
	}
}

impl ConversationRuntime for CodexAdapter {
	fn availability(&self) -> Availability {
		Availability::Unavailable { reason: LIVE_DISPATCH_UNAVAILABLE }
	}
}

#[cfg(test)]
mod tests {
	use crate::{CodexAdapter, CodexContinuity, LIVE_DISPATCH_UNAVAILABLE};
	use decodex_core::{Availability, ConversationRuntime};

	#[test]
	fn foundation_preserves_shared_home_but_live_execution_is_unavailable() {
		let adapter = CodexAdapter::new();

		assert_eq!(adapter.continuity(), CodexContinuity::SharedNormalHome);
		assert_eq!(
			adapter.availability(),
			Availability::Unavailable { reason: LIVE_DISPATCH_UNAVAILABLE }
		);
	}
}
