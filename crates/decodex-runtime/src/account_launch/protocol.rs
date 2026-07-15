use std::{
	fmt::{Debug, Formatter},
	ops::Deref,
};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize as _, Zeroizing};

use decodex_codex::{ThreadId, ThreadSummary};

#[doc(hidden)]
pub const MAX_APP_SERVER_FRAME_BYTES: usize = 1_024 * 1_024;

impl From<&ProtocolThread> for ThreadSummary {
	fn from(value: &ProtocolThread) -> Self {
		Self {
			id: ThreadId::from_protocol(value.id.as_str()),
			archived: value.archived,
			parent_thread_id: value.parent_thread_id.as_deref().map(ThreadId::from_protocol),
		}
	}
}

/// One independently zeroizing string allocated directly by typed Serde decoding.
#[derive(Deserialize)]
#[serde(transparent)]
#[doc(hidden)]
pub struct SensitiveString(Zeroizing<String>);
impl SensitiveString {
	#[doc(hidden)]
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for SensitiveString {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("SensitiveString([REDACTED])")
	}
}
impl Deref for SensitiveString {
	type Target = str;

	fn deref(&self) -> &Self::Target {
		self.as_str()
	}
}
impl Drop for SensitiveString {
	fn drop(&mut self) {
		self.0.zeroize();

		#[cfg(test)]
		sensitive_string_test_counter::increment();
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams<'a> {
	pub client_info: ClientInfo<'a>,
	pub capabilities: InitializeCapabilities,
}

#[derive(Debug, Serialize)]
pub struct ClientInfo<'a> {
	pub name: &'a str,
	pub version: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
	pub experimental_api: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
	pub codex_home: SensitiveString,
	pub platform_family: SensitiveString,
	pub platform_os: SensitiveString,
	pub user_agent: SensitiveString,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
	pub limit: u32,
	pub use_state_db_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
	pub data: Vec<ProtocolThread>,
	#[serde(rename = "nextCursor")]
	pub _next_cursor: Option<SensitiveString>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadParams<'a> {
	pub thread_id: &'a str,
	pub include_turns: bool,
}

