//! Typed decoding for the authenticated Codex account backend API.
//!
//! This module deliberately contains no HTTP client, credential, process, or UI code.  The
//! backend API is the stable provider boundary used by the daemon account observer.

use std::{
	collections::BTreeSet,
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::{
	AccountQuotaObservationError, AccountQuotaWindow, ResetCardDescriptor, ResetCardTimestamp,
};
use serde_json::Value;
use zeroize::Zeroizing;

/// Maximum UTF-8 bytes retained for one exact provider credit identifier.
pub const MAX_EXACT_RESET_CREDIT_ID_BYTES: usize = 1_024;
/// Maximum UTF-8 bytes retained for one provider idempotency key.
pub const MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES: usize = 256;
/// Maximum number of reset-credit details retained from one provider response.
pub const MAX_RESET_CARDS_PER_INVENTORY: usize = decodex_core::MAX_RESET_CARD_ITEMS;

/// Exact provider reset-credit identifier.
///
/// This value is private effect material. It is not a public card identity and must not cross the
/// core, client, or user-interface boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ExactResetCreditId(Zeroizing<String>);
impl ExactResetCreditId {
	/// Validate one exact provider identifier without trimming or normalization.
	pub fn new(value: impl Into<String>) -> Result<Self, AccountApiProtocolError> {
		let value = Zeroizing::new(value.into());
		if !is_bounded_scalar(value.as_str(), MAX_EXACT_RESET_CREDIT_ID_BYTES) {
			return Err(AccountApiProtocolError::InvalidCreditId);
		}
		Ok(Self(value))
	}

	/// Borrow the exact identifier for the provider effect boundary.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ExactResetCreditId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ExactResetCreditId([REDACTED])")
	}
}

/// Exact bounded provider idempotency key.
#[derive(Clone, Eq, PartialEq)]
pub struct ResetCardIdempotencyKey(Zeroizing<String>);
impl ResetCardIdempotencyKey {
	/// Validate a stable scalar key while preserving its exact text for retries.
	pub fn new(value: impl Into<String>) -> Result<Self, AccountApiProtocolError> {
		let value = Zeroizing::new(value.into());
		if !is_bounded_scalar(value.as_str(), MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES) {
			return Err(AccountApiProtocolError::InvalidIdempotencyKey);
		}
		Ok(Self(value))
	}

	/// Borrow the exact key for durable effect preparation.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ResetCardIdempotencyKey {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ResetCardIdempotencyKey([REDACTED])")
	}
}

/// Maximum body size accepted by the account backend decoder.
pub const MAX_ACCOUNT_API_BODY_BYTES: usize = 256 * 1_024;
const MAX_PROFILE_TEXT_BYTES: usize = 256;
const MAX_PROFILE_DAILY_BUCKETS: usize = 36;

/// A bounded profile projection returned by `/wham/profiles/me`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiProfile {
	/// Optional provider display name.
	pub display_name: Option<String>,
	/// Optional provider username.
	pub username: Option<String>,
	/// Lifetime token count.
	pub lifetime_tokens: Option<i64>,
	/// Highest daily token count.
	pub peak_daily_tokens: Option<i64>,
	/// Longest running turn in seconds.
	pub longest_task_seconds: Option<i64>,
	/// Current usage streak in days.
	pub current_streak_days: Option<i32>,
	/// Longest usage streak in days.
	pub longest_streak_days: Option<i32>,
	/// Recent daily usage buckets, in ascending date order.
	pub daily_usage: Vec<AccountApiDailyUsage>,
}

/// One bounded daily token usage fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiDailyUsage {
	/// ISO calendar date returned by the provider.
	pub start_date: String,
	/// Non-negative token count.
	pub tokens: i64,
}

/// One required rate-limit window decoded from `/wham/usage`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountApiQuotaWindow {
	/// The accepted five-hour or seven-day duration.
	pub duration_minutes: u32,
	/// The decoded quota fact or a bounded protocol classification.
	pub result: Result<AccountQuotaWindow, AccountQuotaObservationError>,
}

/// A rate-limit response with its optional reset-credit summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiUsage {
	/// Required quota windows, in five-hour then seven-day order.
	pub quota_windows: [AccountApiQuotaWindow; 2],
	/// Provider-reported available reset-credit count, if supplied.
	pub reported_available_count: Option<u64>,
}

