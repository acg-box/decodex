//! Independent bounded account-profile observation over the fixed ChatGPT endpoint.

use std::{
	io::Read as _,
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use decodex_core::{AccountId, AccountProvider, ProviderIdentity};
use decodex_postgres::{
	AccountProfileDailyUsage, AccountProfileObservation, AccountProfileObservationOutcome,
	AccountProfileSnapshot, PostgresStore,
};
use serde_json::Value;

use crate::host_credentials::{HostCredentialStore, StoredCredential};

const PROFILE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/profiles/me";
const PROFILE_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const PROFILE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROFILE_BODY_BYTES: u64 = 256 * 1_024;
const MAX_DAILY_USAGE: usize = 36;

/// One profile result with current non-secret credential claims.
pub(crate) enum AccountProfileRuntimeResult {
	Current(AccountProfileView),
	Cached { profile: AccountProfileView, refresh_error: AccountProfileRuntimeError },
	Unavailable { claims: AccountProfileClaimsView, error: AccountProfileRuntimeError },
}

/// Daemon-owned refresh state for one exact account revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountProfileRefreshStatus {
	pub(crate) account_revision: i64,
	pub(crate) refresh_error: Option<AccountProfileRuntimeError>,
	pub(crate) snapshot: Option<AccountProfileSnapshot>,
}

impl AccountProfileRuntimeResult {
	pub(crate) fn refresh_status(&self, requested_revision: i64) -> AccountProfileRefreshStatus {
		match self {
			Self::Current(profile) => AccountProfileRefreshStatus {
				account_revision: profile.snapshot.account_revision,
				refresh_error: None,
				snapshot: Some(profile.snapshot.clone()),
			},
			Self::Cached { profile, refresh_error } => AccountProfileRefreshStatus {
				account_revision: profile.snapshot.account_revision,
				refresh_error: Some(*refresh_error),
				snapshot: Some(profile.snapshot.clone()),
			},
			Self::Unavailable { error, .. } => AccountProfileRefreshStatus {
				account_revision: requested_revision,
				refresh_error: Some(*error),
				snapshot: None,
			},
		}
	}
}

/// One persisted profile snapshot plus query-selected current credential claims.
pub(crate) struct AccountProfileView {
	pub(crate) snapshot: AccountProfileSnapshot,
	pub(crate) email: Option<String>,
	pub(crate) plan_type: Option<String>,
}

/// Query-selected claims from the exact current credential binding.
pub(crate) struct AccountProfileClaimsView {
	pub(crate) email: Option<String>,
	pub(crate) plan_type: Option<String>,
}