#[derive(Debug, Deserialize)]
pub struct ThreadReadResponse {
	pub thread: ProtocolThread,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSearchParams<'a> {
	pub search_term: &'a str,
	pub limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSearchResponse {
	pub data: Vec<ProtocolThread>,
	#[serde(rename = "nextCursor")]
	pub _next_cursor: Option<SensitiveString>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReadResponse {
	pub account: Option<ProtocolAccount>,
	pub requires_openai_auth: bool,
}

#[derive(Debug, Deserialize)]
pub struct ProtocolAccount {
	#[serde(rename = "type")]
	pub kind: SensitiveString,
	pub email: Option<SensitiveString>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolThread {
	pub id: SensitiveString,
	#[serde(default)]
	pub archived: bool,
	pub parent_thread_id: Option<SensitiveString>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
	pub code: i64,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse<T> {
	pub id: u64,
	pub result: Option<T>,
	pub error: Option<JsonRpcError>,
}

#[cfg(test)]
pub(crate) fn reset_sensitive_string_drops() {
	sensitive_string_test_counter::reset();
}

#[cfg(test)]
pub(crate) fn sensitive_string_drops() -> usize {
	sensitive_string_test_counter::count()
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod sensitive_string_test_counter {
	use std::cell::Cell;

	thread_local! {
		static DROPS: Cell<usize> = const { Cell::new(0) };
	}

	pub(super) fn increment() {
		DROPS.with(|drops| drops.set(drops.get() + 1));
	}

	pub(super) fn reset() {
		DROPS.with(|drops| drops.set(0));
	}

	pub(super) fn count() -> usize {
		DROPS.with(Cell::get)
	}
}
#[cfg(test)]
mod tests {
	use crate::account_launch::protocol::{
		AccountReadResponse, InitializeResponse, JsonRpcResponse, ThreadListResponse,
		ThreadReadResponse,
	};
	use decodex_codex::BuildId;

	#[test]
	fn build_identity_is_opaque_and_never_exports_credential_shaped_output() {
		let build =
			BuildId::from_attestation("codex-cli api_key=sk-secretsecretsecret", &[7; 32]).unwrap();
		let debug = format!("{build:?}");
		let serialized = serde_json::to_string(&build).unwrap();

		assert_eq!(build.as_str().len(), 71);
		assert!(build.as_str().starts_with("sha256:"));
		assert!(!debug.contains("secret"));
		assert!(!serialized.contains("secret"));
	}

	#[test]
	fn oversized_or_multiline_build_output_is_rejected() {
		assert!(BuildId::from_attestation(&"x".repeat(257), &[0; 32]).is_err());
		assert!(BuildId::from_attestation("codex-cli 1\napi_key=secret", &[0; 32]).is_err());
	}

	#[test]
	fn exact_build_identity_includes_executable_content() {
		let first = BuildId::from_attestation("codex-cli 1", &[1; 32]).unwrap();
		let second = BuildId::from_attestation("codex-cli 1", &[2; 32]).unwrap();

		assert_ne!(first, second);
	}

	#[test]
	fn completed_initialize_fields_zeroize_when_later_fields_fail() {
		for json in [
			br#"{"codexHome":"/tmp/\u0073ecret","platformFamily":"u\u006eix","platformOs":"test"}"#.as_slice(),
			br#"{"codexHome":"/tmp/\u0073ecret","platformFamily":"u\u006eix","platformOs":"test","userAgent":17}"#.as_slice(),
		] {
			super::reset_sensitive_string_drops();

			assert!(serde_json::from_slice::<InitializeResponse>(json).is_err());
			assert_eq!(super::sensitive_string_drops(), 3);
		}
	}

	#[test]
	fn completed_nested_account_fields_zeroize_when_outer_field_fails() {
		super::reset_sensitive_string_drops();

		let json = br#"{"account":{"type":"chat\u0067pt","email":"private\u0040example.test"},"requiresOpenaiAuth":{}}"#;

		assert!(serde_json::from_slice::<AccountReadResponse>(json).is_err());
		assert_eq!(super::sensitive_string_drops(), 2);
	}

	#[test]
	fn completed_nested_thread_fields_zeroize_on_late_element_and_cursor_failures() {
		for json in [
			br#"{"data":[{"id":"first\u002did","archived":false,"parentThreadId":"parent\u002did"},{"id":"second\u002did","archived":{}}],"nextCursor":null}"#.as_slice(),
			br#"{"data":[{"id":"first\u002did","archived":false,"parentThreadId":"parent\u002did"}],"nextCursor":{}}"#.as_slice(),
		] {
			super::reset_sensitive_string_drops();

			assert!(serde_json::from_slice::<ThreadListResponse>(json).is_err());
			assert!(super::sensitive_string_drops() >= 2);
		}
	}

	#[test]
	fn completed_thread_read_fields_zeroize_when_json_rpc_tail_fails() {
		super::reset_sensitive_string_drops();

		let json = br#"{"id":7,"result":{"thread":{"id":"thread\u002did","archived":false,"parentThreadId":"parent\u002did"}},"error":"wrong"}"#;

		assert!(serde_json::from_slice::<JsonRpcResponse<ThreadReadResponse>>(json).is_err());
		assert_eq!(super::sensitive_string_drops(), 2);
	}

	#[test]
	fn successful_typed_debug_output_redacts_every_inbound_string() {
		let response: InitializeResponse = serde_json::from_slice(
			br#"{"codexHome":"/tmp/private","platformFamily":"private","platformOs":"private","userAgent":"private"}"#,
		)
		.unwrap();
		let debug = format!("{response:?}");

		assert!(!debug.contains("private"));
		assert_eq!(debug.matches("[REDACTED]").count(), 4);
	}
}
