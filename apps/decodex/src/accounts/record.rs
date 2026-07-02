use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
	accounts::{
		auth_json::{self, AuthDotJson, CodexTokenData},
		identity::{self, AccountIdentity},
		random_names,
		types::{AccountIdentitySummary, AccountSummary},
		usage_history::{self, DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER},
	},
	prelude::{Result, eyre},
};

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::accounts) struct AccountPoolRecord {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) email: Option<String>,
	#[serde(default, skip_serializing_if = "is_false")]
	pub(in crate::accounts) disabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) cooldown_until: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) last_selected_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_failed_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_failure: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::accounts) last_refresh: Option<String>,
}
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

	pub(in crate::accounts) fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(identity::redact_account_id).as_deref() == Some(selector)
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

	pub(in crate::accounts) fn summary(
		&self,
		fixed_selector: Option<&str>,
		codex_auth: Option<&AccountIdentity>,
		name_offsets: &BTreeMap<String, i64>,
	) -> AccountSummary {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let account_fingerprint = self
			.account_id()
			.map(identity::redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let selector = self.email().unwrap_or_else(|| account_fingerprint.clone());
		let selected = fixed_selector.is_some_and(|fixed| self.matches_account_selector(fixed));
		let access_token_expires_at_unix_epoch = self
			.tokens
			.as_ref()
			.and_then(|tokens| auth_json::jwt_expiration_unix_epoch(&tokens.access_token));
		let refresh_token_present = self
			.tokens
			.as_ref()
			.and_then(|tokens| auth_json::nonblank_string(Some(&tokens.refresh_token)))
			.is_some();
		let status = if self.disabled {
			"disabled"
		} else if self.auth_failure().is_some() {
			"auth_failed"
		} else if self.cooldown_until_unix_epoch.is_some_and(|cooldown_until| cooldown_until > now)
		{
			"cooldown"
		} else if access_token_expires_at_unix_epoch.is_some_and(|expires_at| expires_at <= now) {
			"expired"
		} else if self.account_id().is_none() || !refresh_token_present {
			"unusable"
		} else {
			"available"
		};
		let random_name_seed =
			random_names::random_name_seed_for(account_fingerprint.as_str(), self.email());
		let random_name_key = random_names::random_name_key(&random_name_seed);
		let random_name_offset = name_offsets.get(&random_name_key).copied().unwrap_or_default();
		let recovery_action = usage_history::account_recovery_action(
			status,
			refresh_token_present,
			if self.auth_failure().is_some() { Some("auth_failed") } else { None },
			self.auth_failure().or(Some("local account pool")),
		);

		AccountSummary {
			account_fingerprint,
			email: self.email(),
			selector,
			random_name: random_names::random_name(&random_name_seed, random_name_offset),
			random_name_key,
			random_name_offset,
			status: status.to_owned(),
			selected,
			codex_active: codex_auth
				.is_some_and(|identity| self.matches_account_identity(identity)),
			disabled: self.disabled,
			refresh_token_present,
			access_token_expires_at_unix_epoch,
			last_selected_at_unix_epoch: self.last_selected_at_unix_epoch,
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(
				self.auth_failure()
					.map_or_else(|| String::from("local account pool"), ToOwned::to_owned),
			),
			plan_type: None,
			capacity_multiplier: DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER,
			recovery_action,
			refresh_status: None,
			checked_at_unix_epoch: None,
			primary_window_seconds: None,
			primary_remaining_percent: None,
			primary_resets_at_unix_epoch: None,
			secondary_window_seconds: None,
			secondary_remaining_percent: None,
			secondary_resets_at_unix_epoch: None,
			credits_has_credits: None,
			credits_unlimited: None,
			credits_balance: None,
			rate_limit_reached_type: None,
			profile_display_name: None,
			profile_username: None,
			profile_checked_at_unix_epoch: None,
			profile_lifetime_tokens: None,
			profile_peak_daily_tokens: None,
			profile_longest_task_seconds: None,
			profile_current_streak_days: None,
			profile_longest_streak_days: None,
			profile_daily_usage: Vec::new(),
			seven_day_used_percent: None,
			seven_day_daily_average_percent: None,
			usage_records: Vec::new(),
		}
	}

	pub(in crate::accounts) fn random_name_key(&self) -> String {
		let account_fingerprint = self
			.account_id()
			.map(identity::redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let seed = random_names::random_name_seed_for(account_fingerprint.as_str(), self.email());

		random_names::random_name_key(&seed)
	}
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(in crate::accounts) enum AccountPoolLine {
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
	pub(in crate::accounts) fn into_record(self) -> Result<AccountPoolRecord> {
		match self {
			Self::Flat(record) => Ok(record),
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth,
			} => {
				let mut record = AccountPoolRecord::from_auth(auth)?;

				record.email = auth_json::first_nonblank_string(email, record.email);
				record.disabled = disabled;
				record.cooldown_until_unix_epoch = cooldown_until_unix_epoch;
				record.cooldown_until = cooldown_until;
				record.last_selected_at_unix_epoch = last_selected_at_unix_epoch;
				record.auth_failed_at_unix_epoch = auth_failed_at_unix_epoch;
				record.auth_failure = auth_failure;

				Ok(record)
			},
		}
	}
}

const fn is_false(value: &bool) -> bool {
	!*value
}
