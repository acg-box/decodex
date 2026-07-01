use color_eyre::Report;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent::codex_accounts::{
		CHATGPT_OAUTH_CLIENT_ID, CodexAccountAuthFailure, CodexAccountLogin,
		pool::CodexAccountPool,
		record::{self, AccountPoolRecord},
		refresh::{
			self, ProactiveRefreshError, RefreshRequest, RefreshResponse, RefreshStatus,
			ReportableRefreshError,
		},
	},
	prelude::eyre,
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts) fn proactive_refresh_record(
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

	pub(in crate::agent::codex_accounts::pool) fn refresh_from_records(
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

	pub(in crate::agent::codex_accounts) fn refresh_record(
		&self,
		record: &mut AccountPoolRecord,
	) -> crate::prelude::Result<()> {
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

			if refresh::token_refresh_auth_status(status) {
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
		record::sync_refreshed_record_to_codex_auth(record, &self.codex_auth_path)
	}
}
