#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	io::{self, ErrorKind, Read, Write as _},
	path::{Path, PathBuf},
	process::{self, Child, Command, ExitStatus, Stdio},
	sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
	thread::{self, JoinHandle},
	time::Duration,
};

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent::CodexAccountPool,
	prelude::{Result, eyre},
	runtime,
	state::CodexAccountActivitySummary,
};

const USAGE_ESTIMATE_WINDOW_DAYS: i64 = 7;
const USAGE_ESTIMATE_WINDOW_SECONDS: i64 = USAGE_ESTIMATE_WINDOW_DAYS * 24 * 60 * 60;
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

pub(crate) struct AccountStore {
	accounts_path: PathBuf,
	global_config_path: PathBuf,
	codex_auth_path: PathBuf,
}
impl AccountStore {
	pub(crate) fn global() -> Result<Self> {
		Ok(Self {
			accounts_path: runtime::accounts_path()?,
			global_config_path: runtime::global_config_path()?,
			codex_auth_path: default_codex_auth_json_path()?,
		})
	}

	#[cfg(test)]
	fn new(accounts_path: PathBuf, global_config_path: PathBuf) -> Self {
		let codex_auth_path = accounts_path
			.parent()
			.map(|parent| parent.join("auth.json"))
			.unwrap_or_else(|| PathBuf::from("auth.json"));

		Self { accounts_path, global_config_path, codex_auth_path }
	}

	#[cfg(test)]
	fn new_with_codex_auth_path(
		accounts_path: PathBuf,
		global_config_path: PathBuf,
		codex_auth_path: PathBuf,
	) -> Self {
		Self { accounts_path, global_config_path, codex_auth_path }
	}

	fn list(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.response_from_records(&records)
	}

	fn list_with_cached_usage(&self, force_refresh: bool) -> Result<AccountListResponse> {
		let mut response = self.list()?;

		response.hydrate_usage_from_path(&self.accounts_path, force_refresh);

		Ok(response)
	}

	fn select(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;

		if !records.iter().any(|record| record.matches_account_selector(selector)) {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.write_fixed_account_selector(Some(selector))?;

		self.response_from_records(&records)
	}

	fn clear_selection(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.write_fixed_account_selector(None)?;

		self.response_from_records(&records)
	}

	fn logout(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let mut records = self.load_records()?;
		let selector_matched_fixed =
			self.fixed_account_selector()?.as_deref().is_some_and(|fixed| {
				fixed == selector
					|| records.iter().any(|record| {
						record.matches_account_selector(selector)
							&& record.matches_account_selector(fixed)
					})
			});
		let original_len = records.len();

		records.retain(|record| !record.matches_account_selector(selector));

		if records.len() == original_len {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.save_records(&records)?;

		if selector_matched_fixed {
			self.write_fixed_account_selector(None)?;
		}

		self.response_from_records(&records)
	}

	fn reroll_name(&self, selector: &str, offset: Option<i64>) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;
		let record = records
			.iter()
			.find(|record| record.matches_account_selector(selector))
			.ok_or_else(|| eyre::eyre!("No Decodex account matches selector `{selector}`."))?;
		let key = record.random_name_key();
		let offsets = self.account_name_offsets()?;
		let current = offsets.get(&key).copied().unwrap_or_default();
		let next = offset.map_or_else(
			|| normalize_random_name_offset(current + 1),
			normalize_random_name_offset,
		);

		self.write_account_name_offset(&key, next)?;

		self.response_from_records(&records)
	}

	fn import_auth_json(&self, auth_json_path: &Path) -> Result<AccountListResponse> {
		let input = fs::read_to_string(auth_json_path).map_err(|error| {
			eyre::eyre!("Failed to read Codex auth JSON `{}`: {error}", auth_json_path.display())
		})?;
		let auth = serde_json::from_str::<AuthDotJson>(&input).map_err(|error| {
			eyre::eyre!("Codex auth JSON `{}` is invalid: {error}", auth_json_path.display())
		})?;
		let mut record = AccountPoolRecord::from_auth(auth)?;
		let mut records = self.load_records()?;

		if record.last_refresh.is_none() {
			record.last_refresh = Some(now_rfc3339()?);
		}

		let replace_index = records.iter().position(|candidate| {
			record.account_id().is_some() && candidate.account_id() == record.account_id()
				|| record.email().is_some() && candidate.email() == record.email()
		});

		if let Some(index) = replace_index {
			records[index] = record;
		} else {
			records.push(record);
		}

		self.save_records(&records)?;

		self.response_from_records(&records)
	}

	fn use_for_codex(
		&self,
		selector: &str,
		auth_json_path: Option<&Path>,
	) -> Result<AccountUseResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;
		let record = records
			.iter()
			.find(|record| record.matches_account_selector(selector))
			.ok_or_else(|| eyre::eyre!("No Decodex account matches selector `{selector}`."))?;

		if record.disabled {
			eyre::bail!("Decodex account `{selector}` is disabled and cannot be used by Codex.");
		}

		record.validate_importable()?;

		let target_path = auth_json_path.unwrap_or(&self.codex_auth_path);

		write_auth_json_atomically(target_path, &record.auth_dot_json()?)?;

		Ok(AccountUseResponse {
			codex_auth_path: target_path.display().to_string(),
			account: record.identity_summary(),
		})
	}

