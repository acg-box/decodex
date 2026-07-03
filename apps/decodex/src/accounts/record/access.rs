use crate::{
	accounts::{
		auth_json::{self, AuthDotJson},
		record::model::AccountPoolRecord,
	},
	prelude::{Result, eyre},
};

impl AccountPoolRecord {
	pub(in crate::accounts) fn from_auth(auth: AuthDotJson) -> Result<Self> {
		let record = Self {
			email: auth_json::first_nonblank_string(
				auth.email,
				auth.tokens.as_ref().and_then(|tokens| {
					auth_json::nonblank_string(tokens.email.as_deref())
						.or_else(|| auth_json::jwt_email_claim(tokens.id_token.as_deref()))
				}),
			),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_failed_at_unix_epoch: None,
			auth_failure: None,
			auth_mode: auth.auth_mode,
			openai_api_key: auth.openai_api_key,
			tokens: auth.tokens,
			last_refresh: auth.last_refresh,
		};

		record.validate_importable()?;

		Ok(record)
	}

	pub(in crate::accounts) fn validate_importable(&self) -> Result<()> {
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| auth_json::nonblank_string(Some(&tokens.access_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.access_token`.");
		}
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| auth_json::nonblank_string(Some(&tokens.refresh_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.refresh_token`.");
		}
		if self.account_id().is_none() {
			eyre::bail!("Codex auth JSON is missing `tokens.account_id`.");
		}

		Ok(())
	}
}
