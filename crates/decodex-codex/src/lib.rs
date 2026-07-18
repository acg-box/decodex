//! Typed, fail-closed Codex app-server adapter foundation.
//!
//! This crate owns protocol decoding, capability evidence, and redaction. Private
//! process supervision belongs to the runtime composition owner. This crate deliberately
//! has no child-launch or production turn-dispatch API while the XY-1304 live-routing gate
//! remains failed.
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
mod experiment;

pub use self::{
	capability::{
		Capability, CapabilityCache, CapabilityContradiction, CapabilityProfile, CapabilityState,
		DegradedReason, LiveMethodOutcome, MethodObservation, NegotiationError, UnavailableReason,
		UnsupportedReason,
	},
	dispatch::{DispatchDenied, DispatchGate, DispatchOperation, LIVE_ROUTING_GATE},
	experiment::{PositiveExperimentFact, PositiveExperimentFactKind, TypedThreadStartResponse},
	event::{
		CollaborationActivityKind, CollaborationTool, CollaborationToolCall,
		CollaborationToolStatus, EventDecodeError, NormalizedEvent, NormalizedItemKind, OpaqueId,
		RunLocalActor, ThreadStatus, TurnStatus, normalize_event,
	},
	protocol::{
		ArchiveReconciliationOutcome, ArchiveUnverifiedReason, BuildId, DecodexThreadSearchTerm,
		ExactThreadFacts, ExactThreadId, ExactThreadListFilter, ExactThreadListResult,
		ExactThreadReadResult, LossyThreadHistory, MAX_EXACT_THREAD_ID_BYTES,
		MAX_EXACT_THREAD_LIST_RESULTS, MAX_THREAD_CWD_BYTES, MAX_THREAD_PROVENANCE_BYTES,
		MAX_THREAD_SEARCH_TERM_BYTES, MAX_THREAD_TITLE_BYTES, ThreadArchivedFilter,
		ThreadCreatedAt, ThreadCwd, ThreadId, ThreadProvenance, ThreadSummary, ThreadTitle,
	},
	schema::{
		ACCEPTED_SCHEMA_RECEIPT, REQUIRED_NOTIFICATION_METHODS, REQUIRED_REQUEST_METHODS,
		SchemaContract, SchemaMarker,
	},
};

use decodex_core::{Availability, ConversationRuntime};

/// Live conversation execution remains unavailable until the separate gate passes.
pub const LIVE_DISPATCH_UNAVAILABLE: &str =
	"Codex live dispatch is disabled while the XY-1304 gate is failed";
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
		DispatchGate::failed_xy_1304()
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
