use std::fmt::{Debug, Formatter};

use serde_json::Value;

use crate::{
	ThreadId,
	conversation::{ConversationNotification, MAX_EXACT_TURN_ID_BYTES},
	protocol::{MAX_APP_SERVER_FRAME_BYTES, MAX_EXACT_THREAD_ID_BYTES},
};

const MAX_COLLABORATION_RECEIVERS: usize = 64;
/// Maximum UTF-8 bytes in one user-visible Conversation message delta.
pub const MAX_CONVERSATION_MESSAGE_DELTA_BYTES: usize = 64 * 1_024;

/// Opaque, bounded correlation identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueId(String);
impl OpaqueId {
	fn from_protocol(value: &str) -> Self {
		Self(ThreadId::normalize(value))
	}

	/// Return the UUID or digest used for correlation.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Closed native subagent activity kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollaborationActivityKind {
	/// The child activity started.
	Started,
	/// The child interacted with its parent.
	Interacted,
	/// The child activity was interrupted.
	Interrupted,
	/// A forward-compatible activity kind was discarded.
	Unknown,
}

/// Closed native collaboration tool names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollaborationTool {
	/// Spawn a run-local agent.
	SpawnAgent,
	/// Send input to a run-local agent.
	SendInput,
	/// Resume a run-local agent.
	ResumeAgent,
	/// Wait for run-local agent progress.
	Wait,
	/// Close a run-local agent.
	CloseAgent,
	/// A forward-compatible tool name was discarded.
	Unknown,
}

/// Closed native collaboration tool states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollaborationToolStatus {
	/// The tool call is active.
	InProgress,
	/// The tool call completed.
	Completed,
	/// The tool call failed.
	Failed,
	/// A forward-compatible status was discarded.
	Unknown,
}

/// Closed thread states; active flags and free-form details are discarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadStatus {
	/// Thread state is not loaded.
	NotLoaded,
	/// Thread state is idle.
	Idle,
	/// Thread state reports a system error.
	SystemError,
	/// Thread state is active.
	Active,
	/// A forward-compatible status was discarded.
	Unknown,
}

/// Closed turn states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnStatus {
	/// Turn completed normally.
	Completed,
	/// Turn was interrupted.
	Interrupted,
	/// Turn failed.
	Failed,
	/// Turn remains active.
	InProgress,
	/// A forward-compatible status was discarded.
	Unknown,
}

/// Run-local Codex actor. Optional nickname/role fields are never identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunLocalActor {
	/// Child thread identity used as the runtime actor identity.
	pub id: ThreadId,
	/// Parent thread identity when supplied by Codex.
	pub parent_id: Option<ThreadId>,
	/// Closed activity classification.
	pub activity: CollaborationActivityKind,
	/// Whether non-identity nickname or role metadata was present.
	pub optional_metadata_present: bool,
	/// Opaque turn correlation.
	pub turn_id: OpaqueId,
	/// Opaque item correlation.
	pub item_id: OpaqueId,
	/// Whether the activity item reached its terminal notification.
	pub completed: bool,
}

/// Native collaboration command, with only bounded labels and opaque identifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationToolCall {
	/// Enclosing thread that emitted the item.
	pub thread_id: ThreadId,
	/// Opaque turn correlation.
	pub turn_id: OpaqueId,
	/// Opaque item correlation.
	pub item_id: OpaqueId,
	/// Run-local thread that issued the command.
	pub sender_thread_id: ThreadId,
	/// Run-local target threads.
	pub receiver_thread_ids: Vec<ThreadId>,
	/// Closed Codex collaboration tool classification.
	pub tool: CollaborationTool,
	/// Closed Codex status classification.
	pub status: CollaborationToolStatus,
	/// Whether the containing item reached its terminal notification.
	pub completed: bool,
}

/// Stable normalized item categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedItemKind {
	/// Model-authored user-visible message.
	AgentMessage,
	/// Model reasoning item.
	Reasoning,
	/// Command execution item.
	Command,
	/// File-change item.
	FileChange,
	/// MCP, dynamic, or web tool item.
	Tool,
	/// Forward-compatible unknown item type.
	Unknown,
}