/// Closed profile failure that cannot carry provider bodies, tokens, or raw errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountProfileRuntimeError {
	AccountUnavailable,
	ProductStateUnavailable,
	CredentialUnavailable,
	Unauthorized,
	ProviderUnavailable,
	ProtocolUnavailable,
	AccountChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderProfile {
	display_name: Option<String>,
	username: Option<String>,
	lifetime_tokens: Option<i64>,
	peak_daily_tokens: Option<i64>,
	longest_task_seconds: Option<i64>,
	current_streak_days: Option<i32>,
	longest_streak_days: Option<i32>,
	daily_usage: Vec<AccountProfileDailyUsage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderProfileError {
	Unauthorized,
	Unavailable,
	Protocol,
}

trait AccountProfileProvider: Send + Sync {
	fn observe(
		&self,
		stored: &StoredCredential,
		provider: &ProviderIdentity,
	) -> Result<ProviderProfile, ProviderProfileError>;
}

struct OpenAiAccountProfileProvider {
	client: reqwest::blocking::Client,
}
impl OpenAiAccountProfileProvider {
	fn new() -> Result<Self, AccountProfileRuntimeError> {
		let client = reqwest::blocking::Client::builder()
			.connect_timeout(PROFILE_CONNECT_TIMEOUT)
			.timeout(PROFILE_HTTP_TIMEOUT)
			.redirect(reqwest::redirect::Policy::none())
			.user_agent("decodexd")
			.build()
			.map_err(|_| AccountProfileRuntimeError::ProviderUnavailable)?;
		Ok(Self { client })
	}
}
impl AccountProfileProvider for OpenAiAccountProfileProvider {
	fn observe(
		&self,
		stored: &StoredCredential,
		provider: &ProviderIdentity,
	) -> Result<ProviderProfile, ProviderProfileError> {
		if provider.provider() != AccountProvider::Chatgpt {
			return Err(ProviderProfileError::Protocol);
		}
		let response = self
			.client
			.get(PROFILE_ENDPOINT)
			.bearer_auth(stored.bundle().access_token())
			.header("ChatGPT-Account-Id", provider.account_id())
			.send()
			.map_err(|_| ProviderProfileError::Unavailable)?;
		if response.status() == reqwest::StatusCode::UNAUTHORIZED {
			return Err(ProviderProfileError::Unauthorized);
		}
		if !response.status().is_success() {
			return Err(ProviderProfileError::Unavailable);
		}
		if response.content_length().is_some_and(|length| length > MAX_PROFILE_BODY_BYTES) {
			return Err(ProviderProfileError::Protocol);
		}
		let mut body = Vec::new();
		response
			.take(MAX_PROFILE_BODY_BYTES + 1)
			.read_to_end(&mut body)
			.map_err(|_| ProviderProfileError::Unavailable)?;
		if u64::try_from(body.len()).map_or(true, |length| length > MAX_PROFILE_BODY_BYTES) {
			return Err(ProviderProfileError::Protocol);
		}
		decode_provider_profile(&body)
	}
}

/// Provider primitive for one independent profile request with no retained secret state.
///
/// `AccountObservationService` owns the daemon cadence and calls this primitive.
#[derive(Clone)]
pub(crate) struct AccountProfileRuntime {
	store: PostgresStore,
	credentials: Arc<dyn HostCredentialStore>,
	provider: Option<Arc<dyn AccountProfileProvider>>,
}
impl AccountProfileRuntime {
	/// Construct the fixed production provider boundary.
	pub(crate) fn new(store: PostgresStore, credentials: Arc<dyn HostCredentialStore>) -> Self {
		Self {
			store,
			credentials,
			provider: OpenAiAccountProfileProvider::new()
				.ok()
				.map(|provider| Arc::new(provider) as Arc<dyn AccountProfileProvider>),
		}
	}

	/// Read only the latest daemon-owned profile value and current credential claims.
	///
	/// This path never contacts the provider. The background account-observation service owns
	/// provider refresh and supplies the exact revision-scoped refresh status.
	pub(crate) async fn read_cached(
		&self,
		account_id: &AccountId,
		include_email: bool,
		status: Option<AccountProfileRefreshStatus>,
	) -> AccountProfileRuntimeResult {
		let account = match self.store.read_account_registry(Some(account_id), 1).await {
			Ok(accounts) => match accounts.into_iter().next() {
				Some(account) if !account.tombstoned => account,
				_ =>
					return AccountProfileRuntimeResult::Unavailable {
						claims: ProfileClaims::redacted().into_view(),
						error: AccountProfileRuntimeError::AccountUnavailable,
					},
			},
			Err(_) =>
				return AccountProfileRuntimeResult::Unavailable {
					claims: ProfileClaims::redacted().into_view(),
					error: AccountProfileRuntimeError::ProductStateUnavailable,
				},
		};
		let refresh_error = match status {
			Some(status) if status.account_revision == account.revision => status.refresh_error,
			Some(_) => Some(AccountProfileRuntimeError::AccountChanged),
			None => Some(AccountProfileRuntimeError::ProviderUnavailable),
		};
		let snapshot = match self.store.read_account_profile(account_id).await {
			Ok(Some(snapshot)) => snapshot,
			Ok(None) =>
				return self
					.unavailable(
						account_id,
						include_email,
						refresh_error.unwrap_or(AccountProfileRuntimeError::ProviderUnavailable),
					)
					.await,
			Err(_) =>
				return self
					.unavailable(
						account_id,
						include_email,
						AccountProfileRuntimeError::ProductStateUnavailable,
					)
					.await,
		};
		if snapshot.account_id != *account_id || snapshot.account_revision != account.revision {
			return self
				.unavailable(account_id, include_email, AccountProfileRuntimeError::AccountChanged)
				.await;
		}

		let profile = self.profile_view(snapshot, include_email).await;
		match refresh_error {
			Some(refresh_error) => AccountProfileRuntimeResult::Cached { profile, refresh_error },
			None => AccountProfileRuntimeResult::Current(profile),
		}
	}

	#[allow(clippy::too_many_lines)] // Keep one bounded credential, provider, persistence, and readback sequence auditable.
	pub(crate) async fn query(
		&self,
		account_id: &AccountId,
		include_email: bool,
	) -> AccountProfileRuntimeResult {
		let account = match self.store.read_account_registry(Some(account_id), 1).await {
			Ok(accounts) => match accounts.into_iter().next() {
				Some(account) if !account.tombstoned => account,
				_ =>
					return AccountProfileRuntimeResult::Unavailable {
						claims: ProfileClaims::redacted().into_view(),
						error: AccountProfileRuntimeError::AccountUnavailable,
					},
			},
			Err(_) =>
				return AccountProfileRuntimeResult::Unavailable {
					claims: ProfileClaims::redacted().into_view(),
					error: AccountProfileRuntimeError::ProductStateUnavailable,
				},
		};
		let Some(binding) = account.credential.as_ref() else {
			return self
				.cached(
					account_id,
					include_email,
					AccountProfileRuntimeError::CredentialUnavailable,
				)
				.await;
		};
		let stored = match self.credentials.read_exact(account_id, binding) {
			Ok(stored) => stored,
			Err(_) =>
				return self
					.cached(
						account_id,
						include_email,
						AccountProfileRuntimeError::CredentialUnavailable,
					)
					.await,
		};
		let provider = binding.provider.clone();
		let Some(observer) = self.provider.as_ref().map(Arc::clone) else {
			return self
				.cached(account_id, include_email, AccountProfileRuntimeError::ProviderUnavailable)
				.await;
		};
		let observed =
			tokio::task::spawn_blocking(move || observer.observe(&stored, &provider)).await;
		let profile = match observed {
			Ok(Ok(profile)) => profile,
			Ok(Err(error)) =>
				return self.cached(account_id, include_email, map_provider_error(error)).await,
			Err(_) =>
				return self
					.cached(
						account_id,
						include_email,
						AccountProfileRuntimeError::ProviderUnavailable,
					)
					.await,
		};
		let observed_at_unix_micros = match unix_micros() {
			Some(value) => value,
			None =>
				return self
					.cached(
						account_id,
						include_email,
						AccountProfileRuntimeError::ProviderUnavailable,
					)
					.await,
		};
		let observation = AccountProfileObservation {
			account_id: account_id.clone(),
			account_revision: account.revision,
			provider: binding.provider.clone(),
			observed_at_unix_micros,
			display_name: profile.display_name,
			username: profile.username,
			lifetime_tokens: profile.lifetime_tokens,
			peak_daily_tokens: profile.peak_daily_tokens,
			longest_task_seconds: profile.longest_task_seconds,
			current_streak_days: profile.current_streak_days,
			longest_streak_days: profile.longest_streak_days,
			daily_usage: profile.daily_usage,
		};
		match self.store.observe_account_profile(&observation).await {
			Ok(AccountProfileObservationOutcome::Observed)
			| Ok(AccountProfileObservationOutcome::StaleObservation) => {
				match self.store.read_account_profile(account_id).await {
					Ok(Some(snapshot)) => AccountProfileRuntimeResult::Current(
						self.profile_view(snapshot, include_email).await,
					),
					Ok(None) =>
						self.unavailable(
							account_id,
							include_email,
							AccountProfileRuntimeError::ProductStateUnavailable,
						)
						.await,
					Err(_) =>
						self.unavailable(
							account_id,
							include_email,
							AccountProfileRuntimeError::ProductStateUnavailable,
						)
						.await,
				}
			},
			Ok(AccountProfileObservationOutcome::AccountUnavailable) =>
				self.cached(
					account_id,
					include_email,
					AccountProfileRuntimeError::AccountUnavailable,
				)
				.await,
			Ok(AccountProfileObservationOutcome::StaleAccount) =>
				self.cached(account_id, include_email, AccountProfileRuntimeError::AccountChanged)
					.await,
			Err(_) =>
				self.cached(
					account_id,
					include_email,
					AccountProfileRuntimeError::ProductStateUnavailable,
				)
				.await,
		}
	}

	async fn cached(
		&self,
		account_id: &AccountId,
		include_email: bool,
		refresh_error: AccountProfileRuntimeError,
	) -> AccountProfileRuntimeResult {
		match self.store.read_account_profile(account_id).await {
			Ok(Some(snapshot)) => AccountProfileRuntimeResult::Cached {
				profile: self.profile_view(snapshot, include_email).await,
				refresh_error,
			},
			Ok(None) => self.unavailable(account_id, include_email, refresh_error).await,
			Err(_) =>
				self.unavailable(
					account_id,
					include_email,
					AccountProfileRuntimeError::ProductStateUnavailable,
				)
				.await,
		}
	}

	async fn profile_view(
		&self,
		snapshot: AccountProfileSnapshot,
		include_email: bool,
	) -> AccountProfileView {
		let claims = self
			.current_claims(
				&snapshot.account_id,
				include_email,
				Some(snapshot.account_revision),
				Some(&snapshot.provider),
			)
			.await
			.unwrap_or_else(ProfileClaims::redacted);
		claims.attach(snapshot)
	}

	async fn unavailable(
		&self,
		account_id: &AccountId,
		include_email: bool,
		error: AccountProfileRuntimeError,
	) -> AccountProfileRuntimeResult {
		let claims = self
			.current_claims(account_id, include_email, None, None)
			.await
			.unwrap_or_else(ProfileClaims::redacted);
		AccountProfileRuntimeResult::Unavailable { claims: claims.into_view(), error }
	}

	async fn current_claims(
		&self,
		account_id: &AccountId,
		include_email: bool,
		expected_revision: Option<i64>,
		expected_provider: Option<&ProviderIdentity>,
	) -> Option<ProfileClaims> {
		let account =
			self.store.read_account_registry(Some(account_id), 1).await.ok()?.into_iter().next()?;
		if account.account_id != *account_id || account.tombstoned {
			return None;
		}
		let revision = account.revision;
		let binding = account.credential?;
		if !claims_source_matches(expected_revision, expected_provider, revision, &binding.provider)
		{
			return None;
		}
		let stored = self.credentials.read_exact(account_id, &binding).ok()?;
		if stored.binding() != &binding {
			return None;
		}
		let confirmed =
			self.store.read_account_registry(Some(account_id), 1).await.ok()?.into_iter().next()?;
		if confirmed.account_id != *account_id
			|| confirmed.tombstoned
			|| confirmed.revision != revision
			|| confirmed.credential.as_ref() != Some(&binding)
		{
			return None;
		}
		Some(ProfileClaims::from_stored(&stored, include_email))
	}
}

fn claims_source_matches(
	expected_revision: Option<i64>,
	expected_provider: Option<&ProviderIdentity>,
	current_revision: i64,
	current_provider: &ProviderIdentity,
) -> bool {
	expected_revision.is_none_or(|revision| revision == current_revision)
		&& expected_provider.is_none_or(|provider| provider == current_provider)
}

struct ProfileClaims {
	email: Option<String>,
	plan_type: Option<String>,
}
impl ProfileClaims {
	fn from_stored(stored: &StoredCredential, include_email: bool) -> Self {
		let email = include_email
			.then(|| normalized_bounded_text(stored.bundle().provider_email(), 320))
			.flatten();
		let plan_type =
			stored.bundle().plan_type().and_then(|value| normalized_bounded_text(value, 128));
		Self { email, plan_type }
	}

	const fn redacted() -> Self {
		Self { email: None, plan_type: None }
	}

	fn attach(self, snapshot: AccountProfileSnapshot) -> AccountProfileView {
		AccountProfileView { snapshot, email: self.email, plan_type: self.plan_type }
	}

	fn into_view(self) -> AccountProfileClaimsView {
		AccountProfileClaimsView { email: self.email, plan_type: self.plan_type }
	}
}

fn decode_provider_profile(body: &[u8]) -> Result<ProviderProfile, ProviderProfileError> {
	let payload: Value =
		serde_json::from_slice(body).map_err(|_| ProviderProfileError::Protocol)?;
	let object = payload.as_object().ok_or(ProviderProfileError::Protocol)?;
	let profile = optional_object(object.get("profile"))?;
	let stats = optional_object(object.get("stats"))?;
	let display_name =
		profile.map(|value| optional_text(value.get("display_name"), 256)).transpose()?.flatten();
	let username =
		profile.map(|value| optional_text(value.get("username"), 256)).transpose()?.flatten();
	let lifetime_tokens = stats
		.map(|value| optional_nonnegative_i64(value.get("lifetime_tokens")))
		.transpose()?
		.flatten();
	let mut peak_daily_tokens = stats
		.map(|value| optional_nonnegative_i64(value.get("peak_daily_tokens")))
		.transpose()?
		.flatten();
	let longest_task_seconds = stats
		.map(|value| optional_nonnegative_i64(value.get("longest_running_turn_sec")))
		.transpose()?
		.flatten();
	let current_streak_days = stats
		.map(|value| optional_nonnegative_i32(value.get("current_streak_days")))
		.transpose()?
		.flatten();
	let longest_streak_days = stats
		.map(|value| optional_nonnegative_i32(value.get("longest_streak_days")))
		.transpose()?
		.flatten();
	let mut daily_usage = match stats.and_then(|value| value.get("daily_usage_buckets")) {
		None | Some(Value::Null) => Vec::new(),
		Some(Value::Array(values)) =>
			values.iter().map(decode_daily_usage).collect::<Result<Vec<_>, _>>()?,
		Some(_) => return Err(ProviderProfileError::Protocol),
	};
	daily_usage.sort_by(|left, right| left.start_date.cmp(&right.start_date));
	if daily_usage.windows(2).any(|values| values[0].start_date == values[1].start_date) {
		return Err(ProviderProfileError::Protocol);
	}
	if peak_daily_tokens.is_none() {
		peak_daily_tokens = daily_usage.iter().map(|fact| fact.tokens).max();
	}
	if daily_usage.len() > MAX_DAILY_USAGE {
		daily_usage = daily_usage.split_off(daily_usage.len() - MAX_DAILY_USAGE);
	}
	let result = ProviderProfile {
		display_name,
		username,
		lifetime_tokens,
		peak_daily_tokens,
		longest_task_seconds,
		current_streak_days,
		longest_streak_days,
		daily_usage,
	};
	if result.display_name.is_none()
		&& result.username.is_none()
		&& result.lifetime_tokens.is_none()
		&& result.peak_daily_tokens.is_none()
		&& result.longest_task_seconds.is_none()
		&& result.current_streak_days.is_none()
		&& result.longest_streak_days.is_none()
		&& result.daily_usage.is_empty()
	{
		return Err(ProviderProfileError::Protocol);
	}
	Ok(result)
}

fn optional_object(
	value: Option<&Value>,
) -> Result<Option<&serde_json::Map<String, Value>>, ProviderProfileError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Object(value)) => Ok(Some(value)),
		Some(_) => Err(ProviderProfileError::Protocol),
	}
}

