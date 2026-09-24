//! Connection-owned capabilities; read-only and ordinary clients do not service forms.
use serde::Serialize;
use serde_json::{Value, json};

/// Capabilities supported by this connection's request consumer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
	experimental_api: bool,
	opt_out_notification_methods: &'static [&'static str],
	#[serde(skip_serializing_if = "Option::is_none")]
	extensions: Option<Value>,
}
impl Default for InitializeCapabilities {
	fn default() -> Self {
		Self {
			experimental_api: true,
			opt_out_notification_methods: &["rawResponseItem/completed"],
			extensions: None,
		}
	}
}
impl InitializeCapabilities {
	/// Declare the form route consumed by the retained Chief request handler.
	pub fn for_chief() -> Self {
		Self { extensions: Some(json!({"openai/elicitation":{"form":{}}})), ..Self::default() }
	}
}

#[cfg(test)]
mod tests {
	use super::InitializeCapabilities;
	use serde_json::json;
	#[test]
	fn only_chief_advertises_the_supported_form_extension() {
		let ordinary = serde_json::to_value(InitializeCapabilities::default()).unwrap();
		assert_eq!(
			ordinary,
			json!({"experimentalApi":true,"optOutNotificationMethods":["rawResponseItem/completed"]})
		);
		let mut chief = serde_json::to_value(InitializeCapabilities::for_chief()).unwrap();
		assert_eq!(
			chief.as_object_mut().unwrap().remove("extensions"),
			Some(json!({"openai/elicitation":{"form":{}}}))
		);
		assert_eq!(chief, ordinary);
	}
}
