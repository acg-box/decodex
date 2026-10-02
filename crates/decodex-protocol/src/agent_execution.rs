//! Explicit next-message changes; omitted fields inherit the native task setting.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	ConversationExecutionSettings, ConversationModel, ConversationReasoningEffort, ServiceTier,
};

/// Only settings deliberately changed for this message. An empty value inherits all settings.
/// Full legacy execution objects remain valid and retain their original meaning.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionOverrides {
	/// A newly selected model, if changed.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub model: Option<ConversationModel>,
	/// A newly selected reasoning effort, if changed.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub reasoning_effort: Option<ConversationReasoningEffort>,
	/// Legacy explicit Fast selection. Omitted means inherit; false means standard.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub fast: Option<bool>,
	/// Explicit service tier, including standard. This takes precedence over legacy Fast.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub service_tier: Option<ServiceTier>,
}
impl AgentExecutionOverrides {
	/// Apply explicit per-message settings to native turn parameters.
	pub fn apply_to_native_turn(&self, params: &mut Value) {
		if let Some(model) = &self.model {
			params["model"] = serde_json::json!(model.as_str());
		}
		if let Some(effort) = &self.reasoning_effort {
			params["effort"] = serde_json::json!(effort.as_str());
		}
		if let Some(tier) = self.selected_service_tier() {
			params["serviceTier"] = serde_json::json!(tier.thread_value());
			params["serviceTierForTurn"] = serde_json::json!(tier.as_str());
		}
	}

	/// Whether a message changes no native execution setting.
	pub fn is_empty(&self) -> bool {
		self.model.is_none()
			&& self.reasoning_effort.is_none()
			&& self.fast.is_none()
			&& self.service_tier.is_none()
	}

	/// A deliberate tier change, absent when the message inherits the current task tier.
	pub fn selected_service_tier(&self) -> Option<ServiceTier> {
		self.service_tier.clone().or_else(|| self.fast.map(ServiceTier::from_fast))
	}
}

impl From<ConversationExecutionSettings> for AgentExecutionOverrides {
	fn from(value: ConversationExecutionSettings) -> Self {
		Self {
			model: Some(value.model),
			reasoning_effort: value.reasoning_effort,
			fast: Some(value.fast),
			service_tier: value.service_tier,
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		AgentActionDto, AgentExecutionOverrides, AgentTaskReferenceDto,
		ConversationReasoningEffort, EntityId, HistoryText, WireText,
	};
	#[test]
	fn partial_changes_and_legacy_full_selections_have_distinct_inheritance() {
		let inherited: AgentExecutionOverrides =
			serde_json::from_value(serde_json::json!({})).unwrap();

		assert!(inherited.is_empty());
		assert_eq!(serde_json::to_value(&inherited).unwrap(), serde_json::json!({}));
		assert!(inherited.selected_service_tier().is_none());

		let effort: AgentExecutionOverrides =
			serde_json::from_value(serde_json::json!({"reasoning_effort":"high"})).unwrap();

		assert!(effort.model.is_none() && effort.selected_service_tier().is_none());

		let legacy: AgentExecutionOverrides = serde_json::from_value(
			serde_json::json!({"model":"chosen","reasoning_effort":"medium","fast":false,"service_tier":null}),
		)
		.unwrap();

		assert_eq!(legacy.model.unwrap().as_str(), "chosen");
		assert_eq!(legacy.fast, Some(false));

		let standard: AgentExecutionOverrides =
			serde_json::from_value(serde_json::json!({"service_tier":"default"})).unwrap();

		assert_eq!(standard.selected_service_tier().unwrap().as_str(), "default");
		assert!(
			serde_json::from_value::<AgentExecutionOverrides>(
				serde_json::json!({"approvalPolicy":"never"})
			)
			.is_err()
		);
	}
	#[test]
	fn configured_send_wire_roundtrip_retains_partial_selection_and_task_references() {
		let action = AgentActionDto::SendConfigured {
			root_id: EntityId::new("manager").unwrap(),
			text: HistoryText::new("continue").unwrap(),
			execution: AgentExecutionOverrides {
				reasoning_effort: Some(ConversationReasoningEffort::Medium),
				..Default::default()
			},
			attachments: vec![],
			task_references: vec![AgentTaskReferenceDto {
				work_id: EntityId::new("other").unwrap(),
				thread_id: WireText::new("native-other").unwrap(),
				title: WireText::new("Evidence").unwrap(),
			}],
		};
		let wire = serde_json::to_value(&action).unwrap();

		assert_eq!(wire["data"]["execution"], serde_json::json!({"reasoning_effort":"medium"}));

		let decoded: AgentActionDto = serde_json::from_value(wire).unwrap();

		assert_eq!(decoded, action);
	}
}
#[cfg(test)]
mod creation_tests {
	use serde_json::Value;

	use crate::AgentActionDto;

	#[test]
	fn creation_keeps_legacy_effort_and_nullable_inheritance_distinct() {
		for effort in [serde_json::json!("none"), serde_json::json!("high"), Value::Null] {
			let wire = serde_json::json!({"action":"start_configured","data":{"start":{"root_id":"root","prompt":"hello","model":"model","effort":effort,"cwd":"/tmp","account_id":null,"sandbox":"read_only"},"execution":{"model":"model","reasoning_effort":effort,"fast":false},"attachments":[]}});
			let action: AgentActionDto = serde_json::from_value(wire).unwrap();
			let AgentActionDto::StartConfigured { start, execution, .. } = action else {
				panic!("start")
			};

			assert_eq!(start.effort.as_ref().map(|v| v.as_str()), effort.as_str());
			assert_eq!(execution.reasoning_effort.as_ref().map(|v| v.as_str()), effort.as_str());
		}
	}
}
