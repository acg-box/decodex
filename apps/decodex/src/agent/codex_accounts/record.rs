#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	env,
	error::Error,
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
};

use serde::{Deserialize, Serialize};

use crate::{
	agent::codex_accounts::{
		login::CodexAccountLogin,
		refresh::{
			CodexTokenData, ProactiveRefreshReason, jwt_email_claim, jwt_expiration_unix_epoch,
			rfc3339_unix_epoch,
		},
		usage::{AccountProfileSnapshot, AccountUsageSnapshot, nonblank_string},
	},
	prelude::eyre,
	state::CodexAccountActivitySummary,
};

use super::{
	DEFAULT_PROFILE_ENDPOINT, DEFAULT_USAGE_ENDPOINT, TOKEN_REFRESH_INTERVAL_SECONDS,
	auth_failure::CodexAccountAuthFailure,
};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct AuthDotJson {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(super) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) last_refresh: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct AccountPoolRecord {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) email: Option<String>,
	#[serde(default, skip_serializing_if = "is_false")]
	pub(super) disabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) cooldown_until: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) last_selected_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) auth_failed_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) auth_failure: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(super) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) last_refresh: Option<String>,
}
impl AccountPoolRecord {
	pub(super) fn display_name(&self) -> String {
		self.email()
			.or_else(|| self.account_id().map(redact_account_id))
			.unwrap_or_else(|| String::from("unnamed account"))
	}

	pub(super) fn account_fingerprint(&self) -> Option<String> {
		self.account_id().map(redact_account_id).or_else(|| self.email())
	}

	pub(super) fn auth_failure(&self) -> Option<&str> {
		self.auth_failure
			.as_deref()
			.map(str::trim)
			.filter(|failure| !failure.is_empty())
			.or_else(|| self.auth_failed_at_unix_epoch.map(|_| "authentication failed"))
	}

	pub(super) fn auth_failed_error(&self) -> Option<CodexAccountAuthFailure> {
		self.auth_failure().map(|reason| CodexAccountAuthFailure::from_record(self, reason))
	}

	pub(super) fn mark_auth_failed(&mut self, now_unix_epoch: i64, reason: impl Into<String>) {
		self.auth_failed_at_unix_epoch = Some(now_unix_epoch);
		self.auth_failure = Some(reason.into());
	}

	pub(super) fn clear_auth_failed(&mut self) {
		self.auth_failed_at_unix_epoch = None;
		self.auth_failure = None;
	}

