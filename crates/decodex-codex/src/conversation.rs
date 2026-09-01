//! Pure bounded facts for the ordinary Conversation app-server conversation.
//!
//! These values do not select an account, launch a process, authorize dispatch, retain
//! credentials, or perform app-server I/O. Runtime must prove its ProcessGeneration and
//! ProviderAttempt fences immediately before it converts these facts into an I/O effect.
//! Response decoders accept only the exact method `result` payload after a private transport
//! has removed its envelope.

use std::fmt::{Debug, Formatter};

use serde::{
	Deserialize, Deserializer, Serialize, Serializer,
	de::Error as _,
	ser::{SerializeSeq as _, SerializeStruct as _},
};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::{ExactThreadId, ThreadCwd, protocol::MAX_APP_SERVER_FRAME_BYTES};

/// Maximum UTF-8 bytes in one caller-selected model identifier.
pub const MAX_CONVERSATION_MODEL_BYTES: usize = 128;
/// Maximum UTF-8 bytes in one caller-selected reasoning-effort value.
pub const MAX_CONVERSATION_REASONING_EFFORT_BYTES: usize = 32;
/// Maximum UTF-8 bytes in developer instructions.
pub const MAX_CONVERSATION_INSTRUCTIONS_BYTES: usize = 64 * 1_024;
/// Maximum UTF-8 bytes in one text input item.
pub const MAX_CONVERSATION_TEXT_BYTES: usize = 256 * 1_024;
/// Maximum text items in one turn request.
pub const MAX_CONVERSATION_INPUT_ITEMS: usize = 16;
/// Maximum aggregate UTF-8 bytes in one turn request.
pub const MAX_CONVERSATION_INPUT_BYTES: usize = 256 * 1_024;
/// Maximum UTF-8 bytes in an exact Codex turn identifier.
pub const MAX_EXACT_TURN_ID_BYTES: usize = 1_024;
/// Maximum bytes accepted from one exact Conversation method result payload.
pub const MAX_CONVERSATION_RESPONSE_BYTES: usize = MAX_APP_SERVER_FRAME_BYTES;

/// Closed ordinary Conversation app-server method set.
///
/// `turn/steer` is intentionally absent. Ordinary conversation sends subsequent user input
/// through another `turn/start`; this contract owns no active-turn steering workflow.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConversationMethod {
	/// Start one durable thread.
	ThreadStart,
	/// Resume one caller-supplied exact thread.
	ThreadResume,
	/// Start one turn with explicit model and reasoning input.
	TurnStart,
	/// Interrupt one exact active turn.
	TurnInterrupt,
	/// Explicitly archive one exact thread.
	ThreadArchive,
}
impl ConversationMethod {
	/// Complete bounded method inventory.
	pub const ALL: [Self; 5] = [
		Self::ThreadStart,
		Self::ThreadResume,
		Self::TurnStart,
		Self::TurnInterrupt,
		Self::ThreadArchive,
	];

	/// Exact app-server method spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::ThreadStart => "thread/start",
			Self::ThreadResume => "thread/resume",
			Self::TurnStart => "turn/start",
			Self::TurnInterrupt => "turn/interrupt",
			Self::ThreadArchive => "thread/archive",
		}
	}
}

/// Closed notification set needed to project one ordinary Conversation turn.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConversationNotification {
	/// Bounded user-visible model text.
	AgentMessageDelta,
	/// Terminal state for one exact turn.
	TurnCompleted,
}
impl ConversationNotification {
	/// Complete bounded notification inventory.
	pub const ALL: [Self; 2] = [Self::AgentMessageDelta, Self::TurnCompleted];

	/// Exact app-server notification spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::AgentMessageDelta => "item/agentMessage/delta",
			Self::TurnCompleted => "turn/completed",
		}
	}
}

/// Closed request-construction or response-decoding failure that never echoes rejected input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationContractError {
	/// The exact thread identifier is empty, oversized, or unsafe for an exact request.
	InvalidThreadId,
	/// The exact turn identifier is empty, oversized, or unsafe for an exact request.
	InvalidTurnId,
	/// The working directory is empty, oversized, invalid, or not absolute.
	InvalidCwd,
	/// Developer instructions are empty, oversized, or contain a NUL byte.
	InvalidInstructions,
	/// The caller-selected model is empty, oversized, or contains control text.
	InvalidModel,
	/// The caller-selected reasoning effort is empty, oversized, or contains control text.
	InvalidReasoningEffort,
	/// A user text item is empty, oversized, or contains a NUL byte.
	InvalidText,
	/// A turn contains no text items or exceeds the item-count bound.
	InputItemLimitExceeded,
	/// Aggregate turn input exceeds its byte bound.
	InputByteLimitExceeded,
	/// An app-server result payload exceeded the fixed response bound.
	ResponseLimitExceeded,
	/// JSON, field types, duplicate fields, or the result container shape were invalid.
	MalformedResponse,
	/// An app-server result contained a field outside the pinned method shape.
	UnknownResponseField,
	/// An app-server result omitted a field required by the pinned method shape.
	MissingResponseField,
	/// App-server response facts described an ephemeral thread.
	EphemeralThreadRejected,
	/// An app-server response did not identify the exact requested thread.
	ThreadIdMismatch,
	/// A required app-server response cwd agreement failed.
	CwdMismatch,
	/// The app-server reported a model other than the explicit requested model.
	ModelMismatch,
	/// A `turn/start` result reported a method-invalid turn state.
	InvalidTurnStatus,
	/// A method returned turns or items that must be empty for this contract.
	UnexpectedResponseCollection,
	/// A method returned internally inconsistent configuration or turn metadata.
	ResponseSemanticMismatch,
}

/// Bounded caller-selected model identifier.
#[derive(Clone, Eq, PartialEq)]
pub struct ConversationModel(String);
impl ConversationModel {
	/// Validate explicit caller model input without choosing a default.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();

		validate_label(&value, MAX_CONVERSATION_MODEL_BYTES)
			.map_err(|()| ConversationContractError::InvalidModel)?;

		Ok(Self(value))
	}

	/// Return the exact accepted caller input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for ConversationModel {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ConversationModel([REDACTED])")
	}
}
impl Serialize for ConversationModel {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Bounded caller-selected reasoning-effort value.
#[derive(Clone, Eq, PartialEq)]
pub struct ConversationReasoningEffort(String);
impl ConversationReasoningEffort {
	/// Validate explicit caller reasoning input without choosing a default.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();

		validate_label(&value, MAX_CONVERSATION_REASONING_EFFORT_BYTES)
			.map_err(|()| ConversationContractError::InvalidReasoningEffort)?;

		Ok(Self(value))
	}

	/// Return the exact accepted caller input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for ConversationReasoningEffort {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ConversationReasoningEffort([REDACTED])")
	}
}
impl Serialize for ConversationReasoningEffort {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Exact executable Codex turn identifier.
#[derive(Clone, Eq, PartialEq)]
pub struct ExactTurnId(Zeroizing<String>);
impl ExactTurnId {
	/// Validate and retain one exact protocol identifier byte-for-byte.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = Zeroizing::new(value.into());

		validate_exact_id(value.as_str(), MAX_EXACT_TURN_ID_BYTES)
			.map_err(|()| ConversationContractError::InvalidTurnId)?;

		Ok(Self(value))
	}

	/// Return the exact identifier for one app-server request or equality check.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ExactTurnId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ExactTurnId([REDACTED])")
	}
}
impl Serialize for ExactTurnId {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Bounded caller-supplied developer instructions.
#[derive(Clone, Eq, PartialEq)]
pub struct ConversationInstructions(Zeroizing<String>);
impl ConversationInstructions {
	/// Validate and retain exact developer instructions.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = Zeroizing::new(value.into());

		validate_content(value.as_str(), MAX_CONVERSATION_INSTRUCTIONS_BYTES)
			.map_err(|()| ConversationContractError::InvalidInstructions)?;

		Ok(Self(value))
	}

