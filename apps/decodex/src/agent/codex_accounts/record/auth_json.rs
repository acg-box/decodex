use serde::{Deserialize, Serialize};

use crate::agent::codex_accounts::refresh::CodexTokenData;

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::agent::codex_accounts) struct AuthDotJson {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) last_refresh: Option<String>,
}
