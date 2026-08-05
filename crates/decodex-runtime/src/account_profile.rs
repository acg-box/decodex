//! Independent bounded account-profile observation over the fixed ChatGPT endpoint.

use std::{
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};

use decodex_codex::AccountApiProfile;
use decodex_core::{AccountId, ProviderIdentity};
use decodex_postgres::{
	AccountProfileDailyUsage, AccountProfileObservation, AccountProfileObservationOutcome,
	AccountProfileSnapshot, PostgresStore,
};

use crate::host_credentials::{HostCredentialStore, StoredCredential};

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

/// Read and persist the profile projection produced by the shared backend API adapter.
///
/// `AccountObservationService` owns the daemon cadence and the provider request. This type only
/// persists the decoded result and serves the daemon-owned cache to clients.
#[derive(Clone)]
pub(crate) struct AccountProfileRuntime {
	store: PostgresStore,
	credentials: Arc<dyn HostCredentialStore>,
}
impl AccountProfileRuntime {
	/// Construct the cache projection over the shared provider API boundary.
	pub(crate) fn new(store: PostgresStore, credentials: Arc<dyn HostCredentialStore>) -> Self {
		Self { store, credentials }
	}

	/// Persist one already-decoded profile returned by the shared backend API adapter.
	pub(crate) async fn observe_provider_profile(
		&self,
		account_id: &AccountId,
		account_revision: i64,
		provider: ProviderIdentity,
		profile: AccountApiProfile,
	) -> AccountProfileRefreshStatus {
		let observed_at_unix_micros = match unix_micros() {
			Some(value) => value,
			None => {
				return self
					.status_after_failure(
						account_id,
						account_revision,
						AccountProfileRuntimeError::ProductStateUnavailable,
					)
					.await;
			},
		};
		let observation = AccountProfileObservation {
			account_id: account_id.clone(),
			account_revision,
			provider,
			observed_at_unix_micros,
			display_name: profile.display_name,
			username: profile.username,
			lifetime_tokens: profile.lifetime_tokens,
			peak_daily_tokens: profile.peak_daily_tokens,
			longest_task_seconds: profile.longest_task_seconds,
			current_streak_days: profile.current_streak_days,
			longest_streak_days: profile.longest_streak_days,
			daily_usage: profile
				.daily_usage
				.into_iter()
				.map(|usage| AccountProfileDailyUsage {
					start_date: usage.start_date,
					tokens: usage.tokens,
				})
				.collect(),
		};
		match self.store.observe_account_profile(&observation).await {
			Ok(AccountProfileObservationOutcome::Observed)
			| Ok(AccountProfileObservationOutcome::StaleObservation) =>
				self.read_status(account_id, account_revision, None).await,
			Ok(AccountProfileObservationOutcome::AccountUnavailable)
			| Ok(AccountProfileObservationOutcome::StaleAccount) =>
				self.status_after_failure(
					account_id,
					account_revision,
					AccountProfileRuntimeError::AccountChanged,
				)
				.await,
			Err(_) =>
				self.status_after_failure(
					account_id,
					account_revision,
					AccountProfileRuntimeError::ProductStateUnavailable,
				)
				.await,
		}
	}

	/// Convert an independent provider failure into a cache status without discarding the last
	/// successful snapshot.
	pub(crate) async fn status_after_failure(
		&self,
		account_id: &AccountId,
		requested_revision: i64,
		error: AccountProfileRuntimeError,
	) -> AccountProfileRefreshStatus {
		self.read_status(account_id, requested_revision, Some(error)).await
	}

	async fn read_status(
		&self,
		account_id: &AccountId,
		requested_revision: i64,
		refresh_error: Option<AccountProfileRuntimeError>,
	) -> AccountProfileRefreshStatus {
		match self.store.read_account_profile(account_id).await {
			Ok(Some(snapshot)) if snapshot.account_revision == requested_revision =>
				AccountProfileRefreshStatus {
					account_revision: snapshot.account_revision,
					refresh_error,
					snapshot: Some(snapshot),
				},
			Ok(Some(snapshot)) if refresh_error.is_none() => AccountProfileRefreshStatus {
				account_revision: snapshot.account_revision,
				refresh_error: Some(AccountProfileRuntimeError::AccountChanged),
				snapshot: Some(snapshot),
			},
			_ => AccountProfileRefreshStatus {
				account_revision: requested_revision,
				refresh_error: refresh_error
					.or(Some(AccountProfileRuntimeError::ProductStateUnavailable)),
				snapshot: None,
			},
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

fn normalized_bounded_text(value: &str, maximum: usize) -> Option<String> {
	let value = value.trim();
	(!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control))
		.then(|| value.to_owned())
}

fn unix_micros() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
		.filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
	use decodex_core::{AccountProvider, ProviderIdentity};

	use super::claims_source_matches;

	#[test]
	fn final_claims_require_the_exact_snapshot_revision_and_provider() {
		let first = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-1").unwrap();
		let second = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-2").unwrap();

		assert!(claims_source_matches(Some(7), Some(&first), 7, &first));
		assert!(!claims_source_matches(Some(7), Some(&first), 8, &first));
		assert!(!claims_source_matches(Some(7), Some(&first), 7, &second));
		assert!(claims_source_matches(None, None, 8, &second));
	}
}
