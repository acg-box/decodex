use serde::{Deserialize, Serialize};

use crate::accounts::auth_json::CodexTokenData;

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::accounts) struct AccountPoolRecord {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) email: Option<String>,
	#[serde(default, skip_serializing_if = "is_false")]
	pub(in crate::accounts) disabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) cooldown_until: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) last_selected_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_failed_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_failure: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) last_refresh: Option<String>,
}

pub(super) const fn is_false(value: &bool) -> bool {
	!*value
}
