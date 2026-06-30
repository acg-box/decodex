#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	env,
	error::Error,
	fs::{self, File, OpenOptions},
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
	sync::{Mutex, OnceLock},
	thread,
	time::Duration,
};

use color_eyre::Report;
use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ProjectCodexAccountsConfig, prelude::eyre, runtime, state::CodexAccountActivitySummary,
};

mod auth_failure;
mod login;
mod refresh;
mod selection;
#[cfg(test)] mod tests;
mod usage;

#[cfg(test)] pub(crate) use self::usage::{CreditsSnapshot, UsageWindow};
pub(crate) use self::{
	auth_failure::CodexAccountAuthFailure, login::CodexAccountLogin,
	refresh::ProactiveRefreshReason,
};
use self::{
	refresh::{
		CodexTokenData, ProactiveRefreshError, RefreshRequest, RefreshResponse, RefreshStatus,
		ReportableRefreshError, jwt_email_claim, jwt_expiration_unix_epoch, rfc3339_unix_epoch,
		token_refresh_auth_status,
	},
	selection::{account_summaries, compare_account_candidates},
	usage::{
		AccountProfileSnapshot, AccountUsageSnapshot, UsageProbeError, nonblank_string,
		preserve_cached_usage_windows, profile_snapshot_from_payload, usage_snapshot_from_payload,
	},
};

const DEFAULT_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const DEFAULT_PROFILE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/profiles/me";
const DEFAULT_REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_USER_AGENT: &str = "codex-cli";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_REFRESH_INTERVAL_SECONDS: i64 = 8 * 24 * 60 * 60;
const ACCOUNT_ACTIVITY_CACHE_TTL_SECONDS: i64 = 60;
const ACCOUNT_ACTIVITY_PROBE_MAX_CONCURRENCY: usize = 8;

static ACCOUNT_ACTIVITY_CACHE: OnceLock<Mutex<Option<AccountActivityCacheEntry>>> = OnceLock::new();

pub(crate) trait CodexAccountProvider {
	fn select_account(&self) -> crate::prelude::Result<CodexAccountLogin>;
	fn refresh_account(
		&self,
		previous_account_id: Option<&str>,
	) -> crate::prelude::Result<CodexAccountLogin>;
}

pub(crate) struct CodexAccountPool {
	path: PathBuf,
	usage_endpoint: String,
	profile_endpoint: Option<String>,
	refresh_endpoint: String,
	fixed_account: Option<String>,
	codex_auth_path: PathBuf,
	client: Client,
	selected_account_id: Mutex<Option<String>>,
}
impl CodexAccountPool {
	pub(crate) fn from_config(config: &ProjectCodexAccountsConfig) -> crate::prelude::Result<Self> {
		let fixed_account = runtime::global_fixed_account_selector()?;
		let usage_endpoint = config.usage_endpoint().unwrap_or(DEFAULT_USAGE_ENDPOINT);

		Self::new_with_fixed_account_and_profile_endpoint(
			runtime::accounts_path()?,
			usage_endpoint,
			config.profile_endpoint(),
			config.refresh_endpoint().unwrap_or(DEFAULT_REFRESH_ENDPOINT),
			fixed_account.as_deref(),
		)
	}

	pub(crate) fn from_accounts_path(path: impl AsRef<Path>) -> crate::prelude::Result<Self> {
		Self::new_with_fixed_account(path, DEFAULT_USAGE_ENDPOINT, DEFAULT_REFRESH_ENDPOINT, None)
	}

	fn new_with_fixed_account(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
	) -> crate::prelude::Result<Self> {
		Self::new_with_fixed_account_and_profile_endpoint(
			path,
			usage_endpoint,
			None,
			refresh_endpoint,
			fixed_account,
		)
	}

	fn new_with_fixed_account_and_profile_endpoint(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		profile_endpoint: Option<&str>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
	) -> crate::prelude::Result<Self> {
		Self::new_with_fixed_account_profile_and_codex_auth_path(
			path,
			usage_endpoint,
			profile_endpoint,
			refresh_endpoint,
			fixed_account,
			default_codex_auth_json_path()?,
		)
	}