/// Separate bounded user-visible projection for one ordinary Conversation message delta.
///
/// This projection does not change the authority or redaction behavior of [`NormalizedEvent`].
#[derive(Clone, Eq, PartialEq)]
pub struct ConversationMessageDelta {
	thread_id: ThreadId,
	turn_id: OpaqueId,
	item_id: OpaqueId,
	text: String,
}
impl ConversationMessageDelta {
	/// Opaque thread correlation.
	pub fn thread_id(&self) -> &ThreadId {
		&self.thread_id
	}

	/// Opaque turn correlation.
	pub fn turn_id(&self) -> &OpaqueId {
		&self.turn_id
	}

	/// Opaque message-item correlation.
	pub fn item_id(&self) -> &OpaqueId {
		&self.item_id
	}

	/// Exact bounded user-visible delta text.
	pub fn text(&self) -> &str {
		&self.text
	}
}
impl Debug for ConversationMessageDelta {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ConversationMessageDelta")
			.field("thread_id", &self.thread_id)
			.field("turn_id", &self.turn_id)
			.field("item_id", &self.item_id)
			.field("text", &"[REDACTED]")
			.finish()
	}
}

/// Stable user-visible projection error that never embeds app-server input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationMessageDeltaError {
	/// Input was not valid JSON.
	InvalidJson,
	/// The recognized delta lacked required typed fields or contained invalid text.
	InvalidMessageDelta,
	/// Input exceeded a mechanical frame or text bound.
	LimitExceeded,
}

/// Project one bounded user-visible message delta and discard all other notifications.
pub fn project_conversation_message_delta(
	bytes: &[u8],
) -> Result<Option<ConversationMessageDelta>, ConversationMessageDeltaError> {
	if bytes.len() > MAX_APP_SERVER_FRAME_BYTES {
		return Err(ConversationMessageDeltaError::LimitExceeded);
	}

	let value: Value =
		serde_json::from_slice(bytes).map_err(|_| ConversationMessageDeltaError::InvalidJson)?;
	let Some(method) = value.get("method").and_then(Value::as_str) else {
		return Err(ConversationMessageDeltaError::InvalidMessageDelta);
	};

	if method != ConversationNotification::AgentMessageDelta.as_str() {
		return Ok(None);
	}

	let params = value.get("params").ok_or(ConversationMessageDeltaError::InvalidMessageDelta)?;
	let thread_id = params
		.get("threadId")
		.and_then(Value::as_str)
		.ok_or(ConversationMessageDeltaError::InvalidMessageDelta)?;
	let turn_id = params
		.get("turnId")
		.and_then(Value::as_str)
		.ok_or(ConversationMessageDeltaError::InvalidMessageDelta)?;
	let item_id = params
		.get("itemId")
		.and_then(Value::as_str)
		.ok_or(ConversationMessageDeltaError::InvalidMessageDelta)?;
	let text = params
		.get("delta")
		.and_then(Value::as_str)
		.ok_or(ConversationMessageDeltaError::InvalidMessageDelta)?;

	if !valid_projection_id(thread_id, MAX_EXACT_THREAD_ID_BYTES)
		|| !valid_projection_id(turn_id, MAX_EXACT_TURN_ID_BYTES)
		|| !valid_projection_id(item_id, MAX_EXACT_TURN_ID_BYTES)
	{
		return Err(ConversationMessageDeltaError::InvalidMessageDelta);
	}
	if text.is_empty() || text.contains('\0') {
		return Err(ConversationMessageDeltaError::InvalidMessageDelta);
	}
	if text.len() > MAX_CONVERSATION_MESSAGE_DELTA_BYTES {
		return Err(ConversationMessageDeltaError::LimitExceeded);
	}

	Ok(Some(ConversationMessageDelta {
		thread_id: ThreadId::from_protocol(thread_id),
		turn_id: OpaqueId::from_protocol(turn_id),
		item_id: OpaqueId::from_protocol(item_id),
		text: text.to_owned(),
	}))
}

