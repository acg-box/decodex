use std::{
	path::PathBuf,
	sync::{Mutex, OnceLock},
	thread,
};

use time::OffsetDateTime;

use crate::{
	agent::codex_accounts::{
		AccountPoolRecord, CodexAccountAuthFailure, CodexAccountPool,
		refresh::RefreshStatus,
		usage::{self, AccountUsageSnapshot},
	},
	prelude::{Result, eyre},
	state::CodexAccountActivitySummary,
};

const ACCOUNT_ACTIVITY_CACHE_TTL_SECONDS: i64 = 60;
const ACCOUNT_ACTIVITY_PROBE_MAX_CONCURRENCY: usize = 8;

static ACCOUNT_ACTIVITY_CACHE: OnceLock<Mutex<Option<AccountActivityCacheEntry>>> = OnceLock::new();

impl CodexAccountPool {
	pub(crate) fn account_activity_summaries(&self) -> Result<Vec<CodexAccountActivitySummary>> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.probe_account_activity_summaries(&mut records)
	}

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

	fn probe_account_activity_summaries(
		&self,
		records: &mut [AccountPoolRecord],
	) -> Result<Vec<CodexAccountActivitySummary>> {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut summaries_by_index = vec![None; records.len()];
		let mut probe_inputs = Vec::new();
		let mut records_changed = false;

		for (index, record) in records.iter().enumerate() {
			let Some(configured_summary) = record.configured_activity_summary(now) else {
				continue;
			};

			if configured_summary.status != "available" {
				summaries_by_index[index] = Some(configured_summary);

				continue;
			}

			probe_inputs.push(AccountActivityProbeInput { index, record: record.clone() });
		}
		for chunk in probe_inputs.chunks(ACCOUNT_ACTIVITY_PROBE_MAX_CONCURRENCY) {
			let results = self.probe_account_activity_summaries_parallel_chunk(chunk, now)?;

			for result in results {
				if result.records_changed {
					records[result.index] = result.record;
					records_changed = true;
				}

				summaries_by_index[result.index] = Some(result.summary);
			}
		}

		if records_changed {
			self.save_records(records)?;
		}

		Ok(summaries_by_index.into_iter().flatten().collect())
	}

	fn probe_account_activity_summaries_parallel_chunk(
		&self,
		inputs: &[AccountActivityProbeInput],
		now: i64,
	) -> Result<Vec<AccountActivityProbeResult>> {
		thread::scope(|scope| {
			let handles = inputs
				.iter()
				.cloned()
				.map(|input| scope.spawn(move || self.probe_account_activity_record(input, now)))
				.collect::<Vec<_>>();
			let mut results = Vec::with_capacity(handles.len());

			for handle in handles {
				let result = handle
					.join()
					.map_err(|_| eyre::eyre!("Codex account usage probe worker panicked."))??;

				results.push(result);
			}

			Ok(results)
		})
	}

	fn probe_account_activity_record(
		&self,
		input: AccountActivityProbeInput,
		now: i64,
	) -> Result<AccountActivityProbeResult> {
		let mut record = input.record;
		let mut records_changed = false;
		let refresh_status = match self.proactive_refresh_record(&mut record, now) {
			Ok(status) => {
				if status == RefreshStatus::Succeeded {
					records_changed = true;
				}

				status.as_str()
			},
			Err(error) if error.auth_failed => {
				records_changed = true;

				let summary = record.auth_failed_activity_summary(now);

				return Ok(AccountActivityProbeResult {
					index: input.index,
					record,
					summary,
					records_changed,
				});
			},
			Err(error) if error.requires_skip => {
				let summary = record.probe_failed_activity_summary(now, "failed", &error.source);

				return Ok(AccountActivityProbeResult {
					index: input.index,
					record,
					summary,
					records_changed,
				});
			},
			Err(_error) => "failed",
		};
		let summary = match self.probe_record_usage(&record) {
			Ok(usage) => self.activity_summary_from_usage_probe(&record, usage, refresh_status)?,
			Err(error) if error.unauthorized && record.refresh_token().is_some() => {
				match self.refresh_record(&mut record) {
					Ok(()) => {
						records_changed = true;

						match self.probe_record_usage(&record) {
							Ok(usage) =>
								self.activity_summary_from_usage_probe(&record, usage, "succeeded")?,
							Err(retry_error) =>
								record.probe_failed_activity_summary(now, "failed", &retry_error),
						}
					},
					Err(refresh_error) => {
						if refresh_error.downcast_ref::<CodexAccountAuthFailure>().is_some() {
							records_changed = true;

							record.auth_failed_activity_summary(now)
						} else {
							record.probe_failed_activity_summary(
								now,
								"failed",
								refresh_error.as_ref(),
							)
						}
					},
				}
			},
			Err(error) => record.probe_failed_activity_summary(now, "probe_failed", &error),
		};

		Ok(AccountActivityProbeResult { index: input.index, record, summary, records_changed })
	}

	fn activity_summary_from_usage_probe(
		&self,
		record: &AccountPoolRecord,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> Result<CodexAccountActivitySummary> {
		let profile = self.probe_record_profile(record).ok().flatten();

		record.activity_summary_from_usage_profile(usage, profile, refresh_status)
	}
}

#[derive(Clone, Eq, PartialEq)]
struct AccountActivityCacheKey {
	path: PathBuf,
	usage_endpoint: String,
	profile_endpoint: Option<String>,
	refresh_endpoint: String,
}

#[derive(Clone)]
struct AccountActivityCacheEntry {
	key: AccountActivityCacheKey,
	checked_at_unix_epoch: i64,
	summaries: Vec<CodexAccountActivitySummary>,
}

#[derive(Clone)]
struct AccountActivityProbeInput {
	index: usize,
	record: AccountPoolRecord,
}

struct AccountActivityProbeResult {
	index: usize,
	record: AccountPoolRecord,
	summary: CodexAccountActivitySummary,
	records_changed: bool,
}