	#[cfg(test)]
	fn new_with_fixed_account_and_codex_auth_path(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
		codex_auth_path: impl Into<PathBuf>,
	) -> crate::prelude::Result<Self> {
		Self::new_with_fixed_account_profile_and_codex_auth_path(
			path,
			usage_endpoint,
			None,
			refresh_endpoint,
			fixed_account,
			codex_auth_path,
		)
	}

	fn new_with_fixed_account_profile_and_codex_auth_path(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		profile_endpoint: Option<&str>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
		codex_auth_path: impl Into<PathBuf>,
	) -> crate::prelude::Result<Self> {
		let client = Client::builder().timeout(HTTP_TIMEOUT).build()?;
		let usage_endpoint = usage_endpoint.into();
		let profile_endpoint = profile_endpoint
			.and_then(|endpoint| nonblank_string(Some(endpoint)))
			.or_else(|| default_profile_endpoint_for_usage_endpoint(&usage_endpoint));

		Ok(Self {
			path: path.as_ref().to_path_buf(),
			usage_endpoint,
			profile_endpoint,
			refresh_endpoint: refresh_endpoint.into(),
			fixed_account: fixed_account
				.map(str::trim)
				.filter(|selector| !selector.is_empty())
				.map(str::to_owned),
			codex_auth_path: codex_auth_path.into(),
			client,
			selected_account_id: Mutex::new(None),
		})
	}

	fn load_records(&self) -> crate::prelude::Result<Vec<AccountPoolRecord>> {
		let input = fs::read_to_string(&self.path).map_err(|error| {
			eyre::eyre!("Failed to read Codex accounts `{}`: {error}", self.path.display())
		})?;

		parse_account_records(&input, &self.path)
	}