fn valid_projection_id(value: &str, maximum: usize) -> bool {
	!value.is_empty()
		&& value.len() <= maximum
		&& !value.chars().any(|character| character.is_control())
}

/// Redacted event model exported to domain/runtime callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizedEvent {
	/// Codex created or surfaced a thread.
	ThreadStarted {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Opaque run-local parent identifier.
		parent_thread_id: Option<ThreadId>,
	},
	/// Thread status changed.
	ThreadStatus {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Closed Codex status classification.
		status: ThreadStatus,
	},
	/// A turn began.
	TurnStarted {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Opaque turn identifier.
		turn_id: OpaqueId,
	},
	/// A turn reached a terminal notification.
	TurnCompleted {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Opaque turn identifier.
		turn_id: OpaqueId,
		/// Closed terminal status classification.
		status: TurnStatus,
	},
	/// A non-collaboration item started or completed.
	Item {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Opaque turn identifier.
		turn_id: OpaqueId,
		/// Opaque item identifier.
		item_id: OpaqueId,
		/// Stable normalized item category.
		kind: NormalizedItemKind,
		/// Whether this came from the terminal item notification.
		completed: bool,
	},
	/// Incremental model message observed; free-form content is intentionally discarded.
	MessageDelta {
		/// Opaque thread identifier.
		thread_id: ThreadId,
		/// Opaque turn identifier.
		turn_id: OpaqueId,
	},
	/// Native collaboration normalized as a run-local actor.
	CollaborationActivity(RunLocalActor),
	/// Native collaboration tool call with typed correlation and terminal state.
	CollaborationToolCall(CollaborationToolCall),
	/// Notification not consumed by the bounded adapter contract.
	Ignored,
}

/// Stable decoding error that never embeds raw app-server input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDecodeError {
	/// Input was not valid JSON.
	InvalidJson,
	/// Input lacked a notification method.
	MissingMethod,
	/// A recognized notification lacked required typed fields.
	InvalidKnownEvent,
	/// Input exceeded a mechanical frame or collection bound.
	LimitExceeded,
}

/// Decode and normalize one app-server notification without exposing raw JSON.
pub fn normalize_event(bytes: &[u8]) -> Result<NormalizedEvent, EventDecodeError> {
	if bytes.len() > MAX_APP_SERVER_FRAME_BYTES {
		return Err(EventDecodeError::LimitExceeded);
	}

	let value: Value = serde_json::from_slice(bytes).map_err(|_| EventDecodeError::InvalidJson)?;
	let method =
		value.get("method").and_then(Value::as_str).ok_or(EventDecodeError::MissingMethod)?;
	let params = value.get("params").ok_or(EventDecodeError::InvalidKnownEvent)?;

	match method {
		"thread/started" => {
			let thread = params.get("thread").unwrap_or(params);

			Ok(NormalizedEvent::ThreadStarted {
				thread_id: thread_id(thread, "id")?,
				parent_thread_id: optional_thread_id(thread, "parentThreadId"),
			})
		},
		"thread/status/changed" => Ok(NormalizedEvent::ThreadStatus {
			thread_id: thread_id(params, "threadId")?,
			status: thread_status(params.get("status").ok_or(EventDecodeError::InvalidKnownEvent)?),
		}),
		"turn/started" => Ok(NormalizedEvent::TurnStarted {
			thread_id: thread_id(params, "threadId")?,
			turn_id: nested_id(params, "turn")?,
		}),
		"turn/completed" => {
			let turn = params.get("turn").ok_or(EventDecodeError::InvalidKnownEvent)?;

			Ok(NormalizedEvent::TurnCompleted {
				thread_id: thread_id(params, "threadId")?,
				turn_id: opaque_id(turn, "id")?,
				status: turn_status(string_field(turn, "status")?),
			})
		},
		"item/started" | "item/completed" => normalize_item(params, method == "item/completed"),
		"item/agentMessage/delta" => Ok(NormalizedEvent::MessageDelta {
			thread_id: thread_id(params, "threadId")?,
			turn_id: opaque_id(params, "turnId")?,
		}),
		_ => Ok(NormalizedEvent::Ignored),
	}
}

