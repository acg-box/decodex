use crate::{
	accounts::{
		auth_json::{self, AuthDotJson},
		identity::AccountIdentity,
		record::model::AccountPoolRecord,
		types::AccountIdentitySummary,
	},
	prelude::Result,
};

impl AccountPoolRecord {
	pub(in crate::accounts) fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(crate::accounts::identity::redact_account_id).as_deref()
				== Some(selector)
	}

	pub(in crate::accounts) fn auth_failure(&self) -> Option<&str> {
		self.auth_failure
			.as_deref()
			.map(str::trim)
			.filter(|failure| !failure.is_empty())
			.or_else(|| self.auth_failed_at_unix_epoch.map(|_| "authentication failed"))
	}

	pub(in crate::accounts) fn matches_account_identity(&self, identity: &AccountIdentity) -> bool {
		identity
			.account_id
			.as_deref()
			.is_some_and(|account_id| self.account_id() == Some(account_id))
			|| identity.email.as_deref().is_some_and(|email| self.email().as_deref() == Some(email))
	}

	pub(in crate::accounts) fn account_id(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.and_then(|tokens| tokens.account_id.as_deref())
			.filter(|account_id| !account_id.trim().is_empty())
	}

	pub(in crate::accounts) fn email(&self) -> Option<String> {
		auth_json::nonblank_string(self.email.as_deref())
			.or_else(|| {
				self.tokens
					.as_ref()
					.and_then(|tokens| auth_json::nonblank_string(tokens.email.as_deref()))
			})
			.or_else(|| {
				self.tokens
					.as_ref()
					.and_then(|tokens| auth_json::jwt_email_claim(tokens.id_token.as_deref()))
			})
	}

	pub(in crate::accounts) fn identity(&self) -> AccountIdentity {
		AccountIdentity { account_id: self.account_id().map(str::to_owned), email: self.email() }
	}

	pub(in crate::accounts) fn identity_summary(&self) -> AccountIdentitySummary {
		self.identity().summary()
	}

	pub(in crate::accounts) fn auth_dot_json(&self) -> Result<AuthDotJson> {
		self.validate_importable()?;

		Ok(AuthDotJson {
			email: self.email(),
			auth_mode: self.auth_mode.clone(),
			openai_api_key: self.openai_api_key.clone(),
			tokens: self.tokens.clone(),
			last_refresh: self.last_refresh.clone(),
		})
	}
}