	/// Return the exact accepted instruction text.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ConversationInstructions {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ConversationInstructions([REDACTED])")
	}
}
impl Serialize for ConversationInstructions {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Bounded caller-supplied user text.
#[derive(Clone, Eq, PartialEq)]
pub struct ConversationText(Zeroizing<String>);
impl ConversationText {
	/// Validate and retain one exact user text item.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = Zeroizing::new(value.into());

		validate_content(value.as_str(), MAX_CONVERSATION_TEXT_BYTES)
			.map_err(|()| ConversationContractError::InvalidText)?;

		Ok(Self(value))
	}

	/// Return the exact accepted user text.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ConversationText {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ConversationText([REDACTED])")
	}
}
impl Serialize for ConversationText {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Nonempty mechanically bounded text input for one turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationTurnInput {
	items: Vec<ConversationText>,
}
impl ConversationTurnInput {
	/// Construct one single-item text input.
	pub fn text(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		Ok(Self { items: vec![ConversationText::new(value)?] })
	}

	/// Construct one bounded ordered text collection.
	pub fn from_texts<I, S>(values: I) -> Result<Self, ConversationContractError>
	where
		I: IntoIterator<Item = S>,
		S: Into<String>,
	{
		let mut items = Vec::new();
		let mut total_bytes = 0_usize;

		for value in values {
			if items.len() == MAX_CONVERSATION_INPUT_ITEMS {
				return Err(ConversationContractError::InputItemLimitExceeded);
			}

			let text = ConversationText::new(value)?;

			total_bytes = total_bytes
				.checked_add(text.as_str().len())
				.ok_or(ConversationContractError::InputByteLimitExceeded)?;
			if total_bytes > MAX_CONVERSATION_INPUT_BYTES {
				return Err(ConversationContractError::InputByteLimitExceeded);
			}

			items.push(text);
		}

		if items.is_empty() {
			return Err(ConversationContractError::InputItemLimitExceeded);
		}

		Ok(Self { items })
	}

	/// Return accepted text items in app-server order.
	pub fn items(&self) -> &[ConversationText] {
		&self.items
	}
}

/// Bounded non-ephemeral `thread/start` request facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationThreadStartRequest {
	model: ConversationModel,
	cwd: ThreadCwd,
	developer_instructions: ConversationInstructions,
	fast: bool,
}
impl ConversationThreadStartRequest {
	/// Accept explicit caller configuration for one durable thread.
	pub fn new(
		model: impl Into<String>,
		cwd: impl Into<String>,
		developer_instructions: impl Into<String>,
	) -> Result<Self, ConversationContractError> {
		Ok(Self {
			model: ConversationModel::new(model)?,
			cwd: ThreadCwd::from_protocol(cwd)
				.map_err(|_| ConversationContractError::InvalidCwd)?,
			developer_instructions: ConversationInstructions::new(developer_instructions)?,
			fast: false,
		})
	}

	/// Select request-scoped Codex Fast mode without changing global configuration.
	pub const fn with_fast(mut self, fast: bool) -> Self {
		self.fast = fast;
		self
	}

	/// Caller-selected model sent to the app server.
	pub fn model(&self) -> &ConversationModel {
		&self.model
	}

	/// Exact absolute working directory.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Exact bounded developer instructions.
	pub fn developer_instructions(&self) -> &ConversationInstructions {
		&self.developer_instructions
	}

	/// Ordinary Conversation creation is always persistent.
	pub const fn ephemeral(&self) -> bool {
		false
	}
}
impl Serialize for ConversationThreadStartRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("ConversationThreadStartRequest", 5)?;

		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("cwd", self.cwd.as_str())?;
		request.serialize_field("developerInstructions", self.developer_instructions.as_str())?;
		request.serialize_field("ephemeral", &false)?;
		request.serialize_field("serviceTier", &self.fast.then_some("priority"))?;
		request.end()
	}
}

/// Bounded successful durable `thread/start` response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationThreadStartResponse {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: ConversationModel,
	reasoning_effort: Option<ConversationReasoningEffort>,
}
impl ConversationThreadStartResponse {
	fn from_wire(
		request: &ConversationThreadStartRequest,
		wire: ConversationThreadStartResponseWire,
	) -> Result<Self, ConversationContractError> {
		wire.validate_private_facts()?;
		let facts = validate_thread_response_facts(
			ThreadResponseContext::Start,
			request.cwd(),
			request.model(),
			wire.thread,
			wire.cwd.into_string(),
			wire.model,
			wire.reasoning_effort.map(ConversationReasoningEffortWire::into_string),
		)?;

		Ok(Self {
			thread_id: facts.thread_id,
			cwd: facts.cwd,
			model: facts.model,
			reasoning_effort: facts.reasoning_effort,
		})
	}

	/// Exact durable thread returned by the app server.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Bounded working directory returned by the app server.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Actual bounded model reported by the app server.
	pub fn model(&self) -> &ConversationModel {
		&self.model
	}

	/// Actual bounded reasoning effort reported by the app server, when present.
	pub fn reasoning_effort(&self) -> Option<&ConversationReasoningEffort> {
		self.reasoning_effort.as_ref()
	}
}

/// Decode one bounded exact `thread/start` result and bind it to its request facts.
pub fn decode_conversation_thread_start_response(
	request: &ConversationThreadStartRequest,
	bytes: &[u8],
) -> Result<ConversationThreadStartResponse, ConversationContractError> {
	validate_thread_response_shape(
		bytes,
		THREAD_START_RESPONSE_FIELDS,
		THREAD_START_RESPONSE_REQUIRED_FIELDS,
	)?;
	let wire = decode_response_wire(bytes)?;

	ConversationThreadStartResponse::from_wire(request, wire)
}

/// Bounded exact-ID `thread/resume` request facts.
///
/// This shape has no history or rollout-path field, so no alternate thread can take
/// precedence over `thread_id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationThreadResumeRequest {
	thread_id: ExactThreadId,
	model: ConversationModel,
	cwd: ThreadCwd,
	developer_instructions: ConversationInstructions,
	fast: bool,
}
impl ConversationThreadResumeRequest {
	/// Accept one exact thread and explicit caller configuration.
	pub fn new(
		thread_id: ExactThreadId,
		model: impl Into<String>,
		cwd: impl Into<String>,
		developer_instructions: impl Into<String>,
	) -> Result<Self, ConversationContractError> {
		Ok(Self {
			thread_id,
			model: ConversationModel::new(model)?,
			cwd: ThreadCwd::from_protocol(cwd)
				.map_err(|_| ConversationContractError::InvalidCwd)?,
			developer_instructions: ConversationInstructions::new(developer_instructions)?,
			fast: false,
		})
	}

	/// Select request-scoped Codex Fast mode without changing global configuration.
	pub const fn with_fast(mut self, fast: bool) -> Self {
		self.fast = fast;
		self
	}

	/// Exact caller-supplied thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Caller-selected model sent to the app server.
	pub fn model(&self) -> &ConversationModel {
		&self.model
	}

	/// Exact absolute working directory.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Exact bounded developer instructions.
	pub fn developer_instructions(&self) -> &ConversationInstructions {
		&self.developer_instructions
	}

	/// Ordinary Conversation resume requests load metadata without turn history.
	pub const fn exclude_turns(&self) -> bool {
		true
	}
}
impl Serialize for ConversationThreadResumeRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("ConversationThreadResumeRequest", 6)?;

		request.serialize_field("threadId", self.thread_id.as_str())?;
		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("cwd", self.cwd.as_str())?;
		request.serialize_field("developerInstructions", self.developer_instructions.as_str())?;
		request.serialize_field("excludeTurns", &true)?;
		request.serialize_field("serviceTier", &self.fast.then_some("priority"))?;
		request.end()
	}
}

