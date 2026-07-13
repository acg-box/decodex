use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub(crate) const MAX_APP_SERVER_FRAME_BYTES: usize = 1_024 * 1_024;

/// Exact Codex CLI build identity used as capability-cache authority.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct BuildId(String);
impl BuildId {
	pub(crate) fn from_attestation(
		version: &str,
		executable_digest: &[u8; 32],
	) -> Result<Self, &'static str> {
		if version.trim().is_empty() || version.len() > 256 || version.contains(['\r', '\n', '\0'])
		{
			return Err("Codex build identity is invalid");
		}

		let mut digest = Sha256::new();

		digest.update(version.as_bytes());
		digest.update([0]);
		digest.update(executable_digest);

		Ok(Self(format!("sha256:{}", hex_digest(&digest.finalize()))))
	}

	#[cfg(test)]
	pub(crate) fn for_test(value: &str) -> Self {
		Self::from_attestation(value, &[0; 32]).expect("test build identity must be valid")
	}

	/// Return the opaque exact-build fingerprint.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Opaque Codex thread identifier.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ThreadId(String);
impl ThreadId {
	pub(crate) fn from_protocol(value: String) -> Self {
		Self(Self::normalize(&value))
	}

	pub(crate) fn normalize(value: &str) -> String {
		let bytes = value.as_bytes();
		let is_uuid = bytes.len() == 36
			&& [8, 13, 18, 23].into_iter().all(|index| bytes[index] == b'-')
			&& bytes
				.iter()
				.enumerate()
				.all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());

		if is_uuid {
			value.to_owned()
		} else {
			let digest = Sha256::digest(value.as_bytes());

			format!("sha256:{}", hex_digest(&digest))
		}
	}

	/// Return the opaque exact identifier.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Redacted read-only thread-list projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSummary {
	/// Opaque thread identifier.
	pub id: ThreadId,
	/// Whether Codex reports this thread as archived.
	pub archived: bool,
	/// Opaque parent identifier for a run-local collaboration actor.
	pub parent_thread_id: Option<ThreadId>,
}
impl From<ProtocolThread> for ThreadSummary {
	fn from(value: ProtocolThread) -> Self {
		Self {
			id: ThreadId::from_protocol(value.id),
			archived: value.archived,
			parent_thread_id: value.parent_thread_id.map(ThreadId::from_protocol),
		}
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeParams<'a> {
	pub client_info: ClientInfo<'a>,
	pub capabilities: InitializeCapabilities,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClientInfo<'a> {
	pub name: &'a str,
	pub version: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeCapabilities {
	pub experimental_api: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeResponse {
	pub codex_home: String,
	pub platform_family: String,
	pub platform_os: String,
	pub user_agent: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadListParams {
	pub limit: u32,
	pub use_state_db_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadListResponse {
	pub data: Vec<ProtocolThread>,
	#[serde(rename = "nextCursor")]
	pub _next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadReadParams<'a> {
	pub thread_id: &'a str,
	pub include_turns: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadReadResponse {
	pub thread: ProtocolThread,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadSearchParams<'a> {
	pub search_term: &'a str,
	pub limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadSearchResponse {
	pub data: Vec<ProtocolThread>,
	#[serde(rename = "nextCursor")]
	pub _next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountReadResponse {
	pub account: Option<ProtocolAccount>,
	pub requires_openai_auth: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProtocolAccount {
	#[serde(rename = "type")]
	pub kind: String,
	pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolThread {
	pub id: String,
	#[serde(default)]
	pub archived: bool,
	pub parent_thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcError {
	pub code: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcResponse<T> {
	pub id: u64,
	pub result: Option<T>,
	pub error: Option<JsonRpcError>,
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use crate::BuildId;

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
}
