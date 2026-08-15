use std::{
	fmt::{Debug, Formatter},
	ops::Deref,
};

use serde::{Deserialize, Deserializer, Serialize};
use zeroize::{Zeroize as _, Zeroizing};

use decodex_codex::{
	DecodexThreadSearchTerm, ExactThreadFacts, ExactThreadId, QuickTaskTurnStatus, ThreadCreatedAt,
	ThreadCwd, ThreadId, ThreadProvenance, ThreadSummary, ThreadTitle,
};

#[doc(hidden)]
pub const MAX_APP_SERVER_FRAME_BYTES: usize = 1_024 * 1_024;

impl From<&ProtocolThread> for ThreadSummary {
	fn from(value: &ProtocolThread) -> Self {
		Self {
			id: ThreadId::from_protocol(value.id.as_str()),
			// `thread/list` without an archived filter returns current threads. Newer Codex
			// app-server versions no longer repeat that query fact on each Thread object.
			archived: value.archived.unwrap_or(false),
			parent_thread_id: value.parent_thread_id.as_deref().map(ThreadId::from_protocol),
		}
	}
}

impl TryFrom<&ProtocolThread> for ExactThreadFacts {
	type Error = &'static str;

	fn try_from(value: &ProtocolThread) -> Result<Self, Self::Error> {
		let archived = value.archived.ok_or("Codex thread archived state is missing")?;

		exact_thread_facts(value, archived)
	}
}

pub(crate) fn exact_thread_facts(
	value: &ProtocolThread,
	archived: bool,
) -> Result<ExactThreadFacts, &'static str> {
	Ok(ExactThreadFacts {
		id: ExactThreadId::new(value.id.as_str())?,
		provenance: value
			.thread_source
			.as_deref()
			.map(ThreadProvenance::from_protocol)
			.transpose()?,
		created_at: ThreadCreatedAt::from_protocol(
			value.created_at.ok_or("Codex thread creation timestamp is missing")?,
		)?,
		title: value.name.as_deref().map(ThreadTitle::from_protocol).transpose()?,
		cwd: ThreadCwd::from_protocol(value.cwd.as_deref().ok_or("Codex thread cwd is missing")?)?,
		archived,
	})
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
	pub search_term: &'static str,
	pub limit: u32,
	pub use_state_db_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactThreadListParams<'a> {
	pub search_term: &'a DecodexThreadSearchTerm,
	pub archived: bool,
	pub limit: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactThreadStateListParams<'a> {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub search_term: Option<&'a str>,
	pub archived: bool,
	pub limit: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cursor: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
	pub data: Vec<ProtocolThread>,
	#[serde(rename = "nextCursor")]
	pub next_cursor: Option<SensitiveString>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadParams<'a> {
	pub thread_id: &'a str,
	pub include_turns: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactThreadReadParams<'a> {
	pub thread_id: &'a ExactThreadId,
	pub include_turns: bool,
}