fn normalize_item(params: &Value, completed: bool) -> Result<NormalizedEvent, EventDecodeError> {
	let item = params.get("item").ok_or(EventDecodeError::InvalidKnownEvent)?;
	let item_type = string_field(item, "type")?;

	if item_type == "subAgentActivity" {
		return Ok(NormalizedEvent::CollaborationActivity(RunLocalActor {
			id: thread_id(params, "threadId")?,
			parent_id: Some(thread_id(item, "agentThreadId")?),
			activity: collaboration_activity(string_field(item, "kind")?),
			optional_metadata_present: item.get("agentNickname").is_some()
				|| item.get("agentRole").is_some(),
			turn_id: opaque_id(params, "turnId")?,
			item_id: opaque_id(item, "id")?,
			completed,
		}));
	}
	if item_type == "collabAgentToolCall" {
		let receiver_values = item
			.get("receiverThreadIds")
			.and_then(Value::as_array)
			.ok_or(EventDecodeError::InvalidKnownEvent)?;

		if receiver_values.len() > MAX_COLLABORATION_RECEIVERS {
			return Err(EventDecodeError::LimitExceeded);
		}

		let receivers = receiver_values
			.iter()
			.map(|value| {
				value
					.as_str()
					.map(ThreadId::from_protocol)
					.ok_or(EventDecodeError::InvalidKnownEvent)
			})
			.collect::<Result<Vec<_>, _>>()?;

		return Ok(NormalizedEvent::CollaborationToolCall(CollaborationToolCall {
			thread_id: thread_id(params, "threadId")?,
			turn_id: opaque_id(params, "turnId")?,
			item_id: opaque_id(item, "id")?,
			sender_thread_id: thread_id(item, "senderThreadId")?,
			receiver_thread_ids: receivers,
			tool: collaboration_tool(string_field(item, "tool")?),
			status: collaboration_tool_status(string_field(item, "status")?),
			completed,
		}));
	}

	Ok(NormalizedEvent::Item {
		thread_id: thread_id(params, "threadId")?,
		turn_id: opaque_id(params, "turnId")?,
		item_id: opaque_id(item, "id")?,
		kind: match item_type {
			"agentMessage" => NormalizedItemKind::AgentMessage,
			"reasoning" => NormalizedItemKind::Reasoning,
			"commandExecution" => NormalizedItemKind::Command,
			"fileChange" => NormalizedItemKind::FileChange,
			"mcpToolCall" | "dynamicToolCall" | "webSearch" => NormalizedItemKind::Tool,
			_ => NormalizedItemKind::Unknown,
		},
		completed,
	})
}

fn thread_id(value: &Value, key: &str) -> Result<ThreadId, EventDecodeError> {
	Ok(ThreadId::from_protocol(string_field(value, key)?))
}

fn optional_thread_id(value: &Value, key: &str) -> Option<ThreadId> {
	value.get(key).and_then(Value::as_str).map(ThreadId::from_protocol)
}

fn nested_id(value: &Value, key: &str) -> Result<OpaqueId, EventDecodeError> {
	let nested = value.get(key).ok_or(EventDecodeError::InvalidKnownEvent)?;

	opaque_id(nested, "id")
}

fn opaque_id(value: &Value, key: &str) -> Result<OpaqueId, EventDecodeError> {
	Ok(OpaqueId::from_protocol(string_field(value, key)?))
}

fn string_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, EventDecodeError> {
	value.get(key).and_then(Value::as_str).ok_or(EventDecodeError::InvalidKnownEvent)
}