/// One exact reset credit decoded from the detail endpoint.
#[derive(Clone, Eq, PartialEq)]
pub struct AccountApiResetCredit {
	exact_id: ExactResetCreditId,
	descriptor: ResetCardDescriptor,
}
impl AccountApiResetCredit {
	/// Borrow the exact provider id for the provider effect boundary.
	pub fn exact_id(&self) -> &ExactResetCreditId {
		&self.exact_id
	}

	/// Return the public grant/expiry descriptor.
	pub const fn descriptor(&self) -> ResetCardDescriptor {
		self.descriptor
	}
}
impl std::fmt::Debug for AccountApiResetCredit {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AccountApiResetCredit")
			.field("descriptor", &self.descriptor)
			.finish_non_exhaustive()
	}
}

/// Reset-credit details returned by `/wham/rate-limit-reset-credits`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiResetCredits {
	/// Provider-reported available count.
	pub reported_available_count: u64,
	/// Validated available cards.  The list is empty when details are incomplete.
	pub credits: Vec<AccountApiResetCredit>,
	/// Whether every available credit was safely decoded and matched the count.
	pub details_complete: bool,
}

/// Result returned by the direct reset-credit consume endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountApiConsumeOutcome {
	/// The provider reset one or more windows.
	Reset,
	/// There was no active limit to reset.
	NothingToReset,
	/// No credit was available.
	NoCredit,
	/// The selected credit was already redeemed.
	AlreadyRedeemed,
}

/// Closed protocol error.  It never carries provider response bodies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountApiProtocolError {
	/// The response exceeded the bounded body limit.
	BodyLimitExceeded,
	/// JSON or required field types were invalid.
	MalformedResponse,
	/// A scalar or date was outside the accepted contract.
	InvalidValue,
	/// The provider returned an exact credit identifier outside the bounded scalar contract.
	InvalidCreditId,
	/// A durable provider idempotency key was outside the bounded scalar contract.
	InvalidIdempotencyKey,
	/// The provider returned an unknown consume outcome.
	UnknownConsumeOutcome,
}
impl Error for AccountApiProtocolError {}
impl Display for AccountApiProtocolError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::BodyLimitExceeded => "account API response exceeds the body limit",
			Self::MalformedResponse => "account API response is malformed",
			Self::InvalidValue => "account API response contains an invalid value",
			Self::InvalidCreditId => "account API credit identifier is invalid",
			Self::InvalidIdempotencyKey => "account API idempotency key is invalid",
			Self::UnknownConsumeOutcome => "account API consume outcome is unknown",
		})
	}
}

/// Decode one `/wham/profiles/me` response.
pub fn decode_account_api_profile(
	bytes: &[u8],
) -> Result<AccountApiProfile, AccountApiProtocolError> {
	ensure_body_limit(bytes)?;
	let payload: Value =
		serde_json::from_slice(bytes).map_err(|_| AccountApiProtocolError::MalformedResponse)?;
	let object = payload.as_object().ok_or(AccountApiProtocolError::MalformedResponse)?;
	let profile = optional_object(object.get("profile"))?;
	let stats = required_object(object.get("stats"))?;
	let display_name = profile
		.map(|value| optional_text(value.get("display_name"), MAX_PROFILE_TEXT_BYTES))
		.transpose()?
		.flatten();
	let username = profile
		.map(|value| optional_text(value.get("username"), MAX_PROFILE_TEXT_BYTES))
		.transpose()?
		.flatten();
	let lifetime_tokens = optional_nonnegative_i64(stats.get("lifetime_tokens"))?;
	let mut peak_daily_tokens = optional_nonnegative_i64(stats.get("peak_daily_tokens"))?;
	let longest_task_seconds = optional_nonnegative_i64(stats.get("longest_running_turn_sec"))?;
	let current_streak_days = optional_nonnegative_i32(stats.get("current_streak_days"))?;
	let longest_streak_days = optional_nonnegative_i32(stats.get("longest_streak_days"))?;
	let mut daily_usage = match stats.get("daily_usage_buckets") {
		None | Some(Value::Null) => Vec::new(),
		Some(Value::Array(values)) =>
			values.iter().map(decode_daily_usage).collect::<Result<Vec<_>, _>>()?,
		Some(_) => return Err(AccountApiProtocolError::MalformedResponse),
	};
	daily_usage.sort_by(|left, right| left.start_date.cmp(&right.start_date));
	if daily_usage.windows(2).any(|values| values[0].start_date == values[1].start_date) {
		return Err(AccountApiProtocolError::InvalidValue);
	}
	if peak_daily_tokens.is_none() {
		peak_daily_tokens = daily_usage.iter().map(|fact| fact.tokens).max();
	}
	if daily_usage.len() > MAX_PROFILE_DAILY_BUCKETS {
		daily_usage = daily_usage.split_off(daily_usage.len() - MAX_PROFILE_DAILY_BUCKETS);
	}
	let profile = AccountApiProfile {
		display_name,
		username,
		lifetime_tokens,
		peak_daily_tokens,
		longest_task_seconds,
		current_streak_days,
		longest_streak_days,
		daily_usage,
	};
	Ok(profile)
}

