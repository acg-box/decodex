//! Pure bounded facts for the ordinary Quick Task app-server conversation.
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
pub const MAX_QUICK_TASK_MODEL_BYTES: usize = 128;
/// Maximum UTF-8 bytes in one caller-selected reasoning-effort value.
pub const MAX_QUICK_TASK_REASONING_EFFORT_BYTES: usize = 32;
/// Maximum UTF-8 bytes in developer instructions.
pub const MAX_QUICK_TASK_INSTRUCTIONS_BYTES: usize = 64 * 1_024;
/// Maximum UTF-8 bytes in one text input item.
pub const MAX_QUICK_TASK_TEXT_BYTES: usize = 256 * 1_024;
/// Maximum text items in one turn request.
pub const MAX_QUICK_TASK_INPUT_ITEMS: usize = 16;
/// Maximum aggregate UTF-8 bytes in one turn request.
pub const MAX_QUICK_TASK_INPUT_BYTES: usize = 256 * 1_024;
/// Maximum UTF-8 bytes in an exact Codex turn identifier.
pub const MAX_EXACT_TURN_ID_BYTES: usize = 1_024;
/// Maximum bytes accepted from one exact Quick Task method result payload.
pub const MAX_QUICK_TASK_RESPONSE_BYTES: usize = MAX_APP_SERVER_FRAME_BYTES;

/// Closed ordinary Quick Task app-server method set.
///
/// `turn/steer` is intentionally absent. Ordinary conversation sends subsequent user input
/// through another `turn/start`; this contract owns no active-turn steering workflow.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuickTaskMethod {
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
impl QuickTaskMethod {
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

/// Closed notification set needed to project one ordinary Quick Task turn.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuickTaskNotification {
	/// Bounded user-visible model text.
	AgentMessageDelta,
	/// Terminal state for one exact turn.
	TurnCompleted,
}
impl QuickTaskNotification {
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
pub enum QuickTaskContractError {
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
	/// App-server response cwd facts disagreed with each other or with the request.
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
pub struct QuickTaskModel(String);
impl QuickTaskModel {
	/// Validate explicit caller model input without choosing a default.
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = value.into();

		validate_label(&value, MAX_QUICK_TASK_MODEL_BYTES)
			.map_err(|()| QuickTaskContractError::InvalidModel)?;

		Ok(Self(value))
	}

	/// Return the exact accepted caller input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for QuickTaskModel {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskModel([REDACTED])")
	}
}
impl Serialize for QuickTaskModel {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Bounded caller-selected reasoning-effort value.
#[derive(Clone, Eq, PartialEq)]
pub struct QuickTaskReasoningEffort(String);
impl QuickTaskReasoningEffort {
	/// Validate explicit caller reasoning input without choosing a default.
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = value.into();

		validate_label(&value, MAX_QUICK_TASK_REASONING_EFFORT_BYTES)
			.map_err(|()| QuickTaskContractError::InvalidReasoningEffort)?;

		Ok(Self(value))
	}

	/// Return the exact accepted caller input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for QuickTaskReasoningEffort {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskReasoningEffort([REDACTED])")
	}
}
impl Serialize for QuickTaskReasoningEffort {
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
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = Zeroizing::new(value.into());

		validate_exact_id(value.as_str(), MAX_EXACT_TURN_ID_BYTES)
			.map_err(|()| QuickTaskContractError::InvalidTurnId)?;

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
pub struct QuickTaskInstructions(Zeroizing<String>);
impl QuickTaskInstructions {
	/// Validate and retain exact developer instructions.
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = Zeroizing::new(value.into());

		validate_content(value.as_str(), MAX_QUICK_TASK_INSTRUCTIONS_BYTES)
			.map_err(|()| QuickTaskContractError::InvalidInstructions)?;

		Ok(Self(value))
	}

	/// Return the exact accepted instruction text.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for QuickTaskInstructions {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskInstructions([REDACTED])")
	}
}
impl Serialize for QuickTaskInstructions {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Bounded caller-supplied user text.
#[derive(Clone, Eq, PartialEq)]
pub struct QuickTaskText(Zeroizing<String>);
impl QuickTaskText {
	/// Validate and retain one exact user text item.
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = Zeroizing::new(value.into());

		validate_content(value.as_str(), MAX_QUICK_TASK_TEXT_BYTES)
			.map_err(|()| QuickTaskContractError::InvalidText)?;

		Ok(Self(value))
	}

	/// Return the exact accepted user text.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for QuickTaskText {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskText([REDACTED])")
	}
}
impl Serialize for QuickTaskText {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Nonempty mechanically bounded text input for one turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskTurnInput {
	items: Vec<QuickTaskText>,
}
impl QuickTaskTurnInput {
	/// Construct one single-item text input.
	pub fn text(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		Ok(Self { items: vec![QuickTaskText::new(value)?] })
	}

	/// Construct one bounded ordered text collection.
	pub fn from_texts<I, S>(values: I) -> Result<Self, QuickTaskContractError>
	where
		I: IntoIterator<Item = S>,
		S: Into<String>,
	{
		let mut items = Vec::new();
		let mut total_bytes = 0_usize;

		for value in values {
			if items.len() == MAX_QUICK_TASK_INPUT_ITEMS {
				return Err(QuickTaskContractError::InputItemLimitExceeded);
			}

			let text = QuickTaskText::new(value)?;

			total_bytes = total_bytes
				.checked_add(text.as_str().len())
				.ok_or(QuickTaskContractError::InputByteLimitExceeded)?;
			if total_bytes > MAX_QUICK_TASK_INPUT_BYTES {
				return Err(QuickTaskContractError::InputByteLimitExceeded);
			}

			items.push(text);
		}

		if items.is_empty() {
			return Err(QuickTaskContractError::InputItemLimitExceeded);
		}

		Ok(Self { items })
	}

