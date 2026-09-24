//! Native model settings and durable manual selection receipts.
use crate::{ChiefModelDto, ConversationModel, ConversationReasoningEffort, EntityId, WireText};
use serde::{Deserialize, Serialize};

/// Durable request outcome, not proof that an active turn changed its model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefModelOutcome {
	/// Reserved before the native write.
	Reserved,
	/// Accepted for processing; await a native settings observation.
	Queued,
	/// Delivery is uncertain and must not be replayed.
	Unknown,
	/// Rejected before dispatch or by native validation.
	Rejected,
	/// Current native settings report the requested selection for subsequent turns.
	TargetObserved,
	/// A replacement native owner reports another selection after confirmed process death.
	Superseded,
}

/// Review facts for a task-local model selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefModelSelectionState {
	/// Configured selection and the current native model catalog.
	Available {
		/// Exact local work.
		work_id: EntityId,
		/// Exact native thread.
		thread_id: EntityId,
		/// Opaque source, observation and catalog identity.
		review_token: WireText,
		/// Configured model for subsequent turns.
		model: ConversationModel,
		/// Native provider identity.
		model_provider: WireText,
		/// Configured effort; null is known unset.
		effort: Option<ConversationReasoningEffort>,
		/// Choices advertised by the current native connection.
		models: Vec<ChiefModelDto>,
		/// False when work is not editable.
		can_update: bool,
		/// Last settled selection attempt, if any.
		last_outcome: Option<ChiefModelOutcome>,
	},
	/// A durable request remains unconfirmed. Do not submit another selection.
	Pending {
		/// Requested model, not an active-step assertion.
		model: ConversationModel,
		/// Expected configured effort.
		effort: Option<ConversationReasoningEffort>,
		/// Current unresolved receipt state.
		state: ChiefModelOutcome,
	},
	/// Current source-bound native facts cannot be established.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use crate::ChiefActionDto;
	use serde_json::{Value, json};

	#[test]
	fn model_action_distinguishes_preserved_effort_from_native_none() {
		let preserved = json!({"action":"set_task_model","data":{"work_id":"work","thread_id":"thread","review_token":"review","model":"future-model","effort":null}});
		let action: ChiefActionDto = serde_json::from_value(preserved.clone()).unwrap();
		assert!(matches!(action, ChiefActionDto::SetTaskModel { effort: None, .. }));
		let mut explicit = preserved.clone();
		explicit["data"]["effort"] = json!("none");
		let action: ChiefActionDto = serde_json::from_value(explicit).unwrap();
		assert!(matches!(
			action,
			ChiefActionDto::SetTaskModel {
				effort: Some(crate::ConversationReasoningEffort::None),
				..
			}
		));
		for field in ["serviceTier", "modelProvider", "collaborationMode"] {
			let mut widened = preserved.clone();
			widened["data"][field] = Value::Null;
			assert!(serde_json::from_value::<ChiefActionDto>(widened).is_err());
		}
	}
}