/// Decode one `/wham/usage` response.
pub fn decode_account_api_usage(bytes: &[u8]) -> Result<AccountApiUsage, AccountApiProtocolError> {
	ensure_body_limit(bytes)?;
	let payload: Value =
		serde_json::from_slice(bytes).map_err(|_| AccountApiProtocolError::MalformedResponse)?;
	let object = payload.as_object().ok_or(AccountApiProtocolError::MalformedResponse)?;
	let rate_limit = match object.get("rate_limit") {
		None | Some(Value::Null) => None,
		Some(Value::Object(value)) => Some(value),
		Some(_) => return Err(AccountApiProtocolError::MalformedResponse),
	};
	let quota_windows = decode_quota_windows(rate_limit);
	let reported_available_count = object
		.get("rate_limit_reset_credits")
		.map(|value| decode_reset_credit_summary(Some(value)))
		.transpose()?
		.flatten();
	Ok(AccountApiUsage { quota_windows, reported_available_count })
}

/// Decode one `/wham/rate-limit-reset-credits` response.
pub fn decode_account_api_reset_credits(
	bytes: &[u8],
) -> Result<AccountApiResetCredits, AccountApiProtocolError> {
	ensure_body_limit(bytes)?;
	let payload: Value =
		serde_json::from_slice(bytes).map_err(|_| AccountApiProtocolError::MalformedResponse)?;
	let object = payload.as_object().ok_or(AccountApiProtocolError::MalformedResponse)?;
	let reported_available_count = decode_reset_credit_summary(object.get("available_count"))?
		.ok_or(AccountApiProtocolError::MalformedResponse)?;
	let (credits, mut details_complete) = match object.get("credits") {
		None | Some(Value::Null) => (Vec::new(), false),
		Some(Value::Array(values)) =>
			if values.len() > MAX_RESET_CARDS_PER_INVENTORY {
				(Vec::new(), false)
			} else {
				let mut credits = Vec::with_capacity(values.len());
				let mut identifiers = BTreeSet::new();
				let mut descriptors = BTreeSet::new();
				let mut details_complete = true;
				for value in values {
					let Some(credit) = value.as_object() else {
						return Err(AccountApiProtocolError::MalformedResponse);
					};
					let status = required_text(credit.get("status"))?;
					if status == "redeeming" || status == "redeemed" {
						continue;
					}
					if status != "available"
						|| required_text(credit.get("reset_type"))? != "codexRateLimits"
					{
						details_complete = false;
						continue;
					}
					let id = required_text(credit.get("id"))?;
					let Some(expires_at) = credit.get("expires_at") else {
						details_complete = false;
						continue;
					};
					let granted_at = match parse_provider_timestamp(credit.get("granted_at")) {
						Some(value) => value,
						None => {
							details_complete = false;
							continue;
						},
					};
					let expires_at = match parse_provider_timestamp(Some(expires_at)) {
						Some(value) => value,
						None => {
							details_complete = false;
							continue;
						},
					};
					let Ok(granted_at) = ResetCardTimestamp::from_unix_seconds(granted_at) else {
						details_complete = false;
						continue;
					};
					let Ok(expires_at) = ResetCardTimestamp::from_unix_seconds(expires_at) else {
						details_complete = false;
						continue;
					};
					let Ok(descriptor) = ResetCardDescriptor::new(granted_at, expires_at) else {
						details_complete = false;
						continue;
					};
					let Ok(exact_id) = ExactResetCreditId::new(id.to_owned()) else {
						details_complete = false;
						continue;
					};
					if !identifiers.insert(exact_id.as_str().to_owned())
						|| !descriptors.insert(descriptor)
					{
						details_complete = false;
						continue;
					}
					credits.push(AccountApiResetCredit { exact_id, descriptor });
				}
				(credits, details_complete)
			},
		Some(_) => return Err(AccountApiProtocolError::MalformedResponse),
	};
	details_complete &=
		reported_available_count == u64::try_from(credits.len()).unwrap_or(u64::MAX);
	Ok(AccountApiResetCredits { reported_available_count, credits, details_complete })
}

