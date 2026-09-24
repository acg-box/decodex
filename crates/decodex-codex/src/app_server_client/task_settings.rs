//! Bounded configured-model facts from complete native settings publications.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Known model settings from one native publication, not per-inference usage metadata.
/// Native collaboration instructions and permission settings are deliberately not copied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeTaskModelSettings {
	/// Exact configured model.
	pub model: String,
	/// Exact native provider name; it is not proof of OpenAI authentication requirements.
	pub model_provider: String,
	/// Native effort spelling. Null is known-unset, unlike an absent field.
	pub effort: Option<String>,
	/// Configured native tier. Null is known-unset, unlike an absent field.
	pub service_tier: Option<String>,
}

impl NativeTaskModelSettings {
	/// Project only complete bounded facts. Unknown fields do not widen this projection.
	pub fn from_notification(value: &Value) -> Option<Self> {
		Self::from_fields(value, "effort")
	}

	/// Read configured facts from a start/resume reply, whose effort field has another name.
	pub fn from_thread_response(value: &Value) -> Option<Self> {
		Self::from_fields(value, "reasoningEffort")
	}

	fn from_fields(value: &Value, effort_field: &str) -> Option<Self> {
		let text = |field: &str, limit: usize| {
			value.get(field)?.as_str().filter(|s| valid(s, limit)).map(str::to_owned)
		};
		let optional = |field: &str| match value.get(field)? {
			Value::Null => Some(None),
			Value::String(s) if valid(s, 128) => Some(Some(s.clone())),
			_ => None,
		};
		Some(Self {
			model: text("model", 256)?,
			model_provider: text("modelProvider", 256)?,
			effort: optional(effort_field)?,
			service_tier: optional("serviceTier")?,
		})
	}
}

fn valid(text: &str, limit: usize) -> bool {
	!text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn complete_model_facts_keep_unknown_values_and_distinguish_missing_from_null() {
		let good = json!({"model":"future-model","modelProvider":"custom","effort":null,"serviceTier":"future-tier",
			"collaborationMode":{"settings":{"developer_instructions":"private instructions"}},"approvalPolicy":"never"});
		let projected = NativeTaskModelSettings::from_notification(&good).unwrap();
		assert_eq!(projected.effort, None);
		assert_eq!(projected.service_tier.as_deref(), Some("future-tier"));
		assert!(!serde_json::to_string(&projected).unwrap().contains("private instructions"));
		let mut response = good.clone();
		response.as_object_mut().unwrap().remove("effort");
		response["reasoningEffort"] = Value::Null;
		assert_eq!(NativeTaskModelSettings::from_thread_response(&response), Some(projected));
		assert!(NativeTaskModelSettings::from_notification(&response).is_none());
		for field in ["model", "modelProvider", "effort", "serviceTier"] {
			let mut missing = good.clone();
			missing.as_object_mut().unwrap().remove(field);
			assert!(NativeTaskModelSettings::from_notification(&missing).is_none());
			let mut invalid = good.clone();
			invalid[field] = json!("\n");
			assert!(NativeTaskModelSettings::from_notification(&invalid).is_none());
		}
	}
}