/// Bounded successful exact-ID `thread/resume` response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationThreadResumeResponse {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: ConversationModel,
	reasoning_effort: Option<ConversationReasoningEffort>,
}
impl ConversationThreadResumeResponse {
	fn from_wire(
		request: &ConversationThreadResumeRequest,
		wire: ConversationThreadResumeResponseWire,
	) -> Result<Self, ConversationContractError> {
		wire.validate_private_facts()?;
		let facts = validate_thread_response_facts(
			ThreadResponseContext::Resume(request.thread_id()),
			request.cwd(),
			request.model(),
			wire.thread,
			wire.cwd.into_string(),
			wire.model,
			wire.reasoning_effort.map(ConversationReasoningEffortWire::into_string),
		)?;

		Ok(Self {
			thread_id: facts.thread_id,
			cwd: facts.cwd,
			model: facts.model,
			reasoning_effort: facts.reasoning_effort,
		})
	}

	/// Exact resumed thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Bounded live working directory returned at the top level by the resumed session.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Actual bounded model reported by the app server.
	pub fn model(&self) -> &ConversationModel {
		&self.model
	}

	/// Actual bounded reasoning effort reported by the app server, when present.
	pub fn reasoning_effort(&self) -> Option<&ConversationReasoningEffort> {
		self.reasoning_effort.as_ref()
	}
}

/// Decode one bounded exact `thread/resume` result and bind it to its request facts.
pub fn decode_conversation_thread_resume_response(
	request: &ConversationThreadResumeRequest,
	bytes: &[u8],
) -> Result<ConversationThreadResumeResponse, ConversationContractError> {
	validate_thread_response_shape(
		bytes,
		THREAD_RESUME_RESPONSE_FIELDS,
		THREAD_RESUME_RESPONSE_REQUIRED_FIELDS,
	)?;
	let wire = decode_response_wire(bytes)?;

	ConversationThreadResumeResponse::from_wire(request, wire)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConversationTextInput<'a>(&'a ConversationText);
impl Serialize for ConversationTextInput<'_> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut input = serializer.serialize_struct("ConversationTextInput", 2)?;

		input.serialize_field("type", "text")?;
		input.serialize_field("text", self.0.as_str())?;
		input.end()
	}
}

struct ConversationTextInputs<'a>(&'a [ConversationText]);
impl Serialize for ConversationTextInputs<'_> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut inputs = serializer.serialize_seq(Some(self.0.len()))?;

		for item in self.0 {
			inputs.serialize_element(&ConversationTextInput(item))?;
		}

		inputs.end()
	}
}

/// Bounded `turn/start` request facts with explicit caller-selected execution settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationTurnStartRequest {
	thread_id: ExactThreadId,
	input: ConversationTurnInput,
	model: ConversationModel,
	reasoning_effort: ConversationReasoningEffort,
	fast: bool,
	client_user_message_id: Option<String>,
}
impl ConversationTurnStartRequest {
	/// Accept one bounded turn without selecting or defaulting model settings.
	pub fn new(
		thread_id: ExactThreadId,
		input: ConversationTurnInput,
		model: impl Into<String>,
		reasoning_effort: impl Into<String>,
	) -> Result<Self, ConversationContractError> {
		Ok(Self {
			thread_id,
			input,
			model: ConversationModel::new(model)?,
			reasoning_effort: ConversationReasoningEffort::new(reasoning_effort)?,
			fast: false,
			client_user_message_id: None,
		})
	}

	/// Select request-scoped Codex Fast mode without changing global configuration.
	pub const fn with_fast(mut self, fast: bool) -> Self {
		self.fast = fast;
		self
	}

	/// Attach one caller-stable correlation identity to the user message.
	///
	/// This value is readback correlation only. It grants no retry or replay authority.
	pub fn with_client_user_message_id(
		mut self,
		client_user_message_id: impl Into<String>,
	) -> Result<Self, ConversationContractError> {
		let client_user_message_id = client_user_message_id.into();
		validate_exact_id(&client_user_message_id, MAX_EXACT_TURN_ID_BYTES)
			.map_err(|()| ConversationContractError::InvalidTurnId)?;
		self.client_user_message_id = Some(client_user_message_id);
		Ok(self)
	}

	/// Exact target thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Bounded ordered text input.
	pub fn input(&self) -> &ConversationTurnInput {
		&self.input
	}

	/// Caller-selected model.
	pub fn model(&self) -> &ConversationModel {
		&self.model
	}

	/// Caller-selected reasoning effort.
	pub fn reasoning_effort(&self) -> &ConversationReasoningEffort {
		&self.reasoning_effort
	}
}
impl Serialize for ConversationTurnStartRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("ConversationTurnStartRequest", 6)?;

		request.serialize_field("threadId", self.thread_id.as_str())?;
		request.serialize_field("input", &ConversationTextInputs(self.input.items()))?;
		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("effort", self.reasoning_effort.as_str())?;
		request.serialize_field("serviceTier", &self.fast.then_some("priority"))?;
		request.serialize_field("clientUserMessageId", &self.client_user_message_id)?;
		request.end()
	}
}

/// Closed turn state retained from one typed app-server response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationTurnStatus {
	/// Turn remains active.
	InProgress,
	/// Turn completed normally.
	Completed,
	/// Turn was interrupted.
	Interrupted,
	/// Turn failed.
	Failed,
}

/// Bounded successful `turn/start` response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationTurnStartResponse {
	turn_id: ExactTurnId,
	status: ConversationTurnStatus,
}
impl ConversationTurnStartResponse {
	fn from_wire(
		wire: ConversationTurnStartResponseWire,
	) -> Result<Self, ConversationContractError> {
		wire.turn.validate_start_facts()?;

		Ok(Self {
			turn_id: ExactTurnId::new(wire.turn.id)?,
			status: wire.turn.status.into_contract(),
		})
	}

	/// Exact turn returned by the app server.
	pub fn turn_id(&self) -> &ExactTurnId {
		&self.turn_id
	}

	/// Closed state returned for the turn.
	pub const fn status(&self) -> ConversationTurnStatus {
		self.status
	}
}

/// Decode one bounded exact `turn/start` result.
pub fn decode_conversation_turn_start_response(
	bytes: &[u8],
) -> Result<ConversationTurnStartResponse, ConversationContractError> {
	validate_turn_start_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	ConversationTurnStartResponse::from_wire(wire)
}

/// Exact `turn/interrupt` request facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnInterruptRequest {
	thread_id: ExactThreadId,
	turn_id: ExactTurnId,
}
impl ConversationTurnInterruptRequest {
	/// Bind an interrupt to one exact thread and active turn.
	pub fn new(thread_id: ExactThreadId, turn_id: ExactTurnId) -> Self {
		Self { thread_id, turn_id }
	}

	/// Exact target thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Exact target turn.
	pub fn turn_id(&self) -> &ExactTurnId {
		&self.turn_id
	}
}

/// Successful empty `turn/interrupt` response facts.
///
/// This fact records only protocol acceptance. Terminal turn state still comes from typed
/// notification or readback evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationTurnInterruptResponse {
	_private: (),
}
impl ConversationTurnInterruptResponse {
	fn from_wire(_: ConversationEmptySuccessWire) -> Self {
		Self { _private: () }
	}
}

/// Decode one bounded exact empty `turn/interrupt` result.
pub fn decode_conversation_turn_interrupt_response(
	bytes: &[u8],
) -> Result<ConversationTurnInterruptResponse, ConversationContractError> {
	validate_empty_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	Ok(ConversationTurnInterruptResponse::from_wire(wire))
}

/// Exact explicit `thread/archive` request facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationThreadArchiveRequest {
	thread_id: ExactThreadId,
}
impl ConversationThreadArchiveRequest {
	/// Bind explicit archive to one exact thread.
	pub fn new(thread_id: ExactThreadId) -> Self {
		Self { thread_id }
	}

	/// Exact target thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}
}

/// Successful empty `thread/archive` response facts.
///
/// This fact records only protocol acceptance. It does not replace exact archived-state
/// readback or durable duplicate-prevention authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationThreadArchiveResponse {
	_private: (),
}
impl ConversationThreadArchiveResponse {
	fn from_wire(_: ConversationEmptySuccessWire) -> Self {
		Self { _private: () }
	}
}

