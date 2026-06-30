#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
	prelude::{Result, eyre},
	state::CodexAccountProfileDailyUsageSummary,
};

mod auth_json;
mod login;
mod random_names;
mod store;
mod usage_history;

pub(crate) use self::{
	login::{account_login, run_account_login},
	store::AccountStore,
};

use self::{
	auth_json::{
		AuthDotJson, CodexTokenData, first_nonblank_string, jwt_email_claim,
		jwt_expiration_unix_epoch, nonblank_string,
	},
	random_names::{random_name, random_name_key, random_name_seed_for},
	usage_history::{
		AccountUsageDailySummary, AccountUsageEstimateSummary, DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER,
		account_recovery_action,
	},
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

pub(crate) struct AccountLoginRequest {
	pub(crate) codex_bin: String,
	pub(crate) keep_temp_home: bool,
}

pub(crate) struct AccountImportRequest {
	pub(crate) auth_json_path: PathBuf,
	pub(crate) json: bool,
}

pub(crate) struct AccountUseRequest {
	pub(crate) selector: String,
	pub(crate) auth_json_path: Option<PathBuf>,
	pub(crate) json: bool,
}

#[derive(Serialize)]
pub(crate) struct AccountListResponse {
	pub(crate) accounts_path: String,
	pub(crate) global_config_path: String,
	pub(crate) codex_auth_path: String,
	pub(crate) codex_auth: Option<AccountIdentitySummary>,
	pub(crate) control: AccountControlSummary,
	pub(crate) accounts: Vec<AccountSummary>,
	pub(crate) usage_estimate: Option<AccountUsageEstimateSummary>,
	pub(crate) usage_probe_error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AccountUseResponse {
	pub(crate) codex_auth_path: String,
	pub(crate) account: AccountIdentitySummary,
}

#[derive(Clone, Serialize)]
pub(crate) struct AccountIdentitySummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) selector: String,
}

#[derive(Serialize)]
pub(crate) struct AccountControlSummary {
	pub(crate) mode: String,
	pub(crate) account_selector: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AccountSummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) selector: String,
	pub(crate) random_name: String,
	pub(crate) random_name_key: String,
	pub(crate) random_name_offset: i64,
	pub(crate) status: String,
	pub(crate) selected: bool,
	pub(crate) codex_active: bool,
	pub(crate) disabled: bool,
	pub(crate) refresh_token_present: bool,
	pub(crate) access_token_expires_at_unix_epoch: Option<i64>,
	pub(crate) last_selected_at_unix_epoch: Option<i64>,
	pub(crate) cooldown_until_unix_epoch: Option<i64>,
	pub(crate) note: Option<String>,
	pub(crate) plan_type: Option<String>,
	pub(crate) capacity_multiplier: i64,
	pub(crate) recovery_action: Option<String>,
	pub(crate) refresh_status: Option<String>,
	pub(crate) checked_at_unix_epoch: Option<i64>,
	pub(crate) primary_window_seconds: Option<i64>,
	pub(crate) primary_remaining_percent: Option<i64>,
	pub(crate) primary_resets_at_unix_epoch: Option<i64>,
	pub(crate) secondary_window_seconds: Option<i64>,
	pub(crate) secondary_remaining_percent: Option<i64>,
	pub(crate) secondary_resets_at_unix_epoch: Option<i64>,
	pub(crate) credits_has_credits: Option<bool>,
	pub(crate) credits_unlimited: Option<bool>,
	pub(crate) credits_balance: Option<String>,
	pub(crate) rate_limit_reached_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_username: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_checked_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_lifetime_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_peak_daily_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_task_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_current_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub(crate) profile_daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	pub(crate) seven_day_used_percent: Option<i64>,
	pub(crate) seven_day_daily_average_percent: Option<f64>,
	pub(crate) usage_records: Vec<AccountUsageDailySummary>,
}

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

fn print_list_response(response: &AccountListResponse, json: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(response)?);

		return Ok(());
	}

	println!(
		"Codex account pool: {} ({})",
		response.control.mode,
		response.control.account_selector.as_deref().unwrap_or("balanced selection")
	);
	println!("accounts: {}", response.accounts.len());

	for account in &response.accounts {
		let marker = if account.selected { "*" } else { "-" };
		let email = account.email.as_deref().unwrap_or("no email");

		println!("{marker} {email} {} {}", account.account_fingerprint, account.status);
	}

	Ok(())
}

