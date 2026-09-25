//! Exact native refusals. Callers must also prove request identity and no prior effects.

/// Native refusal before the requested turn is admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDispatchRefusal {
	/// The native server no longer accepts work on this connection.
	ServerDraining,
	/// Current managed requirements reject the retained provider configuration.
	ManagedProviderChanged,
}

/// Classify only fixed native invalid-request messages, never substrings or private data.
/// This is not authority to retry or to discard effects from earlier requests.
pub fn classify_dispatch_refusal(code: i64, message: &str) -> Option<NativeDispatchRefusal> {
	if code != -32600 {
		return None;
	}
	match message {
		"Server is draining; retry after reconnecting" =>
			Some(NativeDispatchRefusal::ServerDraining),
		"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent" =>
			Some(NativeDispatchRefusal::ManagedProviderChanged),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::{NativeDispatchRefusal, classify_dispatch_refusal};
	#[test]
	fn native_refusals_require_exact_code_and_message() {
		for (message, expected) in [
			("Server is draining; retry after reconnecting", NativeDispatchRefusal::ServerDraining),
			(
				"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent",
				NativeDispatchRefusal::ManagedProviderChanged,
			),
		] {
			assert_eq!(classify_dispatch_refusal(-32600, message), Some(expected));
			assert_eq!(classify_dispatch_refusal(-32603, message), None);
			assert_eq!(classify_dispatch_refusal(-32600, &format!("{message} ")), None);
			assert_eq!(classify_dispatch_refusal(-32600, &format!("prefix: {message}")), None);
		}
	}
}
