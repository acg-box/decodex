use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
	agent::CodexAccountPool,
	prelude::{Result, eyre},
	state::CodexAccountActivitySummary,
};

use super::{AccountListResponse, AccountSummary, secure_account_file};

const USAGE_ESTIMATE_WINDOW_DAYS: i64 = 7;
const USAGE_ESTIMATE_WINDOW_SECONDS: i64 = USAGE_ESTIMATE_WINDOW_DAYS * 24 * 60 * 60;
pub(super) const DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER: i64 = 1;
const PRO_ACCOUNT_CAPACITY_MULTIPLIER: i64 = 20;

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
	pub(super) fn new(
		account_count: usize,
		account_estimate_count: usize,
		total_capacity_percent: i64,
		total_used_percent: i64,
	) -> Option<Self> {
		if account_count == 0 || account_estimate_count == 0 {
			return None;
		}

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
	pub(crate) capacity_multiplier: i64,
	pub(crate) checked_at_unix_epoch: i64,
}

impl AccountListResponse {
	pub(super) fn hydrate_usage_from_path(&mut self, accounts_path: &Path, force_refresh: bool) {
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

	pub(super) fn apply_usage_summaries(&mut self, summaries: &[CodexAccountActivitySummary]) {
		for account in &mut self.accounts {
			if let Some(summary) = matching_usage_summary(account, summaries) {
				account.apply_usage_summary(summary);
			}
		}

		self.refresh_usage_estimate();
	}

	pub(super) fn refresh_usage_records(&mut self, accounts_path: &Path) -> Result<()> {
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
		let total_capacity_percent =
			self.accounts.iter().map(AccountSummary::capacity_percent).sum::<i64>();
		let total_used_percent =
			self.accounts.iter().map(AccountSummary::used_capacity_percent).sum::<i64>();

		self.usage_estimate = AccountUsageEstimateSummary::new(
			account_count,
			account_estimate_count,
			total_capacity_percent,
			total_used_percent,
		);
	}
}

impl AccountSummary {
	fn apply_usage_summary(&mut self, summary: &CodexAccountActivitySummary) {
		self.status = summary.status.clone();
		self.plan_type = summary.plan_type.clone();
		self.capacity_multiplier = account_capacity_multiplier(self.plan_type.as_deref());
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
		self.profile_display_name.clone_from(&summary.profile_display_name);
		self.profile_username.clone_from(&summary.profile_username);

		self.profile_checked_at_unix_epoch = summary.profile_checked_at_unix_epoch;
		self.profile_lifetime_tokens = summary.profile_lifetime_tokens;
		self.profile_peak_daily_tokens = summary.profile_peak_daily_tokens;
		self.profile_longest_task_seconds = summary.profile_longest_task_seconds;
		self.profile_current_streak_days = summary.profile_current_streak_days;
		self.profile_longest_streak_days = summary.profile_longest_streak_days;

		self.profile_daily_usage.clone_from(&summary.profile_daily_usage);

		if summary.cooldown_until_unix_epoch.is_some() {
			self.cooldown_until_unix_epoch = summary.cooldown_until_unix_epoch;
		}

		self.note.clone_from(&summary.note);

		self.recovery_action = account_recovery_action(
			self.status.as_str(),
			self.refresh_token_present,
			self.refresh_status.as_deref(),
			self.note.as_deref(),
		);

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
			capacity_multiplier: self.capacity_multiplier,
			window_seconds: basis.window_seconds,
			checked_at_unix_epoch,
			resets_at_unix_epoch: basis.resets_at_unix_epoch,
			primary_window_seconds: self.primary_window_seconds,
			primary_remaining_percent: self.primary_remaining_percent,
			primary_resets_at_unix_epoch: self.primary_resets_at_unix_epoch,
			secondary_window_seconds: self.secondary_window_seconds,
			secondary_remaining_percent: self.secondary_remaining_percent,
			secondary_resets_at_unix_epoch: self.secondary_resets_at_unix_epoch,
		})
	}

	fn capacity_percent(&self) -> i64 {
		normalized_account_capacity_multiplier(self.capacity_multiplier).saturating_mul(100)
	}

	fn used_capacity_percent(&self) -> i64 {
		self.seven_day_used_percent
			.unwrap_or_default()
			.saturating_mul(normalized_account_capacity_multiplier(self.capacity_multiplier))
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
	#[serde(default = "default_account_capacity_multiplier")]
	capacity_multiplier: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	window_seconds: Option<i64>,
	checked_at_unix_epoch: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	resets_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	primary_window_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	primary_remaining_percent: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	primary_resets_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	secondary_window_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	secondary_remaining_percent: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	secondary_resets_at_unix_epoch: Option<i64>,
}
impl AccountUsageHistoryRecord {
	fn daily_summary(&self) -> AccountUsageDailySummary {
		AccountUsageDailySummary {
			date: self.date.clone(),
			used_percent: self.used_percent,
			capacity_multiplier: normalized_account_capacity_multiplier(self.capacity_multiplier),
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

	fn apply_missing_usage_windows(&self, account: &mut AccountSummary, now_unix_epoch: i64) {
		self.apply_missing_primary_usage_window(account, now_unix_epoch);
		self.apply_missing_secondary_usage_window(account, now_unix_epoch);
	}

	fn apply_missing_primary_usage_window(
		&self,
		account: &mut AccountSummary,
		now_unix_epoch: i64,
	) {
		if has_usage_window(account.primary_window_seconds, account.primary_remaining_percent)
			|| !has_current_usage_window(
				self.primary_window_seconds,
				self.primary_remaining_percent,
				self.primary_resets_at_unix_epoch,
				now_unix_epoch,
			) {
			return;
		}

		account.primary_window_seconds = self.primary_window_seconds;
		account.primary_remaining_percent = self.primary_remaining_percent;
		account.primary_resets_at_unix_epoch = self.primary_resets_at_unix_epoch;
	}

	fn apply_missing_secondary_usage_window(
		&self,
		account: &mut AccountSummary,
		now_unix_epoch: i64,
	) {
		let window_seconds = self.secondary_window_seconds.or(self.window_seconds);
		let remaining_percent = self
			.secondary_remaining_percent
			.or_else(|| Some(remaining_percent_from_used(self.used_percent)));
		let resets_at_unix_epoch =
			self.secondary_resets_at_unix_epoch.or(self.resets_at_unix_epoch);

		if has_usage_window(account.secondary_window_seconds, account.secondary_remaining_percent)
			|| !has_current_usage_window(
				window_seconds,
				remaining_percent,
				resets_at_unix_epoch,
				now_unix_epoch,
			) {
			return;
		}

		account.secondary_window_seconds = window_seconds;
		account.secondary_remaining_percent = remaining_percent;
		account.secondary_resets_at_unix_epoch = resets_at_unix_epoch;
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
		let now = OffsetDateTime::now_utc().unix_timestamp();

		for account in accounts {
			let matching_records = self
				.records
				.iter()
				.filter(|record| record.matches_account(account))
				.collect::<Vec<_>>();

			if let Some(latest) =
				matching_records.iter().max_by_key(|record| record.checked_at_unix_epoch)
			{
				if account.seven_day_used_percent.is_none() {
					account.seven_day_used_percent = Some(latest.used_percent);
					account.capacity_multiplier =
						normalized_account_capacity_multiplier(latest.capacity_multiplier);
					account.seven_day_daily_average_percent =
						Some(latest.used_percent as f64 / USAGE_ESTIMATE_WINDOW_DAYS as f64);
				}

				latest.apply_missing_usage_windows(account, now);
			}

			account.usage_records =
				matching_records.iter().map(|record| record.daily_summary()).collect();
		}
	}
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

pub(super) fn usage_history_path(accounts_path: &Path) -> Result<PathBuf> {
	let parent = accounts_path.parent().ok_or_else(|| {
		eyre::eyre!(
			"Decodex accounts path `{}` must have a parent directory.",
			accounts_path.display()
		)
	})?;

	Ok(parent.join("account-usage-history.jsonl"))
}

pub(super) fn usage_record_date(unix_epoch: i64) -> Option<String> {
	OffsetDateTime::from_unix_timestamp(unix_epoch)
		.ok()
		.map(|timestamp| timestamp.date().to_string())
}

fn accepts_secondary_usage_window(window_seconds: Option<i64>) -> bool {
	window_seconds.is_none_or(is_seven_day_usage_window)
}

const fn default_account_capacity_multiplier() -> i64 {
	DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER
}

pub(super) fn account_capacity_multiplier(plan_type: Option<&str>) -> i64 {
	match plan_type.map(str::trim).filter(|value| !value.is_empty()) {
		Some(plan_type) if plan_type.eq_ignore_ascii_case("pro") => PRO_ACCOUNT_CAPACITY_MULTIPLIER,
		_ => DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER,
	}
}

pub(super) fn account_recovery_action(
	status: &str,
	refresh_token_present: bool,
	refresh_status: Option<&str>,
	note: Option<&str>,
) -> Option<String> {
	let status = status.trim().to_ascii_lowercase();
	let refresh_status = refresh_status.unwrap_or_default().trim().to_ascii_lowercase();

	if status == "disabled" || status == "cooldown" {
		return None;
	}
	if status == "auth_failed" || refresh_status == "auth_failed" {
		return Some(String::from("login"));
	}
	if !refresh_token_present {
		return Some(String::from("login"));
	}
	if refresh_status == "failed" {
		let note = note.unwrap_or_default().to_ascii_lowercase();

		if note.contains("401") || note.contains("unauthorized") {
			return Some(String::from("login"));
		}

		return Some(String::from("retry_probe"));
	}

	match status.as_str() {
		"expired" => Some(String::from("refresh")),
		"unusable" => Some(String::from("login")),
		"probe_failed" => Some(String::from("retry_probe")),
		_ => None,
	}
}

fn normalized_account_capacity_multiplier(value: i64) -> i64 {
	value.max(DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER)
}

fn is_seven_day_usage_window(window_seconds: i64) -> bool {
	window_seconds
		.checked_sub(USAGE_ESTIMATE_WINDOW_SECONDS)
		.is_some_and(|delta| delta.abs() <= 3_600)
}

fn has_usage_window(window_seconds: Option<i64>, remaining_percent: Option<i64>) -> bool {
	matches!(window_seconds, Some(seconds) if seconds > 0) && remaining_percent.is_some()
}

fn has_current_usage_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> bool {
	has_usage_window(window_seconds, remaining_percent)
		&& resets_at_unix_epoch.is_some_and(|reset| reset > now_unix_epoch)
}

fn used_percent_from_remaining(remaining_percent: i64) -> i64 {
	100_i64.saturating_sub(remaining_percent).clamp(0, 100)
}

fn remaining_percent_from_used(used_percent: i64) -> i64 {
	100_i64.saturating_sub(used_percent).clamp(0, 100)
}

fn percent_ratio(numerator: i64, denominator: i64) -> f64 {
	if denominator <= 0 {
		return 0.0;
	}

	(numerator as f64 / denominator as f64) * 100.0
}