fn print_use_response(response: &AccountUseResponse, json: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(response)?);

		return Ok(());
	}

	println!(
		"Codex auth now uses {} ({})",
		response.account.email.as_deref().unwrap_or("no email"),
		response.account.account_fingerprint
	);
	println!("auth: {}", response.codex_auth_path);

	Ok(())
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

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use crate::{
		accounts::{AccountPoolRecord, AccountStore, AuthDotJson, CodexTokenData},
		state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary},
	};

	#[test]
	fn imports_auth_json_without_printing_tokens() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let auth_path = temp_dir.path().join("auth.json");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		fs::write(
			&auth_path,
			r#"{
				"email": "copy@example.com",
				"tokens": {
					"access_token": "header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh_token": "refresh-secret",
					"account_id": "acct_123456"
				}
			}"#,
		)
		.expect("auth json should write");

		let response = store.import_auth_json(&auth_path).expect("auth should import");
		let output = serde_json::to_string(&response).expect("response should serialize");

		assert_eq!(response.accounts.len(), 1);
		assert!(output.contains("copy@example.com"));
		assert!(output.contains("...123456"));
		assert!(!output.contains("refresh-secret"));
	}

	#[test]
	fn logout_removes_matching_account() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[AccountPoolRecord {
				email: Some(String::from("copy@example.com")),
				disabled: false,
				cooldown_until_unix_epoch: None,
				cooldown_until: None,
				last_selected_at_unix_epoch: None,
				auth_failed_at_unix_epoch: None,
				auth_failure: None,
				auth_mode: None,
				openai_api_key: None,
				tokens: Some(CodexTokenData {
					email: None,
					id_token: None,
					access_token: String::from("token"),
					refresh_token: String::from("refresh"),
					account_id: Some(String::from("acct_123456")),
				}),
				last_refresh: None,
			}])
			.expect("records should save");

		let response = store.logout("copy@example.com").expect("account should logout");

		assert!(response.accounts.is_empty());
		assert_eq!(fs::read_to_string(&store.accounts_path).expect("accounts should read"), "");
	}

	#[test]
	fn use_for_codex_overwrites_auth_json_from_pool() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let codex_auth_path = temp_dir.path().join(".codex/auth.json");
		let store = AccountStore::new_with_codex_auth_path(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
			codex_auth_path.clone(),
		);

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let response = store
			.use_for_codex("copy@example.com", None)
			.expect("account should become Codex auth");
		let auth_input =
			fs::read_to_string(&codex_auth_path).expect("Codex auth should be written");
		let auth =
			serde_json::from_str::<AuthDotJson>(&auth_input).expect("Codex auth should parse");
		let tokens = auth.tokens.expect("Codex auth should include tokens");

		assert_eq!(response.account.email.as_deref(), Some("copy@example.com"));
		assert_eq!(auth.email.as_deref(), Some("copy@example.com"));
		assert_eq!(tokens.account_id.as_deref(), Some("acct_123456"));
	}

	#[test]
	fn use_for_codex_rejects_auth_failed_account() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let codex_auth_path = temp_dir.path().join(".codex/auth.json");
		let store = AccountStore::new_with_codex_auth_path(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
			codex_auth_path,
		);
		let mut record = account_record(
			"copy@example.com",
			"acct_123456",
			"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
			"refresh-secret",
		);

		record.auth_failed_at_unix_epoch = Some(1_800_000_000);
		record.auth_failure = Some(String::from(
			"Codex account `copy@example.com` token refresh failed with HTTP 401 Unauthorized.",
		));

		store.save_records(&[record]).expect("records should save");

		let error = match store.use_for_codex("copy@example.com", None) {
			Ok(_) => panic!("auth failed account should reject"),
			Err(error) => error,
		};

		assert!(error.to_string().contains("auth_failed"));
	}

	#[test]
	fn list_marks_codex_active_account() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let codex_auth_path = temp_dir.path().join("auth.json");
		let store = AccountStore::new_with_codex_auth_path(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
			codex_auth_path.clone(),
		);

		store
			.save_records(&[
				account_record(
					"copy@example.com",
					"acct_123456",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret",
				),
				account_record(
					"other@example.com",
					"acct_654321",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret-2",
				),
			])
			.expect("records should save");
		store.use_for_codex("other@example.com", None).expect("account should become Codex auth");

		let response = store.list().expect("account list should load");

		assert_eq!(
			response.codex_auth.as_ref().and_then(|auth| auth.email.as_deref()),
			Some("other@example.com")
		);
		assert!(!response.accounts[0].codex_active);
		assert!(response.accounts[1].codex_active);
	}

	#[test]
	fn reroll_name_persists_global_account_name_offset() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let initial = store.list().expect("account list should load");
		let updated =
			store.reroll_name("copy@example.com", None).expect("account name should reroll");
		let reloaded = store.list().expect("account list should reload");

		assert_eq!(initial.accounts[0].random_name_offset, 0);
		assert_eq!(updated.accounts[0].random_name_offset, 1);
		assert_ne!(initial.accounts[0].random_name, updated.accounts[0].random_name);
		assert_eq!(reloaded.accounts[0].random_name, updated.accounts[0].random_name);
		assert_eq!(reloaded.accounts[0].random_name_key, updated.accounts[0].random_name_key);
		assert!(
			fs::read_to_string(&store.global_config_path)
				.expect("global config should read")
				.contains("[codex.account_names.offsets]")
		);
	}

	#[test]
	fn list_response_disambiguates_colliding_random_names() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[
				account_record(
					"first@example.com",
					"acct_000023",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret-1",
				),
				account_record(
					"second@example.com",
					"acct_000030",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret-2",
				),
			])
			.expect("records should save");

		let response = store.list().expect("account list should load");

		assert_eq!(response.accounts[0].random_name, "Reese");
		assert_eq!(response.accounts[1].random_name, "Remy");
		assert_ne!(response.accounts[0].random_name, response.accounts[1].random_name);
	}

	#[test]
	fn list_response_merges_usage_snapshot() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let mut response = store.list().expect("account list should load");

		response.apply_usage_summaries(&[CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			plan_type: Some(String::from("pro")),
			status: String::from("available"),
			refresh_status: String::from("not_needed"),
			checked_at_unix_epoch: Some(1_800_000_000),
			primary_window_seconds: Some(18_000),
			primary_remaining_percent: Some(72),
			primary_resets_at_unix_epoch: Some(1_800_018_000),
			secondary_window_seconds: Some(604_800),
			secondary_remaining_percent: Some(91),
			secondary_resets_at_unix_epoch: Some(1_800_604_800),
			credits_has_credits: Some(true),
			credits_unlimited: Some(false),
			credits_balance: Some(String::from("9.99")),
			rate_limit_reached_type: None,
			profile_lifetime_tokens: Some(47_200_000_000),
			profile_peak_daily_tokens: Some(1_500_000_000),
			profile_longest_task_seconds: Some(10_080),
			profile_current_streak_days: Some(12),
			profile_longest_streak_days: Some(68),
			profile_daily_usage: vec![CodexAccountProfileDailyUsageSummary {
				date: String::from("2026-05-31"),
				tokens: 123_456,
			}],
			..CodexAccountActivitySummary::default()
		}]);

		assert_eq!(response.accounts[0].plan_type.as_deref(), Some("pro"));
		assert_eq!(response.accounts[0].primary_window_seconds, Some(18_000));
		assert_eq!(response.accounts[0].primary_remaining_percent, Some(72));
		assert_eq!(response.accounts[0].secondary_window_seconds, Some(604_800));
		assert_eq!(response.accounts[0].secondary_remaining_percent, Some(91));
		assert_eq!(response.accounts[0].credits_balance.as_deref(), Some("9.99"));
		assert_eq!(response.accounts[0].profile_lifetime_tokens, Some(47_200_000_000));
		assert_eq!(response.accounts[0].profile_peak_daily_tokens, Some(1_500_000_000));
		assert_eq!(response.accounts[0].profile_longest_task_seconds, Some(10_080));
		assert_eq!(response.accounts[0].profile_current_streak_days, Some(12));
		assert_eq!(response.accounts[0].profile_longest_streak_days, Some(68));
		assert_eq!(response.accounts[0].profile_daily_usage[0].date, "2026-05-31");
		assert_eq!(response.accounts[0].seven_day_used_percent, Some(9));
		assert_eq!(response.accounts[0].capacity_multiplier, 20);
		assert_eq!(response.accounts[0].recovery_action, None);

		assert_close(response.accounts[0].seven_day_daily_average_percent, 9.0 / 7.0);
	}

	#[test]
	fn usage_summary_marks_refresh_401_as_login_recovery() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let mut response = store.list().expect("account list should load");

		response.apply_usage_summaries(&[CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			status: String::from("unusable"),
			refresh_status: String::from("failed"),
			note: Some(String::from(
				"usage probe failed: Codex account `copy@example.com` token refresh failed with HTTP 401 Unauthorized.",
			)),
			..CodexAccountActivitySummary::default()
		}]);

		assert_eq!(response.accounts[0].recovery_action.as_deref(), Some("login"));
	}

	#[test]
	fn usage_records_and_pool_estimate_use_seven_day_window() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[
				account_record(
					"copy@example.com",
					"acct_123456",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret",
				),
				account_record(
					"other@example.com",
					"acct_654321",
					"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh-secret-2",
				),
			])
			.expect("records should save");

		let summaries = [
			usage_summary("copy@example.com", "...123456", "pro", 40),
			usage_summary("other@example.com", "...654321", "plus", 70),
		];
		let mut response = store.list().expect("account list should load");

		response.apply_usage_summaries(&summaries);
		response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

		let estimate = response.usage_estimate.as_ref().expect("usage estimate should exist");
		let history_path = super::usage_history_path(&store.accounts_path)
			.expect("usage history path should resolve");
		let history = fs::read_to_string(history_path).expect("usage history should read");
		let record_date =
			super::usage_record_date(1_800_000_000).expect("usage record date should format");

		assert_eq!(estimate.window_days, 7);
		assert_eq!(estimate.account_count, 2);
		assert_eq!(estimate.account_estimate_count, 2);
		assert_eq!(estimate.total_capacity_percent, 2_100);
		assert_eq!(estimate.total_used_percent, 1_230);

		assert_close(Some(estimate.total_used_of_capacity_percent), 58.571);
		assert_close(Some(estimate.average_daily_used_percent), 1_230.0 / 7.0);
		assert_close(Some(estimate.average_daily_pool_percent), 58.571 / 7.0);

		assert_eq!(response.accounts[0].usage_records.len(), 1);
		assert_eq!(response.accounts[0].usage_records[0].date, record_date);
		assert_eq!(response.accounts[0].usage_records[0].used_percent, 60);
		assert_eq!(response.accounts[0].usage_records[0].capacity_multiplier, 20);
		assert_eq!(response.accounts[1].usage_records[0].capacity_multiplier, 1);
		assert_eq!(history.lines().count(), 2);
		assert!(history.contains(r#""used_percent":60"#));
		assert!(history.contains(r#""capacity_multiplier":20"#));
		assert!(history.contains(r#""used_percent":30"#));
		assert!(history.contains(r#""capacity_multiplier":1"#));
	}

	#[test]
	fn capacity_multiplier_counts_only_pro_above_plus_weight() {
		assert_eq!(super::account_capacity_multiplier(Some("pro")), 20);
		assert_eq!(super::account_capacity_multiplier(Some("plus")), 1);
		assert_eq!(super::account_capacity_multiplier(Some("team")), 1);
		assert_eq!(super::account_capacity_multiplier(None), 1);
	}

	#[test]
	fn usage_history_backfills_seven_day_estimate_when_current_windows_are_absent() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let history_path = super::usage_history_path(&store.accounts_path)
			.expect("usage history path should resolve");

		fs::create_dir_all(history_path.parent().expect("history path should have parent"))
			.expect("history dir should create");
		fs::write(
			&history_path,
			r#"{"date":"2026-05-27","account_fingerprint":"...123456","email":"copy@example.com","used_percent":22,"window_seconds":604800,"checked_at_unix_epoch":1800000000,"resets_at_unix_epoch":1800604800}
{"date":"2026-05-28","account_fingerprint":"...123456","email":"copy@example.com","used_percent":63,"window_seconds":604800,"checked_at_unix_epoch":1800000100,"resets_at_unix_epoch":1800604900}
"#,
		)
		.expect("usage history should write");

		let mut response = store.list().expect("account list should load");

		response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

		let estimate = response.usage_estimate.as_ref().expect("usage estimate should exist");

		assert_eq!(response.accounts[0].primary_remaining_percent, None);
		assert_eq!(response.accounts[0].seven_day_used_percent, Some(63));

		assert_close(response.accounts[0].seven_day_daily_average_percent, 63.0 / 7.0);

		assert_eq!(response.accounts[0].usage_records.len(), 2);
		assert_eq!(estimate.account_estimate_count, 1);
		assert_eq!(estimate.total_used_percent, 63);
	}

	#[test]
	fn usage_history_preserves_last_good_windows_across_placeholder_refresh() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);
		let now = time::OffsetDateTime::now_utc().unix_timestamp();

		store
			.save_records(&[account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			)])
			.expect("records should save");

		let good_summary = CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			plan_type: Some(String::from("pro")),
			status: String::from("available"),
			refresh_status: String::from("not_needed"),
			checked_at_unix_epoch: Some(now),
			primary_window_seconds: Some(18_000),
			primary_remaining_percent: Some(72),
			primary_resets_at_unix_epoch: Some(now + 18_000),
			secondary_window_seconds: Some(604_800),
			secondary_remaining_percent: Some(91),
			secondary_resets_at_unix_epoch: Some(now + 604_800),
			..CodexAccountActivitySummary::default()
		};
		let mut response = store.list().expect("account list should load");

		response.apply_usage_summaries(&[good_summary]);
		response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

		let degraded_summary = CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			plan_type: Some(String::from("pro")),
			status: String::from("available"),
			refresh_status: String::from("not_needed"),
			checked_at_unix_epoch: Some(now + 60),
			profile_lifetime_tokens: Some(47_200_000_000),
			..CodexAccountActivitySummary::default()
		};
		let mut degraded_response = store.list().expect("account list should reload");

		degraded_response.apply_usage_summaries(&[degraded_summary]);
		degraded_response
			.refresh_usage_records(&store.accounts_path)
			.expect("usage history should restore usable windows");

		let account = &degraded_response.accounts[0];

		assert_eq!(account.primary_window_seconds, Some(18_000));
		assert_eq!(account.primary_remaining_percent, Some(72));
		assert_eq!(account.primary_resets_at_unix_epoch, Some(now + 18_000));
		assert_eq!(account.secondary_window_seconds, Some(604_800));
		assert_eq!(account.secondary_remaining_percent, Some(91));
		assert_eq!(account.secondary_resets_at_unix_epoch, Some(now + 604_800));
		assert_eq!(account.seven_day_used_percent, Some(9));
		assert_eq!(account.profile_lifetime_tokens, Some(47_200_000_000));
	}

	fn account_record(
		email: &str,
		account_id: &str,
		access_token: &str,
		refresh_token: &str,
	) -> AccountPoolRecord {
		AccountPoolRecord {
			email: Some(String::from(email)),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_failed_at_unix_epoch: None,
			auth_failure: None,
			auth_mode: None,
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from(access_token),
				refresh_token: String::from(refresh_token),
				account_id: Some(String::from(account_id)),
			}),
			last_refresh: None,
		}
	}

	fn usage_summary(
		email: &str,
		account_fingerprint: &str,
		plan_type: &str,
		secondary_remaining_percent: i64,
	) -> CodexAccountActivitySummary {
		CodexAccountActivitySummary {
			account_fingerprint: String::from(account_fingerprint),
			email: Some(String::from(email)),
			plan_type: Some(String::from(plan_type)),
			status: String::from("available"),
			refresh_status: String::from("not_needed"),
			checked_at_unix_epoch: Some(1_800_000_000),
			secondary_window_seconds: Some(604_800),
			secondary_remaining_percent: Some(secondary_remaining_percent),
			secondary_resets_at_unix_epoch: Some(1_800_604_800),
			..CodexAccountActivitySummary::default()
		}
	}

	fn assert_close(value: Option<f64>, expected: f64) {
		let value = value.expect("value should exist");

		assert!(
			(value - expected).abs() < 0.001,
			"expected {value} to be within 0.001 of {expected}"
		);
	}
}
