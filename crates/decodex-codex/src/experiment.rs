//! Pure typed app-server facts for V15 experiment persistence.
//!
//! This module performs no persistence and owns no retry, recovery, adoption, or negative
//! observation policy.

use crate::{ExactThreadId, ThreadCwd, ThreadProvenance, ThreadTitle};

/// Exact successful `thread/start` response fact supplied to the Decodex persistence owner.
#[derive(Debug)]
pub struct TypedThreadStartResponse {
	pub response_id: String,
	pub thread_id: ExactThreadId,
	pub title: ThreadTitle,
	pub cwd: ThreadCwd,
	pub provenance: ThreadProvenance,
	pub ephemeral: bool,
}

/// Exact positive observation emitted by one supported app-server surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositiveExperimentFactKind {
	ThreadListItem,
	ThreadReadItem,
	TurnStartedEvent,
	TurnTerminalEvent,
	MessageItem,
}

/// Positive fact tied to an exact owned thread. Missing facts have no representation here.
#[derive(Debug)]
pub struct PositiveExperimentFact {
	pub kind: PositiveExperimentFactKind,
	pub thread_id: ExactThreadId,
	pub source_id: String,
	pub marker: ThreadProvenance,
}