fn optional_text(
	value: Option<&Value>,
	maximum: usize,
) -> Result<Option<String>, ProviderProfileError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
		Some(Value::String(value)) =>
			normalized_bounded_text(value, maximum).ok_or(ProviderProfileError::Protocol).map(Some),
		Some(_) => Err(ProviderProfileError::Protocol),
	}
}

fn optional_nonnegative_i64(value: Option<&Value>) -> Result<Option<i64>, ProviderProfileError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Number(number)) => {
			let value = number
				.as_i64()
				.filter(|value| *value >= 0)
				.or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
				.ok_or(ProviderProfileError::Protocol)?;
			Ok(Some(value))
		},
		Some(_) => Err(ProviderProfileError::Protocol),
	}
}

fn optional_nonnegative_i32(value: Option<&Value>) -> Result<Option<i32>, ProviderProfileError> {
	optional_nonnegative_i64(value)?
		.map(|value| i32::try_from(value).map_err(|_| ProviderProfileError::Protocol))
		.transpose()
}

fn decode_daily_usage(value: &Value) -> Result<AccountProfileDailyUsage, ProviderProfileError> {
	let value = value.as_object().ok_or(ProviderProfileError::Protocol)?;
	let start_date = optional_text(value.get("start_date"), 10)?
		.filter(|value| canonical_calendar_date(value))
		.ok_or(ProviderProfileError::Protocol)?;
	let tokens =
		optional_nonnegative_i64(value.get("tokens"))?.ok_or(ProviderProfileError::Protocol)?;
	Ok(AccountProfileDailyUsage { start_date, tokens })
}

