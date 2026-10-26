//! Read-only collaboration receipts. Delivery and assessment are separate facts.
use serde::{Deserialize, Serialize};

use crate::{AgentAsyncQuestionDto, AgentDispatchStateDto, EntityId};

/// One exact managed exchange source, independent of chat history pagination.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExchangeDto {
	/// Persistent source event. This is never a display slot or participant-pair key.
	pub event_id: i64,
	/// Exact source event identity.
	pub source_event_id: String,
	/// Work that owns the receipt.
	pub work_id: String,
	/// Responsible manager, or the explicit target of a refused continuation.
	pub other_work_id: Option<String>,
	/// Current explicit execution state of the refused target, independent of that attempt.
	pub target_dispatch_state: Option<AgentDispatchStateDto>,
	/// Native source conversation, when recorded.
	pub thread_id: Option<String>,
	/// Exact native turn, when recorded.
	pub turn_id: Option<String>,
	/// Exact native call item, when recorded.
	pub item_id: Option<String>,
	/// Instruction associated by the acknowledged native turn, never by message text.
	pub instruction_event_id: Option<i64>,
	/// Original continuation or final result excerpt.
	pub excerpt: String,
	/// Some source text was omitted. Open the source to read the full content.
	pub truncated: bool,
	/// Receipt category.
	pub kind: AgentExchangeKind,
	/// Acknowledged receiving turn. Absence does not establish delivery.
	pub delivered_turn_id: Option<String>,
	/// Explicit assessment of this exact result, independent of delivery.
	pub assessment: Option<AgentExchangeAssessment>,
	/// Public assessment excerpt, absent without an explicit assessment.
	pub assessment_note: Option<String>,
	/// Native terminal outcome. A failed or interrupted turn is not a completed result.
	pub outcome: Option<String>,
	/// Source observation time, in Unix microseconds.
	pub created_at_micros: i64,
	/// Exact independent Work request, absent for managed-worker exchanges.
	pub collaboration: Option<AgentPeerExchangeDto>,
}

/// A retained independent contribution cycle. A repair has a different instruction identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPeerExchangeDto {
	/// Business Work identity, distinct from the conversation owner.
	pub work_id: String,
	/// Stable scoped request.
	pub request_id: String,
	/// Immutable instruction for this cycle.
	pub instruction_id: String,
	/// Whether this is the request's current instruction.
	pub current: bool,
	/// Explicit business state, only for the current instruction.
	pub state: Option<String>,
	/// Explicit owner assessment, only for the current instruction.
	pub outcome: Option<String>,
	/// Bounded assessment text, only for the current instruction.
	pub note: Option<String>,
}

/// The source of a managed collaboration record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExchangeKind {
	/// A durable instruction to the original worker.
	Instruction,
	/// A worker completion receipt that requires the manager's judgment.
	Result,
	/// Busy refused the continuation. Nothing was sent or queued.
	NotSent,
	/// A request to an existing independent Agent.
	PeerInstruction,
	/// A returned or declined independent contribution.
	PeerResult,
}

/// An explicit disposition of one result. This does not close its parent goal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExchangeAssessment {
	/// The manager accepted this exact result.
	Accepted,
	/// The manager requested follow-up work.
	FollowUp,
	/// The manager deferred its judgment.
	Wait,
	/// The manager requires a user decision.
	UserDecision,
}

/// Existing native question state. The question owner retains all reply authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExchangeQuestionDto {
	/// Exact managed conversation owner.
	pub work_id: String,
	/// Exact native conversation.
	pub thread_id: String,
	/// Exact native source turn.
	pub turn_id: String,
	/// Exact native source item.
	pub item_id: String,
	/// Original stable question identity and bounded public text.
	pub question: AgentAsyncQuestionDto,
	/// True only while the existing native question projection considers it pending.
	pub pending: bool,
	/// Exact locally retained reply, absent when its body or identity is unavailable.
	pub answer: Option<AgentQuestionAnswerDto>,
}

/// A reply receipt. It does not establish that the question or Work was resolved.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentQuestionAnswerDto {
	/// Exact retained reply event.
	pub event_id: i64,
	/// Stable source identity of the reply receipt.
	pub source_event_id: String,
	/// Public reply excerpt.
	pub text: String,
	/// Full text remains in the original source.
	pub truncated: bool,
	/// Exact acknowledged native receiving turn; absent means delivery is unconfirmed.
	pub delivered_turn_id: Option<String>,
	/// Exact receiving turn's terminal status, when retained.
	pub turn_outcome: Option<String>,
}

/// Bounded collaboration read. Failure is distinct from an empty result.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentExchangesResult {
	/// Latest source records, newest first.
	Available {
		/// Current account and service binding.
		source: EntityId,
		/// Exact retained exchange sources.
		entries: Vec<AgentExchangeDto>,
		/// Current and earlier native questions from their existing owner tables.
		questions: Vec<AgentExchangeQuestionDto>,
		/// The question list is bounded. The source conversation retains older questions.
		questions_truncated: bool,
		/// Cursor for older sources, absent when this is the end.
		next_before: Option<i64>,
	},
	/// Current sources could not be read. Retain the last visible projection.
	Unavailable,
}
