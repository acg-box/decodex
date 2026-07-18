//! Pure typed app-server facts for V15 experiment persistence.
//!
//! This module performs no persistence and owns no retry, recovery, adoption, or negative
//! observation policy. Its Rust values report app-server facts only; constructing one does not
//! prove PostgreSQL provenance or authorize routing, dispatch, or production execution.

use crate::{ExactThreadId, ThreadCwd, ThreadProvenance, ThreadTitle};

/// Exact successful `thread/start` response fact supplied to the Decodex persistence owner.
#[derive(Debug)]
pub struct TypedThreadStartResponse {
	/// Opaque response identity reported by the exact successful app-server call.
	pub response_id: String,
	/// Exact thread identity returned by that successful call.
	pub thread_id: ExactThreadId,
	/// Thread title returned by the app server for lineage comparison by the persistence owner.
	pub title: ThreadTitle,
	/// Thread working directory returned by the app server for lineage comparison.
	pub cwd: ThreadCwd,
	/// Retained provenance marker reported for the created thread.
	pub provenance: ThreadProvenance,
	/// Whether the app server reported the created thread as ephemeral.
	pub ephemeral: bool,
}

/// Exact positive observation emitted by one supported app-server surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositiveExperimentFactKind {
	/// An exact matching item was observed in a `thread/list` response.
	ThreadListItem,
	/// An exact matching item was observed in a `thread/read` response.
	ThreadReadItem,
	/// A start event was observed for the exact thread.
	TurnStartedEvent,
	/// A terminal event was observed for the exact thread; this fact alone does not infer experiment completion.
	TurnTerminalEvent,
	/// An exact message item was observed for the thread.
	MessageItem,
}

/// Positive fact tied to an exact owned thread. Missing facts have no representation here.
#[derive(Debug)]
pub struct PositiveExperimentFact {
	/// Supported app-server surface that supplied the positive observation.
	pub kind: PositiveExperimentFactKind,
	/// Exact observed thread identity; it grants no authority to search for or adopt another thread.
	pub thread_id: ExactThreadId,
	/// Opaque identity of the observed app-server item or event.
	pub source_id: String,
	/// Retained experiment marker observed on the exact thread.
	pub marker: ThreadProvenance,
}
