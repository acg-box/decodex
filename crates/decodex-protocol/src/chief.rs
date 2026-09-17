//! Read-only Chief work projection. This does not report runtime readiness.

use serde::{Deserialize, Serialize};

/// A bounded, selected view of one unresolved provider request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefRequestResult {
	/// The exact unresolved source request.
	Available {
		/// Persistent inbox identity.
		event_id: i64,
		/// Related work identity.
		work_id: String,
		/// Supported provider method.
		method: String,
		/// Selected request fields, bounded by the history text limit.
		request_json: crate::HistoryText,
	},
	/// The event is absent, resolved, unsupported, or cannot be safely projected.
	Unavailable,
}

/// A source-bound readable entry. Raw tool or credential frames are never projected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefHistoryEntryDto {
	/// Exact persistent event identity.
	pub id: i64,
	/// User, assistant, or system event category.
	pub kind: String,
	/// Bounded visible content.
	pub text: String,
	/// Observation time.
	pub created_at_micros: i64,
}

/// Latest bounded readable history for one work identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefHistoryResult {
	/// Verified visible entries; older history or shortened content is explicitly indicated.
	Available {
		/// Source-bound records.
		entries: Vec<ChiefHistoryEntryDto>,
		/// At least one older or shortened entry is not represented here.
		has_more: bool,
	},
	/// The work or source store cannot be read.
	Unavailable,
}

/// Explicit host-selected Chief execution policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefStartDto {
	/// Personal Chief work identity.
	pub root_id: crate::EntityId,
	/// Initial user request.
	pub prompt: crate::HistoryText,
	/// Exact selected provider model.
	pub model: crate::ConversationModel,
	/// Chief reasoning effort.
	pub effort: crate::ConversationReasoningEffort,
	/// Absolute execution directory.
	pub cwd: crate::ConversationWorkingDirectory,
	/// Optional explicit account; otherwise the service selects it.
	pub account_id: Option<crate::EntityId>,
	/// Runtime sandbox selected by the user.
	pub sandbox: ChiefSandboxDto,
}

/// Sandbox modes supported by the initial Chief product.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefSandboxDto {
	/// Permit observation only.
	ReadOnly,
	/// Permit edits within the selected workspace.
	WorkspaceWrite,
	/// User explicitly grants unrestricted local execution for this Chief context.
	FullAccess,
}

/// Explicit Chief operations. Graph judgments remain model-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefActionDto {
	/// Cancel one exact pending model-capacity retry.
	CancelCapacityRetry {
		/// Work that owns the pending retry.
		work_id: crate::EntityId,
		/// Persistent capacity failure event identity.
		event_id: i64,
	},
	/// Respond to one exact pending request after an explicit user decision.
	Respond {
		/// Related work identity.
		work_id: crate::EntityId,
		/// Exact inbox event identity.
		event_id: i64,
		/// Explicit provider response object.
		response_json: crate::HistoryText,
	},
	/// Start or reconnect the personal Chief and enqueue user input.
	Start(ChiefStartDto),
	/// Enqueue subsequent input to the existing Chief.
	Send {
		/// Personal Chief work identity.
		root_id: crate::EntityId,
		/// User input, preserved until processed.
		text: crate::HistoryText,
	},
	/// Stop the exact currently acknowledged work turn.
	Interrupt {
		/// Work identity, not a process ID.
		work_id: crate::EntityId,
		/// Exact turn observed by the caller; never interrupt a later turn silently.
		turn_id: crate::WireText,
	},
	/// Receive an explicitly submitted automation result.
	AutomationResult {
		/// Goal or work the source monitors.
		work_id: crate::EntityId,
		/// Source-owned event identity for deduplication.
		source_event_id: crate::WireText,
		/// Result content, treated as untrusted source data.
		payload: crate::HistoryText,
	},
}

/// Maximum work records in one complete snapshot.
pub const MAX_CHIEF_WORK_ITEMS: usize = 100;
/// Maximum dependency records in one complete snapshot.
pub const MAX_CHIEF_DEPENDENCIES: usize = 500;
/// Maximum undisposed inbox records in one complete snapshot.
pub const MAX_CHIEF_PENDING_EVENTS: usize = 100;
/// Maximum encoded snapshot size, below the transport frame bound.
pub const MAX_CHIEF_SNAPSHOT_BYTES: usize = 128 * 1024;

/// Durable work category, independent of a host project.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefWorkKindDto {
	/// A requested outcome.
	Goal,
	/// Work that supports an outcome.
	Task,
}

/// Explicit work judgment, separate from execution facts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefWorkStatusDto {
	/// No disposition yet.
	Open,
	/// Work is resolved.
	Resolved,
	/// Follow-up work is required.
	FollowUp,
	/// Waiting for a later check.
	Wait,
	/// A user decision is required.
	UserDecision,
}

/// Durable execution evidence. Idle does not mean the provider is ready.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefDispatchStateDto {
	/// No dispatch is claimed.
	Idle,
	/// A dispatch was claimed but has no acknowledgment.
	Dispatching,
	/// One exact turn was acknowledged.
	Running,
	/// Execution outcome is uncertain and requires reconciliation.
	Unknown,
}