	/// Return accepted text items in app-server order.
	pub fn items(&self) -> &[QuickTaskText] {
		&self.items
	}
}

/// Bounded non-ephemeral `thread/start` request facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadStartRequest {
	model: QuickTaskModel,
	cwd: ThreadCwd,
	developer_instructions: QuickTaskInstructions,
}
impl QuickTaskThreadStartRequest {
	/// Accept explicit caller configuration for one durable thread.
	pub fn new(
		model: impl Into<String>,
		cwd: impl Into<String>,
		developer_instructions: impl Into<String>,
	) -> Result<Self, QuickTaskContractError> {
		Ok(Self {
			model: QuickTaskModel::new(model)?,
			cwd: ThreadCwd::from_protocol(cwd).map_err(|_| QuickTaskContractError::InvalidCwd)?,
			developer_instructions: QuickTaskInstructions::new(developer_instructions)?,
		})
	}

	/// Caller-selected model sent to the app server.
	pub fn model(&self) -> &QuickTaskModel {
		&self.model
	}

	/// Exact absolute working directory.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Exact bounded developer instructions.
	pub fn developer_instructions(&self) -> &QuickTaskInstructions {
		&self.developer_instructions
	}

	/// Ordinary Quick Task creation is always persistent.
	pub const fn ephemeral(&self) -> bool {
		false
	}
}
impl Serialize for QuickTaskThreadStartRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("QuickTaskThreadStartRequest", 4)?;

		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("cwd", self.cwd.as_str())?;
		request.serialize_field("developerInstructions", self.developer_instructions.as_str())?;
		request.serialize_field("ephemeral", &false)?;
		request.end()
	}
}

/// Bounded successful durable `thread/start` response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadStartResponse {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: QuickTaskModel,
	reasoning_effort: Option<QuickTaskReasoningEffort>,
}
impl QuickTaskThreadStartResponse {
	fn from_wire(
		request: &QuickTaskThreadStartRequest,
		wire: QuickTaskThreadStartResponseWire,
	) -> Result<Self, QuickTaskContractError> {
		wire.validate_private_facts()?;
		let facts = validate_thread_response_facts(
			None,
			request.cwd(),
			request.model(),
			wire.thread,
			wire.cwd.into_string(),
			wire.model,
			wire.reasoning_effort.map(QuickTaskReasoningEffortWire::into_string),
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
	pub fn model(&self) -> &QuickTaskModel {
		&self.model
	}

	/// Actual bounded reasoning effort reported by the app server, when present.
	pub fn reasoning_effort(&self) -> Option<&QuickTaskReasoningEffort> {
		self.reasoning_effort.as_ref()
	}
}

/// Decode one bounded exact `thread/start` result and bind it to its request facts.
pub fn decode_quick_task_thread_start_response(
	request: &QuickTaskThreadStartRequest,
	bytes: &[u8],
) -> Result<QuickTaskThreadStartResponse, QuickTaskContractError> {
	validate_thread_response_shape(
		bytes,
		THREAD_START_RESPONSE_FIELDS,
		THREAD_START_RESPONSE_REQUIRED_FIELDS,
	)?;
	let wire = decode_response_wire(bytes)?;

	QuickTaskThreadStartResponse::from_wire(request, wire)
}

/// Bounded exact-ID `thread/resume` request facts.
///
/// This shape has no history or rollout-path field, so no alternate thread can take
/// precedence over `thread_id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadResumeRequest {
	thread_id: ExactThreadId,
	model: QuickTaskModel,
	cwd: ThreadCwd,
	developer_instructions: QuickTaskInstructions,
}
impl QuickTaskThreadResumeRequest {
	/// Accept one exact thread and explicit caller configuration.
	pub fn new(
		thread_id: ExactThreadId,
		model: impl Into<String>,
		cwd: impl Into<String>,
		developer_instructions: impl Into<String>,
	) -> Result<Self, QuickTaskContractError> {
		Ok(Self {
			thread_id,
			model: QuickTaskModel::new(model)?,
			cwd: ThreadCwd::from_protocol(cwd).map_err(|_| QuickTaskContractError::InvalidCwd)?,
			developer_instructions: QuickTaskInstructions::new(developer_instructions)?,
		})
	}

	/// Exact caller-supplied thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Caller-selected model sent to the app server.
	pub fn model(&self) -> &QuickTaskModel {
		&self.model
	}

	/// Exact absolute working directory.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Exact bounded developer instructions.
	pub fn developer_instructions(&self) -> &QuickTaskInstructions {
		&self.developer_instructions
	}

	/// Ordinary Quick Task resume requests load metadata without turn history.
	pub const fn exclude_turns(&self) -> bool {
		true
	}
}
impl Serialize for QuickTaskThreadResumeRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("QuickTaskThreadResumeRequest", 5)?;

