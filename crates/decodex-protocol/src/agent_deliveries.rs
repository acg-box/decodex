//! Bounded, read-only delivery timeline queries.
use serde::{Deserialize, Serialize};

use crate::EntityId;

/// Date bounds are inclusive at the start and exclusive at the end.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeliveryQuery {
	/// An owner or worker agent, or all agents.
	pub agent_id: Option<EntityId>,
	/// Inclusive start in Unix microseconds.
	pub after: Option<i64>,
	/// Exclusive end in Unix microseconds.
	pub before: Option<i64>,
	/// Maximum recent entries, between one and 100.
	pub limit: u16,
}

/// Evidence-backed delivery stage, not an inferred percentage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDeliveryKind {
	/// Created work that has no completed outcome yet.
	Requested,
	/// The owner or a worker is executing.
	Working,
	/// The work is explicitly waiting for a later check.
	Waiting,
	/// Execution ended without a confirmed outcome.
	Unconfirmed,
	/// A response is available for review.
	Result,
	/// The work was explicitly resolved.
	Accepted,
	/// A question, approval, decision, or blocked conversation needs user attention.
	NeedsInput,
	/// The latest execution failed.
	Failed,
	/// The latest execution was interrupted.
	Stopped,
}

/// A compact projection of one work item. No tool arguments or raw frames.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeliveryEntry {
	/// Stable work identity used for animation and deduplication.
	pub id: i64,
	/// Source conversation owner.
	pub work_id: String,
	/// Latest delivered request, or conversation title when no request is retained.
	pub title: String,
	/// Current assigned display slot; absent for a retired binding.
	pub slot: Option<u32>,
	/// Current work stage derived from persisted state.
	pub kind: AgentDeliveryKind,
	/// Full final response, or the current progress or attention message.
	pub summary: String,
	/// Direct links in the reported task result, never command output.
	pub artifacts: Vec<AgentDeliveryArtifact>,
	/// Recent meaningful changes, oldest first. Tool activity is excluded.
	pub updates: Vec<AgentDeliveryUpdate>,
	/// Last meaningful work update in Unix microseconds.
	pub updated_at_micros: i64,
}

/// One retained progress report, user choice, or result within the same work item.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeliveryUpdate {
	/// Exact persisted event identity.
	pub id: i64,
	/// Source-backed change category.
	pub kind: AgentDeliveryUpdateKind,
	/// Bounded public text, without transport envelopes or tool output.
	pub summary: String,
	/// Source observation time in Unix microseconds.
	pub created_at_micros: i64,
}

/// The meaning of one retained change; it does not imply task acceptance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDeliveryUpdateKind {
	/// An explicit task progress report.
	Progress,
	/// The user's answer to a question.
	Decision,
	/// A completed response or explicit resolution.
	Result,
}

/// A bounded, openable reference from a task's final answer.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeliveryArtifact {
	/// Short link label.
	pub title: String,
	/// HTTP(S) URL or an encoded absolute file URL.
	pub target: String,
}

/// Full time extent for the selected agent, independent of time bounds and page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeliveryExtent {
	/// Earliest meaningful work update in Unix microseconds, inclusive.
	pub first: i64,
	/// Latest meaningful work update in Unix microseconds, inclusive.
	pub last: i64,
}

/// Read-only feed response; absence is distinct from an empty journal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentDeliveriesResult {
	/// Newest matching entries first.
	Available {
		/// One entry per matching work item.
		entries: Vec<AgentDeliveryEntry>,
		/// Retained history bounds; absent only when the selected agent has no work.
		extent: Option<AgentDeliveryExtent>,
	},
	/// The retained source could not be read.
	Unavailable,
}