	pub(super) fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(redact_account_id).as_deref() == Some(selector)
	}

	pub(super) fn access_token(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.access_token.as_str())
			.filter(|token| !token.trim().is_empty())
	}

	pub(super) fn refresh_token(&self) -> Option<String> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.refresh_token.as_str())
			.filter(|token| !token.trim().is_empty())
			.map(str::to_owned)
	}

	pub(super) fn account_id(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.and_then(|tokens| tokens.account_id.as_deref())
			.filter(|account_id| !account_id.trim().is_empty())
	}

	pub(super) fn email(&self) -> Option<String> {
		nonblank_string(self.email.as_deref())
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| nonblank_string(tokens.email.as_deref()))
			})
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| jwt_email_claim(tokens.id_token.as_deref()))
			})
	}

	pub(super) fn auth_dot_json(&self) -> AuthDotJson {
		AuthDotJson {
			email: self.email(),
			auth_mode: self.auth_mode.clone(),
			openai_api_key: self.openai_api_key.clone(),
			tokens: self.tokens.clone(),
			last_refresh: self.last_refresh.clone(),
		}
	}

	pub(super) fn configured_activity_summary(
		&self,
		now_unix_epoch: i64,
	) -> Option<CodexAccountActivitySummary> {
		let account_fingerprint = self.account_fingerprint()?;
		let status = if self.disabled {
			"disabled"
		} else if self.auth_failure().is_some() {
			"auth_failed"
		} else if self
			.cooldown_until_unix_epoch
			.is_some_and(|cooldown_until| cooldown_until > now_unix_epoch)
		{
			"cooldown"
		} else if self.access_token().is_none() {
			"unusable"
		} else {
			"available"
		};

		Some(CodexAccountActivitySummary {
			account_fingerprint,
			email: self.email(),
			status: String::from(status),
			refresh_status: if self.auth_failure().is_some() {
				String::from("auth_failed")
			} else {
				String::from("not_checked")
			},
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(
				self.auth_failure()
					.map_or_else(|| String::from("configured account"), ToOwned::to_owned),
			),
			..CodexAccountActivitySummary::default()
		})
	}

	pub(super) fn login_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let access_token = self
			.access_token()
			.ok_or_else(|| {
				eyre::eyre!("Codex account `{}` is missing an access token.", self.display_name())
			})?
			.to_owned();
		let account_id = self
			.account_id()
			.ok_or_else(|| {
				eyre::eyre!("Codex account `{}` is missing an account id.", self.display_name())
			})?
			.to_owned();
		let summary = CodexAccountActivitySummary {
			account_fingerprint: redact_account_id(&account_id),
			email: self.email(),
			plan_type: usage.plan_type.clone(),
			status: if usage.is_limited() {
				String::from("usage_limited")
			} else {
				String::from("available")
			},
			refresh_status: refresh_status.to_owned(),
			checked_at_unix_epoch: Some(usage.checked_at_unix_epoch),
			selected_at_unix_epoch: None,
			primary_window_seconds: usage.primary.as_ref().and_then(|window| window.window_seconds),
			primary_remaining_percent: usage
				.primary
				.as_ref()
				.map(|window| window.remaining_percent),
			primary_resets_at_unix_epoch: usage
				.primary
				.as_ref()
				.and_then(|window| window.resets_at_unix_epoch),
			secondary_window_seconds: usage
				.secondary
				.as_ref()
				.and_then(|window| window.window_seconds),
			secondary_remaining_percent: usage
				.secondary
				.as_ref()
				.map(|window| window.remaining_percent),
			secondary_resets_at_unix_epoch: usage
				.secondary
				.as_ref()
				.and_then(|window| window.resets_at_unix_epoch),
			credits_has_credits: usage.credits.as_ref().map(|credits| credits.has_credits),
			credits_unlimited: usage.credits.as_ref().map(|credits| credits.unlimited),
			credits_balance: usage.credits.and_then(|credits| credits.balance),
			rate_limit_reached_type: usage.rate_limit_reached_type,
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(String::from("usage probe ok")),
			..CodexAccountActivitySummary::default()
		};

		Ok(CodexAccountLogin {
			access_token,
			account_id,
			plan_type: summary.plan_type.clone(),
			last_selected_at_unix_epoch: self.last_selected_at_unix_epoch,
			summary,
			account_summaries: Vec::new(),
		})
	}

	pub(super) fn activity_summary_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountActivitySummary> {
		Ok(self.login_from_usage(usage, refresh_status)?.summary)
	}

	pub(super) fn activity_summary_from_usage_profile(
		&self,
		usage: AccountUsageSnapshot,
		profile: Option<AccountProfileSnapshot>,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountActivitySummary> {
		let mut summary = self.activity_summary_from_usage(usage, refresh_status)?;

		if let Some(profile) = profile {
			profile.apply_to_summary(&mut summary);
		}

		Ok(summary)
	}

	pub(super) fn probe_failed_activity_summary(
		&self,
		now_unix_epoch: i64,
		refresh_status: &str,
		error: &(dyn Error + '_),
	) -> CodexAccountActivitySummary {
		let mut summary = self.configured_activity_summary(now_unix_epoch).unwrap_or_default();

		summary.status = if refresh_status == "failed" {
			String::from("unusable")
		} else {
			String::from("probe_failed")
		};
		summary.refresh_status = refresh_status.to_owned();
		summary.note = Some(format!("usage probe failed: {error}"));

		summary
	}

	pub(super) fn auth_failed_activity_summary(
		&self,
		now_unix_epoch: i64,
	) -> CodexAccountActivitySummary {
		let mut summary = self.configured_activity_summary(now_unix_epoch).unwrap_or_default();

		summary.status = String::from("auth_failed");
		summary.refresh_status = String::from("auth_failed");
		summary.note = self.auth_failure().map(ToOwned::to_owned);

		summary
	}

	pub(super) fn proactive_refresh_reason(
		&self,
		now_unix_epoch: i64,
	) -> Option<ProactiveRefreshReason> {
		let tokens = self.tokens.as_ref()?;

		if let Some(expires_at) = jwt_expiration_unix_epoch(&tokens.access_token) {
			return (expires_at <= now_unix_epoch)
				.then_some(ProactiveRefreshReason::AccessTokenExpired);
		}

		let last_refresh = self.last_refresh.as_deref().and_then(rfc3339_unix_epoch)?;

		(last_refresh < now_unix_epoch.saturating_sub(TOKEN_REFRESH_INTERVAL_SECONDS))
			.then_some(ProactiveRefreshReason::LastRefreshStale)
	}
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum AccountPoolLine {
	Wrapped {
		#[serde(skip_serializing_if = "Option::is_none")]
		email: Option<String>,
		#[serde(default, skip_serializing_if = "is_false")]
		disabled: bool,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		last_selected_at_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		auth_failed_at_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		auth_failure: Option<String>,
		auth: AuthDotJson,
	},
	Flat(AccountPoolRecord),
}
impl AccountPoolLine {
	pub(super) fn into_record(self) -> AccountPoolRecord {
		match self {
			Self::Flat(record) => record,
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth,
			} => AccountPoolRecord {
				email: first_nonblank_string(email, auth.email),
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth_mode: auth.auth_mode,
				openai_api_key: auth.openai_api_key,
				tokens: auth.tokens,
				last_refresh: auth.last_refresh,
			},
		}
	}
}

pub(super) fn parse_account_records(
	input: &str,
	path: &Path,
) -> crate::prelude::Result<Vec<AccountPoolRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let parsed = serde_json::from_str::<AccountPoolLine>(trimmed).map_err(|error| {
			eyre::eyre!(
				"Codex accounts `{}` line {line_number} is not a valid auth JSONL entry: {error}",
				path.display()
			)
		})?;

		records.push(parsed.into_record());
	}

	if records.is_empty() {
		eyre::bail!("Codex accounts `{}` does not contain any account records.", path.display());
	}

	Ok(records)
}

