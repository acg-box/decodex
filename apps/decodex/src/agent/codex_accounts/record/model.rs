use serde::{Deserialize, Serialize};

use crate::agent::codex_accounts::{
	auth_failure::CodexAccountAuthFailure,
	record::auth_json::AuthDotJson,
	refresh::{self, CodexTokenData},
	usage,
};

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::agent::codex_accounts) struct AccountPoolRecord {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) email: Option<String>,
	#[serde(default, skip_serializing_if = "super::line::is_false")]
	pub(in crate::agent::codex_accounts) disabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) cooldown_until: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) last_selected_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) auth_failed_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) auth_failure: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::codex_accounts) last_refresh: Option<String>,
}
impl AccountPoolRecord {
	pub(in crate::agent::codex_accounts) fn display_name(&self) -> String {
		self.email()
			.or_else(|| self.account_id().map(redact_account_id))
			.unwrap_or_else(|| String::from("unnamed account"))
	}

	pub(in crate::agent::codex_accounts) fn account_fingerprint(&self) -> Option<String> {
		self.account_id().map(redact_account_id).or_else(|| self.email())
	}

	pub(in crate::agent::codex_accounts) fn auth_failure(&self) -> Option<&str> {
		self.auth_failure
			.as_deref()
			.map(str::trim)
			.filter(|failure| !failure.is_empty())
			.or_else(|| self.auth_failed_at_unix_epoch.map(|_| "authentication failed"))
	}

	pub(in crate::agent::codex_accounts) fn auth_failed_error(
		&self,
	) -> Option<CodexAccountAuthFailure> {
		self.auth_failure().map(|reason| CodexAccountAuthFailure::from_record(self, reason))
	}

	pub(in crate::agent::codex_accounts) fn mark_auth_failed(
		&mut self,
		now_unix_epoch: i64,
		reason: impl Into<String>,
	) {
		self.auth_failed_at_unix_epoch = Some(now_unix_epoch);
		self.auth_failure = Some(reason.into());
	}

	pub(in crate::agent::codex_accounts) fn clear_auth_failed(&mut self) {
		self.auth_failed_at_unix_epoch = None;
		self.auth_failure = None;
	}

	pub(in crate::agent::codex_accounts) fn matches_account_selector(
		&self,
		selector: &str,
	) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(redact_account_id).as_deref() == Some(selector)
	}

	pub(in crate::agent::codex_accounts) fn access_token(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.access_token.as_str())
			.filter(|token| !token.trim().is_empty())
	}

	pub(in crate::agent::codex_accounts) fn refresh_token(&self) -> Option<String> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.refresh_token.as_str())
			.filter(|token| !token.trim().is_empty())
			.map(str::to_owned)
	}

	pub(in crate::agent::codex_accounts) fn account_id(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.and_then(|tokens| tokens.account_id.as_deref())
			.filter(|account_id| !account_id.trim().is_empty())
	}

	pub(in crate::agent::codex_accounts) fn email(&self) -> Option<String> {
		usage::nonblank_string(self.email.as_deref())
			.or_else(|| {
				self.tokens
					.as_ref()
					.and_then(|tokens| usage::nonblank_string(tokens.email.as_deref()))
			})
			.or_else(|| {
				self.tokens
					.as_ref()
					.and_then(|tokens| refresh::jwt_email_claim(tokens.id_token.as_deref()))
			})
	}

	pub(in crate::agent::codex_accounts) fn auth_dot_json(&self) -> AuthDotJson {
		AuthDotJson {
			email: self.email(),
			auth_mode: self.auth_mode.clone(),
			openai_api_key: self.openai_api_key.clone(),
			tokens: self.tokens.clone(),
			last_refresh: self.last_refresh.clone(),
		}
	}
}

pub(in crate::agent::codex_accounts) fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}