/// Decode one direct consume response.
pub fn decode_account_api_consume(
	bytes: &[u8],
) -> Result<AccountApiConsumeOutcome, AccountApiProtocolError> {
	ensure_body_limit(bytes)?;
	let payload: Value =
		serde_json::from_slice(bytes).map_err(|_| AccountApiProtocolError::MalformedResponse)?;
	let object = payload.as_object().ok_or(AccountApiProtocolError::MalformedResponse)?;
	match required_text(object.get("code"))? {
		"reset" => Ok(AccountApiConsumeOutcome::Reset),
		"nothing_to_reset" => Ok(AccountApiConsumeOutcome::NothingToReset),
		"no_credit" => Ok(AccountApiConsumeOutcome::NoCredit),
		"already_redeemed" => Ok(AccountApiConsumeOutcome::AlreadyRedeemed),
		_ => Err(AccountApiProtocolError::UnknownConsumeOutcome),
	}
}

fn decode_quota_windows(
	rate_limit: Option<&serde_json::Map<String, Value>>,
) -> [AccountApiQuotaWindow; 2] {
	[
		decode_quota_window(
			rate_limit.and_then(|value| value.get("primary_window")),
			AccountQuotaWindow::FIVE_HOURS_MINUTES,
		),
		decode_quota_window(
			rate_limit.and_then(|value| value.get("secondary_window")),
			AccountQuotaWindow::SEVEN_DAYS_MINUTES,
		),
	]
}

fn decode_quota_window(value: Option<&Value>, expected_duration: u32) -> AccountApiQuotaWindow {
	let result = (|| {
		let object = value?.as_object()?;
		let duration_seconds = parse_integer(object.get("limit_window_seconds"))?;
		let duration_minutes = u32::try_from(duration_seconds / 60).ok()?;
		if duration_minutes != expected_duration {
			return None;
		}
		let used_percent = u8::try_from(parse_integer(object.get("used_percent"))?).ok()?;
		let reset_at_seconds = parse_integer(object.get("reset_at"))?;
		let reset_at_micros = reset_at_seconds.checked_mul(1_000_000)?;
		AccountQuotaWindow::new(duration_minutes, used_percent, reset_at_micros).ok()
	})();
	AccountApiQuotaWindow {
		duration_minutes: expected_duration,
		result: result.ok_or_else(|| {
			if value.is_none() {
				AccountQuotaObservationError::UnsupportedWindow
			} else {
				AccountQuotaObservationError::ProtocolUnavailable
			}
		}),
	}
}

fn decode_reset_credit_summary(
	value: Option<&Value>,
) -> Result<Option<u64>, AccountApiProtocolError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Object(object)) => {
			let count = parse_integer(object.get("available_count"))
				.ok_or(AccountApiProtocolError::MalformedResponse)?;
			Ok(Some(nonnegative_count(count)?))
		},
		Some(value) => {
			let count =
				parse_integer(Some(value)).ok_or(AccountApiProtocolError::MalformedResponse)?;
			Ok(Some(nonnegative_count(count)?))
		},
	}
}

fn nonnegative_count(value: i64) -> Result<u64, AccountApiProtocolError> {
	(value >= 0)
		.then_some(value)
		.and_then(|value| u64::try_from(value).ok())
		.ok_or(AccountApiProtocolError::InvalidValue)
}

fn ensure_body_limit(bytes: &[u8]) -> Result<(), AccountApiProtocolError> {
	(bytes.len() <= MAX_ACCOUNT_API_BODY_BYTES)
		.then_some(())
		.ok_or(AccountApiProtocolError::BodyLimitExceeded)
}

