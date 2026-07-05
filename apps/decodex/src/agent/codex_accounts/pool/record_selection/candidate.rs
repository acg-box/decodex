use crate::{
	agent::codex_accounts::{
		CodexAccountAuthFailure, CodexAccountLogin, pool::CodexAccountPool,
		record::AccountPoolRecord, refresh::RefreshStatus,
	},
	prelude::eyre,
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts::pool) fn account_candidate_from_record(
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
}
