//! Execution Program state enums.

use serde::{Deserialize, Serialize};

/// Stage for one internal Execution Program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionProgramNodeStage {
	/// Research or evidence-gathering work.
	Research,
	/// Design or architecture-shaping work.
	Design,
	/// Normative specification work.
	Spec,
	/// Runtime schema, storage, or serialization work.
	Schema,
	/// Runtime implementation work.
	Runtime,
	/// Agent/plugin skill or integration work.
	Plugin,
	/// Evaluation, harness, or validation work.
	Eval,
	/// Review, PR, delivery, or handoff work.
	Handoff,
}
impl ExecutionProgramNodeStage {
	/// Stable machine-readable stage name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Research => "research",
			Self::Design => "design",
			Self::Spec => "spec",
			Self::Schema => "schema",
			Self::Runtime => "runtime",
			Self::Plugin => "plugin",
			Self::Eval => "eval",
			Self::Handoff => "handoff",
		}
	}
}

/// Dispatch intent for one internal Execution Program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionQueueIntent {
	/// The node is intentionally not ready for dispatch.
	NotReady,
	/// The node is ready for direct dispatch once mapped to a startable issue.
	ReadyToQueue,
	/// The node is retained in a ready-to-dispatch position.
	Queued,
	/// The node is already active in a lane.
	Active,
	/// The node is intentionally paused.
	Paused,
	/// The node is complete.
	Done,
	/// The node was canceled.
	Canceled,
}
impl ExecutionQueueIntent {
	/// Stable machine-readable dispatch-intent name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::NotReady => "not_ready",
			Self::ReadyToQueue => "ready_to_queue",
			Self::Queued => "queued",
			Self::Active => "active",
			Self::Paused => "paused",
			Self::Done => "done",
			Self::Canceled => "canceled",
		}
	}

	pub(in crate::execution_program) fn is_terminal(self) -> bool {
		matches!(self, Self::Done | Self::Canceled)
	}
}

/// Conflict-domain class for one program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionConflictDomainKind {
	/// A concrete file or path family.
	File,
	/// A module, crate, package, or app surface.
	Module,
	/// Local runtime or repository state.
	State,
	/// Credential, account, or auth-owned surface.
	Credentials,
	/// Tracker ownership, labels, comments, or workflow state.
	TrackerOwnership,
	/// Pull request, review, or landing surface.
	ReviewSurface,
}
impl ExecutionConflictDomainKind {
	/// Stable machine-readable conflict-domain class name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::File => "file",
			Self::Module => "module",
			Self::State => "state",
			Self::Credentials => "credentials",
			Self::TrackerOwnership => "tracker_ownership",
			Self::ReviewSurface => "review_surface",
		}
	}
}

/// Normalized readiness state for one program node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionReadinessState {
	/// Node is intentionally not ready yet.
	NotReady,
	/// Node is startable and may be dispatched directly.
	Ready,
	/// Node cannot start until a concrete blocker clears.
	Blocked,
	/// Node is intentionally paused.
	Paused,
	/// Node is already active.
	Active,
	/// Node is terminal.
	Completed,
	/// Node no longer matches the accepted contract.
	Stale,
}
impl ExecutionReadinessState {
	/// Stable machine-readable state name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::NotReady => "not_ready",
			Self::Ready => "ready",
			Self::Blocked => "blocked",
			Self::Paused => "paused",
			Self::Active => "active",
			Self::Completed => "completed",
			Self::Stale => "stale",
		}
	}
}

/// Durable lifecycle state for one program node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionProgramNodeLifecycleState {
	/// Node exists only as an internal plan and has no normal Linear issue yet.
	Planned,
	/// Node is mapped to a normal Linear issue but is intentionally held.
	Mapped,
	/// Node is ready for direct dispatch.
	Ready,
	/// Node already has a current lane.
	Active,
	/// Node is owned by a retained post-review lane.
	PostReview,
	/// Node is blocked by dependency, conflict, issue, or briefing evidence.
	Blocked,
	/// Node is stopped on human-required issue attention.
	NeedsAttention,
	/// Node is terminal.
	Completed,
	/// Node no longer matches the accepted contract.
	Stale,
	/// Node belongs to a superseded contract.
	Superseded,
}
impl ExecutionProgramNodeLifecycleState {
	/// Stable machine-readable state name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Planned => "planned",
			Self::Mapped => "mapped",
			Self::Ready => "ready",
			Self::Active => "active",
			Self::PostReview => "post_review",
			Self::Blocked => "blocked",
			Self::NeedsAttention => "needs_attention",
			Self::Completed => "completed",
			Self::Stale => "stale",
			Self::Superseded => "superseded",
		}
	}
}

/// Direct dispatch action allowed for a mapped node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionDispatchAction {
	/// Start this mapped node directly from the Execution Program scheduler.
	Dispatch,
}
impl ExecutionDispatchAction {
	/// Stable machine-readable direct-dispatch action.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Dispatch => "dispatch",
		}
	}
}