	pub(crate) fn account_activity_summaries(
		&self,
	) -> crate::prelude::Result<Vec<CodexAccountActivitySummary>> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.probe_account_activity_summaries(&mut records)
	}

	pub(crate) fn account_activity_summaries_cached(
		&self,
		force_refresh: bool,
	) -> crate::prelude::Result<Vec<CodexAccountActivitySummary>> {
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
			preserve_cached_usage_windows(&mut summaries, &entry.summaries, now);
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
	) -> crate::prelude::Result<Vec<CodexAccountActivitySummary>> {
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

	fn lock_records(&self) -> crate::prelude::Result<AccountPoolFileLock> {
		let parent = self.path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Codex accounts path `{}` must have a parent directory.",
				self.path.display()
			)
		})?;
		let file_name = self
			.path
			.file_name()
			.and_then(|name| name.to_str())
			.ok_or_else(|| eyre::eyre!("Codex accounts path must end in a valid file name."))?;
		let lock_path = parent.join(format!(".{file_name}.lock"));
		let file = OpenOptions::new()
			.read(true)
			.write(true)
			.create(true)
			.truncate(false)
			.open(&lock_path)
			.map_err(|error| {
				eyre::eyre!("Failed to open Codex accounts lock `{}`: {error}", lock_path.display())
			})?;

		file.lock().map_err(|error| {
			eyre::eyre!("Failed to lock Codex accounts `{}`: {error}", self.path.display())
		})?;

		Ok(AccountPoolFileLock { _file: file })
	}

	fn save_records(&self, records: &[AccountPoolRecord]) -> crate::prelude::Result<()> {
		let parent = self.path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Codex accounts path `{}` must have a parent directory.",
				self.path.display()
			)
		})?;
		let file_name = self
			.path
			.file_name()
			.and_then(|name| name.to_str())
			.ok_or_else(|| eyre::eyre!("Codex accounts path must end in a valid file name."))?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::write(&temp_path, body)?;
		fs::rename(temp_path, &self.path)?;

		Ok(())
	}

	fn select_from_records(
		&self,
		records: &mut [AccountPoolRecord],
	) -> crate::prelude::Result<CodexAccountLogin> {
		if let Some(selector) = self.fixed_account.as_deref() {
			return self.select_fixed_from_records(records, selector);
		}

		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut candidates = Vec::new();
		let mut skipped = Vec::new();
		let mut records_changed = false;

		for (index, record) in records.iter_mut().enumerate() {
			match self.account_candidate_from_record(
				record,
				index + 1,
				now,
				&mut records_changed,
			)? {
				Ok(candidate) => candidates.push(candidate),
				Err(reason) => skipped.push(reason),
			}
		}

		if records_changed {
			self.save_records(records)?;
		}
		if candidates.is_empty() {
			if let Some(auth_failure) =
				records.iter().find_map(AccountPoolRecord::auth_failed_error)
			{
				return Err(Report::new(auth_failure));
			}

			eyre::bail!(
				"No usable Codex account was available from `{}`. Skipped entries: {}",
				self.path.display(),
				if skipped.is_empty() { String::from("none") } else { skipped.join("; ") }
			);
		}

		candidates.sort_by(compare_account_candidates);

		let mut selected = candidates.remove(0);

		selected.mark_selected(now);

		if let Some(record) =
			records.iter_mut().find(|record| record.account_id() == Some(selected.account_id()))
		{
			record.last_selected_at_unix_epoch = Some(now);
			records_changed = true;
		}

		let account_summaries = account_summaries(&selected, &candidates);
		let selected = selected.with_account_summaries(account_summaries);

		if records_changed {
			self.save_records(records)?;
		}

		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}

	fn select_fixed_from_records(
		&self,
		records: &mut [AccountPoolRecord],
		selector: &str,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let record_index = self.fixed_record_index(records, selector)?;
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut records_changed = false;

		if let Some(auth_failure) = records[record_index].auth_failed_error() {
			return Err(Report::new(auth_failure));
		}

		let mut selected = match self.account_candidate_from_record(
			&mut records[record_index],
			record_index + 1,
			now,
			&mut records_changed,
		)? {
			Ok(candidate) => candidate,
			Err(reason) => {
				eyre::bail!(
					"Configured Codex fixed account `{selector}` from `{}` is not usable: {reason}",
					self.path.display()
				);
			},
		};

		selected.mark_selected(now);

		records[record_index].last_selected_at_unix_epoch = Some(now);
		records_changed = true;

		let account_summaries = account_summaries(&selected, &[]);
		let selected = selected.with_account_summaries(account_summaries);

		if records_changed {
			self.save_records(records)?;
		}

		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}

	fn account_candidate_from_record(
		&self,
		record: &mut AccountPoolRecord,
		line_number: usize,
		now: i64,
		records_changed: &mut bool,
	) -> crate::prelude::Result<std::result::Result<CodexAccountLogin, String>> {
		if record.disabled {
			return Ok(Err(format!("line {line_number} disabled")));
		}

		if let Some(auth_failure) = record.auth_failure() {
			return Ok(Err(format!("line {line_number} auth failed: {auth_failure}")));
		}

		if record.cooldown_until_unix_epoch.is_some_and(|cooldown| cooldown > now) {
			return Ok(Err(format!("line {line_number} cooling down")));
		}
		if record.account_id().is_none() {
			return Ok(Err(format!("line {line_number} missing account id")));
		}
		if record.access_token().is_none() {
			return Ok(Err(format!("line {line_number} missing access token")));
		}

		let refresh_status = match self.proactive_refresh_record(record, now) {
			Ok(status) => {
				if status == RefreshStatus::Succeeded {
					*records_changed = true;
				}

				status.as_str()
			},
			Err(error) if error.auth_failed => {
				*records_changed = true;

				return Ok(Err(format!("{} auth failed: {}", record.display_name(), error.source)));
			},
			Err(error) if error.requires_skip => {
				return Ok(Err(format!(
					"{} proactive refresh failed: {}",
					record.display_name(),
					error.source
				)));
			},
			Err(_error) => RefreshStatus::Failed.as_str(),
		};

		match self.probe_record_usage(record) {
			Ok(usage) => Ok(Ok(record.login_from_usage(usage, refresh_status)?)),
			Err(error) if error.unauthorized && record.refresh_token().is_some() => {
				if let Err(refresh_error) = self.refresh_record(record) {
					if let Some(auth_failure) =
						refresh_error.downcast_ref::<CodexAccountAuthFailure>()
					{
						*records_changed = true;

						return Ok(Err(format!(
							"{} auth failed: {auth_failure}",
							record.display_name()
						)));
					}

					return Err(refresh_error);
				}

				*records_changed = true;

				let usage = self.probe_record_usage(record).map_err(|retry_error| {
					eyre::eyre!(
						"Codex account `{}` refreshed but usage probe still failed: {retry_error}",
						record.display_name()
					)
				})?;

				Ok(Ok(record.login_from_usage(usage, "succeeded")?))
			},
			Err(error) => Ok(Err(format!("{} usage probe failed: {error}", record.display_name()))),
		}
	}

	fn probe_account_activity_summaries(
		&self,
		records: &mut [AccountPoolRecord],
	) -> crate::prelude::Result<Vec<CodexAccountActivitySummary>> {
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
	) -> crate::prelude::Result<Vec<AccountActivityProbeResult>> {
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
	) -> crate::prelude::Result<AccountActivityProbeResult> {
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
	) -> crate::prelude::Result<CodexAccountActivitySummary> {
		let profile = self.probe_record_profile(record).ok().flatten();

		record.activity_summary_from_usage_profile(usage, profile, refresh_status)
	}

	fn proactive_refresh_record(
		&self,
		record: &mut AccountPoolRecord,
		now_unix_epoch: i64,
	) -> std::result::Result<RefreshStatus, ProactiveRefreshError> {
		let Some(reason) = record.proactive_refresh_reason(now_unix_epoch) else {
			return Ok(RefreshStatus::NotNeeded);
		};

		if record.refresh_token().is_none() {
			return Err(ProactiveRefreshError {
				source: ReportableRefreshError::new(format!("missing refresh token for {reason}")),
				requires_skip: reason.requires_valid_token(),
				auth_failed: false,
			});
		}

		self.refresh_record(record).map(|()| RefreshStatus::Succeeded).map_err(|error| {
			let auth_failed = error.downcast_ref::<CodexAccountAuthFailure>().is_some();

			ProactiveRefreshError {
				source: ReportableRefreshError::new(error.to_string()),
				requires_skip: reason.requires_valid_token(),
				auth_failed,
			}
		})
	}

	fn refresh_from_records(
		&self,
		records: &mut [AccountPoolRecord],
		previous_account_id: Option<&str>,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let record_index = if let Some(selector) = self.fixed_account.as_deref() {
			self.fixed_record_index(records, selector)?
		} else {
			let selected_account_id = self.selected_account_id()?;
			let target_account_id = previous_account_id.or(selected_account_id.as_deref());

			records
				.iter()
				.position(|record| {
					target_account_id.is_none_or(|target| record.account_id() == Some(target))
				})
				.ok_or_else(|| {
					eyre::eyre!(
						"Codex account refresh requested an account that is not in the configured accounts."
					)
				})?
		};

		if let Some(auth_failure) = records[record_index].auth_failed_error() {
			return Err(Report::new(auth_failure));
		}
		if let Err(error) = self.refresh_record(&mut records[record_index]) {
			if error.downcast_ref::<CodexAccountAuthFailure>().is_some() {
				self.save_records(records)?;
			}

			return Err(error);
		}

		let usage = self.probe_record_usage(&records[record_index])?;
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut selected = records[record_index].login_from_usage(usage, "succeeded")?;

		selected.mark_selected(now);

		records[record_index].last_selected_at_unix_epoch = Some(now);

		let selected_summary = selected.summary().clone();
		let selected = selected.with_account_summaries(vec![selected_summary]);

		self.save_records(records)?;
		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}

	fn fixed_record_index(
		&self,
		records: &[AccountPoolRecord],
		selector: &str,
	) -> crate::prelude::Result<usize> {
		let matches = records
			.iter()
			.enumerate()
			.filter_map(|(index, record)| {
				record.matches_account_selector(selector).then_some(index)
			})
			.collect::<Vec<_>>();

		match matches.as_slice() {
			[] => eyre::bail!(
				"Configured Codex fixed account `{selector}` does not match any account in `{}`.",
				self.path.display()
			),
			[index] => Ok(*index),
			_ => eyre::bail!(
				"Configured Codex fixed account `{selector}` matched multiple accounts in `{}`.",
				self.path.display()
			),
		}
	}

	fn probe_record_usage(
		&self,
		record: &AccountPoolRecord,
	) -> std::result::Result<AccountUsageSnapshot, UsageProbeError> {
		let access_token = record
			.access_token()
			.ok_or_else(|| UsageProbeError::other("account is missing an access token"))?;
		let account_id = record
			.account_id()
			.ok_or_else(|| UsageProbeError::other("account is missing an account id"))?;
		let response = self
			.client
			.get(&self.usage_endpoint)
			.bearer_auth(access_token)
			.header("ChatGPT-Account-Id", account_id)
			.header("User-Agent", CODEX_USER_AGENT)
			.send()
			.map_err(|error| UsageProbeError::other(error.to_string()))?;
		let status = response.status();

		if status == StatusCode::UNAUTHORIZED {
			return Err(UsageProbeError::unauthorized());
		}
		if !status.is_success() {
			return Err(UsageProbeError::other(format!("usage endpoint returned {status}")));
		}

		let payload = response.json::<Value>().map_err(|error| {
			UsageProbeError::other(format!("usage JSON did not parse: {error}"))
		})?;

		Ok(usage_snapshot_from_payload(&payload, OffsetDateTime::now_utc().unix_timestamp()))
	}

	fn probe_record_profile(
		&self,
		record: &AccountPoolRecord,
	) -> std::result::Result<Option<AccountProfileSnapshot>, UsageProbeError> {
		let Some(profile_endpoint) = self.profile_endpoint.as_deref() else {
			return Ok(None);
		};
		let access_token = record
			.access_token()
			.ok_or_else(|| UsageProbeError::other("account is missing an access token"))?;
		let account_id = record
			.account_id()
			.ok_or_else(|| UsageProbeError::other("account is missing an account id"))?;
		let response = self
			.client
			.get(profile_endpoint)
			.bearer_auth(access_token)
			.header("ChatGPT-Account-Id", account_id)
			.header("User-Agent", CODEX_USER_AGENT)
			.send()
			.map_err(|error| UsageProbeError::other(error.to_string()))?;
		let status = response.status();

		if status == StatusCode::UNAUTHORIZED {
			return Err(UsageProbeError::unauthorized());
		}
		if !status.is_success() {
			return Err(UsageProbeError::other(format!("profile endpoint returned {status}")));
		}

		let payload = response.json::<Value>().map_err(|error| {
			UsageProbeError::other(format!("profile JSON did not parse: {error}"))
		})?;

		Ok(profile_snapshot_from_payload(&payload, OffsetDateTime::now_utc().unix_timestamp()))
	}

	fn refresh_record(&self, record: &mut AccountPoolRecord) -> crate::prelude::Result<()> {
		let display_name = record.display_name();
		let refresh_token = record
			.refresh_token()
			.ok_or_else(|| {
				eyre::eyre!(
					"Codex account `{}` cannot refresh because no refresh token is present.",
					display_name
				)
			})?
			.to_owned();
		let response = self
			.client
			.post(&self.refresh_endpoint)
			.header("Content-Type", "application/json")
			.json(&RefreshRequest {
				client_id: CHATGPT_OAUTH_CLIENT_ID,
				grant_type: "refresh_token",
				refresh_token,
			})
			.send()?;
		let status = response.status();

		if !status.is_success() {
			let reason = format!(
				"Codex account `{}` token refresh failed with HTTP {status}.",
				display_name
			);

			if token_refresh_auth_status(status) {
				record.mark_auth_failed(OffsetDateTime::now_utc().unix_timestamp(), reason.clone());

				return Err(Report::new(CodexAccountAuthFailure::from_record(record, reason)));
			}

			eyre::bail!("{reason}");
		}

		let refresh_response = response.json::<RefreshResponse>()?;
		let tokens = record.tokens.as_mut().ok_or_else(|| {
			eyre::eyre!("Codex account `{display_name}` is missing token storage.")
		})?;

		if let Some(id_token) = refresh_response.id_token {
			tokens.id_token = Some(id_token);
		}
		if let Some(access_token) = refresh_response.access_token {
			tokens.access_token = access_token;
		}
		if let Some(refresh_token) = refresh_response.refresh_token {
			tokens.refresh_token = refresh_token;
		}

		if tokens.access_token.trim().is_empty() {
			eyre::bail!(
				"Codex account `{}` token refresh did not produce a usable access token.",
				display_name
			);
		}

		record.last_refresh = Some(OffsetDateTime::now_utc().format(&Rfc3339)?);

		record.clear_auth_failed();
		self.sync_codex_auth_for_refreshed_record(record)?;

		Ok(())
	}

	fn sync_codex_auth_for_refreshed_record(
		&self,
		record: &AccountPoolRecord,
	) -> crate::prelude::Result<()> {
		sync_refreshed_record_to_codex_auth(record, &self.codex_auth_path)
	}

	fn remember_selected_account(&self, account_id: &str) -> crate::prelude::Result<()> {
		let mut selected = self
			.selected_account_id
			.lock()
			.map_err(|_| eyre::eyre!("Codex accounts selection lock was poisoned."))?;

		*selected = Some(account_id.to_owned());

		Ok(())
	}

	fn selected_account_id(&self) -> crate::prelude::Result<Option<String>> {
		self.selected_account_id
			.lock()
			.map(|selected| selected.clone())
			.map_err(|_| eyre::eyre!("Codex accounts selection lock was poisoned."))
	}
}

