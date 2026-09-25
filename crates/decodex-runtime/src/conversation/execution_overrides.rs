//! Keep displayed defaults separate from explicit native execution overrides.
use super::{
	ConversationThreadResumeRequest, ConversationThreadStartRequest, ConversationTurnStartRequest,
};

pub(super) fn apply_start_overrides(
	mut request: ConversationThreadStartRequest,
	intent: Option<decodex_protocol::ConversationExecutionOverrides>,
) -> ConversationThreadStartRequest {
	if let Some(intent) = intent {
		if !intent.model {
			request = request.inherit_model();
		}
		if !intent.service_tier {
			request = request.inherit_service_tier();
		}
	}
	request
}

pub(super) fn apply_resume_overrides(
	mut request: ConversationThreadResumeRequest,
	intent: Option<decodex_protocol::ConversationExecutionOverrides>,
) -> ConversationThreadResumeRequest {
	if let Some(intent) = intent {
		if !intent.model {
			request = request.inherit_model();
		}
		if !intent.service_tier {
			request = request.inherit_service_tier();
		}
	}
	request
}

pub(super) fn apply_turn_overrides(
	mut request: ConversationTurnStartRequest,
	intent: Option<decodex_protocol::ConversationExecutionOverrides>,
) -> ConversationTurnStartRequest {
	if let Some(intent) = intent {
		if !intent.model {
			request = request.inherit_model();
		}
		if !intent.reasoning {
			request = request.inherit_reasoning_effort();
		}
		if !intent.service_tier {
			request = request.inherit_service_tier();
		}
	}
	request
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_codex::{ConversationTurnInput, ExactThreadId};
	use decodex_protocol::ConversationExecutionOverrides;

	#[test]
	fn legacy_and_each_field_choice_apply_across_resume_turn_and_fallback() {
		let choices = std::iter::once(None).chain((0..8).map(|bits| {
			Some(ConversationExecutionOverrides {
				model: bits & 1 != 0,
				reasoning: bits & 2 != 0,
				service_tier: bits & 4 != 0,
			})
		}));
		for intent in choices {
			let thread = ExactThreadId::new("native-thread").expect("thread");
			let start =
				ConversationThreadStartRequest::new("display-model", "/tmp", "Instructions")
					.expect("start");
			let resume = ConversationThreadResumeRequest::new(
				thread.clone(),
				"display-model",
				"/tmp",
				"Instructions",
			)
			.expect("resume");
			let turn = ConversationTurnStartRequest::new(
				thread,
				ConversationTurnInput::text("Continue").expect("text"),
				"display-model",
				"high",
			)
			.expect("turn");
			let wires = [
				serde_json::to_value(apply_start_overrides(start, intent)).expect("start wire"),
				serde_json::to_value(apply_resume_overrides(resume, intent)).expect("resume wire"),
				serde_json::to_value(apply_turn_overrides(turn, intent)).expect("turn wire"),
			];
			for wire in &wires {
				assert_eq!(wire.get("model").is_some(), intent.is_none_or(|v| v.model));
				assert_eq!(
					wire.get("serviceTier").is_some(),
					intent.is_none_or(|v| v.service_tier)
				);
			}
			assert_eq!(wires[2].get("effort").is_some(), intent.is_none_or(|v| v.reasoning));
			assert_eq!(
				wires[2].get("serviceTierForTurn").is_some(),
				intent.is_none_or(|v| v.service_tier)
			);
			assert_eq!(wires[1]["threadId"], "native-thread");
			assert_eq!(wires[0]["cwd"], "/tmp");
		}
	}
}