		request.serialize_field("threadId", self.thread_id.as_str())?;
		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("cwd", self.cwd.as_str())?;
		request.serialize_field("developerInstructions", self.developer_instructions.as_str())?;
		request.serialize_field("excludeTurns", &true)?;
		request.end()
	}
}

/// Bounded successful exact-ID `thread/resume` response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadResumeResponse {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: QuickTaskModel,
	reasoning_effort: Option<QuickTaskReasoningEffort>,
}
impl QuickTaskThreadResumeResponse {
	fn from_wire(
		request: &QuickTaskThreadResumeRequest,
		wire: QuickTaskThreadResumeResponseWire,
	) -> Result<Self, QuickTaskContractError> {
		wire.validate_private_facts()?;
		let facts = validate_thread_response_facts(
			Some(request.thread_id()),
			request.cwd(),
			request.model(),
			wire.thread,
			wire.cwd.into_string(),
			wire.model,
			wire.reasoning_effort.map(QuickTaskReasoningEffortWire::into_string),
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

	/// Bounded working directory returned by the app server.
	pub fn cwd(&self) -> &ThreadCwd {
		&self.cwd
	}

	/// Actual bounded model reported by the app server.
	pub fn model(&self) -> &QuickTaskModel {
		&self.model
	}

	/// Actual bounded reasoning effort reported by the app server, when present.
	pub fn reasoning_effort(&self) -> Option<&QuickTaskReasoningEffort> {
		self.reasoning_effort.as_ref()
	}
}

/// Decode one bounded exact `thread/resume` result and bind it to its request facts.
pub fn decode_quick_task_thread_resume_response(
	request: &QuickTaskThreadResumeRequest,
	bytes: &[u8],
) -> Result<QuickTaskThreadResumeResponse, QuickTaskContractError> {
	validate_thread_response_shape(
		bytes,
		THREAD_RESUME_RESPONSE_FIELDS,
		THREAD_RESUME_RESPONSE_REQUIRED_FIELDS,
	)?;
	let wire = decode_response_wire(bytes)?;

	QuickTaskThreadResumeResponse::from_wire(request, wire)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QuickTaskTextInput<'a>(&'a QuickTaskText);
impl Serialize for QuickTaskTextInput<'_> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut input = serializer.serialize_struct("QuickTaskTextInput", 2)?;

		input.serialize_field("type", "text")?;
		input.serialize_field("text", self.0.as_str())?;
		input.end()
	}
}

struct QuickTaskTextInputs<'a>(&'a [QuickTaskText]);
impl Serialize for QuickTaskTextInputs<'_> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut inputs = serializer.serialize_seq(Some(self.0.len()))?;

		for item in self.0 {
			inputs.serialize_element(&QuickTaskTextInput(item))?;
		}

		inputs.end()
	}
}

/// Bounded `turn/start` request facts with explicit caller-selected execution settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskTurnStartRequest {
	thread_id: ExactThreadId,
	input: QuickTaskTurnInput,
	model: QuickTaskModel,
	reasoning_effort: QuickTaskReasoningEffort,
}
impl QuickTaskTurnStartRequest {
	/// Accept one bounded turn without selecting or defaulting model settings.
	pub fn new(
		thread_id: ExactThreadId,
		input: QuickTaskTurnInput,
		model: impl Into<String>,
		reasoning_effort: impl Into<String>,
	) -> Result<Self, QuickTaskContractError> {
		Ok(Self {
			thread_id,
			input,
			model: QuickTaskModel::new(model)?,
			reasoning_effort: QuickTaskReasoningEffort::new(reasoning_effort)?,
		})
	}

	/// Exact target thread.
	pub fn thread_id(&self) -> &ExactThreadId {
		&self.thread_id
	}

	/// Bounded ordered text input.
	pub fn input(&self) -> &QuickTaskTurnInput {
		&self.input
	}

	/// Caller-selected model.
	pub fn model(&self) -> &QuickTaskModel {
		&self.model
	}

	/// Caller-selected reasoning effort.
	pub fn reasoning_effort(&self) -> &QuickTaskReasoningEffort {
		&self.reasoning_effort
	}
}
impl Serialize for QuickTaskTurnStartRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut request = serializer.serialize_struct("QuickTaskTurnStartRequest", 4)?;

		request.serialize_field("threadId", self.thread_id.as_str())?;
		request.serialize_field("input", &QuickTaskTextInputs(self.input.items()))?;
		request.serialize_field("model", self.model.as_str())?;
		request.serialize_field("effort", self.reasoning_effort.as_str())?;
		request.end()
	}
}