impl CodexAccountProvider for CodexAccountPool {
	fn select_account(&self) -> crate::prelude::Result<CodexAccountLogin> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.select_from_records(&mut records)
	}

	fn refresh_account(
		&self,
		previous_account_id: Option<&str>,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.refresh_from_records(&mut records, previous_account_id)
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

struct AccountPoolFileLock {
	_file: File,
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
	fn display_name(&self) -> String {
		self.email()
			.or_else(|| self.account_id().map(redact_account_id))
			.unwrap_or_else(|| String::from("unnamed account"))
	}

	fn account_fingerprint(&self) -> Option<String> {
		self.account_id().map(redact_account_id).or_else(|| self.email())
	}

	fn auth_failure(&self) -> Option<&str> {
		self.auth_failure
			.as_deref()
			.map(str::trim)
			.filter(|failure| !failure.is_empty())
			.or_else(|| self.auth_failed_at_unix_epoch.map(|_| "authentication failed"))
	}

	fn auth_failed_error(&self) -> Option<CodexAccountAuthFailure> {
		self.auth_failure().map(|reason| CodexAccountAuthFailure::from_record(self, reason))
	}

	fn mark_auth_failed(&mut self, now_unix_epoch: i64, reason: impl Into<String>) {
		self.auth_failed_at_unix_epoch = Some(now_unix_epoch);
		self.auth_failure = Some(reason.into());
	}

	fn clear_auth_failed(&mut self) {
		self.auth_failed_at_unix_epoch = None;
		self.auth_failure = None;
	}

	fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(redact_account_id).as_deref() == Some(selector)
	}

	fn access_token(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.access_token.as_str())
			.filter(|token| !token.trim().is_empty())
	}

	fn refresh_token(&self) -> Option<String> {
		self.tokens
			.as_ref()
			.map(|tokens| tokens.refresh_token.as_str())
			.filter(|token| !token.trim().is_empty())
			.map(str::to_owned)
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

	fn auth_dot_json(&self) -> AuthDotJson {
		AuthDotJson {
			email: self.email(),
			auth_mode: self.auth_mode.clone(),
			openai_api_key: self.openai_api_key.clone(),
			tokens: self.tokens.clone(),
			last_refresh: self.last_refresh.clone(),
		}
	}

	fn configured_activity_summary(
		&self,
		now_unix_epoch: i64,
	) -> Option<CodexAccountActivitySummary> {
		let account_fingerprint = self.account_fingerprint()?;
		let status = if self.disabled {
			"disabled"
		} else if self.auth_failure().is_some() {
			"auth_failed"
		} else if self
			.cooldown_until_unix_epoch
			.is_some_and(|cooldown_until| cooldown_until > now_unix_epoch)
		{
			"cooldown"
		} else if self.access_token().is_none() {
			"unusable"
		} else {
			"available"
		};

		Some(CodexAccountActivitySummary {
			account_fingerprint,
			email: self.email(),
			status: String::from(status),
			refresh_status: if self.auth_failure().is_some() {
				String::from("auth_failed")
			} else {
				String::from("not_checked")
			},
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(
				self.auth_failure()
					.map_or_else(|| String::from("configured account"), ToOwned::to_owned),
			),
			..CodexAccountActivitySummary::default()
		})
	}

	fn login_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let access_token = self
			.access_token()
			.ok_or_else(|| {
				eyre::eyre!("Codex account `{}` is missing an access token.", self.display_name())
			})?
			.to_owned();
		let account_id = self
			.account_id()
			.ok_or_else(|| {
				eyre::eyre!("Codex account `{}` is missing an account id.", self.display_name())
			})?
			.to_owned();
		let summary = CodexAccountActivitySummary {
			account_fingerprint: redact_account_id(&account_id),
			email: self.email(),
			plan_type: usage.plan_type.clone(),
			status: if usage.is_limited() {
				String::from("usage_limited")
			} else {
				String::from("available")
			},
			refresh_status: refresh_status.to_owned(),
			checked_at_unix_epoch: Some(usage.checked_at_unix_epoch),
			selected_at_unix_epoch: None,
			primary_window_seconds: usage.primary.as_ref().and_then(|window| window.window_seconds),
			primary_remaining_percent: usage
				.primary
				.as_ref()
				.map(|window| window.remaining_percent),
			primary_resets_at_unix_epoch: usage
				.primary
				.as_ref()
				.and_then(|window| window.resets_at_unix_epoch),
			secondary_window_seconds: usage
				.secondary
				.as_ref()
				.and_then(|window| window.window_seconds),
			secondary_remaining_percent: usage
				.secondary
				.as_ref()
				.map(|window| window.remaining_percent),
			secondary_resets_at_unix_epoch: usage
				.secondary
				.as_ref()
				.and_then(|window| window.resets_at_unix_epoch),
			credits_has_credits: usage.credits.as_ref().map(|credits| credits.has_credits),
			credits_unlimited: usage.credits.as_ref().map(|credits| credits.unlimited),
			credits_balance: usage.credits.and_then(|credits| credits.balance),
			rate_limit_reached_type: usage.rate_limit_reached_type,
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(String::from("usage probe ok")),
			..CodexAccountActivitySummary::default()
		};

		Ok(CodexAccountLogin {
			access_token,
			account_id,
			plan_type: summary.plan_type.clone(),
			last_selected_at_unix_epoch: self.last_selected_at_unix_epoch,
			summary,
			account_summaries: Vec::new(),
		})
	}

	fn activity_summary_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountActivitySummary> {
		Ok(self.login_from_usage(usage, refresh_status)?.summary)
	}

	fn activity_summary_from_usage_profile(
		&self,
		usage: AccountUsageSnapshot,
		profile: Option<AccountProfileSnapshot>,
		refresh_status: &str,
	) -> crate::prelude::Result<CodexAccountActivitySummary> {
		let mut summary = self.activity_summary_from_usage(usage, refresh_status)?;

		if let Some(profile) = profile {
			profile.apply_to_summary(&mut summary);
		}

		Ok(summary)
	}

	fn probe_failed_activity_summary(
		&self,
		now_unix_epoch: i64,
		refresh_status: &str,
		error: &(dyn Error + '_),
	) -> CodexAccountActivitySummary {
		let mut summary = self.configured_activity_summary(now_unix_epoch).unwrap_or_default();

		summary.status = if refresh_status == "failed" {
			String::from("unusable")
		} else {
			String::from("probe_failed")
		};
		summary.refresh_status = refresh_status.to_owned();
		summary.note = Some(format!("usage probe failed: {error}"));

		summary
	}

	fn auth_failed_activity_summary(&self, now_unix_epoch: i64) -> CodexAccountActivitySummary {
		let mut summary = self.configured_activity_summary(now_unix_epoch).unwrap_or_default();

		summary.status = String::from("auth_failed");
		summary.refresh_status = String::from("auth_failed");
		summary.note = self.auth_failure().map(ToOwned::to_owned);

		summary
	}

	fn proactive_refresh_reason(&self, now_unix_epoch: i64) -> Option<ProactiveRefreshReason> {
		let tokens = self.tokens.as_ref()?;

		if let Some(expires_at) = jwt_expiration_unix_epoch(&tokens.access_token) {
			return (expires_at <= now_unix_epoch)
				.then_some(ProactiveRefreshReason::AccessTokenExpired);
		}

		let last_refresh = self.last_refresh.as_deref().and_then(rfc3339_unix_epoch)?;

		(last_refresh < now_unix_epoch.saturating_sub(TOKEN_REFRESH_INTERVAL_SECONDS))
			.then_some(ProactiveRefreshReason::LastRefreshStale)
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
	fn into_record(self) -> AccountPoolRecord {
		match self {
			Self::Flat(record) => record,
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth,
			} => AccountPoolRecord {
				email: first_nonblank_string(email, auth.email),
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth_mode: auth.auth_mode,
				openai_api_key: auth.openai_api_key,
				tokens: auth.tokens,
				last_refresh: auth.last_refresh,
			},
		}
	}
}