fn thread_status(value: &Value) -> ThreadStatus {
	let label = value.as_str().or_else(|| value.get("type").and_then(Value::as_str));

	match label {
		Some("notLoaded") => ThreadStatus::NotLoaded,
		Some("idle") => ThreadStatus::Idle,
		Some("systemError") => ThreadStatus::SystemError,
		Some("active") => ThreadStatus::Active,
		_ => ThreadStatus::Unknown,
	}
}

fn turn_status(value: &str) -> TurnStatus {
	match value {
		"completed" => TurnStatus::Completed,
		"interrupted" => TurnStatus::Interrupted,
		"failed" => TurnStatus::Failed,
		"inProgress" => TurnStatus::InProgress,
		_ => TurnStatus::Unknown,
	}
}

fn collaboration_activity(value: &str) -> CollaborationActivityKind {
	match value {
		"started" => CollaborationActivityKind::Started,
		"interacted" => CollaborationActivityKind::Interacted,
		"interrupted" => CollaborationActivityKind::Interrupted,
		_ => CollaborationActivityKind::Unknown,
	}
}

fn collaboration_tool(value: &str) -> CollaborationTool {
	match value {
		"spawnAgent" => CollaborationTool::SpawnAgent,
		"sendInput" => CollaborationTool::SendInput,
		"resumeAgent" => CollaborationTool::ResumeAgent,
		"wait" => CollaborationTool::Wait,
		"closeAgent" => CollaborationTool::CloseAgent,
		_ => CollaborationTool::Unknown,
	}
}