/// Safe work metadata without task instructions or provider output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefWorkItemDto {
	/// Opaque work identity.
	pub id: String,
	/// Optional parent goal identity.
	pub parent_goal_id: Option<String>,
	/// Work category.
	pub kind: ChiefWorkKindDto,
	/// User-facing title.
	pub title: String,
	/// Exact bound thread identity.
	pub codex_thread_id: Option<String>,
	/// Acknowledged turn identity, retained during uncertainty.
	pub active_turn_id: Option<String>,
	/// Durable execution evidence.
	pub dispatch_state: ChiefDispatchStateDto,
	/// Explicit work judgment.
	pub status: ChiefWorkStatusDto,
	/// Next requested check, in Unix microseconds.
	pub next_check_at_micros: Option<i64>,
	/// Creation time, in Unix microseconds.
	pub created_at_micros: i64,
	/// Last modification time, in Unix microseconds.
	pub updated_at_micros: i64,
}

/// One explicit dependency edge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefDependencyDto {
	/// Work that is blocked by the dependency.
	pub work_item_id: String,
	/// Work that must precede it.
	pub depends_on_id: String,
}

/// Pending event metadata. Raw event payload and provider output are excluded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefPendingEventDto {
	/// Durable inbox sequence.
	pub id: i64,
	/// Stable event-source identity.
	pub source_event_id: String,
	/// Related work identity.
	pub work_item_id: String,
	/// Source event category.
	pub event_kind: String,
	/// Receipt time, in Unix microseconds.
	pub created_at_micros: i64,
	/// Whether delivery is claimed. This never means the event is disposed.
	pub delivery_claimed: bool,
}

/// One complete bounded transaction-consistent projection, including a valid empty state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefSnapshotDto {
	/// All work records.
	pub work_items: Vec<ChiefWorkItemDto>,
	/// All explicit dependency edges.
	pub dependencies: Vec<ChiefDependencyDto>,
	/// All undisposed result/event receipts.
	pub pending_events: Vec<ChiefPendingEventDto>,
}

impl ChiefSnapshotDto {
	/// Check count, text, relation, timestamp, and encoded-size bounds.
	pub fn is_valid(&self) -> bool {
		let text = |value: &str, limit| !value.is_empty() && value.len() <= limit;
		let optional =
			|value: &Option<String>| value.as_deref().is_none_or(|value| text(value, 512));
		let ids: std::collections::HashSet<_> =
			self.work_items.iter().map(|item| item.id.as_str()).collect();
		self.work_items.len() <= MAX_CHIEF_WORK_ITEMS
			&& self.dependencies.len() <= MAX_CHIEF_DEPENDENCIES
			&& self.pending_events.len() <= MAX_CHIEF_PENDING_EVENTS
			&& ids.len() == self.work_items.len()
			&& self.work_items.iter().all(|item| {
				text(&item.id, 512)
					&& text(&item.title, 1024)
					&& optional(&item.codex_thread_id)
					&& optional(&item.active_turn_id)
					&& item.parent_goal_id.as_deref().is_none_or(|parent| ids.contains(parent))
					&& item.created_at_micros >= 0
					&& item.updated_at_micros >= item.created_at_micros
					&& item.next_check_at_micros.is_none_or(|time| time >= 0)
			}) && self.dependencies.iter().all(|edge| {
			ids.contains(edge.work_item_id.as_str()) && ids.contains(edge.depends_on_id.as_str())
		}) && self.pending_events.iter().all(|event| {
			event.id > 0
				&& text(&event.source_event_id, 2048)
				&& ids.contains(event.work_item_id.as_str())
				&& text(&event.event_kind, 128)
				&& event.created_at_micros >= 0
		}) && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= MAX_CHIEF_SNAPSHOT_BYTES)
	}
}

/// A complete snapshot or explicit failure; partial graphs are never presented as complete.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefSnapshotResult {
	/// Complete data, including an empty work store.
	Available(ChiefSnapshotDto),
	/// The owner store is unavailable or cannot verify its data.
	Unavailable,
	/// Count or encoded-size bounds prevent a complete response. No records were omitted silently.
	CapacityExceeded {
		/// Total work records in the store.
		work_items: u64,
		/// Total dependency edges in the store.
		dependencies: u64,
		/// Total undisposed inbox events in the store.
		pending_events: u64,
	},
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn chief_snapshot_roundtrip_retains_empty_available_and_explicit_capacity_failure() {
		for value in [
			ChiefSnapshotResult::Available(ChiefSnapshotDto {
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			}),
			ChiefSnapshotResult::Unavailable,
			ChiefSnapshotResult::CapacityExceeded {
				work_items: 101,
				dependencies: 0,
				pending_events: 0,
			},
		] {
			let encoded = serde_json::to_string(&value).unwrap();
			assert_eq!(serde_json::from_str::<ChiefSnapshotResult>(&encoded).unwrap(), value);
		}
		assert!(
			ChiefSnapshotDto { work_items: vec![], dependencies: vec![], pending_events: vec![] }
				.is_valid()
		);
	}
}