pub(super) fn default_codex_auth_json_path() -> crate::prelude::Result<PathBuf> {
	if let Some(codex_home) =
		env::var_os("CODEX_HOME").map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
	{
		return Ok(codex_home.join("auth.json"));
	}

	let Some(home) = env::var_os("HOME") else {
		eyre::bail!("Failed to resolve `$HOME` for the Codex auth JSON path.");
	};

	Ok(PathBuf::from(home).join(".codex").join("auth.json"))
}

pub(super) fn default_profile_endpoint_for_usage_endpoint(usage_endpoint: &str) -> Option<String> {
	(usage_endpoint == DEFAULT_USAGE_ENDPOINT).then(|| DEFAULT_PROFILE_ENDPOINT.to_owned())
}

pub(super) fn sync_refreshed_record_to_codex_auth(
	record: &AccountPoolRecord,
	path: &Path,
) -> crate::prelude::Result<()> {
	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(_) => return Ok(()),
	};
	let auth = match serde_json::from_str::<AuthDotJson>(&input) {
		Ok(auth) => auth,
		Err(_) => return Ok(()),
	};
	let auth_account_id = auth
		.tokens
		.as_ref()
		.and_then(|tokens| tokens.account_id.as_deref())
		.filter(|account_id| !account_id.trim().is_empty());

	if auth_account_id != record.account_id() {
		return Ok(());
	}

	write_auth_json_atomically(path, &record.auth_dot_json())
}

pub(super) fn write_auth_json_atomically(
	path: &Path,
	auth: &AuthDotJson,
) -> crate::prelude::Result<()> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Codex auth JSON path `{}` must have a parent directory.", path.display())
	})?;
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Codex auth JSON path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let mut output = serde_json::to_string_pretty(auth)?;

	output.push('\n');

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;

	secure_account_file(&temp_path)?;

	fs::rename(temp_path, path)?;

	secure_account_file(path)?;

	Ok(())
}

pub(super) fn secure_account_file(path: &Path) -> crate::prelude::Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

fn first_nonblank_string(left: Option<String>, right: Option<String>) -> Option<String> {
	left.filter(|value| !value.trim().is_empty())
		.or_else(|| right.filter(|value| !value.trim().is_empty()))
}

pub(super) fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

const fn is_false(value: &bool) -> bool {
	!*value
}