fn normalized_bounded_text(value: &str, maximum: usize) -> Option<String> {
	let value = value.trim();
	(!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control))
		.then(|| value.to_owned())
}

fn canonical_calendar_date(value: &str) -> bool {
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
	let maximum = match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		2 if leap => 29,
		2 => 28,
		_ => return false,
	};
	year > 0 && (1..=maximum).contains(&day)
}

fn unix_micros() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
		.filter(|value| *value > 0)
}

const fn map_provider_error(error: ProviderProfileError) -> AccountProfileRuntimeError {
	match error {
		ProviderProfileError::Unauthorized => AccountProfileRuntimeError::Unauthorized,
		ProviderProfileError::Unavailable => AccountProfileRuntimeError::ProviderUnavailable,
		ProviderProfileError::Protocol => AccountProfileRuntimeError::ProtocolUnavailable,
	}
}

#[cfg(test)]
mod tests {
	use decodex_core::{AccountProvider, ProviderIdentity};

	use super::{
		ProviderProfileError, canonical_calendar_date, claims_source_matches,
		decode_provider_profile,
	};

	#[test]
	fn final_claims_require_the_exact_snapshot_revision_and_provider() {
		let first = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-1").unwrap();
		let second = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-2").unwrap();

		assert!(claims_source_matches(Some(7), Some(&first), 7, &first));
		assert!(!claims_source_matches(Some(7), Some(&first), 8, &first));
		assert!(!claims_source_matches(Some(7), Some(&first), 7, &second));
		assert!(claims_source_matches(None, None, 8, &second));
	}