	fn load_records(&self) -> Result<Vec<AccountPoolRecord>> {
		let input = match fs::read_to_string(&self.accounts_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex accounts `{}`: {error}",
					self.accounts_path.display()
				);
			},
		};

		parse_account_records(&input, &self.accounts_path)
	}

	fn save_records(&self, records: &[AccountPoolRecord]) -> Result<()> {
		let parent = self.accounts_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex accounts path `{}` must have a parent directory.",
				self.accounts_path.display()
			)
		})?;
		let file_name =
			self.accounts_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
				eyre::eyre!("Decodex accounts path must end in a valid file name.")
			})?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, body)?;

		secure_account_file(&temp_path)?;

		fs::rename(temp_path, &self.accounts_path)?;

		secure_account_file(&self.accounts_path)?;

		Ok(())
	}

	fn response_from_records(&self, records: &[AccountPoolRecord]) -> Result<AccountListResponse> {
		let selector = self.fixed_account_selector()?;
		let codex_auth = self.codex_auth_identity().unwrap_or_default();
		let name_offsets = self.account_name_offsets()?;
		let control = AccountControlSummary {
			mode: if selector.is_some() { String::from("fixed") } else { String::from("balanced") },
			account_selector: selector.clone(),
		};
		let mut accounts = records
			.iter()
			.map(|record| record.summary(selector.as_deref(), codex_auth.as_ref(), &name_offsets))
			.collect::<Vec<_>>();

		assign_unique_random_names(&mut accounts);

		Ok(AccountListResponse {
			accounts_path: self.accounts_path.display().to_string(),
			global_config_path: self.global_config_path.display().to_string(),
			codex_auth_path: self.codex_auth_path.display().to_string(),
			codex_auth: codex_auth.as_ref().map(AccountIdentity::summary),
			control,
			accounts,
			usage_estimate: None,
			usage_probe_error: None,
		})
	}

	fn codex_auth_identity(&self) -> Result<Option<AccountIdentity>> {
		let input = match fs::read_to_string(&self.codex_auth_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => {
				eyre::bail!(
					"Failed to read Codex auth JSON `{}`: {error}",
					self.codex_auth_path.display()
				);
			},
		};
		let auth = serde_json::from_str::<AuthDotJson>(&input).map_err(|error| {
			eyre::eyre!("Codex auth JSON `{}` is invalid: {error}", self.codex_auth_path.display())
		})?;
		let record = AccountPoolRecord::from_auth(auth)?;

		Ok(Some(record.identity()))
	}

	fn fixed_account_selector(&self) -> Result<Option<String>> {
		let document = self.load_global_config_document()?;
		let selector = document
			.get("codex")
			.and_then(toml::Value::as_table)
			.and_then(|codex| codex.get("accounts"))
			.and_then(toml::Value::as_table)
			.and_then(|accounts| accounts.get("fixed_account"))
			.and_then(toml::Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.map(str::to_owned);

		Ok(selector)
	}

	fn account_name_offsets(&self) -> Result<BTreeMap<String, i64>> {
		let document = self.load_global_config_document()?;
		let Some(offsets) = document
			.get("codex")
			.and_then(toml::Value::as_table)
			.and_then(|codex| codex.get("account_names"))
			.and_then(toml::Value::as_table)
			.and_then(|account_names| account_names.get("offsets"))
			.and_then(toml::Value::as_table)
		else {
			return Ok(BTreeMap::new());
		};

		Ok(offsets
			.iter()
			.filter_map(|(key, value)| {
				let key = key.trim();

				(!key.is_empty()).then_some((
					key.to_owned(),
					normalize_random_name_offset(value.as_integer().unwrap_or_default()),
				))
			})
			.collect())
	}

	fn write_account_name_offset(&self, key: &str, offset: i64) -> Result<()> {
		let key = key.trim();

		if key.is_empty() {
			eyre::bail!("Codex account name key cannot be empty.");
		}

		let mut document = self.load_global_config_document()?;
		let offsets = ensure_toml_table(
			ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "account_names")?,
			"offsets",
		)?;

		offsets.insert(key.to_owned(), toml::Value::Integer(normalize_random_name_offset(offset)));

		self.write_global_config_document(&document)
	}

	fn write_fixed_account_selector(&self, selector: Option<&str>) -> Result<()> {
		let mut document = self.load_global_config_document()?;

		match selector.map(str::trim).filter(|value| !value.is_empty()) {
			Some(selector) => {
				let accounts =
					ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "accounts")?;

				accounts.insert(String::from("fixed_account"), selector.to_owned().into());
			},
			None => {
				if let Some(codex) = document.get_mut("codex").and_then(toml::Value::as_table_mut)
					&& let Some(accounts) =
						codex.get_mut("accounts").and_then(toml::Value::as_table_mut)
				{
					accounts.remove("fixed_account");
				}
			},
		}

		self.write_global_config_document(&document)
	}

	fn load_global_config_document(&self) -> Result<toml::Table> {
		let input = match fs::read_to_string(&self.global_config_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(toml::Table::new()),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex global config `{}`: {error}",
					self.global_config_path.display()
				);
			},
		};

		if input.trim().is_empty() { Ok(toml::Table::new()) } else { Ok(toml::from_str(&input)?) }
	}

	fn write_global_config_document(&self, document: &toml::Table) -> Result<()> {
		let parent = self.global_config_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex global config `{}` must have a parent directory.",
				self.global_config_path.display()
			)
		})?;
		let file_name =
			self.global_config_path.file_name().and_then(|name| name.to_str()).ok_or_else(
				|| eyre::eyre!("Decodex global config path must end in a valid file name."),
			)?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let output = toml::to_string_pretty(&document)?;

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, output)?;

		secure_account_file(&temp_path)?;

		fs::rename(temp_path, &self.global_config_path)?;

		secure_account_file(&self.global_config_path)?;

		Ok(())
	}
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
impl AccountListResponse {
	fn hydrate_usage_from_path(&mut self, accounts_path: &Path, force_refresh: bool) {
		if self.accounts.is_empty() {
			return;
		}

		match CodexAccountPool::from_accounts_path(accounts_path)
			.and_then(|pool| pool.account_activity_summaries_cached(force_refresh))
		{
			Ok(summaries) => {
				self.apply_usage_summaries(&summaries);

				if let Err(error) = self.refresh_usage_records(accounts_path) {
					self.usage_probe_error = Some(error.to_string());
				}
			},
			Err(error) => self.usage_probe_error = Some(error.to_string()),
		}
	}

