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

/// A bounded public execution update. Never contains raw provider frames or tool arguments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefActivityDto {
	/// Exact provider turn.
	pub turn_id: String,
	/// Exact provider item.
	pub item_id: String,
	/// Presentation category derived from the native item type.
	pub kind: String,
	/// Running, completed, failed, or declined.
	pub status: String,
	/// Short, human-readable action.
	pub label: String,
	/// Selected public facts such as file paths and exit codes.
	pub detail: String,
	/// Provider-reported duration, when available.
	pub duration_ms: Option<u64>,
}

/// A source-bound readable entry. Raw tool or credential frames are never projected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefHistoryEntryDto {
	/// Native execution activity; absent for conversation messages.
	#[serde(default)]
	pub activity: Option<ChiefActivityDto>,
	/// Total model input and output consumed by this exact turn, when recorded.
	pub usage: Option<ChiefTurnUsageDto>,
	/// Provider-reported execution duration; absent for older records.
	pub duration_ms: Option<u64>,
	/// Exact persistent event identity.
	pub id: i64,
	/// User, assistant, or system event category.
	pub kind: String,
	/// Bounded visible content.
	pub text: String,
	/// Observation time.
	pub created_at_micros: i64,
}

/// Current-turn text observed before final history is available.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefLiveMessageDto {
	/// Provider turn identity.
	pub turn_id: String,
	/// Provider item identity.
	pub item_id: String,
	/// Bounded partial text.
	pub text: String,
	/// Some text was omitted by the projection bound.
	pub truncated: bool,
}

/// Provider counter delta across all model calls in one completed turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefTurnUsageDto {
	/// Input tokens, including cached input.
	pub input_tokens: u64,
	/// Output tokens, including reasoning.
	pub output_tokens: u64,
}

/// Latest provider-observed conversation usage. Counts are cumulative, not per message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefUsageDto {
	/// Cumulative input tokens, including cached input.
	pub input_tokens: u64,
	/// Cumulative output tokens, including reasoning.
	pub output_tokens: u64,
	/// Last reported context token count.
	pub context_tokens: u64,
	/// Provider-reported model capacity, when available.
	pub context_window: Option<u64>,
}

/// Latest bounded readable history for one work identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefHistoryResult {
	/// Verified visible entries; older history or shortened content is explicitly indicated.
	Available {
		/// Unanswered asynchronous questions for the current native thread, independent of history
		/// paging.
		questions: Vec<crate::ChiefAsyncQuestionDto>,
		/// Additional questions exist beyond this bounded page.
		questions_truncated: bool,
		/// Native history recovery is incomplete; historical question cards are withheld.
		questions_recovering: bool,
		/// Current provider precaution, independent of transcript pagination.
		misalignment: Option<Box<ChiefMisalignmentDto>>,
		/// Latest observed usage for the current provider thread.
		usage: Option<ChiefUsageDto>,
		/// Source-bound records.
		entries: Vec<ChiefHistoryEntryDto>,
		/// At least one older or shortened entry is not represented here.
		has_more: bool,
		/// Cursor for an older saved page; independent of text truncation.
		next_before: Option<i64>,
		/// In-progress output; never completion evidence.
		live: Vec<ChiefLiveMessageDto>,
	},
	/// The work or source store cannot be read.
	Unavailable,
}

/// Findings for one provider precaution. The digest binds an explicit acknowledgment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefMisalignmentDto {
	/// Exact thread, turn and findings digest.
	pub review_id: String,
	/// Full provider explanation, at most 64 KiB; absent when not available.
	pub explanation: Option<String>,
	/// Exact continuation text, at most 1024 bytes; never automatically submitted.
	pub continuation: Option<String>,
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

/// A user-selected local file. Images use native vision input; other files are references.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefAttachmentDto {
	/// Absolute local path selected by the user.
	pub path: crate::ConversationWorkingDirectory,
	/// Send this file as a native image input.
	pub image: bool,
}

