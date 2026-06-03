#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	cmp::Ordering,
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs::{self, File, OpenOptions},
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
	sync::{Mutex, OnceLock},
	thread,
	time::Duration,
};

use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ProjectCodexAccountsConfig,
	prelude::eyre,
	runtime,
	state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary},
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
				self.refresh_record(record)?;

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
					Err(refresh_error) =>
						record.probe_failed_activity_summary(now, "failed", refresh_error.as_ref()),
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
			});
		}

		self.refresh_record(record).map(|()| RefreshStatus::Succeeded).map_err(|error| {
			ProactiveRefreshError {
				source: ReportableRefreshError::new(error.to_string()),
				requires_skip: reason.requires_valid_token(),
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

		self.refresh_record(&mut records[record_index])?;

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
			eyre::bail!(
				"Codex account `{}` token refresh failed with HTTP {status}.",
				display_name
			);
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

pub(crate) struct CodexAccountLogin {
	access_token: String,
	account_id: String,
	plan_type: Option<String>,
	last_selected_at_unix_epoch: Option<i64>,
	summary: CodexAccountActivitySummary,
	account_summaries: Vec<CodexAccountActivitySummary>,
}
impl CodexAccountLogin {
	pub(crate) fn access_token(&self) -> &str {
		&self.access_token
	}

	pub(crate) fn account_id(&self) -> &str {
		&self.account_id
	}

	pub(crate) fn plan_type(&self) -> Option<&str> {
		self.plan_type.as_deref()
	}

	pub(crate) fn summary(&self) -> &CodexAccountActivitySummary {
		&self.summary
	}

	pub(crate) fn account_summaries(&self) -> &[CodexAccountActivitySummary] {
		&self.account_summaries
	}

	fn mark_selected(&mut self, selected_at_unix_epoch: i64) {
		if self.summary.status == "available" {
			self.summary.status = String::from("selected");
		}

		self.summary.selected_at_unix_epoch = Some(selected_at_unix_epoch);
	}

	fn with_account_summaries(
		mut self,
		account_summaries: Vec<CodexAccountActivitySummary>,
	) -> Self {
		self.account_summaries = account_summaries;

		self
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
		let account_fingerprint =
			self.account_id().map(redact_account_id).or_else(|| self.email())?;
		let status = if self.disabled {
			"disabled"
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
			refresh_status: String::from("not_checked"),
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(String::from("configured account")),
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

#[derive(Serialize)]
struct RefreshRequest {
	client_id: &'static str,
	grant_type: &'static str,
	refresh_token: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
	id_token: Option<String>,
	access_token: Option<String>,
	refresh_token: Option<String>,
}

#[derive(Debug)]
struct ProactiveRefreshError {
	source: ReportableRefreshError,
	requires_skip: bool,
}

#[derive(Debug)]
struct ReportableRefreshError {
	message: String,
}
impl ReportableRefreshError {
	fn new(message: String) -> Self {
		Self { message }
	}
}

impl Display for ReportableRefreshError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl Error for ReportableRefreshError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AccountUsageSnapshot {
	plan_type: Option<String>,
	primary: Option<UsageWindow>,
	secondary: Option<UsageWindow>,
	credits: Option<CreditsSnapshot>,
	rate_limit_reached_type: Option<String>,
	checked_at_unix_epoch: i64,
}
impl AccountUsageSnapshot {
	fn is_limited(&self) -> bool {
		self.rate_limit_reached_type.is_some()
			|| self.primary.as_ref().is_some_and(UsageWindow::is_depleted)
			|| self.secondary.as_ref().is_some_and(UsageWindow::is_depleted)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AccountProfileSnapshot {
	display_name: Option<String>,
	username: Option<String>,
	lifetime_tokens: Option<i64>,
	peak_daily_tokens: Option<i64>,
	longest_task_seconds: Option<i64>,
	current_streak_days: Option<i64>,
	longest_streak_days: Option<i64>,
	daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	checked_at_unix_epoch: i64,
}
impl AccountProfileSnapshot {
	fn is_empty(&self) -> bool {
		self.display_name.is_none()
			&& self.username.is_none()
			&& self.lifetime_tokens.is_none()
			&& self.peak_daily_tokens.is_none()
			&& self.longest_task_seconds.is_none()
			&& self.current_streak_days.is_none()
			&& self.longest_streak_days.is_none()
			&& self.daily_usage.is_empty()
	}

	fn apply_to_summary(self, summary: &mut CodexAccountActivitySummary) {
		summary.profile_display_name = self.display_name;
		summary.profile_username = self.username;
		summary.profile_checked_at_unix_epoch = Some(self.checked_at_unix_epoch);
		summary.profile_lifetime_tokens = self.lifetime_tokens;
		summary.profile_peak_daily_tokens = self.peak_daily_tokens;
		summary.profile_longest_task_seconds = self.longest_task_seconds;
		summary.profile_current_streak_days = self.current_streak_days;
		summary.profile_longest_streak_days = self.longest_streak_days;
		summary.profile_daily_usage = self.daily_usage;
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsageWindow {
	window_seconds: Option<i64>,
	remaining_percent: i64,
	resets_at_unix_epoch: Option<i64>,
}
impl UsageWindow {
	const fn is_depleted(&self) -> bool {
		self.remaining_percent <= 0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CreditsSnapshot {
	has_credits: bool,
	unlimited: bool,
	balance: Option<String>,
}

#[derive(Debug)]
struct UsageProbeError {
	unauthorized: bool,
	message: String,
}
impl UsageProbeError {
	fn unauthorized() -> Self {
		Self { unauthorized: true, message: String::from("usage endpoint returned 401") }
	}

	fn other(message: impl Into<String>) -> Self {
		Self { unauthorized: false, message: message.into() }
	}
}

impl Display for UsageProbeError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl Error for UsageProbeError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AccountCandidateScore {
	not_limited: bool,
	bottleneck_remaining_percent: i64,
	combined_remaining_score: i64,
	primary_remaining_percent: i64,
	secondary_remaining_percent: i64,
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
	fn into_record(self) -> AccountPoolRecord {
		match self {
			Self::Flat(record) => record,
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth,
			} => AccountPoolRecord {
				email: first_nonblank_string(email, auth.email),
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_mode: auth.auth_mode,
				openai_api_key: auth.openai_api_key,
				tokens: auth.tokens,
				last_refresh: auth.last_refresh,
			},
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefreshStatus {
	NotNeeded,
	Succeeded,
	Failed,
}
impl RefreshStatus {
	const fn as_str(self) -> &'static str {
		match self {
			Self::NotNeeded => "not_needed",
			Self::Succeeded => "succeeded",
			Self::Failed => "failed",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProactiveRefreshReason {
	AccessTokenExpired,
	LastRefreshStale,
}
impl ProactiveRefreshReason {
	const fn requires_valid_token(self) -> bool {
		matches!(self, Self::AccessTokenExpired)
	}
}

impl Display for ProactiveRefreshReason {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::AccessTokenExpired => formatter.write_str("expired access token"),
			Self::LastRefreshStale => formatter.write_str("stale refresh timestamp"),
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

fn usage_snapshot_from_payload(
	payload: &Value,
	checked_at_unix_epoch: i64,
) -> AccountUsageSnapshot {
	let rate_limit = payload.get("rate_limit").filter(|value| !value.is_null());

	AccountUsageSnapshot {
		plan_type: payload.get("plan_type").and_then(json_scalar_to_string),
		primary: rate_limit
			.and_then(|details| usage_window_from_value(details.get("primary_window"))),
		secondary: rate_limit
			.and_then(|details| usage_window_from_value(details.get("secondary_window"))),
		credits: payload.get("credits").and_then(credits_from_value),
		rate_limit_reached_type: rate_limit_reached_type_from_payload(payload),
		checked_at_unix_epoch,
	}
}

fn profile_snapshot_from_payload(
	payload: &Value,
	checked_at_unix_epoch: i64,
) -> Option<AccountProfileSnapshot> {
	let profile = payload.get("profile").filter(|value| !value.is_null());
	let stats = payload.get("stats").filter(|value| !value.is_null());
	let daily_usage = stats
		.and_then(|value| value.get("daily_usage_buckets"))
		.and_then(Value::as_array)
		.map(|items| items.iter().filter_map(profile_daily_usage_from_value).collect::<Vec<_>>())
		.unwrap_or_default();
	let peak_daily_tokens = stats
		.and_then(|value| nonnegative_number_as_i64(value.get("peak_daily_tokens")))
		.or_else(|| daily_usage.iter().map(|record| record.tokens).max());
	let snapshot = AccountProfileSnapshot {
		display_name: profile.and_then(|value| nonblank_json_string(value.get("display_name"))),
		username: profile.and_then(|value| nonblank_json_string(value.get("username"))),
		lifetime_tokens: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("lifetime_tokens"))),
		peak_daily_tokens,
		longest_task_seconds: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("longest_running_turn_sec"))),
		current_streak_days: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("current_streak_days"))),
		longest_streak_days: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("longest_streak_days"))),
		daily_usage,
		checked_at_unix_epoch,
	};

	(!snapshot.is_empty()).then_some(snapshot)
}

fn profile_daily_usage_from_value(value: &Value) -> Option<CodexAccountProfileDailyUsageSummary> {
	let date = nonblank_json_string(value.get("start_date"))?;
	let tokens = nonnegative_number_as_i64(value.get("tokens"))?;

	Some(CodexAccountProfileDailyUsageSummary { date, tokens })
}

fn usage_window_from_value(value: Option<&Value>) -> Option<UsageWindow> {
	let value = value.filter(|value| !value.is_null())?;
	let used_percent = number_as_i64(value.get("used_percent")?)?;
	let window_seconds = value.get("limit_window_seconds").and_then(number_as_i64);

	if window_seconds.is_some_and(|seconds| seconds <= 0) {
		return None;
	}

	let remaining_percent = 100_i64.saturating_sub(used_percent).clamp(0, 100);

	Some(UsageWindow {
		window_seconds,
		remaining_percent,
		resets_at_unix_epoch: value.get("reset_at").and_then(number_as_i64),
	})
}

fn preserve_cached_usage_windows(
	summaries: &mut [CodexAccountActivitySummary],
	cached_summaries: &[CodexAccountActivitySummary],
	now_unix_epoch: i64,
) {
	for summary in summaries {
		if account_summary_is_limited(summary) {
			continue;
		}

		let Some(cached) =
			cached_summaries.iter().find(|cached| account_summaries_match(summary, cached))
		else {
			continue;
		};

		preserve_primary_usage_window(summary, cached, now_unix_epoch);
		preserve_secondary_usage_window(summary, cached, now_unix_epoch);
	}
}

fn preserve_primary_usage_window(
	summary: &mut CodexAccountActivitySummary,
	cached: &CodexAccountActivitySummary,
	now_unix_epoch: i64,
) {
	if has_usage_window(summary.primary_window_seconds, summary.primary_remaining_percent)
		|| !has_current_usage_window(
			cached.primary_window_seconds,
			cached.primary_remaining_percent,
			cached.primary_resets_at_unix_epoch,
			now_unix_epoch,
		) {
		return;
	}

	summary.primary_window_seconds = cached.primary_window_seconds;
	summary.primary_remaining_percent = cached.primary_remaining_percent;
	summary.primary_resets_at_unix_epoch = cached.primary_resets_at_unix_epoch;
}

fn preserve_secondary_usage_window(
	summary: &mut CodexAccountActivitySummary,
	cached: &CodexAccountActivitySummary,
	now_unix_epoch: i64,
) {
	if has_usage_window(summary.secondary_window_seconds, summary.secondary_remaining_percent)
		|| !has_current_usage_window(
			cached.secondary_window_seconds,
			cached.secondary_remaining_percent,
			cached.secondary_resets_at_unix_epoch,
			now_unix_epoch,
		) {
		return;
	}

	summary.secondary_window_seconds = cached.secondary_window_seconds;
	summary.secondary_remaining_percent = cached.secondary_remaining_percent;
	summary.secondary_resets_at_unix_epoch = cached.secondary_resets_at_unix_epoch;
}

const fn has_usage_window(window_seconds: Option<i64>, remaining_percent: Option<i64>) -> bool {
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

fn account_summaries_match(
	left: &CodexAccountActivitySummary,
	right: &CodexAccountActivitySummary,
) -> bool {
	left.account_fingerprint == right.account_fingerprint
		|| left
			.email
			.as_deref()
			.zip(right.email.as_deref())
			.is_some_and(|(left, right)| left == right)
}

fn credits_from_value(value: &Value) -> Option<CreditsSnapshot> {
	if value.is_null() {
		return None;
	}

	Some(CreditsSnapshot {
		has_credits: value.get("has_credits").and_then(Value::as_bool).unwrap_or(true),
		unlimited: value.get("unlimited").and_then(Value::as_bool).unwrap_or(false),
		balance: value.get("balance").and_then(json_scalar_to_string),
	})
}

fn rate_limit_reached_type_from_payload(payload: &Value) -> Option<String> {
	let reached = payload.get("rate_limit_reached_type").filter(|value| !value.is_null())?;

	if let Some(kind) = reached.get("kind").and_then(json_scalar_to_string) {
		return Some(kind);
	}

	json_scalar_to_string(reached)
}

fn number_as_i64(value: &Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn nonnegative_number_as_i64(value: Option<&Value>) -> Option<i64> {
	value.and_then(number_as_i64).map(|number| number.max(0))
}

fn json_scalar_to_string(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

fn nonblank_json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(json_scalar_to_string).and_then(|value| nonblank_string(Some(&value)))
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
	let claims = serde_json::from_slice::<Value>(&payload_bytes).ok()?;

	claims.get("email").and_then(json_scalar_to_string)
}

fn jwt_expiration_unix_epoch(jwt: &str) -> Option<i64> {
	let payload = jwt.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<Value>(&payload_bytes).ok()?;

	claims.get("exp").and_then(number_as_i64)
}

fn rfc3339_unix_epoch(input: &str) -> Option<i64> {
	OffsetDateTime::parse(input, &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
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

fn compare_account_candidates(left: &CodexAccountLogin, right: &CodexAccountLogin) -> Ordering {
	account_candidate_score(right)
		.cmp(&account_candidate_score(left))
		.then_with(|| {
			left.last_selected_at_unix_epoch
				.unwrap_or(0)
				.cmp(&right.last_selected_at_unix_epoch.unwrap_or(0))
		})
		.then_with(|| left.summary.account_fingerprint.cmp(&right.summary.account_fingerprint))
}

fn account_candidate_score(candidate: &CodexAccountLogin) -> AccountCandidateScore {
	let summary = candidate.summary();
	let primary = summary
		.primary_remaining_percent
		.unwrap_or_else(|| summary.secondary_remaining_percent.unwrap_or(0));
	let secondary = summary.secondary_remaining_percent.unwrap_or(primary);

	AccountCandidateScore {
		not_limited: !account_summary_is_limited(summary),
		bottleneck_remaining_percent: primary.min(secondary),
		combined_remaining_score: primary.saturating_mul(secondary),
		primary_remaining_percent: primary,
		secondary_remaining_percent: secondary,
	}
}

fn account_summary_is_limited(summary: &CodexAccountActivitySummary) -> bool {
	summary.rate_limit_reached_type.is_some()
		|| summary.status.to_lowercase().contains("limit")
		|| summary.primary_remaining_percent == Some(0)
		|| summary.secondary_remaining_percent == Some(0)
}

fn account_summaries(
	selected: &CodexAccountLogin,
	candidates: &[CodexAccountLogin],
) -> Vec<CodexAccountActivitySummary> {
	let mut summaries = Vec::with_capacity(candidates.len() + 1);

	summaries.push(selected.summary().clone());
	summaries.extend(candidates.iter().map(|candidate| candidate.summary().clone()));

	summaries
}

fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

const fn is_false(value: &bool) -> bool {
	!*value
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		io::{Read as _, Write as _},
		net::TcpListener,
		thread,
	};

	use tempfile::TempDir;

	use crate::agent::codex_accounts::{
		self, AccountPoolRecord, CodexAccountActivitySummary, CodexAccountLogin, CodexAccountPool,
		CodexAccountProvider, CodexTokenData, CreditsSnapshot, DEFAULT_REFRESH_ENDPOINT, Path,
		ProactiveRefreshReason, UsageWindow, compare_account_candidates,
	};

	#[test]
	fn accounts_accept_flat_and_wrapped_auth_jsonl_records() {
		let input = r#"
			{"email":"primary@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id","access_token":"access","refresh_token":"refresh","account_id":"acct_primary"}}
			{"auth":{"auth_mode":"chatgpt","tokens":{"id_token":"x.eyJlbWFpbCI6IndyYXBwZWRAZXhhbXBsZS5jb20ifQ.y","access_token":"access-2","refresh_token":"refresh-2","account_id":"acct_wrapped"}}}
		"#;
		let records =
			codex_accounts::parse_account_records(input, Path::new("/tmp/accounts.jsonl"))
				.expect("records should parse");

		assert_eq!(records.len(), 2);
		assert_eq!(records[0].account_id(), Some("acct_primary"));
		assert_eq!(records[0].email().as_deref(), Some("primary@example.com"));
		assert_eq!(records[1].account_id(), Some("acct_wrapped"));
		assert_eq!(records[1].email().as_deref(), Some("wrapped@example.com"));
	}

	#[test]
	fn account_selector_matches_email_full_id_and_fingerprint() {
		let record = AccountPoolRecord {
			email: Some(String::from("selected@example.com")),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_mode: Some(String::from("chatgpt")),
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from("access"),
				refresh_token: String::from("refresh"),
				account_id: Some(String::from("acct_fixed_123456")),
			}),
			last_refresh: None,
		};

		assert!(record.matches_account_selector("selected@example.com"));
		assert!(record.matches_account_selector("acct_fixed_123456"));
		assert!(record.matches_account_selector("...123456"));
		assert!(!record.matches_account_selector("other@example.com"));
	}

	#[test]
	fn fixed_account_selection_uses_configured_account_without_balancing() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let usage_endpoint = start_codex_usage_fixture_server(vec![
			r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":85},"secondary_window":{"used_percent":90}}}"#,
		]);

		fs::write(
			&accounts_path,
			r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
		)
		.expect("accounts fixture should write");

		let pool = CodexAccountPool::new_with_fixed_account(
			&accounts_path,
			usage_endpoint,
			DEFAULT_REFRESH_ENDPOINT,
			Some("copy@example.com"),
		)
		.expect("account pool should initialize");
		let account = pool.select_account().expect("fixed account should select");

		assert_eq!(account.account_id(), "acct_copy");
		assert_eq!(account.summary().email.as_deref(), Some("copy@example.com"));
		assert_eq!(account.account_summaries().len(), 1);
	}

	#[test]
	fn account_activity_snapshot_uses_configured_records_without_usage_probe() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");

		fs::write(
			&accounts_path,
			r#"{"email":"snapshot@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-snapshot","refresh_token":"refresh-snapshot","account_id":"acct_snapshot"}}"#,
		)
		.expect("accounts fixture should write");

		let pool = CodexAccountPool::new_with_fixed_account(
			&accounts_path,
			"http://127.0.0.1:9/usage",
			DEFAULT_REFRESH_ENDPOINT,
			None,
		)
		.expect("account pool should initialize");
		let summaries = pool.account_activity_summaries_snapshot().expect("snapshot should load");

		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].email.as_deref(), Some("snapshot@example.com"));
		assert_eq!(summaries[0].status, "available");
		assert_eq!(summaries[0].refresh_status, "not_checked");
		assert_eq!(summaries[0].primary_remaining_percent, None);
		assert_eq!(summaries[0].note.as_deref(), Some("configured account"));
	}

	#[test]
	fn usage_summary_parses_codex_rate_limit_payload() {
		let payload = serde_json::json!({
			"plan_type": "pro",
			"rate_limit": {
				"primary_window": {
					"used_percent": 42,
					"limit_window_seconds": 18_000,
					"reset_at": 1_800_018_000
				},
				"secondary_window": {
					"used_percent": 84,
					"limit_window_seconds": 604_800,
					"reset_at": 1_800_604_800
				}
			},
			"credits": {
				"has_credits": true,
				"unlimited": false,
				"balance": "9.99"
			},
			"rate_limit_reached_type": {
				"kind": "workspace_member_credits_depleted"
			}
		});
		let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

		assert_eq!(summary.plan_type.as_deref(), Some("pro"));
		assert_eq!(
			summary.primary,
			Some(UsageWindow {
				window_seconds: Some(18_000),
				remaining_percent: 58,
				resets_at_unix_epoch: Some(1_800_018_000),
			})
		);
		assert_eq!(
			summary.secondary,
			Some(UsageWindow {
				window_seconds: Some(604_800),
				remaining_percent: 16,
				resets_at_unix_epoch: Some(1_800_604_800),
			})
		);
		assert_eq!(
			summary.credits,
			Some(CreditsSnapshot {
				has_credits: true,
				unlimited: false,
				balance: Some(String::from("9.99")),
			})
		);
		assert_eq!(
			summary.rate_limit_reached_type.as_deref(),
			Some("workspace_member_credits_depleted")
		);
	}

	#[test]
	fn profile_summary_parses_codex_profile_payload() {
		let payload = serde_json::json!({
			"profile": {
				"display_name": "  Copy Account  ",
				"username": "copy"
			},
			"stats": {
				"lifetime_tokens": 47_200_000_000_i64,
				"peak_daily_tokens": 1_500_000_000_i64,
				"longest_running_turn_sec": 10_080,
				"current_streak_days": 12,
				"longest_streak_days": 68,
				"daily_usage_buckets": [
					{ "start_date": "2026-05-30", "tokens": 123_456 },
					{ "start_date": "2026-05-31", "tokens": 789_000 }
				]
			}
		});
		let summary = codex_accounts::profile_snapshot_from_payload(&payload, 1_800_000_000)
			.expect("profile summary should parse");

		assert_eq!(summary.display_name.as_deref(), Some("Copy Account"));
		assert_eq!(summary.username.as_deref(), Some("copy"));
		assert_eq!(summary.lifetime_tokens, Some(47_200_000_000));
		assert_eq!(summary.peak_daily_tokens, Some(1_500_000_000));
		assert_eq!(summary.longest_task_seconds, Some(10_080));
		assert_eq!(summary.current_streak_days, Some(12));
		assert_eq!(summary.longest_streak_days, Some(68));
		assert_eq!(summary.daily_usage.len(), 2);
		assert_eq!(summary.daily_usage[1].date, "2026-05-31");
		assert_eq!(summary.daily_usage[1].tokens, 789_000);
	}

	#[test]
	fn profile_summary_falls_back_to_daily_usage_peak() {
		let payload = serde_json::json!({
			"stats": {
				"daily_usage_buckets": [
					{ "start_date": "2026-05-30", "tokens": 123_456 },
					{ "start_date": "2026-05-31", "tokens": 789_000 }
				]
			}
		});
		let summary = codex_accounts::profile_snapshot_from_payload(&payload, 1_800_000_000)
			.expect("profile summary should parse from daily usage buckets");

		assert_eq!(summary.peak_daily_tokens, Some(789_000));
	}

	#[test]
	fn usage_limit_detects_depleted_windows_without_credit_heuristics() {
		let payload = serde_json::json!({
			"plan_type": "pro",
			"rate_limit": {
				"primary_window": {
					"used_percent": 0,
					"limit_window_seconds": 18_000,
					"reset_at": 1_800_018_000
				},
				"secondary_window": {
					"used_percent": 100,
					"limit_window_seconds": 604_800,
					"reset_at": 1_800_604_800
				}
			},
			"credits": {
				"has_credits": false,
				"unlimited": false,
				"balance": "0"
			},
			"rate_limit_reached_type": null
		});
		let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

		assert_eq!(summary.primary.as_ref().map(|window| window.remaining_percent), Some(100));
		assert_eq!(summary.secondary.as_ref().map(|window| window.remaining_percent), Some(0));
		assert_eq!(summary.credits.as_ref().map(|credits| credits.has_credits), Some(false));
		assert!(summary.is_limited());

		let record = AccountPoolRecord {
			email: Some(String::from("limited@example.com")),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_mode: Some(String::from("chatgpt")),
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from("access"),
				refresh_token: String::from("refresh"),
				account_id: Some(String::from("acct_limited")),
			}),
			last_refresh: None,
		};
		let login = record
			.login_from_usage(summary, "not_needed")
			.expect("limited usage should still produce an account summary");

		assert_eq!(login.summary().status, "usage_limited");

		let available_payload = serde_json::json!({
			"plan_type": "pro",
			"rate_limit": {
				"primary_window": {
					"used_percent": 40,
					"limit_window_seconds": 18_000,
					"reset_at": 1_800_018_000
				},
				"secondary_window": {
					"used_percent": 72,
					"limit_window_seconds": 604_800,
					"reset_at": 1_800_604_800
				}
			},
			"credits": {
				"has_credits": false,
				"unlimited": false,
				"balance": "0"
			},
			"rate_limit_reached_type": null
		});
		let available_summary =
			codex_accounts::usage_snapshot_from_payload(&available_payload, 1_800_000_000);

		assert_eq!(
			available_summary.primary.as_ref().map(|window| window.remaining_percent),
			Some(60)
		);
		assert_eq!(
			available_summary.secondary.as_ref().map(|window| window.remaining_percent),
			Some(28)
		);
		assert!(!available_summary.is_limited());
	}

	#[test]
	fn usage_summary_ignores_zero_second_placeholder_windows() {
		let payload = serde_json::json!({
			"plan_type": "pro",
			"rate_limit": {
				"primary_window": {
					"used_percent": 0,
					"limit_window_seconds": 0,
					"reset_at": 1_800_000_000
				},
				"secondary_window": null
			},
			"rate_limit_reached_type": null
		});
		let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

		assert_eq!(summary.primary, None);
		assert_eq!(summary.secondary, None);
		assert!(!summary.is_limited());

		let record = AccountPoolRecord {
			email: Some(String::from("placeholder@example.com")),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_mode: Some(String::from("chatgpt")),
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from("access"),
				refresh_token: String::from("refresh"),
				account_id: Some(String::from("acct_placeholder")),
			}),
			last_refresh: None,
		};
		let login = record
			.login_from_usage(summary, "not_needed")
			.expect("placeholder usage should still produce an account summary");

		assert_eq!(login.summary().status, "available");
		assert_eq!(login.summary().primary_window_seconds, None);
		assert_eq!(login.summary().primary_remaining_percent, None);
		assert_eq!(login.summary().primary_resets_at_unix_epoch, None);
	}

	#[test]
	fn usage_cache_preserves_current_windows_across_placeholder_refresh() {
		let now = time::OffsetDateTime::now_utc().unix_timestamp();
		let cached = [CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			status: String::from("available"),
			primary_window_seconds: Some(18_000),
			primary_remaining_percent: Some(72),
			primary_resets_at_unix_epoch: Some(now + 18_000),
			secondary_window_seconds: Some(604_800),
			secondary_remaining_percent: Some(91),
			secondary_resets_at_unix_epoch: Some(now + 604_800),
			..CodexAccountActivitySummary::default()
		}];
		let mut refreshed = [CodexAccountActivitySummary {
			account_fingerprint: String::from("...123456"),
			email: Some(String::from("copy@example.com")),
			status: String::from("available"),
			checked_at_unix_epoch: Some(now + 60),
			profile_lifetime_tokens: Some(47_200_000_000),
			..CodexAccountActivitySummary::default()
		}];

		codex_accounts::preserve_cached_usage_windows(&mut refreshed, &cached, now + 60);

		assert_eq!(refreshed[0].primary_window_seconds, Some(18_000));
		assert_eq!(refreshed[0].primary_remaining_percent, Some(72));
		assert_eq!(refreshed[0].primary_resets_at_unix_epoch, Some(now + 18_000));
		assert_eq!(refreshed[0].secondary_window_seconds, Some(604_800));
		assert_eq!(refreshed[0].secondary_remaining_percent, Some(91));
		assert_eq!(refreshed[0].secondary_resets_at_unix_epoch, Some(now + 604_800));
		assert_eq!(refreshed[0].profile_lifetime_tokens, Some(47_200_000_000));
	}

	#[test]
	fn proactive_refresh_prefers_access_token_expiration_then_last_refresh() {
		let mut record = AccountPoolRecord {
			email: Some(String::from("refresh@example.com")),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_mode: Some(String::from("chatgpt")),
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from("x.eyJleHAiOjEwMDB9.y"),
				refresh_token: String::from("refresh"),
				account_id: Some(String::from("acct_refresh")),
			}),
			last_refresh: Some(String::from("2099-01-01T00:00:00Z")),
		};

		assert_eq!(
			record.proactive_refresh_reason(1_001),
			Some(ProactiveRefreshReason::AccessTokenExpired)
		);

		record.tokens.as_mut().expect("tokens should exist").access_token = String::from("opaque");
		record.last_refresh = Some(String::from("2026-01-01T00:00:00Z"));

		assert_eq!(
			record.proactive_refresh_reason(1_768_000_000),
			Some(ProactiveRefreshReason::LastRefreshStale)
		);
	}

	#[test]
	fn token_refresh_syncs_matching_codex_auth_json() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let codex_auth_path = temp_dir.path().join(".codex/auth.json");
		let refresh_endpoint = start_codex_refresh_fixture_server(vec![
			r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
		]);
		let usage_endpoint = start_codex_usage_fixture_server(vec![
			r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
		]);

		fs::write(
			&accounts_path,
			r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
		)
		.expect("accounts fixture should write");
		fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
			.expect("auth parent should create");
		fs::write(
			&codex_auth_path,
			r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-current","access_token":"access-current","refresh_token":"refresh-current","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
		)
		.expect("Codex auth fixture should write");

		let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
			&accounts_path,
			usage_endpoint,
			refresh_endpoint,
			None,
			codex_auth_path.clone(),
		)
		.expect("account pool should initialize");
		let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

		assert_eq!(account.access_token(), "access-new");

		let accounts_input = fs::read_to_string(&accounts_path).expect("accounts should read");

		assert!(accounts_input.contains(r#""access_token":"access-new""#));
		assert!(accounts_input.contains(r#""refresh_token":"refresh-new""#));

		let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
		let codex_auth_json = serde_json::from_str::<serde_json::Value>(&codex_auth)
			.expect("Codex auth should parse");

		assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_sync");
		assert_eq!(codex_auth_json["tokens"]["id_token"], "id-new");
		assert_eq!(codex_auth_json["tokens"]["access_token"], "access-new");
		assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-new");
	}

	#[test]
	fn token_refresh_leaves_nonmatching_codex_auth_json_unchanged() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let codex_auth_path = temp_dir.path().join(".codex/auth.json");
		let refresh_endpoint = start_codex_refresh_fixture_server(vec![
			r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
		]);
		let usage_endpoint = start_codex_usage_fixture_server(vec![
			r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
		]);

		fs::write(
			&accounts_path,
			r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
		)
		.expect("accounts fixture should write");
		fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
			.expect("auth parent should create");
		fs::write(
			&codex_auth_path,
			r#"{"email":"other@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-other","access_token":"access-other","refresh_token":"refresh-other","account_id":"acct_other"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
		)
		.expect("Codex auth fixture should write");

		let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
			&accounts_path,
			usage_endpoint,
			refresh_endpoint,
			None,
			codex_auth_path.clone(),
		)
		.expect("account pool should initialize");
		let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

		assert_eq!(account.access_token(), "access-new");

		let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
		let codex_auth_json = serde_json::from_str::<serde_json::Value>(&codex_auth)
			.expect("Codex auth should parse");

		assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_other");
		assert_eq!(codex_auth_json["tokens"]["id_token"], "id-other");
		assert_eq!(codex_auth_json["tokens"]["access_token"], "access-other");
		assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-other");
	}

	#[test]
	fn account_candidate_sort_prefers_remaining_usage() {
		let mut candidates = [
			CodexAccountLogin {
				access_token: String::from("a"),
				account_id: String::from("acct_a"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: None,
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_a"),
					primary_remaining_percent: Some(10),
					secondary_remaining_percent: Some(90),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
			CodexAccountLogin {
				access_token: String::from("b"),
				account_id: String::from("acct_b"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: None,
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_b"),
					primary_remaining_percent: Some(70),
					secondary_remaining_percent: Some(40),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
		];

		candidates.sort_by(compare_account_candidates);

		assert_eq!(candidates[0].account_id(), "acct_b");
	}

	#[test]
	fn account_candidate_sort_balances_primary_and_secondary_windows() {
		let mut candidates = [
			codex_account_login_for_sort("acct_primary_rich", Some(100), Some(40), None),
			codex_account_login_for_sort("acct_balanced", Some(80), Some(80), None),
		];

		candidates.sort_by(compare_account_candidates);

		assert_eq!(candidates[0].account_id(), "acct_balanced");
	}

	#[test]
	fn account_candidate_sort_does_not_penalize_zero_credits_when_windows_available() {
		let mut candidates = [
			CodexAccountLogin {
				access_token: String::from("a"),
				account_id: String::from("acct_a"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: None,
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_a"),
					primary_remaining_percent: Some(86),
					secondary_remaining_percent: Some(97),
					credits_has_credits: Some(true),
					credits_unlimited: Some(false),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
			CodexAccountLogin {
				access_token: String::from("b"),
				account_id: String::from("acct_b"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: None,
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_b"),
					primary_remaining_percent: Some(100),
					secondary_remaining_percent: Some(100),
					credits_has_credits: Some(false),
					credits_unlimited: Some(false),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
		];

		candidates.sort_by(compare_account_candidates);

		assert_eq!(candidates[0].account_id(), "acct_b");
	}

	#[test]
	fn account_candidate_sort_balances_tied_usage_by_last_selection() {
		let mut candidates = [
			CodexAccountLogin {
				access_token: String::from("a"),
				account_id: String::from("acct_a"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: Some(20),
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_a"),
					primary_remaining_percent: Some(70),
					secondary_remaining_percent: Some(40),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
			CodexAccountLogin {
				access_token: String::from("b"),
				account_id: String::from("acct_b"),
				plan_type: Some(String::from("pro")),
				last_selected_at_unix_epoch: Some(10),
				summary: CodexAccountActivitySummary {
					account_fingerprint: String::from("...acct_b"),
					primary_remaining_percent: Some(70),
					secondary_remaining_percent: Some(40),
					..CodexAccountActivitySummary::default()
				},
				account_summaries: Vec::new(),
			},
		];

		candidates.sort_by(compare_account_candidates);

		assert_eq!(candidates[0].account_id(), "acct_b");
	}

	#[test]
	fn account_pool_rotates_equal_full_usage_across_dispatches() {
		const FULL_USAGE: &str = r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#;

		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let usage_endpoint = start_codex_usage_fixture_server(vec![
			FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE,
			FULL_USAGE, FULL_USAGE,
		]);

		fs::write(
			&accounts_path,
			r#"{"email":"a@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-a","refresh_token":"refresh-a","account_id":"acct_a"}}
{"email":"b@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-b","refresh_token":"refresh-b","account_id":"acct_b"}}
{"email":"c@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-c","refresh_token":"refresh-c","account_id":"acct_c"}}
"#,
		)
		.expect("accounts fixture should write");

		let pool = CodexAccountPool::new_with_fixed_account(
			&accounts_path,
			usage_endpoint,
			DEFAULT_REFRESH_ENDPOINT,
			None,
		)
		.expect("account pool should initialize");
		let selected = (0..3)
			.map(|_| {
				pool.select_account()
					.expect("full usage account should select")
					.account_id()
					.to_owned()
			})
			.collect::<Vec<_>>();

		assert_eq!(selected, vec!["acct_a", "acct_b", "acct_c"]);
	}

	fn codex_account_login_for_sort(
		account_id: &str,
		primary_remaining_percent: Option<i64>,
		secondary_remaining_percent: Option<i64>,
		last_selected_at_unix_epoch: Option<i64>,
	) -> CodexAccountLogin {
		CodexAccountLogin {
			access_token: String::from("access"),
			account_id: account_id.to_owned(),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch,
			summary: CodexAccountActivitySummary {
				account_fingerprint: format!("...{account_id}"),
				primary_remaining_percent,
				secondary_remaining_percent,
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		}
	}

	fn start_codex_usage_fixture_server(responses: Vec<&'static str>) -> String {
		let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
		let address = listener.local_addr().expect("usage fixture address should resolve");

		thread::spawn(move || {
			for body in responses {
				let (mut stream, _peer) =
					listener.accept().expect("usage fixture should accept request");
				let mut buffer = [0_u8; 4_096];
				let _bytes_read = stream.read(&mut buffer).expect("request should read");
				let response = format!(
					"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
					body.len(),
					body
				);

				stream.write_all(response.as_bytes()).expect("usage response should write");
			}
		});

		format!("http://{address}/usage")
	}

	fn start_codex_refresh_fixture_server(responses: Vec<&'static str>) -> String {
		let listener = TcpListener::bind("127.0.0.1:0").expect("refresh fixture should bind");
		let address = listener.local_addr().expect("refresh fixture address should resolve");

		thread::spawn(move || {
			for body in responses {
				let (mut stream, _peer) =
					listener.accept().expect("refresh fixture should accept request");
				let mut buffer = [0_u8; 4_096];
				let _bytes_read = stream.read(&mut buffer).expect("refresh request should read");
				let response = format!(
					"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
					body.len(),
					body
				);

				stream.write_all(response.as_bytes()).expect("refresh response should write");
			}
		});

		format!("http://{address}/oauth/token")
	}
}