fn is_bounded_scalar(value: &str, maximum: usize) -> bool {
	!value.is_empty()
		&& value.len() <= maximum
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

fn optional_object(
	value: Option<&Value>,
) -> Result<Option<&serde_json::Map<String, Value>>, AccountApiProtocolError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Object(value)) => Ok(Some(value)),
		Some(_) => Err(AccountApiProtocolError::MalformedResponse),
	}
}

fn required_object(
	value: Option<&Value>,
) -> Result<&serde_json::Map<String, Value>, AccountApiProtocolError> {
	match value {
		Some(Value::Object(value)) => Ok(value),
		_ => Err(AccountApiProtocolError::MalformedResponse),
	}
}

fn optional_text(
	value: Option<&Value>,
	maximum: usize,
) -> Result<Option<String>, AccountApiProtocolError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
		Some(Value::String(value))
			if value.len() <= maximum && !value.chars().any(char::is_control) =>
			Ok(Some(value.clone())),
		Some(Value::String(_)) => Err(AccountApiProtocolError::InvalidValue),
		Some(_) => Err(AccountApiProtocolError::MalformedResponse),
	}
}

fn required_text(value: Option<&Value>) -> Result<&str, AccountApiProtocolError> {
	match value {
		Some(Value::String(value))
			if !value.is_empty()
				&& value.len() <= MAX_EXACT_RESET_CREDIT_ID_BYTES
				&& !value.chars().any(char::is_control) =>
			Ok(value),
		_ => Err(AccountApiProtocolError::MalformedResponse),
	}
}

fn optional_nonnegative_i64(value: Option<&Value>) -> Result<Option<i64>, AccountApiProtocolError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(value) => parse_integer(Some(value))
			.filter(|value| *value >= 0)
			.map(Some)
			.ok_or(AccountApiProtocolError::InvalidValue),
	}
}

fn optional_nonnegative_i32(value: Option<&Value>) -> Result<Option<i32>, AccountApiProtocolError> {
	optional_nonnegative_i64(value)?
		.map(|value| i32::try_from(value).map_err(|_| AccountApiProtocolError::InvalidValue))
		.transpose()
}

fn decode_daily_usage(value: &Value) -> Result<AccountApiDailyUsage, AccountApiProtocolError> {
	let object = value.as_object().ok_or(AccountApiProtocolError::MalformedResponse)?;
	let start_date = match object.get("start_date") {
		Some(Value::String(value)) if value.len() == 10 && canonical_calendar_date(value) =>
			value.clone(),
		_ => return Err(AccountApiProtocolError::InvalidValue),
	};
	let tokens = parse_integer(object.get("tokens"))
		.filter(|value| *value >= 0)
		.ok_or(AccountApiProtocolError::InvalidValue)?;
	Ok(AccountApiDailyUsage { start_date, tokens })
}

fn parse_integer(value: Option<&Value>) -> Option<i64> {
	match value? {
		Value::Number(value) =>
			value.as_i64().or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok())),
		Value::String(value) => value.parse().ok(),
		_ => None,
	}
}

fn parse_provider_timestamp(value: Option<&Value>) -> Option<i64> {
	match value? {
		Value::Number(_) => parse_integer(value),
		Value::String(value) => value.parse().ok().or_else(|| parse_rfc3339_utc_seconds(value)),
		_ => None,
	}
}

fn parse_rfc3339_utc_seconds(value: &str) -> Option<i64> {
	let value = value.strip_suffix('Z')?;
	let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
	let bytes = whole.as_bytes();
	if bytes.len() != 19
		|| bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| bytes[10] != b'T'
		|| bytes[13] != b':'
		|| bytes[16] != b':'
		|| bytes
			.iter()
			.enumerate()
			.any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13 | 16) && !byte.is_ascii_digit())
		|| !fraction.is_empty() && !fraction.bytes().all(|byte| byte.is_ascii_digit())
	{
		return None;
	}
	let number = |start: usize, end: usize| {
		std::str::from_utf8(&bytes[start..end]).ok()?.parse::<i64>().ok()
	};
	let year = number(0, 4)?;
	let month = number(5, 7)?;
	let day = number(8, 10)?;
	let hour = number(11, 13)?;
	let minute = number(14, 16)?;
	let second = number(17, 19)?;
	let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
	let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
	if !(1..=12).contains(&month)
		|| day < 1
		|| day > month_days[usize::try_from(month - 1).ok()?]
		|| hour > 23
		|| minute > 59
		|| second > 59
	{
		return None;
	}
	let adjusted_year = year - i64::from(month <= 2);
	let era = adjusted_year.div_euclid(400);
	let year_of_era = adjusted_year - era * 400;
	let adjusted_month = month + if month > 2 { -3 } else { 9 };
	let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	let days = era * 146_097 + day_of_era - 719_468;
	days.checked_mul(86_400)?.checked_add(hour * 3_600 + minute * 60 + second)
}

