#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::prelude::{Result, eyre};

mod auth_json;
mod login;
mod output;
mod random_names;
mod store;
mod types;
mod usage_history;

pub(crate) use self::{
	login::{account_login, run_account_login},
	store::AccountStore,
	types::{
		AccountControlSummary, AccountIdentitySummary, AccountImportRequest, AccountListResponse,
		AccountLoginRequest, AccountSummary, AccountUseRequest, AccountUseResponse,
	},
};

use self::{
	auth_json::{
		AuthDotJson, CodexTokenData, first_nonblank_string, jwt_email_claim,
		jwt_expiration_unix_epoch, nonblank_string,
	},
	output::{print_list_response, print_use_response},
	random_names::{random_name, random_name_key, random_name_seed_for},
	usage_history::{DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER, account_recovery_action},
};

#[cfg(test)]
use self::usage_history::{account_capacity_multiplier, usage_history_path, usage_record_date};

const ACCOUNT_RANDOM_NAMES: &[&str] = &[
	"Alex", "Avery", "Bailey", "Blake", "Casey", "Charlie", "Clara", "Dana", "Drew", "Eden",
	"Elliot", "Emery", "Evan", "Finley", "Harper", "Hayden", "Iris", "Jamie", "Jordan", "Kai",
	"Kendall", "Lane", "Liam", "Logan", "Mason", "Maya", "Mia", "Morgan", "Noah", "Nora", "Owen",
	"Paige", "Parker", "Quinn", "Reese", "Remy", "Riley", "Rowan", "Sage", "Sasha", "Sidney",
	"Taylor", "Theo", "Val",
];

#[derive(Clone)]
struct AccountIdentity {
	account_id: Option<String>,
	email: Option<String>,
}
impl AccountIdentity {
	fn summary(&self) -> AccountIdentitySummary {
		let account_fingerprint = self
			.account_id
			.as_deref()
			.map(redact_account_id)
			.or_else(|| self.email.clone())
			.unwrap_or_else(|| String::from("unknown"));
		let selector = self.email.clone().unwrap_or_else(|| account_fingerprint.clone());

		AccountIdentitySummary { account_fingerprint, email: self.email.clone(), selector }
	}
}

#[derive(Clone, Deserialize, Serialize)]
struct AccountPoolRecord {
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
	#[serde(skip_serializing_if = "Option::is_none")]
	auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_refresh: Option<String>,
}
impl AccountPoolRecord {
	fn from_auth(auth: AuthDotJson) -> Result<Self> {
		let record = Self {
			email: first_nonblank_string(
				auth.email,
				auth.tokens.as_ref().and_then(|tokens| {
					nonblank_string(tokens.email.as_deref())
						.or_else(|| jwt_email_claim(tokens.id_token.as_deref()))
				}),
			),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_failed_at_unix_epoch: None,
			auth_failure: None,
			auth_mode: auth.auth_mode,
			openai_api_key: auth.openai_api_key,
			tokens: auth.tokens,
			last_refresh: auth.last_refresh,
		};

		record.validate_importable()?;

		Ok(record)
	}

	fn validate_importable(&self) -> Result<()> {
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.access_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.access_token`.");
		}
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.refresh_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.refresh_token`.");
		}
		if self.account_id().is_none() {
			eyre::bail!("Codex auth JSON is missing `tokens.account_id`.");
		}

