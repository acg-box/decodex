//! Explicit one-shot answers kept outside the source conversation and desktop draft.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// Maximum UTF-8 prompt size accepted by native sampling.
pub const MAX_AGENT_SAMPLE_PROMPT_BYTES: usize = 4_000;
/// Maximum UTF-8 assistant text returned by native sampling.
pub const MAX_AGENT_SAMPLE_TEXT_BYTES: usize = 64 * 1_024;

/// Native model response and its reported usage, without a new parent turn.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSampleAnswer {
	/// Latest completed source turn used for this answer.
	pub source_turn_id: WireText,
	/// Native model response identity.
	pub response_id: WireText,
	/// Absent when the model attempted a tool call or returned no text.
	pub text: Option<String>,
	/// Input tokens, when reported by native execution.
	pub input_tokens: Option<i64>,
	/// Cached input tokens, when reported by native execution.
	pub cached_input_tokens: Option<i64>,
	/// Output tokens, when reported by native execution.
	pub output_tokens: Option<i64>,
}
impl AgentSampleAnswer {
	/// Validate native correlation, usage and bounded plain text before display.
	pub fn is_valid(&self) -> bool {
		[&self.source_turn_id, &self.response_id].iter().all(|id| {
			!id.as_str().is_empty()
				&& id.as_str().len() <= 512
				&& !id.as_str().chars().any(char::is_control)
		}) && self.text.as_ref().is_none_or(|text| {
			!text.trim().is_empty()
				&& text.len() <= MAX_AGENT_SAMPLE_TEXT_BYTES
				&& !text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
		}) && [self.input_tokens, self.cached_input_tokens, self.output_tokens]
			.iter()
			.flatten()
			.all(|n| *n >= 0)
	}
}

/// State of the latest explicit sample request for one local task.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSamplePhase {
	/// No sample has been requested.
	Idle,
	/// Native sampling is in progress; closing the panel does not cancel inference.
	Pending,
	/// Cancellation was requested; native child cleanup is not yet confirmed.
	Cancelling,
	/// Native confirmed that the exact request was cancelled and cleaned up.
	Cancelled,
	/// A current response with assistant text is available.
	Ready,
	/// Native returned no text, including an attempted tool call that was not executed.
	NoText,
	/// Sampling failed; no inference retry was made.
	Failed,
	/// Transport or timeout prevented confirmation; never automatically replay inference.
	Unconfirmed,
	/// The parent, account, process or native history changed.
	Obsolete,
	/// The selected native runtime does not expose this method.
	Unsupported,
}

/// Read-only state; only an explicit generation command starts a model request.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSampleStatus {
	/// Local task owner.
	pub work_id: EntityId,
	/// Native parent selected when the command was accepted.
	pub thread_id: Option<WireText>,
	/// Original idempotent generation command identity.
	pub request_id: Option<WireText>,
	/// Explicit question associated with this request.
	pub prompt: Option<WireText>,
	/// Latest request state.
	pub phase: AgentSamplePhase,
	/// Available only for a current completed native result.
	pub answer: Option<AgentSampleAnswer>,
}
impl AgentSampleStatus {
	/// Construct empty state without inference.
	pub const fn idle(work_id: EntityId) -> Self {
		Self {
			work_id,
			thread_id: None,
			request_id: None,
			phase: AgentSamplePhase::Idle,
			prompt: None,
			answer: None,
		}
	}

	/// Check correlation and result presence before presentation.
	pub fn is_valid(&self) -> bool {
		let bound = self.thread_id.as_ref().is_some_and(|v| !v.as_str().is_empty())
			&& self.request_id.as_ref().is_some_and(|v| !v.as_str().is_empty())
			&& self.prompt.as_ref().is_some_and(|v| {
				!v.as_str().trim().is_empty() && v.as_str().len() <= MAX_AGENT_SAMPLE_PROMPT_BYTES
			});
		if self.phase == AgentSamplePhase::Idle {
			return self.thread_id.is_none()
				&& self.request_id.is_none()
				&& self.prompt.is_none()
				&& self.answer.is_none();
		}
		bound
			&& match (&self.phase, &self.answer) {
				(AgentSamplePhase::Ready, Some(answer)) =>
					answer.is_valid() && answer.text.as_ref().is_some_and(|v| !v.trim().is_empty()),
				(AgentSamplePhase::NoText, Some(answer)) =>
					answer.is_valid() && answer.text.is_none(),
				(AgentSamplePhase::Ready | AgentSamplePhase::NoText, _) | (_, Some(_)) => false,
				_ => true,
			}
	}
}