/// Explicit Chief operations. Graph judgments remain model-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefActionDto {
	/// Install the exact plugin whose current catalog details the user reviewed.
	InstallSuggestedPlugin {
		/// Owning task identity.
		work_id: crate::EntityId,
		/// Exact live native suggestion event.
		event_id: i64,
		/// Review identity returned by installation inspection.
		review_token: crate::WireText,
	},
	/// Explicitly restore the exact archived native thread selected by the user.
	RestoreArchivedThread {
		/// Current local work identity.
		work_id: crate::EntityId,
		/// Native thread identity shown by archive inspection.
		thread_id: crate::WireText,
	},
	/// Explicitly synchronize shared installed plugins and reload loaded native MCP runtimes.
	RefreshIntegrations {
		/// Task from which the user requested the shared refresh.
		work_id: crate::EntityId,
	},
	/// Associate a user-selected HTTP(S) link with the current native task thread.
	AddResourceLink {
		/// Exact local task identity.
		work_id: crate::EntityId,
		/// User-visible link title.
		title: crate::WireText,
		/// HTTP(S) resource address; this does not fetch its contents.
		url: crate::WireText,
	},
	/// Remove the current native association; never delete the referenced resource.
	RemoveResource {
		/// Exact local task identity.
		work_id: crate::EntityId,
		/// Application-defined native attachment category.
		attachment_type: crate::WireText,
		/// Native identity within the category.
		identity_key: crate::WireText,
	},
	/// Start with explicit per-message execution settings and attachments.
	StartConfigured {
		/// Initial Chief context.
		start: ChiefStartDto,
		/// Settings captured when the user sends.
		execution: crate::ConversationExecutionSettings,
		/// User-selected files, bounded by the service.
		attachments: Vec<ChiefAttachmentDto>,
	},
	/// Continue the same Chief with settings and attachments captured at send time.
	SendConfigured {
		/// Existing manager identity.
		root_id: crate::EntityId,
		/// User-authored message.
		text: crate::HistoryText,
		/// Settings for this message.
		execution: crate::ConversationExecutionSettings,
		/// User-selected files, bounded by the service.
		attachments: Vec<ChiefAttachmentDto>,
	},
	/// Supplement one exact running turn through native Codex steering.
	Steer {
		/// Target manager identity.
		work_id: crate::EntityId,
		/// Running turn observed at send time.
		turn_id: crate::WireText,
		/// User-authored supplementary input.
		text: crate::HistoryText,
		/// Files captured at send time.
		attachments: Vec<ChiefAttachmentDto>,
	},
	/// Acknowledge the exact findings displayed by the client and request continuation.
	ContinueMisalignment {
		/// Work owning the paused thread.
		work_id: crate::EntityId,
		/// Digest of the displayed thread, turn and findings.
		review_id: crate::WireText,
	},
	/// Submit explicit user approval context for an exact observed Guardian denial.
	/// This does not execute the action or start another turn.
	ApproveGuardianDenial {
		/// Work that owns the reviewed thread.
		work_id: crate::EntityId,
		/// Durable review row shown to the user.
		review_row: i64,
		/// Digest of the displayed observation, including the exact action.
		review_digest: crate::WireText,
	},

	/// Answer one source-bound asynchronous question with an explicit user message.
	AnswerQuestion {
		/// Work that owns the original question.
		work_id: crate::EntityId,
		/// Stable native question identity.
		question_id: crate::WireText,
		/// Explicit free text or user-selected option.
		answer: crate::HistoryText,
	},
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
	/// An executable subordinate Chief.
	Manager,
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

/// A project directory owned by one executable Chief.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefWorkspaceDto {
	/// Manager identity, also the workspace identity.
	pub chief_id: String,
	/// User-facing project name.
	pub name: String,
	/// Canonical existing execution directory inherited by descendants.
	pub directory: String,
}