	#[test]
	fn provider_profile_parser_bounds_sorts_and_derives_the_daily_peak() {
		let dates = (22..=30)
			.map(|day| format!("2026-06-{day:02}"))
			.chain((1..=31).map(|day| format!("2026-07-{day:02}")))
			.collect::<Vec<_>>();
		let buckets = dates
			.into_iter()
			.enumerate()
			.rev()
			.map(|(index, start_date)| {
				serde_json::json!({
					"start_date": start_date,
					"tokens": if index == 0 { 9_999 } else { index * 100 },
				})
			})
			.collect::<Vec<_>>();
		let payload = serde_json::to_vec(&serde_json::json!({
			"profile": {"display_name": "  Iris  ", "username": "iris"},
			"stats": {
				"lifetime_tokens": 123456,
				"longest_running_turn_sec": 900,
				"current_streak_days": 4,
				"longest_streak_days": 11,
				"daily_usage_buckets": buckets,
			},
			"ignored_future_field": true,
		}))
		.unwrap();
		let profile = decode_provider_profile(&payload).expect("bounded provider profile parses");

		assert_eq!(profile.display_name.as_deref(), Some("Iris"));
		assert_eq!(profile.daily_usage.len(), 36);
		assert_eq!(profile.daily_usage.first().unwrap().start_date, "2026-06-26");
		assert_eq!(profile.daily_usage.last().unwrap().start_date, "2026-07-31");
		assert_eq!(profile.peak_daily_tokens, Some(9_999));
	}

	#[test]
	fn provider_profile_parser_rejects_empty_duplicate_or_malformed_facts() {
		assert_eq!(
			decode_provider_profile(br#"{"profile":null,"stats":null}"#),
			Err(ProviderProfileError::Protocol),
		);
		let duplicate = br#"{
			"stats":{"daily_usage_buckets":[
				{"start_date":"2026-07-28","tokens":1},
				{"start_date":"2026-07-28","tokens":2}
			]}
		}"#;
		assert_eq!(decode_provider_profile(duplicate), Err(ProviderProfileError::Protocol));
		assert_eq!(
			decode_provider_profile(br#"{"profile":{"display_name":"   "}}"#),
			Err(ProviderProfileError::Protocol),
		);
		assert!(!canonical_calendar_date("2026-02-29"));
		assert!(canonical_calendar_date("2024-02-29"));
	}
}