		Ok(())
	}

	fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(redact_account_id).as_deref() == Some(selector)
	}

	fn auth_failure(&self) -> Option<&str> {
		self.auth_failure
			.as_deref()
			.map(str::trim)
			.filter(|failure| !failure.is_empty())
			.or_else(|| self.auth_failed_at_unix_epoch.map(|_| "authentication failed"))
	}

	fn matches_account_identity(&self, identity: &AccountIdentity) -> bool {
		identity
			.account_id
			.as_deref()
			.is_some_and(|account_id| self.account_id() == Some(account_id))
			|| identity.email.as_deref().is_some_and(|email| self.email().as_deref() == Some(email))
	}

	fn account_id(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.and_then(|tokens| tokens.account_id.as_deref())
			.filter(|account_id| !account_id.trim().is_empty())
	}

	fn email(&self) -> Option<String> {
		nonblank_string(self.email.as_deref())
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| nonblank_string(tokens.email.as_deref()))
			})
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| jwt_email_claim(tokens.id_token.as_deref()))
			})
	}

	fn identity(&self) -> AccountIdentity {
		AccountIdentity { account_id: self.account_id().map(str::to_owned), email: self.email() }
	}

	fn identity_summary(&self) -> AccountIdentitySummary {
		self.identity().summary()
	}

	fn auth_dot_json(&self) -> Result<AuthDotJson> {
		self.validate_importable()?;

		Ok(AuthDotJson {
			email: self.email(),
			auth_mode: self.auth_mode.clone(),
			openai_api_key: self.openai_api_key.clone(),
			tokens: self.tokens.clone(),
			last_refresh: self.last_refresh.clone(),
		})
	}

	fn summary(
		&self,
		fixed_selector: Option<&str>,
		codex_auth: Option<&AccountIdentity>,
		name_offsets: &BTreeMap<String, i64>,
	) -> AccountSummary {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let account_fingerprint = self
			.account_id()
			.map(redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let selector = self.email().unwrap_or_else(|| account_fingerprint.clone());
		let selected = fixed_selector.is_some_and(|fixed| self.matches_account_selector(fixed));
		let access_token_expires_at_unix_epoch =
			self.tokens.as_ref().and_then(|tokens| jwt_expiration_unix_epoch(&tokens.access_token));
		let refresh_token_present = self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.refresh_token)))
			.is_some();
		let status = if self.disabled {
			"disabled"
		} else if self.auth_failure().is_some() {
			"auth_failed"
		} else if self.cooldown_until_unix_epoch.is_some_and(|cooldown_until| cooldown_until > now)
		{
			"cooldown"
		} else if access_token_expires_at_unix_epoch.is_some_and(|expires_at| expires_at <= now) {
			"expired"
		} else if self.account_id().is_none() || !refresh_token_present {
			"unusable"
		} else {
			"available"
		};
		let random_name_seed = random_name_seed_for(account_fingerprint.as_str(), self.email());
		let random_name_key = random_name_key(&random_name_seed);
		let random_name_offset = name_offsets.get(&random_name_key).copied().unwrap_or_default();
		let recovery_action = account_recovery_action(
			status,
			refresh_token_present,
			if self.auth_failure().is_some() { Some("auth_failed") } else { None },
			self.auth_failure().or(Some("local account pool")),
		);

		AccountSummary {
			account_fingerprint,
			email: self.email(),
			selector,
			random_name: random_name(&random_name_seed, random_name_offset),
			random_name_key,
			random_name_offset,
			status: status.to_owned(),
			selected,
			codex_active: codex_auth
				.is_some_and(|identity| self.matches_account_identity(identity)),
			disabled: self.disabled,
			refresh_token_present,
			access_token_expires_at_unix_epoch,
			last_selected_at_unix_epoch: self.last_selected_at_unix_epoch,
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(
				self.auth_failure()
					.map_or_else(|| String::from("local account pool"), ToOwned::to_owned),
			),
			plan_type: None,
			capacity_multiplier: DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER,
			recovery_action,
			refresh_status: None,
			checked_at_unix_epoch: None,
			primary_window_seconds: None,
			primary_remaining_percent: None,
			primary_resets_at_unix_epoch: None,
			secondary_window_seconds: None,
			secondary_remaining_percent: None,
			secondary_resets_at_unix_epoch: None,
			credits_has_credits: None,
			credits_unlimited: None,
			credits_balance: None,
			rate_limit_reached_type: None,
			profile_display_name: None,
			profile_username: None,
			profile_checked_at_unix_epoch: None,
			profile_lifetime_tokens: None,
			profile_peak_daily_tokens: None,
			profile_longest_task_seconds: None,
			profile_current_streak_days: None,
			profile_longest_streak_days: None,
			profile_daily_usage: Vec::new(),
			seven_day_used_percent: None,
			seven_day_daily_average_percent: None,
			usage_records: Vec::new(),
		}
	}

	fn random_name_key(&self) -> String {
		let account_fingerprint = self
			.account_id()
			.map(redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let seed = random_name_seed_for(account_fingerprint.as_str(), self.email());

		random_name_key(&seed)
	}
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum AccountPoolLine {
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
	fn into_record(self) -> Result<AccountPoolRecord> {
		match self {
			Self::Flat(record) => Ok(record),
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth,
			} => {
				let mut record = AccountPoolRecord::from_auth(auth)?;

				record.email = first_nonblank_string(email, record.email);
				record.disabled = disabled;
				record.cooldown_until_unix_epoch = cooldown_until_unix_epoch;
				record.cooldown_until = cooldown_until;
				record.last_selected_at_unix_epoch = last_selected_at_unix_epoch;
				record.auth_failed_at_unix_epoch = auth_failed_at_unix_epoch;
				record.auth_failure = auth_failure;

				Ok(record)
			},
		}
	}
}