/// One complete bounded transaction-consistent projection, including a valid empty state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefSnapshotDto {
	/// Persisted project scopes.
	pub workspaces: Vec<ChiefWorkspaceDto>,
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
		self.workspaces.len() <= MAX_CHIEF_WORK_ITEMS
			&& self.workspaces.iter().all(|workspace| {
				text(&workspace.name, 256)
					&& text(&workspace.directory, 4096)
					&& self.work_items.iter().any(|work| {
						work.id == workspace.chief_id && work.kind == ChiefWorkKindDto::Manager
					})
			}) && self.work_items.len() <= MAX_CHIEF_WORK_ITEMS
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
	#[test]
	fn guardian_approval_command_carries_only_saved_review_identity() {
		let mut value = serde_json::json!({"action":"approve_guardian_denial","data":{
			"work_id":"chief","review_row":7,"review_digest":"a".repeat(64)}});
		let command: super::ChiefActionDto = serde_json::from_value(value.clone()).unwrap();
		assert_eq!(serde_json::to_value(command).unwrap(), value);
		value["data"]["event"] =
			serde_json::json!({"action":{"type":"command","command":"injected"}});
		assert!(serde_json::from_value::<super::ChiefActionDto>(value).is_err());
	}
	use super::*;

	#[test]
	fn chief_snapshot_roundtrip_retains_empty_available_and_explicit_capacity_failure() {
		for value in [
			ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
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
			ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![]
			}
			.is_valid()
		);
	}
}

/// Native model choices from the currently connected Codex process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefModelDto {
	/// Exact model identifier used in turn requests.
	pub model: crate::ConversationModel,
	/// Provider display name.
	pub name: String,
	/// Reasoning levels understood by this client and advertised by Codex.
	pub efforts: Vec<crate::ConversationReasoningEffort>,
	/// Provider default, when understood by this client.
	pub default_effort: Option<crate::ConversationReasoningEffort>,
	/// The provider offers the priority service tier for this model.
	pub supports_fast: bool,
	/// The provider accepts image input for this model.
	pub supports_images: bool,
	/// Provider availability information for the current account, when supplied.
	pub availability: Option<String>,
	/// Informational upgrade or retirement notice; selection stays explicit.
	pub upgrade: Option<ChiefModelUpgradeDto>,
}

/// Provider-advertised model replacement information.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefModelUpgradeDto {
	/// Suggested replacement, never selected automatically.
	pub model: crate::ConversationModel,
	/// Provider-authored explanation.
	pub notice: Option<String>,
	/// Informational retirement time as Unix seconds, when supplied.
	pub retirement_at: Option<i64>,
}

/// Read-only capability evidence. Absence never means a disabled feature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefCapabilitiesResult {
	/// Observed on the currently owned connection; no model turn was started.
	Available {
		/// Complete bounded visible model catalog.
		models: Vec<ChiefModelDto>,
		/// Effective Memory feature flag from experimentalFeature/list, when available.
		memory_enabled: Option<bool>,
	},
	/// The connection or complete catalog could not be read.
	Unavailable,
}

/// Selected readable tool evidence for one exact native item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefActivityDetailResult {
	/// Public tool output or file changes, bounded and credential-filtered.
	Available {
		/// Plain text evidence; never executable markup.
		text: String,
		/// Some output was omitted by the byte bound.
		truncated: bool,
	},
	/// The source cannot be confirmed or this item has no supported public detail.
	Unavailable,
}

/// One native resource association; its payload is display data, not executable input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefResourceDto {
	/// Stable native association identity.
	pub id: String,
	/// Application-defined resource category.
	pub attachment_type: String,
	/// Exact identity within that category.
	pub identity_key: String,
	/// Bounded JSON metadata for inspection.
	pub payload_json: String,
	/// Payload text exceeded the display bound or contained private credential material.
	pub payload_omitted: bool,
	/// Native creation timestamp in seconds.
	pub created_at: i64,
}

/// Native association reads distinguish a confirmed empty list from unavailable storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefResourcesResult {
	/// Complete native list under the public response bound.
	Available {
		/// Resource associations for the exact requested work.
		resources: Vec<ChiefResourceDto>,
	},
	/// This native provider does not implement resource associations.
	Unsupported,
	/// The complete list exceeds the display bound.
	CapacityExceeded,
	/// No authoritative result is available for the current thread and connection.
	Unavailable,
}