fn collaboration_tool_status(value: &str) -> CollaborationToolStatus {
	match value {
		"inProgress" => CollaborationToolStatus::InProgress,
		"completed" => CollaborationToolStatus::Completed,
		"failed" => CollaborationToolStatus::Failed,
		_ => CollaborationToolStatus::Unknown,
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		CollaborationActivityKind, CollaborationTool, CollaborationToolStatus, NormalizedEvent,
		OpaqueId, RunLocalActor, ThreadId, ThreadStatus, TurnStatus, event,
	};

	#[test]
	fn message_delta_discards_all_free_form_content() {
		let event = event::normalize_event(
			br#"{"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","delta":"Bearer abcdefghijklmnop sk-secretsecretsecret"}}"#,
		)
		.unwrap();

		assert_eq!(
			event,
			NormalizedEvent::MessageDelta {
				thread_id: ThreadId::from_protocol("thread-1"),
				turn_id: OpaqueId::from_protocol("turn-1"),
			}
		);
		assert!(!format!("{event:?}").contains("secret"));
	}

	#[test]
	fn oversized_public_event_input_is_rejected_before_parsing() {
		let oversized = vec![b' '; super::MAX_APP_SERVER_FRAME_BYTES + 1];

		assert_eq!(event::normalize_event(&oversized), Err(super::EventDecodeError::LimitExceeded));
	}

	#[test]
	fn collaboration_receiver_count_is_bounded() {
		let receivers = (0..=super::MAX_COLLABORATION_RECEIVERS)
			.map(|index| format!("thread-{index}"))
			.collect::<Vec<_>>();
		let event = serde_json::json!({
			"method": "item/completed",
			"params": {
				"threadId": "thread",
				"turnId": "turn",
				"item": {
					"id": "item",
					"type": "collabAgentToolCall",
					"receiverThreadIds": receivers,
					"senderThreadId": "sender",
					"status": "completed",
					"tool": "sendInput"
				}
			}
		});

		assert_eq!(
			event::normalize_event(&serde_json::to_vec(&event).unwrap()),
			Err(super::EventDecodeError::LimitExceeded)
		);
	}

	#[test]
	fn collaboration_identity_uses_thread_ids_not_optional_role_fields() {
		let event = event::normalize_event(
			br#"{"method":"item/completed","params":{"threadId":"child","turnId":"turn","item":{"id":"item","type":"subAgentActivity","kind":"interacted","agentThreadId":"parent","agentNickname":"Reviewer","agentRole":"reviewer"}}}"#,
		)
		.unwrap();

		assert_eq!(
			event,
			NormalizedEvent::CollaborationActivity(RunLocalActor {
				id: ThreadId::from_protocol("child"),
				parent_id: Some(ThreadId::from_protocol("parent")),
				activity: CollaborationActivityKind::Interacted,
				optional_metadata_present: true,
				turn_id: OpaqueId::from_protocol("turn"),
				item_id: OpaqueId::from_protocol("item"),
				completed: true,
			})
		);
	}

	#[test]
	fn collaboration_tool_call_preserves_typed_correlation_and_status() {
		let event = event::normalize_event(br#"{"method":"item/completed","params":{"threadId":"parent","turnId":"turn","item":{"id":"tool-item","type":"collabAgentToolCall","senderThreadId":"parent","receiverThreadIds":["child"],"tool":"spawnAgent","status":"completed","prompt":"api_key=secret"}}}"#).unwrap();

		assert!(matches!(event, NormalizedEvent::CollaborationToolCall(ref call)
			if call.thread_id == ThreadId::from_protocol("parent")
				&& call.sender_thread_id == ThreadId::from_protocol("parent")
				&& call.receiver_thread_ids[0] == ThreadId::from_protocol("child")
			&& call.tool == CollaborationTool::SpawnAgent
			&& call.status == CollaborationToolStatus::Completed && call.completed));
		assert!(!format!("{event:?}").contains("secret"));
	}

	#[test]
	fn malformed_input_error_never_echoes_raw_json() {
		let secret = br#"{"accessToken":"sk-secretsecretsecret""#;
		let error = event::normalize_event(secret).unwrap_err();

		assert_eq!(format!("{error:?}"), "InvalidJson");
	}

	#[test]
	fn credential_shaped_identifiers_are_exported_only_as_digests() {
		let event = event::normalize_event(
			br#"{"method":"turn/started","params":{"threadId":"sk-thread-secret","turn":{"id":"api_key=turn-secret"}}}"#,
		).unwrap();
		let debug = format!("{event:?}");

		assert!(!debug.contains("secret"));
		assert!(debug.contains("sha256:"));
	}

	#[test]
	fn protocol_labels_are_closed_and_never_echo_unknown_or_credential_shaped_text() {
		for input in [
			br#"{"method":"thread/status/changed","params":{"threadId":"thread","status":{"type":"Bearer-secret"}}}"#.as_slice(),
			br#"{"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"sk-secret"}}}"#.as_slice(),
			br#"{"method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"id":"item","type":"collabAgentToolCall","senderThreadId":"thread","receiverThreadIds":[],"tool":"api_key=secret","status":"Bearer-secret"}}}"#.as_slice(),
		] {
			let event = event::normalize_event(input).unwrap();
			let debug = format!("{event:?}");

			assert!(!debug.contains("secret"));
			assert!(debug.contains("Unknown"));
		}
	}

	#[test]
	fn oversized_protocol_labels_collapse_to_closed_unknown_variants() {
		let oversized = format!(
			r#"{{"method":"turn/completed","params":{{"threadId":"thread","turn":{{"id":"turn","status":"{}"}}}}}}"#,
			"api_key=secret".repeat(1_000)
		);
		let event = event::normalize_event(oversized.as_bytes()).unwrap();
		let debug = format!("{event:?}");

		assert!(matches!(
			event,
			NormalizedEvent::TurnCompleted { status: TurnStatus::Unknown, .. }
		));
		assert!(!debug.contains("secret"));
		assert!(debug.len() < 256);
	}

	#[test]
	fn known_status_variants_are_structurally_normalized() {
		let thread = event::normalize_event(
			br#"{"method":"thread/status/changed","params":{"threadId":"thread","status":{"type":"active","activeFlags":["waiting"]}}}"#,
		)
		.unwrap();
		let turn = event::normalize_event(
			br#"{"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"failed"}}}"#,
		)
		.unwrap();

		assert!(matches!(
			thread,
			NormalizedEvent::ThreadStatus { status: ThreadStatus::Active, .. }
		));
		assert!(matches!(turn, NormalizedEvent::TurnCompleted { status: TurnStatus::Failed, .. }));
	}
}
