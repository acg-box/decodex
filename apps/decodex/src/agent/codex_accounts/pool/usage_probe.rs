use reqwest::StatusCode;
use serde_json::Value;
use time::OffsetDateTime;

use crate::agent::codex_accounts::{
	CODEX_USER_AGENT,
	pool::CodexAccountPool,
	record::AccountPoolRecord,
	usage::{self, AccountProfileSnapshot, AccountUsageSnapshot, UsageProbeError},
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts) fn probe_record_usage(
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

		Ok(usage::usage_snapshot_from_payload(&payload, OffsetDateTime::now_utc().unix_timestamp()))
	}

	pub(in crate::agent::codex_accounts) fn probe_record_profile(
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

		Ok(usage::profile_snapshot_from_payload(
			&payload,
			OffsetDateTime::now_utc().unix_timestamp(),
		))
	}
}
