//! Explicit next-message changes; omitted fields inherit the native task setting.
use serde::{Deserialize, Serialize};

/// Only settings deliberately changed for this message. An empty value inherits all settings.
/// Full legacy execution objects remain valid and retain their original meaning.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefExecutionOverrides {
	/// A newly selected model, if changed.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub model: Option<crate::ConversationModel>,
	/// A newly selected reasoning effort, if changed.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub reasoning_effort: Option<crate::ConversationReasoningEffort>,
	/// Legacy explicit Fast selection. Omitted means inherit; false means standard.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub fast: Option<bool>,
	/// Explicit service tier, including standard. This takes precedence over legacy Fast.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub service_tier: Option<crate::ServiceTier>,
}

impl From<crate::ConversationExecutionSettings> for ChiefExecutionOverrides {
	fn from(value: crate::ConversationExecutionSettings) -> Self {
		Self {
			model: Some(value.model),
			reasoning_effort: Some(value.reasoning_effort),
			fast: Some(value.fast),
			service_tier: value.service_tier,
		}
	}
}

impl ChiefExecutionOverrides {
	/// Whether a message changes no native execution setting.
	pub fn is_empty(&self) -> bool {
		self.model.is_none()
			&& self.reasoning_effort.is_none()
			&& self.fast.is_none()
			&& self.service_tier.is_none()
	}

	/// A deliberate tier change, absent when the message inherits the current task tier.
	pub fn selected_service_tier(&self) -> Option<crate::ServiceTier> {
		self.service_tier.clone().or_else(|| self.fast.map(crate::ServiceTier::from_fast))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn partial_changes_and_legacy_full_selections_have_distinct_inheritance() {
		let inherited: ChiefExecutionOverrides = serde_json::from_value(json!({})).unwrap();
		assert!(inherited.is_empty());
		assert_eq!(serde_json::to_value(&inherited).unwrap(), json!({}));
		assert!(inherited.selected_service_tier().is_none());
		let effort: ChiefExecutionOverrides =
			serde_json::from_value(json!({"reasoning_effort":"high"})).unwrap();
		assert!(effort.model.is_none() && effort.selected_service_tier().is_none());
		let legacy: ChiefExecutionOverrides = serde_json::from_value(
			json!({"model":"chosen","reasoning_effort":"medium","fast":false,"service_tier":null}),
		)
		.unwrap();
		assert_eq!(legacy.model.unwrap().as_str(), "chosen");
		assert_eq!(legacy.fast, Some(false));
		let standard: ChiefExecutionOverrides =
			serde_json::from_value(json!({"service_tier":"default"})).unwrap();
		assert_eq!(standard.selected_service_tier().unwrap().as_str(), "default");
		assert!(
			serde_json::from_value::<ChiefExecutionOverrides>(json!({"approvalPolicy":"never"}))
				.is_err()
		);
	}
}
