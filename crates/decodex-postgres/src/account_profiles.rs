//! Bounded non-secret account-profile snapshot persistence.

use decodex_core::{AccountId, AccountProvider, ProviderIdentity};

use crate::{PostgresStore, StoreError};

const OBSERVE_ACCOUNT_PROFILE_SQL: &str = "SELECT decodex.observe_account_profile_exact(\
	$1::text::uuid,$2,$3::text::decodex.account_provider_kind,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
	$13::text[],$14::bigint[])";
const READ_ACCOUNT_PROFILE_SQL: &str = "SELECT account_id::text,account_revision,\
	provider_kind::text,provider_account_id,observed_at_micros,display_name,username,\
	lifetime_tokens,peak_daily_tokens,longest_task_seconds,current_streak_days,longest_streak_days,\
	daily_start_dates,daily_tokens FROM decodex.read_account_profile_exact($1::text::uuid)";

/// One bounded daily provider usage fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileDailyUsage {
	/// Canonical provider calendar date in `YYYY-MM-DD` form.
	pub start_date: String,
	/// Non-negative token count.
	pub tokens: i64,
}

/// Complete non-secret profile observation accepted from the fixed provider endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileObservation {
	/// Canonical vNext account identity.
	pub account_id: AccountId,
	/// Exact account revision observed before the provider request.
	pub account_revision: i64,
	/// Exact credential provider identity observed before the provider request.
	pub provider: ProviderIdentity,
	/// Daemon-owned provider observation time in Unix microseconds.
	pub observed_at_unix_micros: i64,
	/// Optional bounded provider display name.
	pub display_name: Option<String>,
	/// Optional bounded provider username.
	pub username: Option<String>,
	/// Optional non-negative lifetime token count.
	pub lifetime_tokens: Option<i64>,
	/// Optional non-negative peak daily token count.
	pub peak_daily_tokens: Option<i64>,
	/// Optional non-negative longest task duration.
	pub longest_task_seconds: Option<i64>,
	/// Optional non-negative current streak.
	pub current_streak_days: Option<i32>,
	/// Optional non-negative longest streak.
	pub longest_streak_days: Option<i32>,
	/// At most 36 unique ascending daily facts.
	pub daily_usage: Vec<AccountProfileDailyUsage>,
}

/// Latest persisted account-profile snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProfileSnapshot {
	/// Canonical vNext account identity.
	pub account_id: AccountId,
	/// Account revision fenced when the observation was committed.
	pub account_revision: i64,
	/// Provider binding fenced when the observation was committed.
	pub provider: ProviderIdentity,
	/// Observation time in Unix microseconds.
	pub observed_at_unix_micros: i64,
	/// Optional bounded provider display name.
	pub display_name: Option<String>,
	/// Optional bounded provider username.
	pub username: Option<String>,
	/// Optional non-negative lifetime token count.
	pub lifetime_tokens: Option<i64>,
	/// Optional non-negative peak daily token count.
	pub peak_daily_tokens: Option<i64>,
	/// Optional non-negative longest task duration.
	pub longest_task_seconds: Option<i64>,
	/// Optional non-negative current streak.
	pub current_streak_days: Option<i32>,
	/// Optional non-negative longest streak.
	pub longest_streak_days: Option<i32>,
	/// At most 36 unique ascending daily facts.
	pub daily_usage: Vec<AccountProfileDailyUsage>,
}

/// Exact disposition of one profile persistence attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountProfileObservationOutcome {
	/// The complete snapshot and daily facts committed.
	Observed,
	/// The account is missing or tombstoned.
	AccountUnavailable,
	/// The account revision or provider identity changed.
	StaleAccount,
	/// A later or equal provider observation already exists.
	StaleObservation,
}

impl PostgresStore {
	/// Persist one complete bounded snapshot under exact account and provider fences.
	pub async fn observe_account_profile(
		&self,
		observation: &AccountProfileObservation,
	) -> Result<AccountProfileObservationOutcome, StoreError> {
		validate_observation(observation)?;
		let dates =
			observation.daily_usage.iter().map(|fact| fact.start_date.clone()).collect::<Vec<_>>();
		let tokens = observation.daily_usage.iter().map(|fact| fact.tokens).collect::<Vec<_>>();
		let result: String = self
			.pool()
			.get()
			.await?
			.query_one(
				OBSERVE_ACCOUNT_PROFILE_SQL,
				&[
					&observation.account_id.as_str(),
					&observation.account_revision,
					&provider_text(observation.provider.provider()),
					&observation.provider.account_id(),
					&observation.observed_at_unix_micros,
					&observation.display_name,
					&observation.username,
					&observation.lifetime_tokens,
					&observation.peak_daily_tokens,
					&observation.longest_task_seconds,
					&observation.current_streak_days,
					&observation.longest_streak_days,
					&dates,
					&tokens,
				],
			)
			.await?
			.get(0);
		match result.as_str() {
			"observed" => Ok(AccountProfileObservationOutcome::Observed),
			"account_unavailable" => Ok(AccountProfileObservationOutcome::AccountUnavailable),
			"stale_account" => Ok(AccountProfileObservationOutcome::StaleAccount),
			"stale_observation" => Ok(AccountProfileObservationOutcome::StaleObservation),
			"invalid_fact" =>
				Err(StoreError::InvalidInput("account profile observation is invalid")),
			_ => Err(StoreError::Incompatible(
				"account profile observation returned an unknown result".into(),
			)),
		}
	}

