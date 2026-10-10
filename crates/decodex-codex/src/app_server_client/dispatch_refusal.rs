//! Exact native refusals. Callers must also prove request identity and no prior effects.

/// Native refusal before the requested turn is admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDispatchRefusal {
	/// The native server no longer accepts work on this connection.
	ServerDraining,
	/// Current managed requirements reject the retained provider configuration.
	ManagedProviderChanged,
}

/// Classify the native shutdown reason, with exact-message fallback for older servers.
/// This is not authority to retry or to discard effects from earlier requests.
pub fn classify_dispatch_refusal(
	code: i64,
	message: &str,
	data: Option<&serde_json::Value>,
) -> Option<NativeDispatchRefusal> {
	if code != -32_600 {
		return None;
	}

	if data.and_then(|value| value.get("reason")).and_then(serde_json::Value::as_str)
		== Some("serverShuttingDown")
	{
		return Some(NativeDispatchRefusal::ServerDraining);
	}
	match message {
		"Server is draining; retry after reconnecting" if data.is_none() =>
			Some(NativeDispatchRefusal::ServerDraining),
		"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent" =>
			Some(NativeDispatchRefusal::ManagedProviderChanged),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use crate::app_server_client::dispatch_refusal::{self, NativeDispatchRefusal};

	#[test]
	fn native_refusals_require_exact_code_and_message() {
		for (message, expected) in [
			("Server is draining; retry after reconnecting", NativeDispatchRefusal::ServerDraining),
			(
				"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent",
				NativeDispatchRefusal::ManagedProviderChanged,
			),
		] {
			assert_eq!(
				dispatch_refusal::classify_dispatch_refusal(-32_600, message, None),
				Some(expected)
			);
			assert_eq!(dispatch_refusal::classify_dispatch_refusal(-32_603, message, None), None);
			assert_eq!(
				dispatch_refusal::classify_dispatch_refusal(-32_600, &format!("{message} "), None),
				None
			);
			assert_eq!(
				dispatch_refusal::classify_dispatch_refusal(
					-32_600,
					&format!("prefix: {message}"),
					None
				),
				None
			);
		}
	}
	#[test]
	fn structured_shutdown_never_falls_back_over_present_data() {
		let reason = serde_json::json!({"reason":"serverShuttingDown"});
		assert_eq!(
			dispatch_refusal::classify_dispatch_refusal(
				-32_600,
				"New shutdown wording",
				Some(&reason)
			),
			Some(NativeDispatchRefusal::ServerDraining)
		);
		assert_eq!(
			dispatch_refusal::classify_dispatch_refusal(
				-32_603,
				"New shutdown wording",
				Some(&reason)
			),
			None
		);
		for data in [
			serde_json::json!({}),
			serde_json::json!({"reason":"anotherReason"}),
			serde_json::json!({"reason":7}),
		] {
			assert_eq!(
				dispatch_refusal::classify_dispatch_refusal(
					-32_600,
					"Server is draining; retry after reconnecting",
					Some(&data)
				),
				None
			);
		}
	}
}