/// Decode one bounded exact empty `thread/archive` result.
pub fn decode_conversation_thread_archive_response(
	bytes: &[u8],
) -> Result<ConversationThreadArchiveResponse, ConversationContractError> {
	validate_empty_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	Ok(ConversationThreadArchiveResponse::from_wire(wire))
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationThreadStartResponseWire {
	thread: ConversationThreadResponseWire,
	model: String,
	model_provider: String,
	service_tier: Option<String>,
	cwd: ConversationAbsolutePathWire,
	#[serde(default)]
	runtime_workspace_roots: Vec<ConversationAbsolutePathWire>,
	#[serde(default)]
	instruction_sources: Vec<ConversationInstructionSourceWire>,
	approval_policy: ConversationAskForApprovalWire,
	approvals_reviewer: ConversationApprovalsReviewerWire,
	sandbox: ConversationSandboxPolicyWire,
	#[serde(default)]
	active_permission_profile: Option<ConversationActivePermissionProfileWire>,
	reasoning_effort: Option<ConversationReasoningEffortWire>,
	#[serde(default)]
	multi_agent_mode: ConversationMultiAgentModeWire,
}
impl ConversationThreadStartResponseWire {
	fn validate_private_facts(&self) -> Result<(), ConversationContractError> {
		self.sandbox.validate()?;
		if self.multi_agent_mode != ConversationMultiAgentModeWire::ExplicitRequestOnly {
			return Err(ConversationContractError::ResponseSemanticMismatch);
		}

		Ok(())
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationThreadResumeResponseWire {
	thread: ConversationThreadResponseWire,
	model: String,
	model_provider: String,
	service_tier: Option<String>,
	cwd: ConversationAbsolutePathWire,
	#[serde(default)]
	runtime_workspace_roots: Vec<ConversationAbsolutePathWire>,
	#[serde(default)]
	instruction_sources: Vec<ConversationInstructionSourceWire>,
	approval_policy: ConversationAskForApprovalWire,
	approvals_reviewer: ConversationApprovalsReviewerWire,
	sandbox: ConversationSandboxPolicyWire,
	#[serde(default)]
	active_permission_profile: Option<ConversationActivePermissionProfileWire>,
	reasoning_effort: Option<ConversationReasoningEffortWire>,
	#[serde(default)]
	multi_agent_mode: ConversationMultiAgentModeWire,
	#[serde(default)]
	initial_turns_page: Option<ConversationForbiddenValueWire>,
	#[serde(default)]
	turns_backwards_cursor: Option<String>,
	#[serde(default)]
	items_backwards_cursor: Option<String>,
}
impl ConversationThreadResumeResponseWire {
	fn validate_private_facts(&self) -> Result<(), ConversationContractError> {
		self.sandbox.validate()?;
		if self.multi_agent_mode != ConversationMultiAgentModeWire::ExplicitRequestOnly {
			return Err(ConversationContractError::ResponseSemanticMismatch);
		}
		if self.initial_turns_page.is_some() {
			return Err(ConversationContractError::UnexpectedResponseCollection);
		}

		Ok(())
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationThreadResponseWire {
	id: String,
	#[serde(default)]
	extra: Option<ConversationThreadExtraWire>,
	session_id: String,
	forked_from_id: Option<String>,
	parent_thread_id: Option<String>,
	preview: String,
	ephemeral: bool,
	#[serde(default)]
	history_mode: ConversationThreadHistoryModeWire,
	model_provider: String,
	created_at: i64,
	updated_at: i64,
	recency_at: Option<i64>,
	status: ConversationThreadStatusWire,
	path: Option<String>,
	cwd: ConversationAbsolutePathWire,
	cli_version: String,
	source: ConversationSessionSourceWire,
	can_accept_direct_input: Option<bool>,
	thread_source: Option<ConversationThreadSourceWire>,
	agent_nickname: Option<String>,
	agent_role: Option<String>,
	git_info: Option<ConversationGitInfoWire>,
	name: Option<String>,
	#[serde(default)]
	section: Option<ConversationThreadSectionWire>,
	#[serde(default)]
	section_entered_at: Option<i64>,
	turns: Vec<ConversationForbiddenValueWire>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationThreadSectionWire {
	id: String,
	name: String,
	#[serde(default)]
	appearance: Option<ConversationThreadSectionAppearanceWire>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationThreadSectionAppearanceWire {
	icon: Option<String>,
	color: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationGitInfoWire {
	sha: Option<String>,
	branch: Option<String>,
	origin_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationThreadExtraWire {}

#[derive(Clone, Eq, PartialEq)]
struct ConversationAbsolutePathWire(String);
impl ConversationAbsolutePathWire {
	fn into_string(self) -> String {
		self.0
	}
}
impl<'de> Deserialize<'de> for ConversationAbsolutePathWire {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;

		ThreadCwd::from_protocol(value.clone()).map_err(D::Error::custom)?;

		Ok(Self(value))
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(transparent)]
struct ConversationInstructionSourceWire(String);

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ConversationAskForApprovalWire {
	#[serde(rename = "untrusted")]
	UnlessTrusted,
	OnRequest,
	Granular(ConversationGranularApprovalWire),
	Never,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationGranularApprovalWire {
	sandbox_approval: bool,
	rules: bool,
	#[serde(default)]
	skill_approval: bool,
	#[serde(default)]
	request_permissions: bool,
	mcp_elicitations: bool,
}

#[derive(Deserialize)]
enum ConversationApprovalsReviewerWire {
	#[serde(rename = "user")]
	User,
	#[serde(rename = "auto_review")]
	AutoReview,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationActivePermissionProfileWire {
	id: String,
	#[serde(default)]
	extends: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum ConversationSandboxPolicyWire {
	DangerFullAccess,
	#[serde(rename_all = "camelCase")]
	ReadOnly {
		#[serde(default)]
		network_access: bool,
		#[serde(default)]
		access: Option<ConversationLegacyReadOnlyAccessWire>,
	},
	#[serde(rename_all = "camelCase")]
	ExternalSandbox {
		#[serde(default)]
		network_access: ConversationNetworkAccessWire,
	},
	#[serde(rename_all = "camelCase")]
	WorkspaceWrite {
		#[serde(default)]
		writable_roots: Vec<ConversationAbsolutePathWire>,
		#[serde(default)]
		read_only_access: Option<ConversationLegacyReadOnlyAccessWire>,
		#[serde(default)]
		network_access: bool,
		#[serde(default)]
		exclude_tmpdir_env_var: bool,
		#[serde(default)]
		exclude_slash_tmp: bool,
	},
}
impl ConversationSandboxPolicyWire {
	fn validate(&self) -> Result<(), ConversationContractError> {
		match self {
			Self::ReadOnly {
				access: Some(ConversationLegacyReadOnlyAccessWire::Restricted),
				..
			}
			| Self::WorkspaceWrite {
				read_only_access: Some(ConversationLegacyReadOnlyAccessWire::Restricted),
				..
			} => Err(ConversationContractError::MalformedResponse),
			_ => Ok(()),
		}
	}
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum ConversationLegacyReadOnlyAccessWire {
	FullAccess,
	Restricted,
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum ConversationNetworkAccessWire {
	#[default]
	Restricted,
	Enabled,
}

#[allow(dead_code)]
#[derive(Clone, Eq, PartialEq)]
enum ConversationReasoningEffortWire {
	None,
	Minimal,
	Low,
	Medium,
	High,
	XHigh,
	Max,
	Ultra,
	Custom(String),
}
impl ConversationReasoningEffortWire {
	fn into_string(self) -> String {
		match self {
			Self::None => "none".to_owned(),
			Self::Minimal => "minimal".to_owned(),
			Self::Low => "low".to_owned(),
			Self::Medium => "medium".to_owned(),
			Self::High => "high".to_owned(),
			Self::XHigh => "xhigh".to_owned(),
			Self::Max => "max".to_owned(),
			Self::Ultra => "ultra".to_owned(),
			Self::Custom(value) => value,
		}
	}
}
impl<'de> Deserialize<'de> for ConversationReasoningEffortWire {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;
		let effort = match value.as_str() {
			"none" => Self::None,
			"minimal" => Self::Minimal,
			"low" => Self::Low,
			"medium" => Self::Medium,
			"high" => Self::High,
			"xhigh" => Self::XHigh,
			"max" => Self::Max,
			"ultra" => Self::Ultra,
			"" => return Err(D::Error::custom("reasoning effort must not be empty")),
			_ => Self::Custom(value),
		};

		Ok(effort)
	}
}

#[allow(dead_code)]
#[derive(Clone, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum ConversationMultiAgentModeWire {
	None,
	Custom(String),
	#[default]
	ExplicitRequestOnly,
	Proactive,
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ConversationThreadHistoryModeWire {
	#[default]
	Legacy,
	Paginated,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum ConversationThreadStatusWire {
	NotLoaded,
	Idle,
	SystemError,
	Active {
		#[serde(rename = "activeFlags")]
		active_flags: Vec<ConversationThreadActiveFlagWire>,
	},
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ConversationThreadActiveFlagWire {
	WaitingOnApproval,
	WaitingOnUserInput,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ConversationSessionSourceWire {
	Cli,
	#[serde(rename = "vscode")]
	VsCode,
	Exec,
	AppServer,
	Custom(String),
	SubAgent(ConversationSubAgentSourceWire),
	#[serde(other)]
	Unknown,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConversationSubAgentSourceWire {
	Review,
	Compact,
	ThreadSpawn(ConversationThreadSpawnSourceWire),
	MemoryConsolidation,
	Other(String),
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationThreadSpawnSourceWire {
	parent_thread_id: ConversationUuidWire,
	depth: i32,
	#[serde(default)]
	agent_path: Option<ConversationAgentPathWire>,
	#[serde(default)]
	agent_nickname: Option<String>,
	#[serde(default)]
	agent_role: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(transparent)]
struct ConversationUuidWire(#[serde(deserialize_with = "deserialize_canonical_uuid")] String);

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(transparent)]
struct ConversationAgentPathWire(#[serde(deserialize_with = "deserialize_agent_path")] String);

#[allow(dead_code)]
enum ConversationThreadSourceWire {
	User,
	Subagent,
	Feature(String),
	MemoryConsolidation,
}
impl<'de> Deserialize<'de> for ConversationThreadSourceWire {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;
		Ok(match value.as_str() {
			"user" => Self::User,
			"subagent" => Self::Subagent,
			"memory_consolidation" => Self::MemoryConsolidation,
			_ => Self::Feature(value),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationTurnStartResponseWire {
	turn: ConversationTurnResponseWire,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationTurnResponseWire {
	id: String,
	items: Vec<ConversationForbiddenValueWire>,
	#[serde(default)]
	items_view: ConversationTurnItemsViewWire,
	status: ConversationTurnStatusWire,
	error: Option<ConversationTurnErrorWire>,
	started_at: Option<i64>,
	completed_at: Option<i64>,
	duration_ms: Option<i64>,
}
impl ConversationTurnResponseWire {
	fn validate_start_facts(&self) -> Result<(), ConversationContractError> {
		if !self.items.is_empty() {
			return Err(ConversationContractError::UnexpectedResponseCollection);
		}
		if self.status != ConversationTurnStatusWire::InProgress {
			return Err(ConversationContractError::InvalidTurnStatus);
		}
		if self.items_view != ConversationTurnItemsViewWire::NotLoaded
			|| self.error.is_some()
			|| self.started_at.is_some()
			|| self.completed_at.is_some()
			|| self.duration_ms.is_some()
		{
			return Err(ConversationContractError::ResponseSemanticMismatch);
		}

		Ok(())
	}
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum ConversationTurnItemsViewWire {
	NotLoaded,
	Summary,
	#[default]
	Full,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum ConversationTurnStatusWire {
	Completed,
	Interrupted,
	Failed,
	InProgress,
}
impl ConversationTurnStatusWire {
	const fn into_contract(self) -> ConversationTurnStatus {
		match self {
			Self::Completed => ConversationTurnStatus::Completed,
			Self::Interrupted => ConversationTurnStatus::Interrupted,
			Self::Failed => ConversationTurnStatus::Failed,
			Self::InProgress => ConversationTurnStatus::InProgress,
		}
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationTurnErrorWire {
	message: String,
	codex_error_info: Option<ConversationCodexErrorInfoWire>,
	#[serde(default)]
	additional_details: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ConversationCodexErrorInfoWire {
	ContextWindowExceeded,
	SessionBudgetExceeded,
	UsageLimitExceeded,
	ServerOverloaded,
	CyberPolicy,
	HttpConnectionFailed(ConversationHttpStatusWire),
	ResponseStreamConnectionFailed(ConversationHttpStatusWire),
	InternalServerError,
	Unauthorized,
	BadRequest,
	ThreadRollbackFailed,
	SandboxError,
	ResponseStreamDisconnected(ConversationHttpStatusWire),
	ResponseTooManyFailedAttempts(ConversationHttpStatusWire),
	ActiveTurnNotSteerable(ConversationActiveTurnNotSteerableWire),
	Other,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationHttpStatusWire {
	http_status_code: Option<u16>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationActiveTurnNotSteerableWire {
	turn_kind: ConversationNonSteerableTurnKindWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ConversationNonSteerableTurnKindWire {
	Review,
	Compact,
}

enum ConversationForbiddenValueWire {}
impl<'de> Deserialize<'de> for ConversationForbiddenValueWire {
	fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Err(D::Error::custom("value is forbidden for this method"))
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationEmptySuccessWire {}

struct ValidatedThreadResponseFacts {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: ConversationModel,
	reasoning_effort: Option<ConversationReasoningEffort>,
}

#[derive(Clone, Copy)]
enum ThreadResponseContext<'a> {
	Start,
	Resume(&'a ExactThreadId),
}

fn validate_thread_response_facts(
	context: ThreadResponseContext<'_>,
	expected_cwd: &ThreadCwd,
	expected_model: &ConversationModel,
	thread: ConversationThreadResponseWire,
	response_cwd: String,
	response_model: String,
	response_reasoning_effort: Option<String>,
) -> Result<ValidatedThreadResponseFacts, ConversationContractError> {
	if thread.ephemeral {
		return Err(ConversationContractError::EphemeralThreadRejected);
	}
	if !thread.turns.is_empty() {
		return Err(ConversationContractError::UnexpectedResponseCollection);
	}

	let thread_id =
		ExactThreadId::new(thread.id).map_err(|_| ConversationContractError::InvalidThreadId)?;
	if matches!(context, ThreadResponseContext::Resume(expected) if expected != &thread_id) {
		return Err(ConversationContractError::ThreadIdMismatch);
	}

	let thread_cwd = ThreadCwd::from_protocol(thread.cwd.into_string())
		.map_err(|_| ConversationContractError::InvalidCwd)?;
	let cwd = ThreadCwd::from_protocol(response_cwd)
		.map_err(|_| ConversationContractError::InvalidCwd)?;
	if expected_cwd != &cwd {
		return Err(ConversationContractError::CwdMismatch);
	}
	if matches!(context, ThreadResponseContext::Start) && thread_cwd != cwd {
		return Err(ConversationContractError::CwdMismatch);
	}

	let model = ConversationModel::new(response_model)?;
	if expected_model != &model {
		return Err(ConversationContractError::ModelMismatch);
	}

	let reasoning_effort =
		response_reasoning_effort.map(ConversationReasoningEffort::new).transpose()?;

	Ok(ValidatedThreadResponseFacts { thread_id, cwd, model, reasoning_effort })
}

const THREAD_START_RESPONSE_FIELDS: &[&str] = &[
	"thread",
	"model",
	"modelProvider",
	"serviceTier",
	"cwd",
	"runtimeWorkspaceRoots",
	"instructionSources",
	"approvalPolicy",
	"approvalsReviewer",
	"sandbox",
	"activePermissionProfile",
	"reasoningEffort",
	"multiAgentMode",
];
const THREAD_START_RESPONSE_REQUIRED_FIELDS: &[&str] =
	&["thread", "model", "modelProvider", "cwd", "approvalPolicy", "approvalsReviewer", "sandbox"];
const THREAD_RESUME_RESPONSE_FIELDS: &[&str] = &[
	"thread",
	"model",
	"modelProvider",
	"serviceTier",
	"cwd",
	"runtimeWorkspaceRoots",
	"instructionSources",
	"approvalPolicy",
	"approvalsReviewer",
	"sandbox",
	"activePermissionProfile",
	"reasoningEffort",
	"multiAgentMode",
	"initialTurnsPage",
	"turnsBackwardsCursor",
	"itemsBackwardsCursor",
];
const THREAD_RESUME_RESPONSE_REQUIRED_FIELDS: &[&str] =
	&["thread", "model", "modelProvider", "cwd", "approvalPolicy", "approvalsReviewer", "sandbox"];
const THREAD_RESPONSE_FIELDS: &[&str] = &[
	"id",
	"extra",
	"sessionId",
	"forkedFromId",
	"parentThreadId",
	"preview",
	"ephemeral",
	"historyMode",
	"modelProvider",
	"createdAt",
	"updatedAt",
	"recencyAt",
	"status",
	"path",
	"cwd",
	"cliVersion",
	"source",
	"canAcceptDirectInput",
	"threadSource",
	"agentNickname",
	"agentRole",
	"gitInfo",
	"name",
	"section",
	"sectionEnteredAt",
	"turns",
];
const THREAD_RESPONSE_REQUIRED_FIELDS: &[&str] = &[
	"id",
	"sessionId",
	"preview",
	"ephemeral",
	"modelProvider",
	"createdAt",
	"updatedAt",
	"status",
	"cwd",
	"cliVersion",
	"source",
	"turns",
];
const TURN_START_RESPONSE_FIELDS: &[&str] = &["turn"];
const TURN_START_RESPONSE_REQUIRED_FIELDS: &[&str] = &["turn"];
const TURN_RESPONSE_FIELDS: &[&str] =
	&["id", "items", "itemsView", "status", "error", "startedAt", "completedAt", "durationMs"];
const TURN_RESPONSE_REQUIRED_FIELDS: &[&str] = &["id", "items", "status"];

fn validate_thread_response_shape(
	bytes: &[u8],
	response_fields: &[&str],
	required_response_fields: &[&str],
) -> Result<(), ConversationContractError> {
	let value = decode_bounded_response_value(bytes)?;
	let response = response_object(&value)?;
	validate_object_fields(response, response_fields, required_response_fields)?;
	if response.get("initialTurnsPage").is_some_and(|page| !page.is_null()) {
		return Err(ConversationContractError::UnexpectedResponseCollection);
	}

	let thread = response
		.get("thread")
		.and_then(Value::as_object)
		.ok_or(ConversationContractError::MalformedResponse)?;
	validate_object_fields(thread, THREAD_RESPONSE_FIELDS, THREAD_RESPONSE_REQUIRED_FIELDS)?;

	let turns = thread
		.get("turns")
		.and_then(Value::as_array)
		.ok_or(ConversationContractError::MalformedResponse)?;
	if !turns.is_empty() {
		return Err(ConversationContractError::UnexpectedResponseCollection);
	}

	Ok(())
}

fn validate_turn_start_response_shape(bytes: &[u8]) -> Result<(), ConversationContractError> {
	let value = decode_bounded_response_value(bytes)?;
	let response = response_object(&value)?;
	validate_object_fields(
		response,
		TURN_START_RESPONSE_FIELDS,
		TURN_START_RESPONSE_REQUIRED_FIELDS,
	)?;

	let turn = response
		.get("turn")
		.and_then(Value::as_object)
		.ok_or(ConversationContractError::MalformedResponse)?;
	validate_object_fields(turn, TURN_RESPONSE_FIELDS, TURN_RESPONSE_REQUIRED_FIELDS)?;

	let items = turn
		.get("items")
		.and_then(Value::as_array)
		.ok_or(ConversationContractError::MalformedResponse)?;
	if !items.is_empty() {
		return Err(ConversationContractError::UnexpectedResponseCollection);
	}

	Ok(())
}

fn validate_empty_response_shape(bytes: &[u8]) -> Result<(), ConversationContractError> {
	let value = decode_bounded_response_value(bytes)?;
	let response = response_object(&value)?;
	validate_object_fields(response, &[], &[])
}

fn decode_bounded_response_value(bytes: &[u8]) -> Result<Value, ConversationContractError> {
	if bytes.len() > MAX_CONVERSATION_RESPONSE_BYTES {
		return Err(ConversationContractError::ResponseLimitExceeded);
	}

	serde_json::from_slice(bytes).map_err(|_| ConversationContractError::MalformedResponse)
}

fn decode_response_wire<'de, T>(bytes: &'de [u8]) -> Result<T, ConversationContractError>
where
	T: Deserialize<'de>,
{
	if bytes.len() > MAX_CONVERSATION_RESPONSE_BYTES {
		return Err(ConversationContractError::ResponseLimitExceeded);
	}

	serde_json::from_slice(bytes).map_err(|error| {
		let message = error.to_string();

		if message.contains("unknown field") {
			ConversationContractError::UnknownResponseField
		} else if message.contains("missing field") {
			ConversationContractError::MissingResponseField
		} else {
			ConversationContractError::MalformedResponse
		}
	})
}

fn response_object(value: &Value) -> Result<&Map<String, Value>, ConversationContractError> {
	value.as_object().ok_or(ConversationContractError::MalformedResponse)
}

fn validate_object_fields(
	object: &Map<String, Value>,
	allowed: &[&str],
	required: &[&str],
) -> Result<(), ConversationContractError> {
	if object.keys().any(|field| !allowed.contains(&field.as_str())) {
		return Err(ConversationContractError::UnknownResponseField);
	}
	if required.iter().any(|field| !object.contains_key(*field)) {
		return Err(ConversationContractError::MissingResponseField);
	}

	Ok(())
}

fn deserialize_canonical_uuid<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	let value = String::deserialize(deserializer)?;
	let bytes = value.as_bytes();
	let valid = bytes.len() == 36
		&& bytes.iter().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => *byte == b'-',
			_ => byte.is_ascii_hexdigit(),
		});

	if !valid {
		return Err(D::Error::custom("thread source id is not a canonical UUID"));
	}

	Ok(value)
}

fn deserialize_agent_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	let value = String::deserialize(deserializer)?;
	if value == "/morpheus" {
		return Ok(value);
	}

	let Some(path) = value.strip_prefix("/root") else {
		return Err(D::Error::custom("agent path is invalid"));
	};
	if path.is_empty() {
		return Ok(value);
	}
	let Some(path) = path.strip_prefix('/') else {
		return Err(D::Error::custom("agent path is invalid"));
	};
	if path.is_empty()
		|| path.ends_with('/')
		|| path.split('/').any(|segment| {
			segment.is_empty()
				|| matches!(segment, "root" | "." | "..")
				|| !segment
					.bytes()
					.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
		}) {
		return Err(D::Error::custom("agent path is invalid"));
	}

	Ok(value)
}

fn validate_label(value: &str, maximum: usize) -> Result<(), ()> {
	if value.is_empty()
		|| value.len() > maximum
		|| value.chars().any(|character| character.is_control())
	{
		return Err(());
	}

	Ok(())
}

fn validate_content(value: &str, maximum: usize) -> Result<(), ()> {
	if value.is_empty() || value.len() > maximum || value.contains('\0') {
		return Err(());
	}

	Ok(())
}

fn validate_exact_id(value: &str, maximum: usize) -> Result<(), ()> {
	validate_label(value, maximum)?;

	if value.contains(['"', '\\']) {
		return Err(());
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};

	use crate::ExactThreadId;

	use super::{
		ConversationContractError, ConversationThreadResumeRequest, ConversationThreadStartRequest,
		ConversationTurnInput, ConversationTurnStartRequest, ConversationTurnStatus,
		MAX_CONVERSATION_RESPONSE_BYTES, decode_conversation_thread_archive_response,
		decode_conversation_thread_resume_response, decode_conversation_thread_start_response,
		decode_conversation_turn_interrupt_response, decode_conversation_turn_start_response,
	};

	fn exact_thread() -> ExactThreadId {
		ExactThreadId::new("thread-1").expect("fixture thread ID must be valid")
	}

	fn start_request() -> ConversationThreadStartRequest {
		ConversationThreadStartRequest::new("gpt-5", "/workspace", "Follow the request.")
			.expect("fixture start request must be valid")
	}

	fn resume_request() -> ConversationThreadResumeRequest {
		ConversationThreadResumeRequest::new(
			exact_thread(),
			"gpt-5",
			"/workspace",
			"Follow the request.",
		)
		.expect("fixture resume request must be valid")
	}

	fn thread_response(thread_id: &str, model: &str, cwd: &str) -> Value {
		json!({
			"thread": {
				"id": thread_id,
				"sessionId": "session-1",
				"preview": "",
				"ephemeral": false,
				"modelProvider": "openai",
				"createdAt": 1,
				"updatedAt": 1,
				"status": {"type": "idle"},
				"cwd": cwd,
				"cliVersion": "test",
				"source": "appServer",
				"turns": [],
			},
			"model": model,
			"modelProvider": "openai",
			"cwd": cwd,
			"approvalPolicy": "never",
			"approvalsReviewer": "user",
			"sandbox": {"type": "dangerFullAccess"},
			"reasoningEffort": "high",
			"multiAgentMode": "explicitRequestOnly",
		})
	}

	#[test]
	fn resume_serialization_excludes_turns_and_omits_history_path_and_page_fields() {
		let encoded = serde_json::to_value(resume_request()).expect("request must serialize");

		assert_eq!(
			encoded,
			json!({
				"threadId": "thread-1",
				"model": "gpt-5",
				"cwd": "/workspace",
				"developerInstructions": "Follow the request.",
				"excludeTurns": true,
				"serviceTier": null,
			})
		);
		let object = encoded.as_object().expect("request must remain an object");
		for forbidden in [
			"history",
			"historyMode",
			"path",
			"initialTurnsPage",
			"turnsBackwardsCursor",
			"itemsBackwardsCursor",
		] {
			assert!(!object.contains_key(forbidden));
		}
	}

	#[test]
	fn fast_mode_is_explicitly_request_scoped_for_thread_start_and_resume() {
		let start = serde_json::to_value(start_request().with_fast(true))
			.expect("fast thread start request must serialize");
		let resume = serde_json::to_value(resume_request().with_fast(true))
			.expect("fast thread resume request must serialize");

		assert_eq!(start.get("serviceTier"), Some(&json!("priority")));
		assert_eq!(resume.get("serviceTier"), Some(&json!("priority")));
	}

	#[test]
	fn turn_execution_settings_include_model_effort_and_explicit_fast_tier() {
		let turn = ConversationTurnStartRequest::new(
			exact_thread(),
			ConversationTurnInput::text("Continue.").expect("turn input is valid"),
			"gpt-5.6-terra",
			"xhigh",
		)
		.expect("turn request is valid")
		.with_client_user_message_id("50000000-0000-4000-8000-000000000001")
		.expect("client user-message identity is valid")
		.with_fast(true);
		let encoded = serde_json::to_value(turn).expect("turn request must serialize");

		assert_eq!(encoded.get("model"), Some(&json!("gpt-5.6-terra")));
		assert_eq!(encoded.get("effort"), Some(&json!("xhigh")));
		assert_eq!(encoded.get("serviceTier"), Some(&json!("priority")));
		assert_eq!(
			encoded.get("clientUserMessageId"),
			Some(&json!("50000000-0000-4000-8000-000000000001")),
		);
	}

	#[test]
	fn pinned_method_results_mint_only_typed_conversation_success() {
		let start_request = start_request();
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let thread_bytes = serde_json::to_vec(&canonical).expect("fixture response must serialize");
		let start = decode_conversation_thread_start_response(&start_request, &thread_bytes)
			.expect("pinned thread/start response must decode");

		assert_eq!(start.thread_id().as_str(), "thread-1");
		assert_eq!(start.cwd().as_str(), "/workspace");
		assert_eq!(start.model().as_str(), "gpt-5");
		assert_eq!(start.reasoning_effort().map(|effort| effort.as_str()), Some("high"));

		let mut canonical_auto_review = canonical.clone();
		canonical_auto_review["approvalsReviewer"] = json!("auto_review");
		assert!(
			decode_conversation_thread_start_response(
				&start_request,
				&serde_json::to_vec(&canonical_auto_review).unwrap(),
			)
			.is_ok()
		);

		let resume = decode_conversation_thread_resume_response(&resume_request(), &thread_bytes)
			.expect("pinned thread/resume response must decode");

		assert_eq!(resume.thread_id().as_str(), "thread-1");
		assert_eq!(resume.cwd().as_str(), "/workspace");
		assert_eq!(resume.model().as_str(), "gpt-5");

		let turn = decode_conversation_turn_start_response(
			&serde_json::to_vec(&json!({
				"turn": {
					"id": "turn-1",
					"items": [],
					"itemsView": "notLoaded",
					"status": "inProgress",
				},
			}))
			.expect("fixture response must serialize"),
		)
		.expect("pinned turn/start response must decode");

		assert_eq!(turn.turn_id().as_str(), "turn-1");
		assert_eq!(turn.status(), ConversationTurnStatus::InProgress);
		assert!(decode_conversation_turn_interrupt_response(b"{}").is_ok());
		assert!(decode_conversation_thread_archive_response(b"{}").is_ok());
	}

	#[test]
	fn current_thread_section_fields_decode_when_omitted_null_or_populated() {
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let omitted = canonical.clone();

		let mut null = canonical.clone();
		null["thread"]["section"] = Value::Null;
		null["thread"]["sectionEnteredAt"] = Value::Null;

		let mut populated = canonical;
		populated["thread"]["section"] = json!({"id": "section-1", "name": "Active"});
		populated["thread"]["sectionEnteredAt"] = json!(2);

		for (case, response) in [("omitted", omitted), ("null", null), ("populated", populated)] {
			let bytes = serde_json::to_vec(&response).expect("fixture response must serialize");

			assert!(
				decode_conversation_thread_start_response(&start_request(), &bytes).is_ok(),
				"thread/start must accept {case} section fields",
			);
			assert!(
				decode_conversation_thread_resume_response(&resume_request(), &bytes).is_ok(),
				"thread/resume must accept {case} section fields",
			);
		}
	}

	#[test]
	fn thread_section_appearance_decodes_when_omitted_null_or_populated() {
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let cases = [
			("omitted", json!({"id": "section-1", "name": "Active"})),
			("null", json!({"id": "section-1", "name": "Active", "appearance": null})),
			(
				"populated",
				json!({
					"id": "section-1",
					"name": "Active",
					"appearance": {"icon": "folder", "color": "blue"},
				}),
			),
			(
				"populated with omitted metadata",
				json!({"id": "section-1", "name": "Active", "appearance": {}}),
			),
			(
				"populated with null metadata",
				json!({
					"id": "section-1",
					"name": "Active",
					"appearance": {"icon": null, "color": null},
				}),
			),
		];

		for (case, section) in cases {
			let mut response = canonical.clone();
			response["thread"]["section"] = section;
			let bytes = serde_json::to_vec(&response).expect("fixture response must serialize");

			assert!(
				decode_conversation_thread_start_response(&start_request(), &bytes).is_ok(),
				"thread/start must accept {case} section appearance",
			);
			assert!(
				decode_conversation_thread_resume_response(&resume_request(), &bytes).is_ok(),
				"thread/resume must accept {case} section appearance",
			);
		}
	}

	#[test]
	fn malformed_or_unknown_thread_section_appearance_fields_are_rejected() {
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let cases = [
			(
				"non-object appearance",
				json!("folder"),
				ConversationContractError::MalformedResponse,
			),
			(
				"non-string icon",
				json!({"icon": 1, "color": null}),
				ConversationContractError::MalformedResponse,
			),
			(
				"non-string color",
				json!({"icon": null, "color": true}),
				ConversationContractError::MalformedResponse,
			),
			(
				"unknown appearance field",
				json!({"icon": null, "color": null, "unexpected": true}),
				ConversationContractError::UnknownResponseField,
			),
		];

		for (case, appearance, expected) in cases {
			let mut response = canonical.clone();
			response["thread"]["section"] = json!({
				"id": "section-1",
				"name": "Active",
				"appearance": appearance,
			});
			let bytes = serde_json::to_vec(&response).expect("fixture response must serialize");

			assert_eq!(
				decode_conversation_thread_start_response(&start_request(), &bytes).map(|_| ()),
				Err(expected),
				"thread/start {case}",
			);
			assert_eq!(
				decode_conversation_thread_resume_response(&resume_request(), &bytes).map(|_| ()),
				Err(expected),
				"thread/resume {case}",
			);
		}
	}

	#[test]
	fn malformed_or_unknown_thread_section_fields_are_rejected() {
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let cases = [
			(
				"missing id",
				json!({"name": "Active"}),
				ConversationContractError::MissingResponseField,
			),
			(
				"missing name",
				json!({"id": "section-1"}),
				ConversationContractError::MissingResponseField,
			),
			(
				"non-string id",
				json!({"id": 1, "name": "Active"}),
				ConversationContractError::MalformedResponse,
			),
			(
				"non-string name",
				json!({"id": "section-1", "name": 1}),
				ConversationContractError::MalformedResponse,
			),
			(
				"non-object section",
				json!("section-1"),
				ConversationContractError::MalformedResponse,
			),
			(
				"unknown nested field",
				json!({"id": "section-1", "name": "Active", "unexpected": true}),
				ConversationContractError::UnknownResponseField,
			),
		];

		for (case, section, expected) in cases {
			let mut response = canonical.clone();
			response["thread"]["section"] = section;

			assert_eq!(
				decode_conversation_thread_start_response(
					&start_request(),
					&serde_json::to_vec(&response).expect("fixture response must serialize"),
				)
				.map(|_| ()),
				Err(expected),
				"{case}",
			);
		}

		let mut malformed_entered_at = canonical;
		malformed_entered_at["thread"]["sectionEnteredAt"] = json!("2");

		assert_eq!(
			decode_conversation_thread_start_response(
				&start_request(),
				&serde_json::to_vec(&malformed_entered_at)
					.expect("fixture response must serialize"),
			)
			.map(|_| ()),
			Err(ConversationContractError::MalformedResponse),
			"non-integer section-entered timestamp",
		);
	}

	#[test]
	fn resume_uses_live_response_cwd_when_persisted_thread_cwd_differs() {
		let mut response = thread_response("thread-1", "gpt-5", "/workspace");
		response["thread"]["cwd"] = json!("/persisted");
		let bytes = serde_json::to_vec(&response).expect("fixture response must serialize");

		let resume = decode_conversation_thread_resume_response(&resume_request(), &bytes)
			.expect("resume must allow distinct persisted thread metadata cwd");

		assert_eq!(resume.cwd().as_str(), "/workspace");
	}

	#[test]
	fn resume_rejects_live_response_cwd_that_differs_from_request() {
		let mut response = thread_response("thread-1", "gpt-5", "/other");
		response["thread"]["cwd"] = json!("/persisted");

		assert_eq!(
			decode_conversation_thread_resume_response(
				&resume_request(),
				&serde_json::to_vec(&response).expect("fixture response must serialize"),
			)
			.map(|_| ()),
			Err(ConversationContractError::CwdMismatch),
		);
	}

	#[test]
	fn start_rejects_nested_thread_cwd_that_differs_from_live_response() {
		let mut response = thread_response("thread-1", "gpt-5", "/workspace");
		response["thread"]["cwd"] = json!("/persisted");

		assert_eq!(
			decode_conversation_thread_start_response(
				&start_request(),
				&serde_json::to_vec(&response).expect("fixture response must serialize"),
			)
			.map(|_| ()),
			Err(ConversationContractError::CwdMismatch),
		);
	}

	#[test]
	fn resume_rejects_relative_persisted_thread_cwd() {
		let mut response = thread_response("thread-1", "gpt-5", "/workspace");
		response["thread"]["cwd"] = json!("persisted");

		assert_eq!(
			decode_conversation_thread_resume_response(
				&resume_request(),
				&serde_json::to_vec(&response).expect("fixture response must serialize"),
			)
			.map(|_| ()),
			Err(ConversationContractError::MalformedResponse),
		);
	}

	#[test]
	fn malformed_or_unknown_wire_shape_failures_never_mint_conversation_success() {
		let start_request = start_request();
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");

		let mut unknown_nested = canonical.clone();
		unknown_nested["thread"]["unexpected"] = json!(true);

		let mut legacy_reviewer = canonical.clone();
		legacy_reviewer["approvalsReviewer"] = json!("guardian_subagent");

		let mut legacy_agent_type = canonical.clone();
		legacy_agent_type["thread"]["source"] = json!({
			"subAgent": {
				"thread_spawn": {
					"parent_thread_id": "00000000-0000-4000-8000-000000000001",
					"depth": 1,
					"agent_type": "reviewer",
				},
			},
		});

		let duplicate_nested = serde_json::to_string(&canonical)
			.expect("fixture response must serialize")
			.replacen(r#""id":"thread-1""#, r#""id":"thread-1","id":"thread-1""#, 1)
			.into_bytes();

		let mut malformed_nested = canonical.clone();
		malformed_nested["sandbox"] = json!({"type": "readOnly", "access": {"type": "restricted"}});

		let cases = [
			(
				"unknown nested field",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&unknown_nested).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::UnknownResponseField,
			),
			(
				"legacy approvals reviewer",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&legacy_reviewer).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::MalformedResponse,
			),
			(
				"legacy thread-spawn agent type",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&legacy_agent_type).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::UnknownResponseField,
			),
			(
				"duplicate nested field",
				decode_conversation_thread_start_response(&start_request, &duplicate_nested)
					.map(|_| ()),
				ConversationContractError::MalformedResponse,
			),
			(
				"malformed nested value",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&malformed_nested).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::MalformedResponse,
			),
		];

		for (case, actual, expected) in cases {
			assert_eq!(actual, Err(expected), "{case}");
		}
	}

	#[test]
	fn semantic_identity_collection_or_bounds_failures_never_mint_conversation_success() {
		let start_request = start_request();
		let resume_request = resume_request();
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");

		let mut nonempty_turns = canonical.clone();
		nonempty_turns["thread"]["turns"] = json!([{}]);

		let mut wrong_thread = canonical.clone();
		wrong_thread["thread"]["id"] = json!("thread-2");

		let mut wrong_model = canonical.clone();
		wrong_model["model"] = json!("gpt-other");

		let mut wrong_cwd = canonical;
		wrong_cwd["cwd"] = json!("/other");

		let nonempty_items = json!({
			"turn": {
				"id": "turn-1",
				"items": [{}],
				"itemsView": "notLoaded",
				"status": "inProgress",
			},
		});
		let oversized = vec![b' '; MAX_CONVERSATION_RESPONSE_BYTES + 1];

		let cases = [
			(
				"nonempty turns",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&nonempty_turns).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::UnexpectedResponseCollection,
			),
			(
				"nonempty items",
				decode_conversation_turn_start_response(
					&serde_json::to_vec(&nonempty_items).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::UnexpectedResponseCollection,
			),
			(
				"wrong thread",
				decode_conversation_thread_resume_response(
					&resume_request,
					&serde_json::to_vec(&wrong_thread).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::ThreadIdMismatch,
			),
			(
				"wrong model",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&wrong_model).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::ModelMismatch,
			),
			(
				"wrong cwd",
				decode_conversation_thread_start_response(
					&start_request,
					&serde_json::to_vec(&wrong_cwd).unwrap(),
				)
				.map(|_| ()),
				ConversationContractError::CwdMismatch,
			),
			(
				"oversized result",
				decode_conversation_turn_interrupt_response(&oversized).map(|_| ()),
				ConversationContractError::ResponseLimitExceeded,
			),
		];

		for (case, actual, expected) in cases {
			assert_eq!(actual, Err(expected), "{case}");
		}
	}
}
