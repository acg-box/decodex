use std::error::Error;

use crate::{
	agent::codex_accounts::{
		TOKEN_REFRESH_INTERVAL_SECONDS,
		login::CodexAccountLogin,
		record::model::{self, AccountPoolRecord},
		refresh::{self, ProactiveRefreshReason},
		usage::{AccountProfileSnapshot, AccountUsageSnapshot, ResetCreditsSnapshot},
	},
	prelude::{Result, eyre},
	state::{CodexAccountActivitySummary, CodexAccountResetCreditSummary},
};

impl AccountPoolRecord {
	pub(in crate::agent::codex_accounts) fn configured_activity_summary(
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

	pub(in crate::agent::codex_accounts) fn login_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> Result<CodexAccountLogin> {
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
			account_fingerprint: model::redact_account_id(&account_id),
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

	pub(in crate::agent::codex_accounts) fn activity_summary_from_usage(
		&self,
		usage: AccountUsageSnapshot,
		refresh_status: &str,
	) -> Result<CodexAccountActivitySummary> {
		Ok(self.login_from_usage(usage, refresh_status)?.summary)
	}

	pub(in crate::agent::codex_accounts) fn activity_summary_from_usage_profile(
		&self,
		usage: AccountUsageSnapshot,
		profile: Option<AccountProfileSnapshot>,
		reset_credits: Option<ResetCreditsSnapshot>,
		refresh_status: &str,
	) -> Result<CodexAccountActivitySummary> {
		let mut summary = self.activity_summary_from_usage(usage, refresh_status)?;

		if let Some(profile) = profile {
			profile.apply_to_summary(&mut summary);
		}
		if let Some(reset_credits) = reset_credits {
			summary.reset_credits_available_count = reset_credits.available_count;
			summary.reset_credits_total_earned_count = reset_credits.total_earned_count;
			summary.reset_credits_checked_at_unix_epoch = Some(reset_credits.checked_at_unix_epoch);
			summary.reset_credits = reset_credits
				.credits
				.into_iter()
				.map(|credit| CodexAccountResetCreditSummary {
					granted_at_unix_epoch: credit.granted_at_unix_epoch,
					expires_at_unix_epoch: credit.expires_at_unix_epoch,
					status: credit.status,
				})
				.collect();
		}

		Ok(summary)
	}

	pub(in crate::agent::codex_accounts) fn probe_failed_activity_summary(
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

	pub(in crate::agent::codex_accounts) fn auth_failed_activity_summary(
		&self,
		now_unix_epoch: i64,
	) -> CodexAccountActivitySummary {
		let mut summary = self.configured_activity_summary(now_unix_epoch).unwrap_or_default();

		summary.status = String::from("auth_failed");
		summary.refresh_status = String::from("auth_failed");
		summary.note = self.auth_failure().map(ToOwned::to_owned);

		summary
	}

	pub(in crate::agent::codex_accounts) fn proactive_refresh_reason(
		&self,
		now_unix_epoch: i64,
	) -> Option<ProactiveRefreshReason> {
		let tokens = self.tokens.as_ref()?;

		if let Some(expires_at) = refresh::jwt_expiration_unix_epoch(&tokens.access_token) {
			return (expires_at <= now_unix_epoch)
				.then_some(ProactiveRefreshReason::AccessTokenExpired);
		}

		let last_refresh = self.last_refresh.as_deref().and_then(refresh::rfc3339_unix_epoch)?;

		(last_refresh < now_unix_epoch.saturating_sub(TOKEN_REFRESH_INTERVAL_SECONDS))
			.then_some(ProactiveRefreshReason::LastRefreshStale)
	}
}
