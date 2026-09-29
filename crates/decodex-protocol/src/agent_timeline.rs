//! Public native timeline projection. History does not authorize new agent work.
use serde::{Deserialize, Serialize};

/// One exact native page, oldest entry first. The continuation reads older entries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentTimelinePage {
	/// Live turn whose provider explicitly requests a safety buffering indicator.
	/// This is connection state, not saved history or an approval request.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub safety_buffering_turn_id: Option<String>,
	/// Thread whose native cursor and entries this page contains.
	pub thread_id: String,
	/// Stable native entries in canonical order, including equal-position boundaries.
	pub entries: Vec<AgentTimelineEntry>,
	/// Opaque cursor for the next older page, bound to this thread.
	pub next_cursor: Option<String>,
	/// Weather cards keyed by the exact native turn.
	#[serde(default)]
	pub weather: std::collections::BTreeMap<String, Vec<crate::WeatherForecast>>,
	/// Voice session active immediately before the first entry, if any.
	pub active_realtime_session_at_page_start: Option<String>,
}

/// A canonical position with its own typed identity; position alone is not unique.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentTimelineEntry {
	/// Native rollout position, never a local receipt timestamp.
	pub position: u64,
	/// Public content and exact native identities.
	pub content: AgentTimelineContent,
}

/// Public content of the exact item referenced by a native voice promotion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentTimelinePromotedContent {
	/// Bounded message text; raw tool output is not included.
	pub text: String,
	/// Some content was omitted by the display bound.
	pub truncated: bool,
	/// Selected public activity facts, if applicable.
	pub activity: Option<crate::AgentActivityDto>,
	/// Original attachment indices for exact native preview requests.
	pub attachments: Vec<AgentTimelineAttachment>,
}

/// Bounded public failure message from a native terminal boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentTimelineError {
	/// Readable message; credential material is omitted by the service.
	pub message: String,
	/// The message was shortened or sensitive content was omitted.
	pub truncated: bool,
}

/// A bounded description of non-text content, without embedded bytes or signed URLs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentTimelineAttachment {
	/// Index in native user content, dynamic contentItems, or MCP result.content.
	/// Zero for a standalone image result/view item.
	pub index: u32,
	/// Native content kind, or `unknown` for an unsupported content variant.
	pub kind: String,
	/// Readable name or media label. This is not a filesystem authority.
	pub label: String,
	/// Where the native item keeps its content. Resolve through the owning thread.
	pub source: AgentTimelineAttachmentSource,
}

/// Native attachment storage categories. These do not authorize a content read.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTimelineAttachmentSource {
	/// A path on the app-server host, which may differ from the UI host.
	Local,
	/// A remote URI retained by the native item.
	Remote,
	/// Embedded native media bytes.
	Inline,
	/// A provider-owned file identity.
	Stored,
	/// A structured skill or mention target.
	Reference,
	/// The native content variant or source is not supported.
	Unknown,
}

/// Timeline content safe for presentation without raw tool arguments or credentials.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentTimelineContent {
	/// An ordinary conversation or tool item.
	Item {
		/// Native turn containing the item.
		turn_id: String,
		/// Native item identity for exact detail reads.
		item_id: String,
		/// Native item kind; unknown kinds remain visible as unsupported items.
		kind: String,
		/// Native assistant message phase. Missing phases remain unknown.
		#[serde(default)]
		phase: Option<String>,
		/// Public message text, if present.
		text: String,
		/// Some message content was omitted by the display bound.
		truncated: bool,
		/// Existing public activity projection, if applicable.
		activity: Option<crate::AgentActivityDto>,
		/// Native tool metadata declares an interactive App UI resource.
		#[serde(default)]
		app_ui: bool,
		/// Non-text content descriptors in original source order.
		attachments: Vec<AgentTimelineAttachment>,
	},
	/// A native voice session began or closed.
	VoiceBoundary {
		/// Stable boundary item identity, distinct even for reused session IDs.
		item_id: String,
		/// Native realtime session identity.
		session_id: String,
		/// Native boundary type.
		kind: String,
		/// Native closing outcome, absent for a session start.
		outcome: Option<String>,
	},
	/// A committed speech segment. Streaming drafts are not durable history.
	Speech {
		/// Stable segment identity.
		item_id: String,
		/// Native realtime session identity.
		session_id: String,
		/// User or assistant speaker.
		role: String,
		/// Canonical committed text, bounded for display.
		text: String,
		/// The display bound omitted part of the segment.
		truncated: bool,
	},
	/// An existing agent item promoted into the voice conversation.
	Promotion {
		/// Stable promotion identity; do not use the referenced item as this row's ID.
		item_id: String,
		/// Native realtime session identity.
		session_id: String,
		/// Referenced native turn.
		turn_id: String,
		/// Referenced agent item, which can appear elsewhere in the timeline.
		agent_item_id: String,
		/// Native presentation kind.
		presentation: String,
		/// Exact referenced content, including when it is outside this page.
		resolved: Option<AgentTimelinePromotedContent>,
		/// Visualization index when presentation selects one inline visualization.
		index: Option<u32>,
	},
	/// Native turn start or terminal boundary.
	TurnBoundary {
		/// Exact native turn.
		turn_id: String,
		/// Whether this is a terminal boundary.
		completed: bool,
		/// Native terminal status, absent at turn start.
		status: Option<String>,
		/// Provider duration, never reconstructed from page order.
		duration_ms: Option<u64>,
		/// Saved provider usage for this exact completed turn. Native timeline has no usage field.
		usage_summary: Option<String>,
		/// Compact token counts for this exact turn.
		#[serde(default)]
		usage: Option<crate::AgentTurnUsageDto>,
		/// Public provider failure message, when present.
		error: Option<AgentTimelineError>,
	},
}

/// Account-bound native timeline observation; failures never imply empty history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentTimelineResult {
	/// Complete bounded page from the current account and task binding.
	Available {
		/// Exact requested local task.
		work_id: crate::EntityId,
		/// Account that authenticated this read.
		account_id: crate::EntityId,
		/// Native timeline page.
		page: AgentTimelinePage,
	},
	/// Incomplete recent display content; never execution or pagination evidence.
	Summary {
		/// Exact requested local task.
		work_id: crate::EntityId,
		/// Account that authenticated the read.
		account_id: crate::EntityId,
		/// Exact native conversation.
		thread_id: String,
		/// Recent prompts and final replies without canonical timeline positions.
		items: Vec<AgentTimelineContent>,
	},
	/// This thread or server cannot serve a native timeline. Local history remains available.
	Unsupported,
	/// The page exceeds the public wire bound.
	CapacityExceeded,
	/// The read failed or the account/task binding changed.
	Unavailable,
}
