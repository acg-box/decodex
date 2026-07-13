use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Exact Codex CLI build identity used as capability-cache authority.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct BuildId(String);
impl BuildId {
	/// Convert bounded version output into an opaque exact-build fingerprint.
	pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = value.into();

		if value.trim().is_empty() || value.len() > 256 || value.contains(['\r', '\n', '\0']) {
			return Err("Codex build identity is invalid");
		}

		let digest = Sha256::digest(value.as_bytes());

		Ok(Self(format!("sha256:{}", hex_digest(&digest))))
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

#[derive(Debug, Default, Serialize)]
pub(crate) struct ThreadListParams {
	pub limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadListResponse {
	pub data: Vec<ProtocolThread>,
	pub next_cursor: Option<String>,
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
		let build = BuildId::new("codex-cli api_key=sk-secretsecretsecret").unwrap();
		let debug = format!("{build:?}");
		let serialized = serde_json::to_string(&build).unwrap();

		assert_eq!(build.as_str().len(), 71);
		assert!(build.as_str().starts_with("sha256:"));
		assert!(!debug.contains("secret"));
		assert!(!serialized.contains("secret"));
	}

	#[test]
	fn oversized_or_multiline_build_output_is_rejected() {
		assert!(BuildId::new("x".repeat(257)).is_err());
		assert!(BuildId::new("codex-cli 1\napi_key=secret").is_err());
	}
}
