//! Connection-owned capabilities; read-only and ordinary clients do not service forms.
use serde::Serialize;
use serde_json::{self, Value};

/// Capabilities supported by this connection's request consumer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
	experimental_api: bool,
	opt_out_notification_methods: &'static [&'static str],
	#[serde(skip_serializing_if = "Option::is_none")]
	extensions: Option<Value>,
}
impl InitializeCapabilities {
	/// Declare the native form route consumed by the retained Agent.
	pub fn for_agent() -> Self {
		Self {
			extensions: Some(serde_json::json!({"openai/form":{},"openai/standard-form-input":{}})),
			..Self::default()
		}
	}
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

#[cfg(test)]
mod tests {

	use crate::app_server_client::initialize::InitializeCapabilities;

	#[test]
	fn only_agent_advertises_the_supported_form_extension() {
		let ordinary = serde_json::to_value(InitializeCapabilities::default()).unwrap();

		assert_eq!(
			ordinary,
			serde_json::json!({"experimentalApi":true,"optOutNotificationMethods":["rawResponseItem/completed"]})
		);

		let mut agent = serde_json::to_value(InitializeCapabilities::for_agent()).unwrap();

		assert_eq!(
			agent.as_object_mut().unwrap().remove("extensions"),
			Some(serde_json::json!({"openai/form":{},"openai/standard-form-input":{}}))
		);
		assert_eq!(agent, ordinary);
	}
}