fn parse_account_records(
	input: &str,
	path: &Path,
) -> crate::prelude::Result<Vec<AccountPoolRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let parsed = serde_json::from_str::<AccountPoolLine>(trimmed).map_err(|error| {
			eyre::eyre!(
				"Codex accounts `{}` line {line_number} is not a valid auth JSONL entry: {error}",
				path.display()
			)
		})?;

		records.push(parsed.into_record());
	}

	if records.is_empty() {
		eyre::bail!("Codex accounts `{}` does not contain any account records.", path.display());
	}

	Ok(records)
}

fn default_codex_auth_json_path() -> crate::prelude::Result<PathBuf> {
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

fn default_profile_endpoint_for_usage_endpoint(usage_endpoint: &str) -> Option<String> {
	(usage_endpoint == DEFAULT_USAGE_ENDPOINT).then(|| DEFAULT_PROFILE_ENDPOINT.to_owned())
}

fn sync_refreshed_record_to_codex_auth(
	record: &AccountPoolRecord,
	path: &Path,
) -> crate::prelude::Result<()> {
	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(_) => return Ok(()),
	};
	let auth = match serde_json::from_str::<AuthDotJson>(&input) {
		Ok(auth) => auth,
		Err(_) => return Ok(()),
	};
	let auth_account_id = auth
		.tokens
		.as_ref()
		.and_then(|tokens| tokens.account_id.as_deref())
		.filter(|account_id| !account_id.trim().is_empty());

	if auth_account_id != record.account_id() {
		return Ok(());
	}

	write_auth_json_atomically(path, &record.auth_dot_json())
}

fn write_auth_json_atomically(path: &Path, auth: &AuthDotJson) -> crate::prelude::Result<()> {
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

fn secure_account_file(path: &Path) -> crate::prelude::Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

fn first_nonblank_string(left: Option<String>, right: Option<String>) -> Option<String> {
	left.filter(|value| !value.trim().is_empty())
		.or_else(|| right.filter(|value| !value.trim().is_empty()))
}

fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

const fn is_false(value: &bool) -> bool {
	!*value
}