	fn apply_usage_summaries(&mut self, summaries: &[CodexAccountActivitySummary]) {
		for account in &mut self.accounts {
			if let Some(summary) = matching_usage_summary(account, summaries) {
				account.apply_usage_summary(summary);
			}
		}

		self.refresh_usage_estimate();
	}

	fn refresh_usage_records(&mut self, accounts_path: &Path) -> Result<()> {
		let history_path = usage_history_path(accounts_path)?;
		let mut history = AccountUsageHistory::read(&history_path)?;

		history
			.merge_current_records(self.accounts.iter().filter_map(AccountSummary::usage_record));
		history.write(&history_path)?;
		history.apply_to_accounts(&mut self.accounts);
		self.refresh_usage_estimate();

		Ok(())
	}

	fn refresh_usage_estimate(&mut self) {
		let account_count = self.accounts.len();
		let account_estimate_count =
			self.accounts.iter().filter(|account| account.seven_day_used_percent.is_some()).count();
		let total_used_percent =
			self.accounts.iter().filter_map(|account| account.seven_day_used_percent).sum::<i64>();

		self.usage_estimate = AccountUsageEstimateSummary::new(
			account_count,
			account_estimate_count,
			total_used_percent,
		);
	}
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

#[derive(Clone, Serialize)]
pub(crate) struct AccountUsageEstimateSummary {
	pub(crate) window_days: i64,
	pub(crate) account_count: usize,
	pub(crate) account_estimate_count: usize,
	pub(crate) total_capacity_percent: i64,
	pub(crate) total_used_percent: i64,
	pub(crate) total_used_of_capacity_percent: f64,
	pub(crate) average_daily_used_percent: f64,
	pub(crate) average_daily_pool_percent: f64,
}
impl AccountUsageEstimateSummary {
	fn new(
		account_count: usize,
		account_estimate_count: usize,
		total_used_percent: i64,
	) -> Option<Self> {
		if account_count == 0 || account_estimate_count == 0 {
			return None;
		}

		let total_capacity_percent =
			i64::try_from(account_count).unwrap_or(i64::MAX / 100).saturating_mul(100);
		let total_used_of_capacity_percent =
			percent_ratio(total_used_percent, total_capacity_percent);

		Some(Self {
			window_days: USAGE_ESTIMATE_WINDOW_DAYS,
			account_count,
			account_estimate_count,
			total_capacity_percent,
			total_used_percent,
			total_used_of_capacity_percent,
			average_daily_used_percent: total_used_percent as f64
				/ USAGE_ESTIMATE_WINDOW_DAYS as f64,
			average_daily_pool_percent: total_used_of_capacity_percent
				/ USAGE_ESTIMATE_WINDOW_DAYS as f64,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AccountUsageDailySummary {
	pub(crate) date: String,
	pub(crate) used_percent: i64,
	pub(crate) checked_at_unix_epoch: i64,
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
	pub(crate) seven_day_used_percent: Option<i64>,
	pub(crate) seven_day_daily_average_percent: Option<f64>,
	pub(crate) usage_records: Vec<AccountUsageDailySummary>,
}
impl AccountSummary {
	fn apply_usage_summary(&mut self, summary: &CodexAccountActivitySummary) {
		self.status = summary.status.clone();
		self.plan_type = summary.plan_type.clone();
		self.refresh_status = Some(summary.refresh_status.clone());
		self.checked_at_unix_epoch = summary.checked_at_unix_epoch;
		self.primary_window_seconds = summary.primary_window_seconds;
		self.primary_remaining_percent = summary.primary_remaining_percent;
		self.primary_resets_at_unix_epoch = summary.primary_resets_at_unix_epoch;
		self.secondary_window_seconds = summary.secondary_window_seconds;
		self.secondary_remaining_percent = summary.secondary_remaining_percent;
		self.secondary_resets_at_unix_epoch = summary.secondary_resets_at_unix_epoch;
		self.credits_has_credits = summary.credits_has_credits;
		self.credits_unlimited = summary.credits_unlimited;

		self.credits_balance.clone_from(&summary.credits_balance);
		self.rate_limit_reached_type.clone_from(&summary.rate_limit_reached_type);

		if summary.cooldown_until_unix_epoch.is_some() {
			self.cooldown_until_unix_epoch = summary.cooldown_until_unix_epoch;
		}

		self.note.clone_from(&summary.note);
		self.apply_usage_estimate();
	}

	fn apply_usage_estimate(&mut self) {
		let Some(basis) = SevenDayUsageBasis::from_account(self) else {
			self.seven_day_used_percent = None;
			self.seven_day_daily_average_percent = None;

			return;
		};

		self.seven_day_used_percent = Some(basis.used_percent);
		self.seven_day_daily_average_percent =
			Some(basis.used_percent as f64 / USAGE_ESTIMATE_WINDOW_DAYS as f64);
	}

	fn usage_record(&self) -> Option<AccountUsageHistoryRecord> {
		let basis = SevenDayUsageBasis::from_account(self)?;
		let checked_at_unix_epoch = self.checked_at_unix_epoch?;

		Some(AccountUsageHistoryRecord {
			date: usage_record_date(checked_at_unix_epoch)?,
			account_fingerprint: self.account_fingerprint.clone(),
			email: self.email.clone(),
			used_percent: basis.used_percent,
			window_seconds: basis.window_seconds,
			checked_at_unix_epoch,
			resets_at_unix_epoch: basis.resets_at_unix_epoch,
		})
	}
}

#[derive(Clone, Copy)]
struct SevenDayUsageBasis {
	used_percent: i64,
	window_seconds: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
}
impl SevenDayUsageBasis {
	fn from_account(account: &AccountSummary) -> Option<Self> {
		let secondary = Self::from_window(
			account.secondary_remaining_percent,
			account.secondary_window_seconds,
			account.secondary_resets_at_unix_epoch,
		);

		if let Some(basis) = secondary
			&& accepts_secondary_usage_window(basis.window_seconds)
		{
			return Some(basis);
		}

		Self::from_window(
			account.primary_remaining_percent,
			account.primary_window_seconds,
			account.primary_resets_at_unix_epoch,
		)
		.filter(|basis| basis.window_seconds.is_some_and(is_seven_day_usage_window))
	}

	fn from_window(
		remaining_percent: Option<i64>,
		window_seconds: Option<i64>,
		resets_at_unix_epoch: Option<i64>,
	) -> Option<Self> {
		Some(Self {
			used_percent: used_percent_from_remaining(remaining_percent?),
			window_seconds,
			resets_at_unix_epoch,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct AccountUsageHistoryRecord {
	date: String,
	account_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	used_percent: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	window_seconds: Option<i64>,
	checked_at_unix_epoch: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	resets_at_unix_epoch: Option<i64>,
}
impl AccountUsageHistoryRecord {
	fn daily_summary(&self) -> AccountUsageDailySummary {
		AccountUsageDailySummary {
			date: self.date.clone(),
			used_percent: self.used_percent,
			checked_at_unix_epoch: self.checked_at_unix_epoch,
		}
	}

	fn is_recent(&self, now_unix_epoch: i64) -> bool {
		now_unix_epoch.saturating_sub(self.checked_at_unix_epoch) <= USAGE_ESTIMATE_WINDOW_SECONDS
	}

	fn matches_account(&self, account: &AccountSummary) -> bool {
		self.account_fingerprint == account.account_fingerprint
			|| self
				.email
				.as_deref()
				.zip(account.email.as_deref())
				.is_some_and(|(left, right)| left == right)
	}

	fn same_daily_slot(&self, other: &Self) -> bool {
		self.date == other.date
			&& (self.account_fingerprint == other.account_fingerprint
				|| self
					.email
					.as_deref()
					.zip(other.email.as_deref())
					.is_some_and(|(left, right)| left == right))
	}
}

#[derive(Default)]
struct AccountUsageHistory {
	records: Vec<AccountUsageHistoryRecord>,
}
impl AccountUsageHistory {
	fn read(path: &Path) -> Result<Self> {
		let input = match fs::read_to_string(path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
			Err(error) => {
				eyre::bail!("Failed to read account usage history `{}`: {error}", path.display());
			},
		};

		Ok(Self { records: parse_usage_history_records(&input, path)? })
	}

	fn merge_current_records(
		&mut self,
		current_records: impl Iterator<Item = AccountUsageHistoryRecord>,
	) {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let current_records = current_records.collect::<Vec<_>>();

		self.records.retain(|record| {
			record.is_recent(now)
				&& !current_records.iter().any(|current| current.same_daily_slot(record))
		});
		self.records.extend(current_records);
		self.records.sort_by(|left, right| {
			left.date
				.cmp(&right.date)
				.then_with(|| left.account_fingerprint.cmp(&right.account_fingerprint))
		});
	}

	fn write(&self, path: &Path) -> Result<()> {
		let parent = path.parent().ok_or_else(|| {
			eyre::eyre!("Account usage history path `{}` must have a parent.", path.display())
		})?;
		let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
			eyre::eyre!("Account usage history path must end in a valid file name.")
		})?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in &self.records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, body)?;

		secure_account_file(&temp_path)?;

		fs::rename(temp_path, path)?;

		secure_account_file(path)?;

		Ok(())
	}

	fn apply_to_accounts(&self, accounts: &mut [AccountSummary]) {
		for account in accounts {
			let matching_records = self
				.records
				.iter()
				.filter(|record| record.matches_account(account))
				.collect::<Vec<_>>();

			if account.seven_day_used_percent.is_none()
				&& let Some(latest) =
					matching_records.iter().max_by_key(|record| record.checked_at_unix_epoch)
			{
				account.seven_day_used_percent = Some(latest.used_percent);
				account.seven_day_daily_average_percent =
					Some(latest.used_percent as f64 / USAGE_ESTIMATE_WINDOW_DAYS as f64);
			}

			account.usage_records =
				matching_records.iter().map(|record| record.daily_summary()).collect();
		}
	}
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
struct AuthDotJson {
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_refresh: Option<String>,
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
			note: Some(String::from("local account pool")),
			plan_type: None,
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
struct CodexTokenData {
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	id_token: Option<String>,
	access_token: String,
	refresh_token: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	account_id: Option<String>,
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
				auth,
			} => {
				let mut record = AccountPoolRecord::from_auth(auth)?;

				record.email = first_nonblank_string(email, record.email);
				record.disabled = disabled;
				record.cooldown_until_unix_epoch = cooldown_until_unix_epoch;
				record.cooldown_until = cooldown_until;
				record.last_selected_at_unix_epoch = last_selected_at_unix_epoch;

				Ok(record)
			},
		}
	}
}

enum LoginPipeEvent {
	Chunk(Vec<u8>),
	ReaderFailed(String),
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

pub(crate) fn run_account_login(request: &AccountLoginRequest) -> Result<()> {
	let response = account_login(request, |chunk| {
		print!("{chunk}");

		io::stdout().flush()?;

		Ok(())
	})?;

	print_list_response(&response, false)
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

pub(crate) fn account_login(
	request: &AccountLoginRequest,
	on_output: impl FnMut(&str) -> Result<()>,
) -> Result<AccountListResponse> {
	let temp_home = create_login_home()?;
	let status = run_codex_device_login(&request.codex_bin, &temp_home, on_output)?;

	if !status.success() {
		cleanup_login_home(&temp_home, request.keep_temp_home);

		eyre::bail!("Codex account login failed with status {status}.");
	}

	let auth_json_path = temp_home.join("auth.json");
	let store = AccountStore::global()?;
	let import_result = store.import_auth_json(&auth_json_path);

	cleanup_login_home(&temp_home, request.keep_temp_home);

	import_result
}

fn matching_usage_summary<'a>(
	account: &AccountSummary,
	summaries: &'a [CodexAccountActivitySummary],
) -> Option<&'a CodexAccountActivitySummary> {
	summaries.iter().find(|summary| {
		account
			.email
			.as_deref()
			.zip(summary.email.as_deref())
			.is_some_and(|(account_email, summary_email)| account_email == summary_email)
			|| account.account_fingerprint == summary.account_fingerprint
	})
}

fn run_codex_device_login(
	codex_bin: &str,
	temp_home: &Path,
	on_output: impl FnMut(&str) -> Result<()>,
) -> Result<ExitStatus> {
	let mut child = Command::new(codex_bin)
		.arg("login")
		.arg("--device-auth")
		.env("CODEX_HOME", temp_home)
		.env("CODEX_SQLITE_HOME", temp_home)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|error| {
			eyre::eyre!("Failed to start `{codex_bin}` for Codex account login: {error}")
		})?;
	let stdout =
		child.stdout.take().ok_or_else(|| eyre::eyre!("Failed to capture Codex login stdout."))?;
	let stderr =
		child.stderr.take().ok_or_else(|| eyre::eyre!("Failed to capture Codex login stderr."))?;
	let (sender, receiver) = mpsc::channel();
	let stdout_reader = spawn_login_pipe_reader(stdout, sender.clone());
	let stderr_reader = spawn_login_pipe_reader(stderr, sender);
	let status = wait_for_login_child(child, receiver, on_output)?;

	join_login_pipe_reader(stdout_reader)?;
	join_login_pipe_reader(stderr_reader)?;

	Ok(status)
}

fn spawn_login_pipe_reader(
	mut reader: impl Read + Send + 'static,
	sender: Sender<LoginPipeEvent>,
) -> JoinHandle<()> {
	thread::spawn(move || {
		let mut buffer = [0_u8; 4_096];

		loop {
			match reader.read(&mut buffer) {
				Ok(0) => return,
				Ok(len) =>
					if sender.send(LoginPipeEvent::Chunk(buffer[..len].to_vec())).is_err() {
						return;
					},
				Err(error) => {
					let _ = sender.send(LoginPipeEvent::ReaderFailed(error.to_string()));

					return;
				},
			}
		}
	})
}

fn wait_for_login_child(
	mut child: Child,
	receiver: Receiver<LoginPipeEvent>,
	mut on_output: impl FnMut(&str) -> Result<()>,
) -> Result<ExitStatus> {
	let mut reader_error = None;

	loop {
		while let Ok(event) = receiver.try_recv() {
			handle_login_pipe_event(event, &mut on_output, &mut reader_error)?;
		}

		if let Some(status) = child.try_wait()? {
			while let Ok(event) = receiver.try_recv() {
				handle_login_pipe_event(event, &mut on_output, &mut reader_error)?;
			}

			if let Some(error) = reader_error {
				eyre::bail!("Failed while reading Codex login output: {error}");
			}

			return Ok(status);
		}

		match receiver.recv_timeout(Duration::from_millis(50)) {
			Ok(event) => handle_login_pipe_event(event, &mut on_output, &mut reader_error)?,
			Err(RecvTimeoutError::Timeout) => {},
			Err(RecvTimeoutError::Disconnected) => {
				let status = child.wait()?;

				if let Some(error) = reader_error {
					eyre::bail!("Failed while reading Codex login output: {error}");
				}

				return Ok(status);
			},
		}
	}
}

fn handle_login_pipe_event(
	event: LoginPipeEvent,
	on_output: &mut impl FnMut(&str) -> Result<()>,
	reader_error: &mut Option<String>,
) -> Result<()> {
	match event {
		LoginPipeEvent::Chunk(chunk) => on_output(&String::from_utf8_lossy(&chunk))?,
		LoginPipeEvent::ReaderFailed(error) => *reader_error = Some(error),
	}

	Ok(())
}

fn join_login_pipe_reader(handle: JoinHandle<()>) -> Result<()> {
	handle.join().map_err(|_| eyre::eyre!("Codex login output reader panicked."))
}

fn parse_account_records(input: &str, path: &Path) -> Result<Vec<AccountPoolRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let parsed = serde_json::from_str::<AccountPoolLine>(trimmed).map_err(|error| {
			eyre::eyre!(
				"Decodex accounts `{}` line {line_number} is not a valid auth JSONL entry: {error}",
				path.display()
			)
		})?;

		records.push(parsed.into_record()?);
	}

	Ok(records)
}

fn parse_usage_history_records(input: &str, path: &Path) -> Result<Vec<AccountUsageHistoryRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let record =
			serde_json::from_str::<AccountUsageHistoryRecord>(trimmed).map_err(|error| {
				eyre::eyre!(
					"Decodex account usage history `{}` line {line_number} is invalid: {error}",
					path.display()
				)
			})?;