/// Closed turn state retained from one typed app-server response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskTurnStatus {
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
pub struct QuickTaskTurnStartResponse {
	turn_id: ExactTurnId,
	status: QuickTaskTurnStatus,
}
impl QuickTaskTurnStartResponse {
	fn from_wire(wire: QuickTaskTurnStartResponseWire) -> Result<Self, QuickTaskContractError> {
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
	pub const fn status(&self) -> QuickTaskTurnStatus {
		self.status
	}
}

/// Decode one bounded exact `turn/start` result.
pub fn decode_quick_task_turn_start_response(
	bytes: &[u8],
) -> Result<QuickTaskTurnStartResponse, QuickTaskContractError> {
	validate_turn_start_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	QuickTaskTurnStartResponse::from_wire(wire)
}

/// Exact `turn/interrupt` request facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickTaskTurnInterruptRequest {
	thread_id: ExactThreadId,
	turn_id: ExactTurnId,
}
impl QuickTaskTurnInterruptRequest {
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
pub struct QuickTaskTurnInterruptResponse {
	_private: (),
}
impl QuickTaskTurnInterruptResponse {
	fn from_wire(_: QuickTaskEmptySuccessWire) -> Self {
		Self { _private: () }
	}
}

/// Decode one bounded exact empty `turn/interrupt` result.
pub fn decode_quick_task_turn_interrupt_response(
	bytes: &[u8],
) -> Result<QuickTaskTurnInterruptResponse, QuickTaskContractError> {
	validate_empty_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	Ok(QuickTaskTurnInterruptResponse::from_wire(wire))
}

/// Exact explicit `thread/archive` request facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickTaskThreadArchiveRequest {
	thread_id: ExactThreadId,
}
impl QuickTaskThreadArchiveRequest {
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
pub struct QuickTaskThreadArchiveResponse {
	_private: (),
}
impl QuickTaskThreadArchiveResponse {
	fn from_wire(_: QuickTaskEmptySuccessWire) -> Self {
		Self { _private: () }
	}
}

/// Decode one bounded exact empty `thread/archive` result.
pub fn decode_quick_task_thread_archive_response(
	bytes: &[u8],
) -> Result<QuickTaskThreadArchiveResponse, QuickTaskContractError> {
	validate_empty_response_shape(bytes)?;
	let wire = decode_response_wire(bytes)?;

	Ok(QuickTaskThreadArchiveResponse::from_wire(wire))
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskThreadStartResponseWire {
	thread: QuickTaskThreadResponseWire,
	model: String,
	model_provider: String,
	service_tier: Option<String>,
	cwd: QuickTaskAbsolutePathWire,
	#[serde(default)]
	runtime_workspace_roots: Vec<QuickTaskAbsolutePathWire>,
	#[serde(default)]
	instruction_sources: Vec<QuickTaskInstructionSourceWire>,
	approval_policy: QuickTaskAskForApprovalWire,
	approvals_reviewer: QuickTaskApprovalsReviewerWire,
	sandbox: QuickTaskSandboxPolicyWire,
	#[serde(default)]
	active_permission_profile: Option<QuickTaskActivePermissionProfileWire>,
	reasoning_effort: Option<QuickTaskReasoningEffortWire>,
	#[serde(default)]
	multi_agent_mode: QuickTaskMultiAgentModeWire,
}
impl QuickTaskThreadStartResponseWire {
	fn validate_private_facts(&self) -> Result<(), QuickTaskContractError> {
		self.sandbox.validate()?;
		if self.multi_agent_mode != QuickTaskMultiAgentModeWire::ExplicitRequestOnly {
			return Err(QuickTaskContractError::ResponseSemanticMismatch);
		}

		Ok(())
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskThreadResumeResponseWire {
	thread: QuickTaskThreadResponseWire,
	model: String,
	model_provider: String,
	service_tier: Option<String>,
	cwd: QuickTaskAbsolutePathWire,
	#[serde(default)]
	runtime_workspace_roots: Vec<QuickTaskAbsolutePathWire>,
	#[serde(default)]
	instruction_sources: Vec<QuickTaskInstructionSourceWire>,
	approval_policy: QuickTaskAskForApprovalWire,
	approvals_reviewer: QuickTaskApprovalsReviewerWire,
	sandbox: QuickTaskSandboxPolicyWire,
	#[serde(default)]
	active_permission_profile: Option<QuickTaskActivePermissionProfileWire>,
	reasoning_effort: Option<QuickTaskReasoningEffortWire>,
	#[serde(default)]
	multi_agent_mode: QuickTaskMultiAgentModeWire,
	#[serde(default)]
	initial_turns_page: Option<QuickTaskForbiddenValueWire>,
	#[serde(default)]
	turns_backwards_cursor: Option<String>,
	#[serde(default)]
	items_backwards_cursor: Option<String>,
}
impl QuickTaskThreadResumeResponseWire {
	fn validate_private_facts(&self) -> Result<(), QuickTaskContractError> {
		self.sandbox.validate()?;
		if self.multi_agent_mode != QuickTaskMultiAgentModeWire::ExplicitRequestOnly {
			return Err(QuickTaskContractError::ResponseSemanticMismatch);
		}
		if self.initial_turns_page.is_some() {
			return Err(QuickTaskContractError::UnexpectedResponseCollection);
		}

		Ok(())
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskThreadResponseWire {
	id: String,
	#[serde(default)]
	extra: Option<QuickTaskThreadExtraWire>,
	session_id: String,
	forked_from_id: Option<String>,
	parent_thread_id: Option<String>,
	preview: String,
	ephemeral: bool,
	#[serde(default)]
	history_mode: QuickTaskThreadHistoryModeWire,
	model_provider: String,
	created_at: i64,
	updated_at: i64,
	recency_at: Option<i64>,
	status: QuickTaskThreadStatusWire,
	path: Option<String>,
	cwd: QuickTaskAbsolutePathWire,
	cli_version: String,
	source: QuickTaskSessionSourceWire,
	can_accept_direct_input: Option<bool>,
	thread_source: Option<QuickTaskThreadSourceWire>,
	agent_nickname: Option<String>,
	agent_role: Option<String>,
	git_info: Option<QuickTaskGitInfoWire>,
	name: Option<String>,
	#[serde(default)]
	section: Option<QuickTaskThreadSectionWire>,
	#[serde(default)]
	section_entered_at: Option<i64>,
	turns: Vec<QuickTaskForbiddenValueWire>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskThreadSectionWire {
	id: String,
	name: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskGitInfoWire {
	sha: Option<String>,
	branch: Option<String>,
	origin_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickTaskThreadExtraWire {}

#[derive(Clone, Eq, PartialEq)]
struct QuickTaskAbsolutePathWire(String);
impl QuickTaskAbsolutePathWire {
	fn into_string(self) -> String {
		self.0
	}
}
impl<'de> Deserialize<'de> for QuickTaskAbsolutePathWire {
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
struct QuickTaskInstructionSourceWire(String);

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum QuickTaskAskForApprovalWire {
	#[serde(rename = "untrusted")]
	UnlessTrusted,
	OnRequest,
	Granular(QuickTaskGranularApprovalWire),
	Never,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickTaskGranularApprovalWire {
	sandbox_approval: bool,
	rules: bool,
	#[serde(default)]
	skill_approval: bool,
	#[serde(default)]
	request_permissions: bool,
	mcp_elicitations: bool,
}

#[derive(Deserialize)]
enum QuickTaskApprovalsReviewerWire {
	#[serde(rename = "user")]
	User,
	#[serde(rename = "auto_review")]
	AutoReview,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskActivePermissionProfileWire {
	id: String,
	#[serde(default)]
	extends: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum QuickTaskSandboxPolicyWire {
	DangerFullAccess,
	#[serde(rename_all = "camelCase")]
	ReadOnly {
		#[serde(default)]
		network_access: bool,
		#[serde(default)]
		access: Option<QuickTaskLegacyReadOnlyAccessWire>,
	},
	#[serde(rename_all = "camelCase")]
	ExternalSandbox {
		#[serde(default)]
		network_access: QuickTaskNetworkAccessWire,
	},
	#[serde(rename_all = "camelCase")]
	WorkspaceWrite {
		#[serde(default)]
		writable_roots: Vec<QuickTaskAbsolutePathWire>,
		#[serde(default)]
		read_only_access: Option<QuickTaskLegacyReadOnlyAccessWire>,
		#[serde(default)]
		network_access: bool,
		#[serde(default)]
		exclude_tmpdir_env_var: bool,
		#[serde(default)]
		exclude_slash_tmp: bool,
	},
}
impl QuickTaskSandboxPolicyWire {
	fn validate(&self) -> Result<(), QuickTaskContractError> {
		match self {
			Self::ReadOnly {
				access: Some(QuickTaskLegacyReadOnlyAccessWire::Restricted), ..
			}
			| Self::WorkspaceWrite {
				read_only_access: Some(QuickTaskLegacyReadOnlyAccessWire::Restricted),
				..
			} => Err(QuickTaskContractError::MalformedResponse),
			_ => Ok(()),
		}
	}
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum QuickTaskLegacyReadOnlyAccessWire {
	FullAccess,
	Restricted,
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum QuickTaskNetworkAccessWire {
	#[default]
	Restricted,
	Enabled,
}

#[allow(dead_code)]
#[derive(Clone, Eq, PartialEq)]
enum QuickTaskReasoningEffortWire {
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
impl QuickTaskReasoningEffortWire {
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
impl<'de> Deserialize<'de> for QuickTaskReasoningEffortWire {
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
enum QuickTaskMultiAgentModeWire {
	None,
	Custom(String),
	#[default]
	ExplicitRequestOnly,
	Proactive,
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum QuickTaskThreadHistoryModeWire {
	#[default]
	Legacy,
	Paginated,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum QuickTaskThreadStatusWire {
	NotLoaded,
	Idle,
	SystemError,
	Active {
		#[serde(rename = "activeFlags")]
		active_flags: Vec<QuickTaskThreadActiveFlagWire>,
	},
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum QuickTaskThreadActiveFlagWire {
	WaitingOnApproval,
	WaitingOnUserInput,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum QuickTaskSessionSourceWire {
	Cli,
	#[serde(rename = "vscode")]
	VsCode,
	Exec,
	AppServer,
	Custom(String),
	SubAgent(QuickTaskSubAgentSourceWire),
	#[serde(other)]
	Unknown,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum QuickTaskSubAgentSourceWire {
	Review,
	Compact,
	ThreadSpawn(QuickTaskThreadSpawnSourceWire),
	MemoryConsolidation,
	Other(String),
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickTaskThreadSpawnSourceWire {
	parent_thread_id: QuickTaskUuidWire,
	depth: i32,
	#[serde(default)]
	agent_path: Option<QuickTaskAgentPathWire>,
	#[serde(default)]
	agent_nickname: Option<String>,
	#[serde(default)]
	agent_role: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(transparent)]
struct QuickTaskUuidWire(#[serde(deserialize_with = "deserialize_canonical_uuid")] String);

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(transparent)]
struct QuickTaskAgentPathWire(#[serde(deserialize_with = "deserialize_agent_path")] String);

#[allow(dead_code)]
enum QuickTaskThreadSourceWire {
	User,
	Subagent,
	Feature(String),
	MemoryConsolidation,
}
impl<'de> Deserialize<'de> for QuickTaskThreadSourceWire {
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
struct QuickTaskTurnStartResponseWire {
	turn: QuickTaskTurnResponseWire,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskTurnResponseWire {
	id: String,
	items: Vec<QuickTaskForbiddenValueWire>,
	#[serde(default)]
	items_view: QuickTaskTurnItemsViewWire,
	status: QuickTaskTurnStatusWire,
	error: Option<QuickTaskTurnErrorWire>,
	started_at: Option<i64>,
	completed_at: Option<i64>,
	duration_ms: Option<i64>,
}
impl QuickTaskTurnResponseWire {
	fn validate_start_facts(&self) -> Result<(), QuickTaskContractError> {
		if !self.items.is_empty() {
			return Err(QuickTaskContractError::UnexpectedResponseCollection);
		}
		if self.status != QuickTaskTurnStatusWire::InProgress {
			return Err(QuickTaskContractError::InvalidTurnStatus);
		}
		if self.items_view != QuickTaskTurnItemsViewWire::NotLoaded
			|| self.error.is_some()
			|| self.started_at.is_some()
			|| self.completed_at.is_some()
			|| self.duration_ms.is_some()
		{
			return Err(QuickTaskContractError::ResponseSemanticMismatch);
		}

		Ok(())
	}
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum QuickTaskTurnItemsViewWire {
	NotLoaded,
	Summary,
	#[default]
	Full,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum QuickTaskTurnStatusWire {
	Completed,
	Interrupted,
	Failed,
	InProgress,
}
impl QuickTaskTurnStatusWire {
	const fn into_contract(self) -> QuickTaskTurnStatus {
		match self {
			Self::Completed => QuickTaskTurnStatus::Completed,
			Self::Interrupted => QuickTaskTurnStatus::Interrupted,
			Self::Failed => QuickTaskTurnStatus::Failed,
			Self::InProgress => QuickTaskTurnStatus::InProgress,
		}
	}
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskTurnErrorWire {
	message: String,
	codex_error_info: Option<QuickTaskCodexErrorInfoWire>,
	#[serde(default)]
	additional_details: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum QuickTaskCodexErrorInfoWire {
	ContextWindowExceeded,
	SessionBudgetExceeded,
	UsageLimitExceeded,
	ServerOverloaded,
	CyberPolicy,
	HttpConnectionFailed(QuickTaskHttpStatusWire),
	ResponseStreamConnectionFailed(QuickTaskHttpStatusWire),
	InternalServerError,
	Unauthorized,
	BadRequest,
	ThreadRollbackFailed,
	SandboxError,
	ResponseStreamDisconnected(QuickTaskHttpStatusWire),
	ResponseTooManyFailedAttempts(QuickTaskHttpStatusWire),
	ActiveTurnNotSteerable(QuickTaskActiveTurnNotSteerableWire),
	Other,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskHttpStatusWire {
	http_status_code: Option<u16>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuickTaskActiveTurnNotSteerableWire {
	turn_kind: QuickTaskNonSteerableTurnKindWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum QuickTaskNonSteerableTurnKindWire {
	Review,
	Compact,
}

enum QuickTaskForbiddenValueWire {}
impl<'de> Deserialize<'de> for QuickTaskForbiddenValueWire {
	fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Err(D::Error::custom("value is forbidden for this method"))
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickTaskEmptySuccessWire {}

struct ValidatedThreadResponseFacts {
	thread_id: ExactThreadId,
	cwd: ThreadCwd,
	model: QuickTaskModel,
	reasoning_effort: Option<QuickTaskReasoningEffort>,
}

fn validate_thread_response_facts(
	expected_thread_id: Option<&ExactThreadId>,
	expected_cwd: &ThreadCwd,
	expected_model: &QuickTaskModel,
	thread: QuickTaskThreadResponseWire,
	response_cwd: String,
	response_model: String,
	response_reasoning_effort: Option<String>,
) -> Result<ValidatedThreadResponseFacts, QuickTaskContractError> {
	if thread.ephemeral {
		return Err(QuickTaskContractError::EphemeralThreadRejected);
	}
	if !thread.turns.is_empty() {
		return Err(QuickTaskContractError::UnexpectedResponseCollection);
	}

	let thread_id =
		ExactThreadId::new(thread.id).map_err(|_| QuickTaskContractError::InvalidThreadId)?;
	if expected_thread_id.is_some_and(|expected| expected != &thread_id) {
		return Err(QuickTaskContractError::ThreadIdMismatch);
	}

	let thread_cwd = ThreadCwd::from_protocol(thread.cwd.into_string())
		.map_err(|_| QuickTaskContractError::InvalidCwd)?;
	let cwd =
		ThreadCwd::from_protocol(response_cwd).map_err(|_| QuickTaskContractError::InvalidCwd)?;
	if thread_cwd != cwd || expected_cwd != &cwd {
		return Err(QuickTaskContractError::CwdMismatch);
	}

	let model = QuickTaskModel::new(response_model)?;
	if expected_model != &model {
		return Err(QuickTaskContractError::ModelMismatch);
	}

	let reasoning_effort =
		response_reasoning_effort.map(QuickTaskReasoningEffort::new).transpose()?;

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
) -> Result<(), QuickTaskContractError> {
	let value = decode_bounded_response_value(bytes)?;
	let response = response_object(&value)?;
	validate_object_fields(response, response_fields, required_response_fields)?;
	if response.get("initialTurnsPage").is_some_and(|page| !page.is_null()) {
		return Err(QuickTaskContractError::UnexpectedResponseCollection);
	}

	let thread = response
		.get("thread")
		.and_then(Value::as_object)
		.ok_or(QuickTaskContractError::MalformedResponse)?;
	validate_object_fields(thread, THREAD_RESPONSE_FIELDS, THREAD_RESPONSE_REQUIRED_FIELDS)?;

	let turns = thread
		.get("turns")
		.and_then(Value::as_array)
		.ok_or(QuickTaskContractError::MalformedResponse)?;
	if !turns.is_empty() {
		return Err(QuickTaskContractError::UnexpectedResponseCollection);
	}

	Ok(())
}

fn validate_turn_start_response_shape(bytes: &[u8]) -> Result<(), QuickTaskContractError> {
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
		.ok_or(QuickTaskContractError::MalformedResponse)?;
	validate_object_fields(turn, TURN_RESPONSE_FIELDS, TURN_RESPONSE_REQUIRED_FIELDS)?;

	let items = turn
		.get("items")
		.and_then(Value::as_array)
		.ok_or(QuickTaskContractError::MalformedResponse)?;
	if !items.is_empty() {
		return Err(QuickTaskContractError::UnexpectedResponseCollection);
	}

	Ok(())
}

fn validate_empty_response_shape(bytes: &[u8]) -> Result<(), QuickTaskContractError> {
	let value = decode_bounded_response_value(bytes)?;
	let response = response_object(&value)?;
	validate_object_fields(response, &[], &[])
}

fn decode_bounded_response_value(bytes: &[u8]) -> Result<Value, QuickTaskContractError> {
	if bytes.len() > MAX_QUICK_TASK_RESPONSE_BYTES {
		return Err(QuickTaskContractError::ResponseLimitExceeded);
	}

	serde_json::from_slice(bytes).map_err(|_| QuickTaskContractError::MalformedResponse)
}

fn decode_response_wire<'de, T>(bytes: &'de [u8]) -> Result<T, QuickTaskContractError>
where
	T: Deserialize<'de>,
{
	if bytes.len() > MAX_QUICK_TASK_RESPONSE_BYTES {
		return Err(QuickTaskContractError::ResponseLimitExceeded);
	}

	serde_json::from_slice(bytes).map_err(|error| {
		let message = error.to_string();

		if message.contains("unknown field") {
			QuickTaskContractError::UnknownResponseField
		} else if message.contains("missing field") {
			QuickTaskContractError::MissingResponseField
		} else {
			QuickTaskContractError::MalformedResponse
		}
	})
}

fn response_object(value: &Value) -> Result<&Map<String, Value>, QuickTaskContractError> {
	value.as_object().ok_or(QuickTaskContractError::MalformedResponse)
}

fn validate_object_fields(
	object: &Map<String, Value>,
	allowed: &[&str],
	required: &[&str],
) -> Result<(), QuickTaskContractError> {
	if object.keys().any(|field| !allowed.contains(&field.as_str())) {
		return Err(QuickTaskContractError::UnknownResponseField);
	}
	if required.iter().any(|field| !object.contains_key(*field)) {
		return Err(QuickTaskContractError::MissingResponseField);
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
		MAX_QUICK_TASK_RESPONSE_BYTES, QuickTaskContractError, QuickTaskThreadResumeRequest,
		QuickTaskThreadStartRequest, QuickTaskTurnStatus,
		decode_quick_task_thread_archive_response, decode_quick_task_thread_resume_response,
		decode_quick_task_thread_start_response, decode_quick_task_turn_interrupt_response,
		decode_quick_task_turn_start_response,
	};

	fn exact_thread() -> ExactThreadId {
		ExactThreadId::new("thread-1").expect("fixture thread ID must be valid")
	}

	fn start_request() -> QuickTaskThreadStartRequest {
		QuickTaskThreadStartRequest::new("gpt-5", "/workspace", "Follow the request.")
			.expect("fixture start request must be valid")
	}

	fn resume_request() -> QuickTaskThreadResumeRequest {
		QuickTaskThreadResumeRequest::new(
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
	fn pinned_method_results_mint_only_typed_quick_task_success() {
		let start_request = start_request();
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let thread_bytes = serde_json::to_vec(&canonical).expect("fixture response must serialize");
		let start = decode_quick_task_thread_start_response(&start_request, &thread_bytes)
			.expect("pinned thread/start response must decode");

		assert_eq!(start.thread_id().as_str(), "thread-1");
		assert_eq!(start.cwd().as_str(), "/workspace");
		assert_eq!(start.model().as_str(), "gpt-5");
		assert_eq!(start.reasoning_effort().map(|effort| effort.as_str()), Some("high"));

		let mut canonical_auto_review = canonical.clone();
		canonical_auto_review["approvalsReviewer"] = json!("auto_review");
		assert!(
			decode_quick_task_thread_start_response(
				&start_request,
				&serde_json::to_vec(&canonical_auto_review).unwrap(),
			)
			.is_ok()
		);

		let resume = decode_quick_task_thread_resume_response(&resume_request(), &thread_bytes)
			.expect("pinned thread/resume response must decode");

		assert_eq!(resume.thread_id().as_str(), "thread-1");
		assert_eq!(resume.cwd().as_str(), "/workspace");
		assert_eq!(resume.model().as_str(), "gpt-5");

		let turn = decode_quick_task_turn_start_response(
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
		assert_eq!(turn.status(), QuickTaskTurnStatus::InProgress);
		assert!(decode_quick_task_turn_interrupt_response(b"{}").is_ok());
		assert!(decode_quick_task_thread_archive_response(b"{}").is_ok());
	}

	#[test]
	fn current_null_thread_section_fields_decode() {
		let mut current = thread_response("thread-1", "gpt-5", "/workspace");
		current["thread"]["section"] = Value::Null;
		current["thread"]["sectionEnteredAt"] = Value::Null;
		let bytes = serde_json::to_vec(&current).expect("fixture response must serialize");

		assert!(decode_quick_task_thread_start_response(&start_request(), &bytes).is_ok());
		assert!(decode_quick_task_thread_resume_response(&resume_request(), &bytes).is_ok());
	}

	#[test]
	fn populated_thread_section_decodes() {
		let mut current = thread_response("thread-1", "gpt-5", "/workspace");
		current["thread"]["section"] = json!({"id": "section-1", "name": "Active"});
		current["thread"]["sectionEnteredAt"] = json!(2);

		assert!(
			decode_quick_task_thread_start_response(
				&start_request(),
				&serde_json::to_vec(&current).expect("fixture response must serialize"),
			)
			.is_ok()
		);
	}

	#[test]
	fn legacy_omitted_thread_section_fields_decode() {
		let legacy = thread_response("thread-1", "gpt-5", "/workspace");
		let thread = legacy["thread"].as_object().expect("fixture thread must be an object");

		assert!(!thread.contains_key("section"));
		assert!(!thread.contains_key("sectionEnteredAt"));
		assert!(
			decode_quick_task_thread_start_response(
				&start_request(),
				&serde_json::to_vec(&legacy).expect("fixture response must serialize"),
			)
			.is_ok()
		);
	}

	#[test]
	fn malformed_or_unknown_thread_section_is_rejected() {
		let canonical = thread_response("thread-1", "gpt-5", "/workspace");
		let cases = [
			(
				"missing name",
				json!({"id": "section-1"}),
				QuickTaskContractError::MissingResponseField,
			),
			(
				"invalid id type",
				json!({"id": 1, "name": "Active"}),
				QuickTaskContractError::MalformedResponse,
			),
			(
				"unknown field",
				json!({"id": "section-1", "name": "Active", "unexpected": true}),
				QuickTaskContractError::UnknownResponseField,
			),
		];

		for (case, section, expected) in cases {
			let mut response = canonical.clone();
			response["thread"]["section"] = section;

			assert_eq!(
				decode_quick_task_thread_start_response(
					&start_request(),
					&serde_json::to_vec(&response).expect("fixture response must serialize"),
				)
				.map(|_| ()),
				Err(expected),
				"{case}",
			);
		}

		let mut invalid_entered_at = canonical;
		invalid_entered_at["thread"]["sectionEnteredAt"] = json!("2");

		assert_eq!(
			decode_quick_task_thread_start_response(
				&start_request(),
				&serde_json::to_vec(&invalid_entered_at).expect("fixture response must serialize"),
			)
			.map(|_| ()),
			Err(QuickTaskContractError::MalformedResponse),
			"invalid section-entered timestamp",
		);
	}

	#[test]
	fn malformed_or_unknown_wire_shape_failures_never_mint_quick_task_success() {
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
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&unknown_nested).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::UnknownResponseField,
			),
			(
				"legacy approvals reviewer",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&legacy_reviewer).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::MalformedResponse,
			),
			(
				"legacy thread-spawn agent type",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&legacy_agent_type).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::UnknownResponseField,
			),
			(
				"duplicate nested field",
				decode_quick_task_thread_start_response(&start_request, &duplicate_nested)
					.map(|_| ()),
				QuickTaskContractError::MalformedResponse,
			),
			(
				"malformed nested value",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&malformed_nested).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::MalformedResponse,
			),
		];

		for (case, actual, expected) in cases {
			assert_eq!(actual, Err(expected), "{case}");
		}
	}

	#[test]
	fn semantic_identity_collection_or_bounds_failures_never_mint_quick_task_success() {
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
		let oversized = vec![b' '; MAX_QUICK_TASK_RESPONSE_BYTES + 1];

		let cases = [
			(
				"nonempty turns",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&nonempty_turns).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::UnexpectedResponseCollection,
			),
			(
				"nonempty items",
				decode_quick_task_turn_start_response(
					&serde_json::to_vec(&nonempty_items).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::UnexpectedResponseCollection,
			),
			(
				"wrong thread",
				decode_quick_task_thread_resume_response(
					&resume_request,
					&serde_json::to_vec(&wrong_thread).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::ThreadIdMismatch,
			),
			(
				"wrong model",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&wrong_model).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::ModelMismatch,
			),
			(
				"wrong cwd",
				decode_quick_task_thread_start_response(
					&start_request,
					&serde_json::to_vec(&wrong_cwd).unwrap(),
				)
				.map(|_| ()),
				QuickTaskContractError::CwdMismatch,
			),
			(
				"oversized result",
				decode_quick_task_turn_interrupt_response(&oversized).map(|_| ()),
				QuickTaskContractError::ResponseLimitExceeded,
			),
		];

		for (case, actual, expected) in cases {
			assert_eq!(actual, Err(expected), "{case}");
		}
	}
}