fn canonical_calendar_date(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 10 {
		return false;
	}
	if bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| bytes
			.iter()
			.enumerate()
			.any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
	{
		return false;
	}
	let parse = |start: usize, end: usize| {
		std::str::from_utf8(&bytes[start..end]).ok()?.parse::<i64>().ok()
	};
	let (Some(year), Some(month), Some(day)) = (parse(0, 4), parse(5, 7), parse(8, 10)) else {
		return false;
	};
	let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
	let maximum = match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		2 if leap => 29,
		2 => 28,
		_ => return false,
	};
	year > 0 && (1..=maximum).contains(&day)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decodes_upstream_profile_shape_and_bounds_daily_buckets() {
		let buckets = (0..40)
			.map(|index| {
				let month = index / 4 + 1;
				let day = index % 4 + 1;
				serde_json::json!({
					"start_date": format!("2026-{month:02}-{day:02}"),
					"tokens": index,
				})
			})
			.collect::<Vec<_>>();
		let body = serde_json::json!({
			"profile": {"display_name": "Val", "username": "val"},
			"stats": {"lifetime_tokens": 12, "daily_usage_buckets": buckets}
		})
		.to_string();
		let profile = decode_account_api_profile(body.as_bytes()).expect("profile should decode");
		assert_eq!(profile.display_name.as_deref(), Some("Val"));
		assert_eq!(profile.peak_daily_tokens, Some(39));
		assert_eq!(profile.daily_usage.len(), MAX_PROFILE_DAILY_BUCKETS);
	}

	#[test]
	fn accepts_upstream_profile_with_empty_optional_stats() {
		let profile = decode_account_api_profile(br#"{"stats":{}}"#)
			.expect("the upstream stats fields are individually optional");
		assert_eq!(profile.daily_usage, Vec::new());
		assert_eq!(profile.lifetime_tokens, None);
	}

	#[test]
	fn decodes_snake_case_usage_windows_and_summary() {
		let body = serde_json::json!({
			"plan_type": "pro",
			"rate_limit": {
				"primary_window": {"used_percent": 12, "limit_window_seconds": 18000, "reset_at": 1_800_000_000},
				"secondary_window": {"used_percent": 34, "limit_window_seconds": 604800, "reset_at": 1_800_100_000}
			},
			"rate_limit_reset_credits": {"available_count": 2}
		})
		.to_string();
		let usage = decode_account_api_usage(body.as_bytes()).expect("usage should decode");
		assert_eq!(usage.reported_available_count, Some(2));
		assert_eq!(usage.quota_windows[0].result.unwrap().used_percent, 12);
		assert_eq!(usage.quota_windows[1].result.unwrap().duration_minutes, 10_080);
	}

	#[test]
	fn decodes_string_timestamps_and_direct_consume_code() {
		let body = serde_json::json!({
			"available_count": 1,
			"credits": [{
				"id": "credit-1",
				"reset_type": "codexRateLimits",
				"status": "available",
				"granted_at": "2027-01-15T08:00:00Z",
				"expires_at": "2027-01-15T09:00:00Z"
			}]
		})
		.to_string();
		let credits =
			decode_account_api_reset_credits(body.as_bytes()).expect("credits should decode");
		assert!(credits.details_complete);
		assert_eq!(credits.credits[0].descriptor().granted_at().unix_seconds(), 1_800_000_000);
		assert_eq!(
			decode_account_api_consume(br#"{"code":"nothing_to_reset","windows_reset":0}"#)
				.unwrap(),
			AccountApiConsumeOutcome::NothingToReset
		);
	}
}
