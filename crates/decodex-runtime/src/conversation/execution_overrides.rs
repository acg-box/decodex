//! Keep displayed defaults separate from explicit native execution overrides.
use crate::conversation::{
	ConversationThreadResumeRequest, ConversationThreadStartRequest, ConversationTurnStartRequest,
};
use decodex_protocol::ConversationExecutionOverrides;

pub(super) fn apply_start_overrides(
	mut request: ConversationThreadStartRequest,
	intent: Option<ConversationExecutionOverrides>,
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

pub(super) fn inherit_resume_settings(
	request: ConversationThreadResumeRequest,
) -> ConversationThreadResumeRequest {
	request.inherit_native_settings()
}

pub(super) fn apply_turn_overrides(
	mut request: ConversationTurnStartRequest,
	intent: Option<ConversationExecutionOverrides>,
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
	use std::iter;

	use crate::conversation::execution_overrides::{
		self, ConversationThreadResumeRequest, ConversationThreadStartRequest,
		ConversationTurnStartRequest,
	};
	use decodex_codex::{ConversationTurnInput, ExactThreadId};
	use decodex_protocol::ConversationExecutionOverrides;

	#[test]
	fn resume_does_not_reapply_creation_configuration() {
		let request = ConversationThreadResumeRequest::new(
			ExactThreadId::new("native-thread").unwrap(),
			"stale-model",
			"/tmp",
			"Stale creation instructions",
		)
		.unwrap();
		let wire =
			serde_json::to_value(execution_overrides::inherit_resume_settings(request)).unwrap();

		assert_eq!(wire, serde_json::json!({"threadId":"native-thread", "excludeTurns":true}));
	}

	#[test]
	fn legacy_and_each_field_choice_apply_across_resume_turn_and_fallback() {
		let choices = iter::once(None).chain((0..8).map(|bits| {
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
				serde_json::to_value(execution_overrides::apply_start_overrides(start, intent))
					.expect("start wire"),
				serde_json::to_value(execution_overrides::inherit_resume_settings(resume))
					.expect("resume wire"),
				serde_json::to_value(execution_overrides::apply_turn_overrides(turn, intent))
					.expect("turn wire"),
			];

			for wire in [&wires[0], &wires[2]] {
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
			assert_eq!(
				wires[1],
				serde_json::json!({"threadId":"native-thread","excludeTurns":true})
			);
			assert_eq!(wires[0]["cwd"], "/tmp");
		}
	}
}
