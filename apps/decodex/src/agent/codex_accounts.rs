use std::{
	cmp::Ordering,
	error::Error,
	fmt::{self, Display, Formatter},
	fs,
	path::{Path, PathBuf},
	process,
	sync::Mutex,
	time::Duration,
};

use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ProjectCodexAccountsConfig, prelude::eyre, state::CodexAccountActivitySummary,
};

const DEFAULT_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const DEFAULT_REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_USER_AGENT: &str = "codex-cli";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

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
	refresh_endpoint: String,
	client: Client,
	selected_account_id: Mutex<Option<String>>,
}
impl CodexAccountPool {
	pub(crate) fn from_config(config: &ProjectCodexAccountsConfig) -> crate::prelude::Result<Self> {
		Self::new(
			config.path(),
			config.usage_endpoint().unwrap_or(DEFAULT_USAGE_ENDPOINT),
			config.refresh_endpoint().unwrap_or(DEFAULT_REFRESH_ENDPOINT),
		)
	}

	pub(crate) fn new(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
	) -> crate::prelude::Result<Self> {
		let client = Client::builder().timeout(HTTP_TIMEOUT).build()?;

		Ok(Self {
			path: path.as_ref().to_path_buf(),
			usage_endpoint: usage_endpoint.into(),
			refresh_endpoint: refresh_endpoint.into(),
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
		let mut records = self.load_records()?;

		self.probe_account_activity_summaries(&mut records)
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
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut candidates = Vec::new();
		let mut skipped = Vec::new();
		let mut records_changed = false;

		for (index, record) in records.iter_mut().enumerate() {
			if record.disabled {
				skipped.push(format!("line {} disabled", index + 1));

				continue;
			}
			if record.cooldown_until_unix_epoch.is_some_and(|cooldown| cooldown > now) {
				skipped.push(format!("line {} cooling down", index + 1));

				continue;
			}
			if record.account_id().is_none() {
				skipped.push(format!("line {} missing account id", index + 1));

				continue;
			}
			if record.access_token().is_none() {
				skipped.push(format!("line {} missing access token", index + 1));

				continue;
			}

			match self.probe_record_usage(record) {
				Ok(usage) => candidates.push(record.login_from_usage(usage, "not_needed")?),
				Err(error) if error.unauthorized && record.refresh_token().is_some() => {
					self.refresh_record(record)?;

					records_changed = true;

					let usage = self.probe_record_usage(record).map_err(|retry_error| {
						eyre::eyre!(
							"Codex account `{}` refreshed but usage probe still failed: {retry_error}",
							record.display_name()
						)
					})?;

					candidates.push(record.login_from_usage(usage, "succeeded")?);
				},
				Err(error) => {
					skipped.push(format!("{} usage probe failed: {error}", record.display_name()));
				},
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

		let account_summaries = account_summaries(&selected, &candidates);
		let selected = selected.with_account_summaries(account_summaries);

		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}

	fn probe_account_activity_summaries(
		&self,
		records: &mut [AccountPoolRecord],
	) -> crate::prelude::Result<Vec<CodexAccountActivitySummary>> {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut summaries = Vec::new();
		let mut records_changed = false;

		for record in records.iter_mut() {
			let Some(configured_summary) = record.configured_activity_summary(now) else {
				continue;
			};

			if configured_summary.status != "available" {
				summaries.push(configured_summary);

				continue;
			}

			match self.probe_record_usage(record) {
				Ok(usage) => {
					summaries.push(record.activity_summary_from_usage(usage, "not_needed")?);
				},
				Err(error) if error.unauthorized && record.refresh_token().is_some() => {
					match self.refresh_record(record) {
						Ok(()) => {
							records_changed = true;

							match self.probe_record_usage(record) {
								Ok(usage) => {
									summaries.push(
										record.activity_summary_from_usage(usage, "succeeded")?,
									);
								},
								Err(retry_error) => {
									summaries.push(record.probe_failed_activity_summary(
										now,
										"failed",
										&retry_error,
									));
								},
							}
						},
						Err(refresh_error) => {
							summaries.push(record.probe_failed_activity_summary(
								now,
								"failed",
								refresh_error.as_ref(),
							));
						},
					}
				},
				Err(error) => {
					summaries.push(record.probe_failed_activity_summary(
						now,
						"probe_failed",
						&error,
					));
				},
			}
		}

		if records_changed {
			self.save_records(records)?;
		}

		Ok(summaries)
	}

	fn refresh_from_records(
		&self,
		records: &mut [AccountPoolRecord],
		previous_account_id: Option<&str>,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let selected_account_id = self.selected_account_id()?;
		let target_account_id = previous_account_id.or(selected_account_id.as_deref());
		let Some(record_index) = records.iter().position(|record| {
			target_account_id.is_none_or(|target| record.account_id() == Some(target))
		}) else {
			eyre::bail!(
				"Codex account refresh requested an account that is not in the configured accounts."
			);
		};

		self.refresh_record(&mut records[record_index])?;

		let usage = self.probe_record_usage(&records[record_index])?;
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut selected = records[record_index].login_from_usage(usage, "succeeded")?;

		selected.mark_selected(now);

		let selected_summary = selected.summary().clone();
		let selected = selected.with_account_summaries(vec![selected_summary]);

		self.save_records(records)?;
		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
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

		Ok(())
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
		let mut records = self.load_records()?;

		self.select_from_records(&mut records)
	}

	fn refresh_account(
		&self,
		previous_account_id: Option<&str>,
	) -> crate::prelude::Result<CodexAccountLogin> {
		let mut records = self.load_records()?;

		self.refresh_from_records(&mut records, previous_account_id)
	}
}

pub(crate) struct CodexAccountLogin {
	access_token: String,
	account_id: String,
	plan_type: Option<String>,
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
		auth: AuthDotJson,
	},
	Flat(AccountPoolRecord),
}
impl AccountPoolLine {
	fn into_record(self) -> AccountPoolRecord {
		match self {
			Self::Flat(record) => record,
			Self::Wrapped { email, disabled, cooldown_until_unix_epoch, cooldown_until, auth } =>
				AccountPoolRecord {
					email: first_nonblank_string(email, auth.email),
					disabled,
					cooldown_until_unix_epoch,
					cooldown_until,
					auth_mode: auth.auth_mode,
					openai_api_key: auth.openai_api_key,
					tokens: auth.tokens,
					last_refresh: auth.last_refresh,
				},
		}
	}
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
		};

		Ok(CodexAccountLogin {
			access_token,
			account_id,
			plan_type: summary.plan_type.clone(),
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

fn usage_window_from_value(value: Option<&Value>) -> Option<UsageWindow> {
	let value = value.filter(|value| !value.is_null())?;
	let used_percent = number_as_i64(value.get("used_percent")?)?;
	let remaining_percent = 100_i64.saturating_sub(used_percent).clamp(0, 100);

	Some(UsageWindow {
		window_seconds: value.get("limit_window_seconds").and_then(number_as_i64),
		remaining_percent,
		resets_at_unix_epoch: value.get("reset_at").and_then(number_as_i64),
	})
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

fn json_scalar_to_string(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
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
		.then_with(|| left.summary.account_fingerprint.cmp(&right.summary.account_fingerprint))
}

fn account_candidate_score(candidate: &CodexAccountLogin) -> i64 {
	let summary = candidate.summary();
	let primary = summary.primary_remaining_percent.unwrap_or(0);
	let secondary = summary.secondary_remaining_percent.unwrap_or(primary);
	let mut score = primary.saturating_mul(1_000).saturating_add(secondary.saturating_mul(10));

	if account_summary_is_limited(summary) {
		score = score.saturating_sub(200_000);
	}

	score
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
	use crate::agent::codex_accounts::{
		self, AccountPoolRecord, CodexAccountActivitySummary, CodexAccountLogin, CodexTokenData,
		CreditsSnapshot, Path, UsageWindow, compare_account_candidates,
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
	fn account_candidate_sort_prefers_remaining_usage() {
		let mut candidates = [
			CodexAccountLogin {
				access_token: String::from("a"),
				account_id: String::from("acct_a"),
				plan_type: Some(String::from("pro")),
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
	fn account_candidate_sort_does_not_penalize_zero_credits_when_windows_available() {
		let mut candidates = [
			CodexAccountLogin {
				access_token: String::from("a"),
				account_id: String::from("acct_a"),
				plan_type: Some(String::from("pro")),
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
}
