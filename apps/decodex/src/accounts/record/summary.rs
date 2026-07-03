use std::collections::BTreeMap;

use time::OffsetDateTime;

use crate::accounts::{
	auth_json,
	identity::AccountIdentity,
	random_names,
	record::model::AccountPoolRecord,
	types::AccountSummary,
	usage_history::{self, DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER},
};

impl AccountPoolRecord {
	pub(in crate::accounts) fn summary(
		&self,
		fixed_selector: Option<&str>,
		codex_auth: Option<&AccountIdentity>,
		name_offsets: &BTreeMap<String, i64>,
	) -> AccountSummary {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let account_fingerprint = self
			.account_id()
			.map(crate::accounts::identity::redact_account_id)
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
			.map(crate::accounts::identity::redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let seed = random_names::random_name_seed_for(account_fingerprint.as_str(), self.email());

		random_names::random_name_key(&seed)
	}
}