		records.push(record);
	}

	Ok(records)
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

fn create_login_home() -> Result<PathBuf> {
	let root = env::temp_dir().join(format!(
		"decodex-codex-login-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp()
	));

	fs::create_dir_all(&root)?;

	secure_account_file(&root)?;

	Ok(root)
}

fn default_codex_auth_json_path() -> Result<PathBuf> {
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

fn write_auth_json_atomically(path: &Path, auth: &AuthDotJson) -> Result<()> {
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

fn cleanup_login_home(path: &Path, keep: bool) {
	if keep {
		eprintln!("temporary Codex login home preserved at {}", path.display());

		return;
	}

	if let Err(error) = fs::remove_dir_all(path) {
		eprintln!(
			"warning: failed to remove temporary Codex login home `{}`: {error}",
			path.display()
		);
	}
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

fn ensure_toml_table<'a>(parent: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
	if !parent.contains_key(key) {
		parent.insert(String::from(key), toml::Value::Table(toml::Table::new()));
	}

	parent
		.get_mut(key)
		.and_then(toml::Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("`{key}` in Decodex global config must be a table."))
}

fn first_nonblank_string(left: Option<String>, right: Option<String>) -> Option<String> {
	left.filter(|value| !value.trim().is_empty())
		.or_else(|| right.filter(|value| !value.trim().is_empty()))
}

fn nonblank_string(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn jwt_email_claim(id_token: Option<&str>) -> Option<String> {
	let payload = id_token?.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok()?;

	claims.get("email").and_then(json_scalar_to_string)
}

fn jwt_expiration_unix_epoch(jwt: &str) -> Option<i64> {
	let payload = jwt.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok()?;

	claims.get("exp").and_then(number_as_i64)
}

fn parse_base64_url(input: &str) -> Option<Vec<u8>> {
	let mut output = Vec::with_capacity(input.len() * 3 / 4);
	let mut accumulator = 0_u32;
	let mut bits = 0_u32;

	for byte in input.bytes().take_while(|byte| *byte != b'=') {
		accumulator = (accumulator << 6) | u32::from(base64_url_value(byte)?);
		bits += 6;

		if bits >= 8 {
			bits -= 8;

			output.push(((accumulator >> bits) & 0xff) as u8);
		}
	}

	Some(output)
}

const fn base64_url_value(byte: u8) -> Option<u8> {
	match byte {
		b'A'..=b'Z' => Some(byte - b'A'),
		b'a'..=b'z' => Some(byte - b'a' + 26),
		b'0'..=b'9' => Some(byte - b'0' + 52),
		b'-' => Some(62),
		b'_' => Some(63),
		_ => None,
	}
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
	match value {
		serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
		serde_json::Value::Number(number) => Some(number.to_string()),
		serde_json::Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

fn number_as_i64(value: &serde_json::Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn usage_history_path(accounts_path: &Path) -> Result<PathBuf> {
	let parent = accounts_path.parent().ok_or_else(|| {
		eyre::eyre!(
			"Decodex accounts path `{}` must have a parent directory.",
			accounts_path.display()
		)
	})?;

	Ok(parent.join("account-usage-history.jsonl"))
}

fn usage_record_date(unix_epoch: i64) -> Option<String> {
	OffsetDateTime::from_unix_timestamp(unix_epoch)
		.ok()
		.map(|timestamp| timestamp.date().to_string())
}

fn accepts_secondary_usage_window(window_seconds: Option<i64>) -> bool {
	window_seconds.is_none_or(is_seven_day_usage_window)
}

fn is_seven_day_usage_window(window_seconds: i64) -> bool {
	window_seconds
		.checked_sub(USAGE_ESTIMATE_WINDOW_SECONDS)
		.is_some_and(|delta| delta.abs() <= 3_600)
}

fn used_percent_from_remaining(remaining_percent: i64) -> i64 {
	100_i64.saturating_sub(remaining_percent).clamp(0, 100)
}

fn percent_ratio(numerator: i64, denominator: i64) -> f64 {
	if denominator <= 0 {
		return 0.0;
	}

	(numerator as f64 / denominator as f64) * 100.0
}

fn random_name_seed_for(account_fingerprint: &str, email: Option<String>) -> String {
	if !account_fingerprint.trim().is_empty() {
		return account_fingerprint.to_owned();
	}

	if let Some(email) = email.filter(|value| !value.trim().is_empty()) {
		return email;
	}

	String::from("account")
}

fn random_name_key(seed: &str) -> String {
	format!("{:08x}", account_identity_hash(seed))
}

fn random_name(seed: &str, offset: i64) -> String {
	let index = (u64::from(account_identity_hash(seed))
		+ u64::try_from(normalize_random_name_offset(offset)).unwrap_or_default())
		% u64::try_from(ACCOUNT_RANDOM_NAMES.len()).unwrap_or(1);

	ACCOUNT_RANDOM_NAMES[usize::try_from(index).unwrap_or_default()].to_owned()
}

fn assign_unique_random_names(accounts: &mut [AccountSummary]) {
	if accounts.len() < 2 {
		return;
	}

	let mut account_indexes = (0..accounts.len()).collect::<Vec<_>>();

	account_indexes.sort_by(|left, right| {
		accounts[*left]
			.random_name_key
			.cmp(&accounts[*right].random_name_key)
			.then_with(|| accounts[*left].selector.cmp(&accounts[*right].selector))
	});

	let mut used_names = BTreeSet::new();

	for index in account_indexes {
		let preferred_index = random_name_index(&accounts[index].random_name).unwrap_or_default();
		let name = unique_random_name_from(preferred_index, &used_names);

		used_names.insert(name.clone());

		accounts[index].random_name = name;
	}
}

fn random_name_index(name: &str) -> Option<usize> {
	ACCOUNT_RANDOM_NAMES.iter().position(|candidate| *candidate == name)
}

fn unique_random_name_from(start_index: usize, used_names: &BTreeSet<String>) -> String {
	for probe in 0..ACCOUNT_RANDOM_NAMES.len() {
		let name = ACCOUNT_RANDOM_NAMES[(start_index + probe) % ACCOUNT_RANDOM_NAMES.len()];

		if !used_names.contains(name) {
			return name.to_owned();
		}
	}

	let base_name = ACCOUNT_RANDOM_NAMES[start_index % ACCOUNT_RANDOM_NAMES.len()];
	let mut suffix = 2;

	loop {
		let name = format!("{base_name} {suffix}");

		if !used_names.contains(&name) {
			return name;
		}

		suffix += 1;
	}
}

fn account_identity_hash(value: &str) -> u32 {
	let text = if value.trim().is_empty() { "account" } else { value };
	let mut hash = 2_166_136_261_u32;

	for unit in text.encode_utf16() {
		hash ^= u32::from(unit);
		hash = hash.wrapping_mul(16_777_619);
	}

	hash
}

fn normalize_random_name_offset(offset: i64) -> i64 {
	offset.rem_euclid(i64::try_from(ACCOUNT_RANDOM_NAMES.len()).unwrap_or(1))
}

fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

fn now_rfc3339() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
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
		state::CodexAccountActivitySummary,
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
			..CodexAccountActivitySummary::default()
		}]);

		assert_eq!(response.accounts[0].plan_type.as_deref(), Some("pro"));
		assert_eq!(response.accounts[0].primary_window_seconds, Some(18_000));
		assert_eq!(response.accounts[0].primary_remaining_percent, Some(72));
		assert_eq!(response.accounts[0].secondary_window_seconds, Some(604_800));
		assert_eq!(response.accounts[0].secondary_remaining_percent, Some(91));
		assert_eq!(response.accounts[0].credits_balance.as_deref(), Some("9.99"));
		assert_eq!(response.accounts[0].seven_day_used_percent, Some(9));

		assert_close(response.accounts[0].seven_day_daily_average_percent, 9.0 / 7.0);
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
			usage_summary("copy@example.com", "...123456", 40),
			usage_summary("other@example.com", "...654321", 70),
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
		assert_eq!(estimate.total_capacity_percent, 200);
		assert_eq!(estimate.total_used_percent, 90);

		assert_close(Some(estimate.total_used_of_capacity_percent), 45.0);
		assert_close(Some(estimate.average_daily_used_percent), 90.0 / 7.0);
		assert_close(Some(estimate.average_daily_pool_percent), 45.0 / 7.0);

		assert_eq!(response.accounts[0].usage_records.len(), 1);
		assert_eq!(response.accounts[0].usage_records[0].date, record_date);
		assert_eq!(response.accounts[0].usage_records[0].used_percent, 60);
		assert_eq!(history.lines().count(), 2);
		assert!(history.contains(r#""used_percent":60"#));
		assert!(history.contains(r#""used_percent":30"#));
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
		secondary_remaining_percent: i64,
	) -> CodexAccountActivitySummary {
		CodexAccountActivitySummary {
			account_fingerprint: String::from(account_fingerprint),
			email: Some(String::from(email)),
			plan_type: Some(String::from("pro")),
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
