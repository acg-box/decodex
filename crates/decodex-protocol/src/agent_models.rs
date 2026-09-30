//! Native model settings and durable manual selection receipts.
use crate::{AgentModelDto, ConversationModel, ConversationReasoningEffort, EntityId, WireText};
use serde::{Deserialize, Serialize};

/// Durable request outcome, not proof that an active turn changed its model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModelOutcome {
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
	/// A replacement native owner reviewed current settings after confirmed process death.
	Superseded,
}

/// Original response to one model settings request, independent of later observations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModelResponse {
	/// Reserved before a response was recorded.
	Reserved,
	/// Native accepted the request for processing.
	Queued,
	/// Native or source validation rejected the request.
	Rejected,
	/// Delivery remains uncertain; do not replay the request.
	Unknown,
}

/// Historical model request evidence; it does not describe the current inference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentModelSelectionReceipt {
	/// Requested model.
	pub model: ConversationModel,
	/// Expected configured effort, including a known unset value.
	pub effort: Option<ConversationReasoningEffort>,
	/// True for an explicit selection; false for automatic fallback.
	pub manual: bool,
	/// Original response, retained after confirmation or reconciliation.
	pub response: AgentModelResponse,
	/// Matching native settings were observed after the request.
	pub target_observed: bool,
	/// Current settings were reviewed after the original process was confirmed dead.
	pub reconciled: bool,
}

/// Review facts for a task-local model selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentModelSelectionState {
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
		models: Vec<AgentModelDto>,
		/// False when work is not editable.
		can_update: bool,
		/// Last settled selection attempt, if any.
		last_outcome: Option<AgentModelOutcome>,
		/// Latest request across current and preserved journals, with its original evidence.
		last_receipt: Option<AgentModelSelectionReceipt>,
	},
	/// A durable request remains unconfirmed. Do not submit another selection.
	Pending {
		/// Requested model, not an active-step assertion.
		model: ConversationModel,
		/// Expected configured effort.
		effort: Option<ConversationReasoningEffort>,
		/// Current unresolved receipt state.
		state: AgentModelOutcome,
		/// Historical request evidence; refreshing this state does not resend the request.
		last_receipt: Option<AgentModelSelectionReceipt>,
	},
	/// Current source-bound native facts cannot be established.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use crate::AgentActionDto;
	use serde_json::{Value, json};

	#[test]
	fn model_action_distinguishes_preserved_effort_from_native_none() {
		let preserved = json!({"action":"set_task_model","data":{"work_id":"work","thread_id":"thread","review_token":"review","model":"future-model","effort":null}});
		let action: AgentActionDto = serde_json::from_value(preserved.clone()).unwrap();

		assert!(matches!(action, AgentActionDto::SetTaskModel { effort: None, .. }));

		let mut explicit = preserved.clone();

		explicit["data"]["effort"] = json!("none");

		let action: AgentActionDto = serde_json::from_value(explicit).unwrap();

		assert!(matches!(
			action,
			AgentActionDto::SetTaskModel {
				effort: Some(crate::ConversationReasoningEffort::None),
				..
			}
		));

		for field in ["serviceTier", "modelProvider", "collaborationMode"] {
			let mut widened = preserved.clone();

			widened["data"][field] = Value::Null;

			assert!(serde_json::from_value::<AgentActionDto>(widened).is_err());
		}
	}
}
