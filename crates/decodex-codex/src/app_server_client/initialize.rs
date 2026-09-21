//! Client capabilities shared by direct and admitted Decodex connections.
use serde::Serialize;
use serde_json::{Value, json};

/// Declare supported form handling without advertising device verification or MCP apps UI.
/// Unsupported form semantics remain visible for an explicit decline or cancellation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
	experimental_api: bool,
	opt_out_notification_methods: &'static [&'static str],
	extensions: Value,
}

impl Default for InitializeCapabilities {
	fn default() -> Self {
		Self {
			experimental_api: true,
			opt_out_notification_methods: &["rawResponseItem/completed"],
			extensions: json!({"openai/elicitation": {"form": {}}}),
		}
	}
}