pub(crate) fn run_account_list(json: bool) -> Result<()> {
	print_list_response(&account_list()?, json)
}

pub(crate) fn run_account_select(selector: &str, json: bool) -> Result<()> {
	print_list_response(&account_select(selector)?, json)
}

pub(crate) fn run_account_clear(json: bool) -> Result<()> {
	print_list_response(&account_clear()?, json)
}

pub(crate) fn run_account_logout(selector: &str, json: bool) -> Result<()> {
	print_list_response(&account_logout(selector)?, json)
}

pub(crate) fn run_account_import(request: &AccountImportRequest) -> Result<()> {
	print_list_response(&account_import(&request.auth_json_path)?, request.json)
}

pub(crate) fn run_account_use(request: &AccountUseRequest) -> Result<()> {
	print_use_response(&account_use(request)?, request.json)
}

pub(crate) fn account_list() -> Result<AccountListResponse> {
	AccountStore::global()?.list()
}

pub(crate) fn account_list_with_cached_usage(force_refresh: bool) -> Result<AccountListResponse> {
	AccountStore::global()?.list_with_cached_usage(force_refresh)
}

pub(crate) fn hydrate_account_list_usage(mut response: AccountListResponse) -> AccountListResponse {
	let accounts_path = PathBuf::from(&response.accounts_path);

	response.hydrate_usage_from_path(&accounts_path, false);

	response
}

pub(crate) fn account_select(selector: &str) -> Result<AccountListResponse> {
	AccountStore::global()?.select(selector)
}

pub(crate) fn account_clear() -> Result<AccountListResponse> {
	AccountStore::global()?.clear_selection()
}

pub(crate) fn account_logout(selector: &str) -> Result<AccountListResponse> {
	AccountStore::global()?.logout(selector)
}

pub(crate) fn account_reroll_name(
	selector: &str,
	offset: Option<i64>,
) -> Result<AccountListResponse> {
	AccountStore::global()?.reroll_name(selector, offset)
}

pub(crate) fn account_import(auth_json_path: &Path) -> Result<AccountListResponse> {
	AccountStore::global()?.import_auth_json(auth_json_path)
}

pub(crate) fn account_use(request: &AccountUseRequest) -> Result<AccountUseResponse> {
	AccountStore::global()?.use_for_codex(&request.selector, request.auth_json_path.as_deref())
}

fn secure_account_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

const fn is_false(value: &bool) -> bool {
	!*value
}

#[cfg(test)] mod tests;
