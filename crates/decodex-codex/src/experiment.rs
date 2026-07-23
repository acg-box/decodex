//! Pure typed app-server facts for V22 retained-title experiment persistence.
//!
//! This module performs no persistence and owns no retry, recovery, adoption, or negative
//! observation policy. Its Rust values report app-server facts only; constructing one does not
//! prove PostgreSQL provenance or authorize routing, dispatch, or production execution.

use crate::{ExactThreadId, ThreadCwd, ThreadProvenance, ThreadTitle};

/// Exact serialized JSON-RPC request identity retained at an external-effect boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRpcRequestFact {
	/// Positive numeric JSON-RPC request identity.
	pub id: i64,
	/// Lowercase SHA-256 of the exact serialized request frame, including its newline.
	pub digest: String,
}

/// Exact raw JSON-RPC response identity retained at an external-effect boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRpcResponseFact {
	/// Positive numeric JSON-RPC response identity.
	pub id: i64,
	/// Lowercase SHA-256 of the exact raw response frame, including its newline.
	pub digest: String,
}

/// Exact successful `thread/start` response fact supplied to the Decodex persistence owner.
#[derive(Debug)]
pub struct TypedThreadStartResponse {
	/// Exact request identity and digest for this one `thread/start` effect.
	pub request: ExactRpcRequestFact,
	/// Exact response identity and digest from the successful `thread/start` call.
	pub response: ExactRpcResponseFact,
	/// Exact thread identity returned by that successful call.
	pub thread_id: ExactThreadId,
	/// Nullable name returned by `thread/start`. The pinned build must return no name.
	pub returned_name: Option<ThreadTitle>,
	/// Thread working directory returned by the app server for lineage comparison.
	pub cwd: ThreadCwd,
	/// Retained provenance marker reported for the created thread.
	pub provenance: ThreadProvenance,
	/// Whether the app server reported the created thread as ephemeral.
	pub ephemeral: bool,
}

/// Exact prepared `thread/name/set` request fact authorized by a fresh durable title fence.
#[derive(Debug)]
pub struct TypedThreadNameSetRequest {
	/// Exact serialized request identity and digest.
	pub request: ExactRpcRequestFact,
	/// Exact thread identity returned by the bound `thread/start` response.
	pub thread_id: ExactThreadId,
	/// Exact immutable prepared title sent to the app server.
	pub title: ThreadTitle,
}

/// Exact positive `thread/read` response that can attest a retained title.
#[derive(Debug)]
pub struct TypedRetainedTitleReadResponse {
	/// Exact serialized request identity and digest.
	pub request: ExactRpcRequestFact,
	/// Exact raw response identity and digest.
	pub response: ExactRpcResponseFact,
	/// Exact thread identity requested and returned.
	pub thread_id: ExactThreadId,
	/// Exact title returned by the positive readback.
	pub title: ThreadTitle,
	/// Exact working directory returned by the positive readback.
	pub cwd: ThreadCwd,
	/// Exact retained provenance marker returned by the positive readback.
	pub provenance: ThreadProvenance,
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
	/// A terminal event was observed for the exact thread; this fact alone does not infer
	/// experiment completion.
	TurnTerminalEvent,
	/// An exact message item was observed for the thread.
	MessageItem,
}

/// Positive fact tied to an exact owned thread. Missing facts have no representation here.
#[derive(Debug)]
pub struct PositiveExperimentFact {
	/// Supported app-server surface that supplied the positive observation.
	pub kind: PositiveExperimentFactKind,
	/// Exact observed thread identity; it grants no authority to search for or adopt another
	/// thread.
	pub thread_id: ExactThreadId,
	/// Opaque identity of the observed app-server item or event.
	pub source_id: String,
	/// Retained experiment marker observed on the exact thread.
	pub marker: ThreadProvenance,
}