	/// Read the latest snapshot only while its provider still matches a visible account.
	pub async fn read_account_profile(
		&self,
		account_id: &AccountId,
	) -> Result<Option<AccountProfileSnapshot>, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(READ_ACCOUNT_PROFILE_SQL, &[&account_id.as_str()])
			.await?;
		row.map(|row| {
			let stored_account_id = AccountId::new(row.try_get::<_, String>(0)?).map_err(|_| {
				StoreError::Incompatible("stored profile account is invalid".into())
			})?;
			if stored_account_id != *account_id {
				return Err(StoreError::Incompatible(
					"stored profile account does not match the query".into(),
				));
			}
			let provider = match row.try_get::<_, String>(2)?.as_str() {
				"chatgpt" => AccountProvider::Chatgpt,
				_ =>
					return Err(StoreError::Incompatible(
						"stored profile provider is invalid".into(),
					)),
			};
			let provider =
				ProviderIdentity::new(provider, row.try_get::<_, String>(3)?).map_err(|_| {
					StoreError::Incompatible("stored profile binding is invalid".into())
				})?;
			let dates = row.try_get::<_, Vec<String>>(12)?;
			let tokens = row.try_get::<_, Vec<i64>>(13)?;
			if dates.len() != tokens.len() || dates.len() > 36 {
				return Err(StoreError::Incompatible(
					"stored profile daily usage is incomplete".into(),
				));
			}
			let daily_usage = dates
				.into_iter()
				.zip(tokens)
				.map(|(start_date, tokens)| AccountProfileDailyUsage { start_date, tokens })
				.collect::<Vec<_>>();
			let snapshot = AccountProfileSnapshot {
				account_id: stored_account_id,
				account_revision: row.try_get(1)?,
				provider,
				observed_at_unix_micros: row.try_get(4)?,
				display_name: row.try_get(5)?,
				username: row.try_get(6)?,
				lifetime_tokens: row.try_get(7)?,
				peak_daily_tokens: row.try_get(8)?,
				longest_task_seconds: row.try_get(9)?,
				current_streak_days: row.try_get(10)?,
				longest_streak_days: row.try_get(11)?,
				daily_usage,
			};
			validate_snapshot(&snapshot)?;
			Ok(snapshot)
		})
		.transpose()
	}
}

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_account_profile_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 2] = [OBSERVE_ACCOUNT_PROFILE_SQL, READ_ACCOUNT_PROFILE_SQL];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
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
	.map_err(|_| StoreError::Incompatible("stored account profile is invalid".into()))
}

#[allow(clippy::too_many_arguments)]
fn validate_shape(
	account_revision: i64,
	observed_at_unix_micros: i64,
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
		|| !(1..=253_402_300_799_999_999).contains(&observed_at_unix_micros)
		|| display_name.is_some_and(|value| !bounded_text(value, 256))
		|| username.is_some_and(|value| !bounded_text(value, 256))
		|| lifetime_tokens.is_some_and(|value| value < 0)
		|| peak_daily_tokens.is_some_and(|value| value < 0)
		|| longest_task_seconds.is_some_and(|value| value < 0)
		|| current_streak_days.is_some_and(|value| value < 0)
		|| longest_streak_days.is_some_and(|value| value < 0)
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

#[cfg(test)]
mod tests {
	use decodex_core::{AccountId, AccountProvider, ProviderIdentity};

	use super::{AccountProfileDailyUsage, AccountProfileObservation, validate_observation};

	fn observation() -> AccountProfileObservation {
		AccountProfileObservation {
			account_id: AccountId::new("40000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: 3,
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, "provider-1").unwrap(),
			observed_at_unix_micros: 1_700_000_000_000_000,
			display_name: Some("Iris".into()),
			username: Some("iris".into()),
			lifetime_tokens: Some(10_000),
			peak_daily_tokens: Some(2_000),
			longest_task_seconds: Some(800),
			current_streak_days: Some(3),
			longest_streak_days: Some(9),
			daily_usage: vec![
				AccountProfileDailyUsage { start_date: "2026-07-27".into(), tokens: 400 },
				AccountProfileDailyUsage { start_date: "2026-07-28".into(), tokens: 700 },
			],
		}
	}

	#[test]
	fn profile_observation_requires_a_bounded_unique_ascending_projection() {
		assert!(validate_observation(&observation()).is_ok());

		let mut duplicate = observation();
		duplicate.daily_usage[1].start_date = duplicate.daily_usage[0].start_date.clone();
		assert!(validate_observation(&duplicate).is_err());

		let mut empty = observation();
		empty.display_name = None;
		empty.username = None;
		empty.lifetime_tokens = None;
		empty.peak_daily_tokens = None;
		empty.longest_task_seconds = None;
		empty.current_streak_days = None;
		empty.longest_streak_days = None;
		empty.daily_usage.clear();
		assert!(validate_observation(&empty).is_err());
	}
}