#[derive(Debug, Deserialize)]
pub struct ThreadReadResponse {
	pub thread: ProtocolThread,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadArchiveParams<'a> {
	pub thread_id: &'a ExactThreadId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadArchiveResponse {}

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
	/// Legacy app-server versions repeated the list filter on every Thread. Current versions do
	/// not.
	#[serde(default)]
	pub archived: Option<bool>,
	pub parent_thread_id: Option<SensitiveString>,
	pub created_at: Option<i64>,
	pub name: Option<SensitiveString>,
	/// Current app-server fallback title material when no explicit name exists.
	#[serde(default)]
	pub preview: Option<SensitiveString>,
	pub cwd: Option<SensitiveString>,
	pub thread_source: Option<SensitiveString>,
	pub ephemeral: Option<bool>,
	#[serde(default)]
	pub turns: Vec<ProtocolTurn>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolTurn {
	pub id: SensitiveString,
	pub status: ProtocolTurnStatus,
	#[serde(default)]
	pub items: Vec<ProtocolThreadItem>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolTurnStatus {
	Completed,
	Interrupted,
	Failed,
	InProgress,
}
impl ProtocolTurnStatus {
	pub const fn into_quick_task(self) -> QuickTaskTurnStatus {
		match self {
			Self::Completed => QuickTaskTurnStatus::Completed,
			Self::Interrupted => QuickTaskTurnStatus::Interrupted,
			Self::Failed => QuickTaskTurnStatus::Failed,
			Self::InProgress => QuickTaskTurnStatus::InProgress,
		}
	}
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolThreadItem {
	#[serde(rename = "type")]
	pub kind: SensitiveString,
	#[serde(default)]
	pub client_id: Option<SensitiveString>,
	#[serde(default)]
	pub text: Option<SensitiveString>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
	pub code: i64,
	#[serde(rename = "message")]
	_message: SensitiveString,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse<T> {
	/// Legacy JSON-RPC version marker. Codex app-server JSONL responses omit this field, while
	/// older compatible peers can still send the standard marker.
	#[serde(default, deserialize_with = "deserialize_present_jsonrpc_version")]
	pub jsonrpc: Option<SensitiveString>,
	pub id: u64,
	pub result: Option<T>,
	pub error: Option<JsonRpcError>,
}
impl<T> JsonRpcResponse<T> {
	/// Accept the native Codex JSONL envelope and the legacy standard JSON-RPC marker only.
	pub fn has_compatible_version(&self) -> bool {
		self.jsonrpc.as_ref().is_none_or(|version| version.as_str() == "2.0")
	}
}

fn deserialize_present_jsonrpc_version<'de, D>(
	deserializer: D,
) -> Result<Option<SensitiveString>, D::Error>
where
	D: Deserializer<'de>,
{
	SensitiveString::deserialize(deserializer).map(Some)
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
		AccountReadResponse, ExactThreadReadParams, InitializeResponse, JsonRpcResponse,
		ThreadArchiveParams, ThreadListResponse, ThreadReadResponse,
	};
	use decodex_codex::{BuildId, ExactThreadFacts, ExactThreadId};

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
	fn exact_id_request_params_preserve_every_supported_punctuation_byte() {
		let raw = "thread:XY-1317/non-uuid_Case-Sensitive._~:@+$,;=[]{}()!%&'*? #";
		let exact = ExactThreadId::new(raw).unwrap();
		let read =
			serde_json::to_value(ExactThreadReadParams { thread_id: &exact, include_turns: true })
				.unwrap();
		let archive = serde_json::to_value(ThreadArchiveParams { thread_id: &exact }).unwrap();

		assert_eq!(read["threadId"], raw);
		assert_eq!(read["includeTurns"], true);
		assert_eq!(archive["threadId"], raw);
		assert!(!serde_json::to_string(&read).unwrap().contains("\\\\"));
	}

	#[test]
	fn exact_fact_conversion_retains_zeroizing_redacted_owned_text() {
		super::reset_sensitive_string_drops();

		let response: ThreadReadResponse = serde_json::from_slice(
			br#"{"thread":{"id":"thread:private-id","archived":false,"parentThreadId":null,"createdAt":1784073600,"name":"Decodex private title","cwd":"/tmp/private-repository","threadSource":"decodex.private"}}"#,
		)
		.unwrap();
		let facts = ExactThreadFacts::try_from(&response.thread).unwrap();

		drop(response);

		assert_eq!(super::sensitive_string_drops(), 4);
		assert_eq!(facts.id.as_str(), "thread:private-id");
		assert_eq!(facts.title.as_ref().unwrap().as_str(), "Decodex private title");
		assert_eq!(facts.cwd.as_str(), "/tmp/private-repository");
		assert_eq!(facts.provenance.as_ref().unwrap().as_str(), "decodex.private");
		assert!(!format!("{facts:?}").contains("private"));
	}

	#[test]
	fn current_thread_shape_accepts_filter_owned_archive_state() {
		let response: ThreadReadResponse = serde_json::from_slice(
			br#"{"thread":{"id":"thread:current-shape","parentThreadId":null,"createdAt":1784073600,"preview":"Decodex current shape","cwd":"/tmp/private-repository","threadSource":null}}"#,
		)
		.unwrap();

		assert_eq!(response.thread.archived, None);
		let facts = super::exact_thread_facts(&response.thread, true).unwrap();
		assert!(facts.archived);
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

		let json = br#"{"jsonrpc":"2.0","id":7,"result":{"thread":{"id":"thread\u002did","archived":false,"parentThreadId":"parent\u002did"}},"error":"wrong"}"#;

		assert!(serde_json::from_slice::<JsonRpcResponse<ThreadReadResponse>>(json).is_err());
		assert_eq!(super::sensitive_string_drops(), 3);
	}

	#[test]
	fn app_server_response_accepts_bare_or_legacy_v2_envelopes_and_rejects_other_markers() {
		for json in [
			br#"{"id":7,"result":{}}"#.as_slice(),
			br#"{"jsonrpc":"2.0","id":7,"result":{}}"#.as_slice(),
		] {
			let response =
				serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(json).unwrap();

			assert!(response.has_compatible_version());
		}

		let wrong = serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(
			br#"{"jsonrpc":"1.0","id":7,"result":{}}"#,
		)
		.unwrap();

		assert!(!wrong.has_compatible_version());

		for json in [
			br#"{"jsonrpc":null,"id":7,"result":{}}"#.as_slice(),
			br#"{"jsonrpc":2,"id":7,"result":{}}"#.as_slice(),
			br#"{"jsonrpc":{},"id":7,"result":{}}"#.as_slice(),
		] {
			assert!(serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(json).is_err());
		}
	}

	#[test]
	fn legacy_json_rpc_marker_is_zeroized_without_changing_bare_envelope_counts() {
		super::reset_sensitive_string_drops();

		let bare = serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(
			br#"{"id":7,"result":{}}"#,
		)
		.unwrap();

		drop(bare);
		assert_eq!(super::sensitive_string_drops(), 0);

		let legacy = serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(
			br#"{"jsonrpc":"2.0","id":7,"result":{}}"#,
		)
		.unwrap();

		drop(legacy);
		assert_eq!(super::sensitive_string_drops(), 1);
	}

	#[test]
	fn app_server_errors_require_and_zeroize_the_native_message() {
		super::reset_sensitive_string_drops();

		let response = serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(
			br#"{"id":7,"error":{"code":-32601,"message":"private provider detail"}}"#,
		)
		.unwrap();
		let debug = format!("{:?}", response.error.as_ref().unwrap());

		assert!(!debug.contains("private provider detail"));
		drop(response);
		assert_eq!(super::sensitive_string_drops(), 1);

		for json in [
			br#"{"id":7,"error":{"code":-32601}}"#.as_slice(),
			br#"{"id":7,"error":{"code":-32601,"message":null}}"#.as_slice(),
			br#"{"id":7,"error":{"code":-32601,"message":7}}"#.as_slice(),
		] {
			assert!(serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(json).is_err());
		}
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
