use std::sync::{Mutex, OnceLock};

use time::OffsetDateTime;

use crate::{
	agent::codex_accounts::{
		CodexAccountPool,
		activity::model::{AccountActivityCacheEntry, AccountActivityCacheKey},
		usage,
	},
	prelude::{Result, eyre},
	state::CodexAccountActivitySummary,
};

const ACCOUNT_ACTIVITY_CACHE_TTL_SECONDS: i64 = 60;

static ACCOUNT_ACTIVITY_CACHE: OnceLock<Mutex<Option<AccountActivityCacheEntry>>> = OnceLock::new();

impl CodexAccountPool {
	pub(crate) fn account_activity_summaries_cached(
		&self,
		force_refresh: bool,
	) -> Result<Vec<CodexAccountActivitySummary>> {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let cache_key = self.cache_key();
		let cache = ACCOUNT_ACTIVITY_CACHE.get_or_init(|| Mutex::new(None));

		if !force_refresh {
			let cached = cache
				.lock()
				.map_err(|error| eyre::eyre!("Codex account usage cache is poisoned: {error}"))?;

			if let Some(entry) = cached.as_ref()
				&& entry.key == cache_key
				&& now.saturating_sub(entry.checked_at_unix_epoch)
					< ACCOUNT_ACTIVITY_CACHE_TTL_SECONDS
			{
				return Ok(entry.summaries.clone());
			}
		}

		let mut summaries = self.account_activity_summaries()?;
		let mut cached = cache
			.lock()
			.map_err(|error| eyre::eyre!("Codex account usage cache is poisoned: {error}"))?;

		if let Some(entry) = cached.as_ref()
			&& entry.key == cache_key
		{
			usage::preserve_cached_usage_windows(&mut summaries, &entry.summaries, now);
		}

		*cached = Some(AccountActivityCacheEntry {
			key: cache_key,
			checked_at_unix_epoch: now,
			summaries: summaries.clone(),
		});

		Ok(summaries)
	}

	pub(crate) fn account_activity_summaries_snapshot(
		&self,
	) -> Result<Vec<CodexAccountActivitySummary>> {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let cache_key = self.cache_key();
		let cache = ACCOUNT_ACTIVITY_CACHE.get_or_init(|| Mutex::new(None));
		let cached = cache
			.lock()
			.map_err(|error| eyre::eyre!("Codex account usage cache is poisoned: {error}"))?;

		if let Some(entry) = cached.as_ref()
			&& entry.key == cache_key
		{
			return Ok(entry.summaries.clone());
		}

		drop(cached);

		let _guard = self.lock_records()?;
		let records = self.load_records()?;

		Ok(records.iter().filter_map(|record| record.configured_activity_summary(now)).collect())
	}

	fn cache_key(&self) -> AccountActivityCacheKey {
		AccountActivityCacheKey {
			path: self.path.clone(),
			usage_endpoint: self.usage_endpoint.clone(),
			profile_endpoint: self.profile_endpoint.clone(),
			refresh_endpoint: self.refresh_endpoint.clone(),
		}
	}
}
