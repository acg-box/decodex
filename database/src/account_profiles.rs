//! Bounded non-secret account-profile snapshots.

use decodex_core::{AccountId, AccountProvider, ProviderIdentity};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{SqliteStore, StoreError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileDailyUsage {
	pub start_date: String,
	pub tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileObservation {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub provider: ProviderIdentity,
	pub observed_at_unix_micros: i64,
	pub display_name: Option<String>,
	pub username: Option<String>,
	pub lifetime_tokens: Option<i64>,
	pub peak_daily_tokens: Option<i64>,
	pub longest_task_seconds: Option<i64>,
	pub current_streak_days: Option<i32>,
	pub longest_streak_days: Option<i32>,
	pub daily_usage: Vec<AccountProfileDailyUsage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileSnapshot {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub provider: ProviderIdentity,
	pub observed_at_unix_micros: i64,
	pub display_name: Option<String>,
	pub username: Option<String>,
	pub lifetime_tokens: Option<i64>,
	pub peak_daily_tokens: Option<i64>,
	pub longest_task_seconds: Option<i64>,
	pub current_streak_days: Option<i32>,
	pub longest_streak_days: Option<i32>,
	pub daily_usage: Vec<AccountProfileDailyUsage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountProfileObservationOutcome {
	Observed,
	AccountUnavailable,
	StaleAccount,
	StaleObservation,
}

impl SqliteStore {
	pub async fn observe_account_profile(
		&self,
		observation: &AccountProfileObservation,
	) -> Result<AccountProfileObservationOutcome, StoreError> {
		validate_observation(observation)?;
		let observation = observation.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(super::account_lifecycle::sql_error)?;
			let account = transaction
				.query_row(
					"SELECT revision, provider, provider_account_id, tombstoned_at_micros IS NOT NULL
					 FROM accounts WHERE account_id = ?1",
					params![observation.account_id.as_str()],
					|row| {
						Ok((
							row.get::<_, i64>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, String>(2)?,
							row.get::<_, bool>(3)?,
						))
					},
				)
				.optional()
				.map_err(super::account_lifecycle::sql_error)?;
			let Some((revision, provider, provider_account, tombstoned)) = account else {
				return Ok(AccountProfileObservationOutcome::AccountUnavailable);
			};
			if tombstoned {
				return Ok(AccountProfileObservationOutcome::AccountUnavailable);
			}
			if revision != observation.account_revision
				|| provider != provider_text(observation.provider.provider())
				|| provider_account != observation.provider.account_id()
			{
				return Ok(AccountProfileObservationOutcome::StaleAccount);
			}
			let prior = transaction
				.query_row(
					"SELECT observed_at_micros FROM account_profile_snapshots WHERE account_id = ?1",
					params![observation.account_id.as_str()],
					|row| row.get::<_, i64>(0),
				)
				.optional()
				.map_err(super::account_lifecycle::sql_error)?;
			if prior.is_some_and(|prior| prior >= observation.observed_at_unix_micros) {
				return Ok(AccountProfileObservationOutcome::StaleObservation);
			}
			transaction
				.execute(
					"INSERT INTO account_profile_snapshots (
					   account_id, account_revision, provider, provider_account_id,
					   observed_at_micros, display_name, username, lifetime_tokens,
					   peak_daily_tokens, longest_task_seconds, current_streak_days,
					   longest_streak_days
					 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
					 ON CONFLICT(account_id) DO UPDATE SET
					   account_revision = excluded.account_revision,
					   provider = excluded.provider,
					   provider_account_id = excluded.provider_account_id,
					   observed_at_micros = excluded.observed_at_micros,
					   display_name = excluded.display_name,
					   username = excluded.username,
					   lifetime_tokens = excluded.lifetime_tokens,
					   peak_daily_tokens = excluded.peak_daily_tokens,
					   longest_task_seconds = excluded.longest_task_seconds,
					   current_streak_days = excluded.current_streak_days,
					   longest_streak_days = excluded.longest_streak_days",
					params![
						observation.account_id.as_str(),
						observation.account_revision,
						provider_text(observation.provider.provider()),
						observation.provider.account_id(),
						observation.observed_at_unix_micros,
						observation.display_name,
						observation.username,
						observation.lifetime_tokens,
						observation.peak_daily_tokens,
						observation.longest_task_seconds,
						observation.current_streak_days,
						observation.longest_streak_days,
					],
				)
				.map_err(super::account_lifecycle::sql_error)?;
			transaction
				.execute(
					"DELETE FROM account_profile_daily_usage WHERE account_id = ?1",
					params![observation.account_id.as_str()],
				)
				.map_err(super::account_lifecycle::sql_error)?;
			for fact in &observation.daily_usage {
				transaction
					.execute(
						"INSERT INTO account_profile_daily_usage (
						   account_id, start_date, tokens, observed_at_micros
						 ) VALUES (?1, ?2, ?3, ?4)",
						params![
							observation.account_id.as_str(),
							fact.start_date,
							fact.tokens,
							observation.observed_at_unix_micros,
						],
					)
					.map_err(super::account_lifecycle::sql_error)?;
			}
			transaction.commit().map_err(super::account_lifecycle::sql_error)?;
			Ok(AccountProfileObservationOutcome::Observed)
		})
		.await
	}

	pub async fn read_account_profile(
		&self,
		account_id: &AccountId,
	) -> Result<Option<AccountProfileSnapshot>, StoreError> {
		let account_id = account_id.clone();
		self.run(move |connection| {
			let row = connection
				.query_row(
					"SELECT profile.account_revision, profile.provider,
					        profile.provider_account_id, profile.observed_at_micros,
					        profile.display_name, profile.username, profile.lifetime_tokens,
					        profile.peak_daily_tokens, profile.longest_task_seconds,
					        profile.current_streak_days, profile.longest_streak_days
					 FROM account_profile_snapshots AS profile
					 JOIN accounts AS account USING (account_id)
					 WHERE profile.account_id = ?1 AND account.tombstoned_at_micros IS NULL
					   AND profile.provider = account.provider
					   AND profile.provider_account_id = account.provider_account_id",
					params![account_id.as_str()],
					|row| {
						Ok((
							row.get::<_, i64>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, String>(2)?,
							row.get::<_, i64>(3)?,
							row.get::<_, Option<String>>(4)?,
							row.get::<_, Option<String>>(5)?,
							row.get::<_, Option<i64>>(6)?,
							row.get::<_, Option<i64>>(7)?,
							row.get::<_, Option<i64>>(8)?,
							row.get::<_, Option<i32>>(9)?,
							row.get::<_, Option<i32>>(10)?,
						))
					},
				)
				.optional()
				.map_err(super::account_lifecycle::sql_error)?;
			let Some((
				revision,
				provider,
				provider_account,
				observed,
				display,
				username,
				lifetime,
				peak,
				longest_task,
				current_streak,
				longest_streak,
			)) = row
			else {
				return Ok(None);
			};
			let provider = match provider.as_str() {
				"chatgpt" => AccountProvider::Chatgpt,
				_ => return Err(incompatible()),
			};
			let provider =
				ProviderIdentity::new(provider, provider_account).map_err(|_| incompatible())?;
			let mut statement = connection
				.prepare(
					"SELECT start_date, tokens FROM account_profile_daily_usage
					 WHERE account_id = ?1 ORDER BY start_date",
				)
				.map_err(super::account_lifecycle::sql_error)?;
			let rows = statement
				.query_map(params![account_id.as_str()], |row| {
					Ok(AccountProfileDailyUsage { start_date: row.get(0)?, tokens: row.get(1)? })
				})
				.map_err(super::account_lifecycle::sql_error)?;
			let snapshot = AccountProfileSnapshot {
				account_id,
				account_revision: revision,
				provider,
				observed_at_unix_micros: observed,
				display_name: display,
				username,
				lifetime_tokens: lifetime,
				peak_daily_tokens: peak,
				longest_task_seconds: longest_task,
				current_streak_days: current_streak,
				longest_streak_days: longest_streak,
				daily_usage: rows
					.collect::<Result<Vec<_>, _>>()
					.map_err(super::account_lifecycle::sql_error)?,
			};
			validate_snapshot(&snapshot)?;
			Ok(Some(snapshot))
		})
		.await
	}
}

fn validate_observation(observation: &AccountProfileObservation) -> Result<(), StoreError> {
	validate_shape(
		observation.account_revision,
		observation.observed_at_unix_micros,
		observation.display_name.as_deref(),
		observation.username.as_deref(),
		observation.lifetime_tokens,
		observation.peak_daily_tokens,
		observation.longest_task_seconds,
		observation.current_streak_days,
		observation.longest_streak_days,
		&observation.daily_usage,
	)
}

fn validate_snapshot(snapshot: &AccountProfileSnapshot) -> Result<(), StoreError> {
	validate_shape(
		snapshot.account_revision,
		snapshot.observed_at_unix_micros,
		snapshot.display_name.as_deref(),
		snapshot.username.as_deref(),
		snapshot.lifetime_tokens,
		snapshot.peak_daily_tokens,
		snapshot.longest_task_seconds,
		snapshot.current_streak_days,
		snapshot.longest_streak_days,
		&snapshot.daily_usage,
	)
	.map_err(|_| incompatible())
}

#[allow(clippy::too_many_arguments)]
fn validate_shape(
	account_revision: i64,
	observed_at: i64,
	display_name: Option<&str>,
	username: Option<&str>,
	lifetime_tokens: Option<i64>,
	peak_daily_tokens: Option<i64>,
	longest_task_seconds: Option<i64>,
	current_streak_days: Option<i32>,
	longest_streak_days: Option<i32>,
	daily_usage: &[AccountProfileDailyUsage],
) -> Result<(), StoreError> {
	if account_revision < 1
		|| !(1..=253_402_300_799_999_999).contains(&observed_at)
		|| display_name.is_some_and(|value| !bounded_text(value, 256))
		|| username.is_some_and(|value| !bounded_text(value, 256))
		|| [lifetime_tokens, peak_daily_tokens, longest_task_seconds]
			.into_iter()
			.flatten()
			.any(|value| value < 0)
		|| [current_streak_days, longest_streak_days].into_iter().flatten().any(|value| value < 0)
		|| daily_usage.len() > 36
	{
		return Err(StoreError::InvalidInput("account profile observation is invalid"));
	}
	if display_name.is_none()
		&& username.is_none()
		&& lifetime_tokens.is_none()
		&& peak_daily_tokens.is_none()
		&& longest_task_seconds.is_none()
		&& current_streak_days.is_none()
		&& longest_streak_days.is_none()
		&& daily_usage.is_empty()
	{
		return Err(StoreError::InvalidInput("account profile observation is empty"));
	}
	let mut previous = None;
	for fact in daily_usage {
		if fact.tokens < 0
			|| !canonical_date(&fact.start_date)
			|| previous.is_some_and(|value| value >= fact.start_date.as_str())
		{
			return Err(StoreError::InvalidInput("account profile daily usage is invalid"));
		}
		previous = Some(fact.start_date.as_str());
	}
	Ok(())
}

fn bounded_text(value: &str, maximum: usize) -> bool {
	!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn canonical_date(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 10
		|| bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| bytes
			.iter()
			.enumerate()
			.any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
	{
		return false;
	}
	let parse = |start: usize, end: usize| value[start..end].parse::<u32>().ok();
	let (Some(year), Some(month), Some(day)) = (parse(0, 4), parse(5, 7), parse(8, 10)) else {
		return false;
	};
	let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
	let maximum_day = match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		2 if leap => 29,
		2 => 28,
		_ => return false,
	};
	year > 0 && (1..=maximum_day).contains(&day)
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn incompatible() -> StoreError {
	StoreError::Incompatible("stored account profile is malformed".to_owned())
}
