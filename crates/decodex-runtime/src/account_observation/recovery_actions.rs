//! Revalidate backend recovery destinations without sending a purchase, reset, or nudge.
use reqwest::Url;

use crate::account_observation::{AccountId, AccountObservationService};
use decodex_codex::AccountApiRecoveryContext;
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryBanner, AccountRecoveryDestination,
	AccountRecoveryPreparation, AccountRecoveryResult, AccountRecoveryState, WireText,
};

impl AccountObservationService {
	pub(crate) async fn prepare_recovery(
		&self,
		expected: &AccountRecoveryResult,
		action: AccountRecoveryAction,
	) -> AccountRecoveryPreparation {
		let current = self.recovery(&expected.account_id, expected.account_revision).await;
		let (AccountRecoveryState::Current(before), AccountRecoveryState::Current(banner)) =
			(&expected.state, &current.state)
		else {
			return AccountRecoveryPreparation::Unavailable;
		};

		if before != banner
			|| !expected.valid_for(&current.account_id, current.account_revision)
			|| expected.observed_at_unix_micros > current.observed_at_unix_micros
			|| !banner.actions.iter().any(|cta| cta.action == action)
		{
			return AccountRecoveryPreparation::Unavailable;
		}

		let Ok(id) = AccountId::new(current.account_id.as_str()) else {
			return AccountRecoveryPreparation::Unavailable;
		};
		let cached = self.state.read().await.banners.get(&id).cloned();
		let Some(cached) = cached.filter(|entry| {
			entry.current && Some(entry.observed_at) == current.observed_at_unix_micros
		}) else {
			return AccountRecoveryPreparation::Unavailable;
		};
		let Some(context) = cached.context else {
			return AccountRecoveryPreparation::Unavailable;
		};
		let Some(destination) = destination(&context, banner, action) else {
			return AccountRecoveryPreparation::Unavailable;
		};

		if self.recovery(&expected.account_id, expected.account_revision).await != current {
			return AccountRecoveryPreparation::Unavailable;
		}

		AccountRecoveryPreparation::Ready { source: Box::new(current), action, destination }
	}
}

fn destination(
	context: &AccountApiRecoveryContext,
	banner: &AccountRecoveryBanner,
	action: AccountRecoveryAction,
) -> Option<AccountRecoveryDestination> {
	let usage = "https://chatgpt.com/settings/usage";
	let url = match action {
		AccountRecoveryAction::ResetUsage => return Some(AccountRecoveryDestination::ResetPicker),
		AccountRecoveryAction::NotifyOwner =>
			return Some(AccountRecoveryDestination::RequestCredits),
		AccountRecoveryAction::RequestIncrease => match &banner.request_url {
			Some(url) => return validated_url(url.as_str()),
			None => return Some(AccountRecoveryDestination::RequestUsageIncrease),
		},
		AccountRecoveryAction::AddCredits =>
			if workspace(context.plan_type.as_deref()?)? {
				"https://chatgpt.com/admin/billing?codex_credit_action=add_credits".into()
			} else {
				format!("{usage}?credits_modal=true")
			},
		AccountRecoveryAction::BuyReset => "https://chatgpt.com/codex/purchase/reset".into(),
		AccountRecoveryAction::ViewUsage => usage.into(),
		AccountRecoveryAction::ViewWorkspaceUsage =>
			"https://chatgpt.com/admin/usage-limits/workspace".into(),
		AccountRecoveryAction::PlusPricing => "https://chatgpt.com/explore/plus".into(),
		AccountRecoveryAction::ProPricing => "https://chatgpt.com/explore/pro".into(),
		AccountRecoveryAction::Pricing => {
			let plan = context.plan_type.as_deref()?;

			workspace(plan)?;

			let target = if matches!(plan, "plus" | "prolite") { "pro" } else { "plus" };
			let mut url = Url::parse("https://chatgpt.com/").ok()?;

			url.query_pairs_mut()
				.append_pair("cta_tab", "personal")
				.append_pair("highlight_plan", target);

			if plan == "prolite" {
				url.query_pairs_mut().append_pair("pro_variant", "2x");
			}

			url.set_fragment(Some("pricing"));

			url.into()
		},
	};
	let mut url = Url::parse(&url).ok()?;

	if !matches!(url.scheme(), "https" | "http")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	if url.host_str() == Some("chatgpt.com") && url.path().starts_with("/admin/") {
		if context.provider_account_id.is_empty() {
			return None;
		}

		url.query_pairs_mut().append_pair("account_id", &context.provider_account_id);
	}

	Some(AccountRecoveryDestination::OpenUrl(WireText::new(url.as_str()).ok()?))
}

fn workspace(plan: &str) -> Option<bool> {
	match plan {
		"team"
		| "self_serve_business_prolite"
		| "self_serve_business_usage_based"
		| "business"
		| "ent26"
		| "enterprise_cbp_automation"
		| "enterprise_cbp_usage_based"
		| "enterprise"
		| "edu"
		| "edu_plus"
		| "edu_pro" => Some(true),
		"free" | "go" | "plus" | "pro" | "prolite" | "promax" => Some(false),
		_ => None,
	}
}

fn validated_url(raw: &str) -> Option<AccountRecoveryDestination> {
	let url = Url::parse(raw).ok()?;

	if !matches!(url.scheme(), "https" | "http")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}

	Some(AccountRecoveryDestination::OpenUrl(WireText::new(url.as_str()).ok()?))
}

#[cfg(test)]
mod tests {
	use crate::account_observation::recovery_actions::{
		self, AccountApiRecoveryContext, AccountRecoveryAction, AccountRecoveryBanner,
		AccountRecoveryDestination, WireText,
	};
	fn banner() -> AccountRecoveryBanner {
		AccountRecoveryBanner {
			banner_type: WireText::new("limit").unwrap(),
			title: WireText::new("Limit").unwrap(),
			description: WireText::new("Description").unwrap(),
			reset_at: None,
			model_slug: None,
			blocked_model_slug: None,
			fallback_model_slugs: Vec::new(),
			dismissible: false,
			actions: Vec::new(),
			request_url: None,
		}
	}
	fn url(
		context: &AccountApiRecoveryContext,
		banner: &AccountRecoveryBanner,
		action: AccountRecoveryAction,
	) -> reqwest::Url {
		let Some(AccountRecoveryDestination::OpenUrl(url)) =
			recovery_actions::destination(context, banner, action)
		else {
			panic!("url")
		};

		reqwest::Url::parse(url.as_str()).unwrap()
	}
	#[test]
	fn workspace_routes_encode_provider_identity_and_unknown_plans_do_not_guess() {
		let mut context = AccountApiRecoveryContext {
			provider_account_id: "workspace &other=x".into(),
			plan_type: Some("edu_plus".into()),
		};
		let banner = banner();
		let target = url(&context, &banner, AccountRecoveryAction::AddCredits);

		assert_eq!(target.path(), "/admin/billing");
		assert_eq!(
			target.query_pairs().find(|(k, _)| k == "account_id").unwrap().1,
			context.provider_account_id
		);
		assert!(!target.query_pairs().any(|(k, _)| k == "other"));

		context.plan_type = Some("prolite".into());

		assert_eq!(
			url(&context, &banner, AccountRecoveryAction::AddCredits).path(),
			"/settings/usage"
		);

		let target = url(&context, &banner, AccountRecoveryAction::Pricing);

		assert!(target.query_pairs().any(|(k, v)| k == "highlight_plan" && v == "pro"));
		assert!(target.query_pairs().any(|(k, v)| k == "pro_variant" && v == "2x"));
		assert_eq!(target.fragment(), Some("pricing"));

		context.plan_type = Some("future-plan".into());

		assert!(
			recovery_actions::destination(&context, &banner, AccountRecoveryAction::AddCredits)
				.is_none()
		);
		assert!(
			recovery_actions::destination(&context, &banner, AccountRecoveryAction::Pricing)
				.is_none()
		);
	}
	#[test]
	fn pro_max_credit_recovery_uses_personal_usage_without_account_identity() {
		let context = AccountApiRecoveryContext {
			provider_account_id: "private-personal-account".into(),
			plan_type: Some("promax".into()),
		};
		let target = url(&context, &banner(), AccountRecoveryAction::AddCredits);

		assert_eq!(target.as_str(), "https://chatgpt.com/settings/usage?credits_modal=true");
		assert_eq!(
			url(&context, &banner(), AccountRecoveryAction::ViewUsage).as_str(),
			"https://chatgpt.com/settings/usage"
		);
	}
	#[test]
	fn request_url_does_not_change_effect_or_leak_workspace_identity() {
		let context = AccountApiRecoveryContext {
			provider_account_id: "private-workspace".into(),
			plan_type: None,
		};
		let mut banner = banner();

		assert_eq!(
			recovery_actions::destination(
				&context,
				&banner,
				AccountRecoveryAction::RequestIncrease
			),
			Some(AccountRecoveryDestination::RequestUsageIncrease)
		);
		assert_eq!(
			recovery_actions::destination(&context, &banner, AccountRecoveryAction::ResetUsage),
			Some(AccountRecoveryDestination::ResetPicker)
		);
		assert_eq!(
			recovery_actions::destination(&context, &banner, AccountRecoveryAction::NotifyOwner),
			Some(AccountRecoveryDestination::RequestCredits)
		);

		banner.request_url =
			Some(WireText::new("https://chatgpt.com/admin/custom?ticket=1").unwrap());

		let target = url(&context, &banner, AccountRecoveryAction::RequestIncrease);

		assert_eq!(target.query(), Some("ticket=1"));

		for invalid in ["javascript:alert(1)", "https://user@host/path", "file:///tmp/example"] {
			banner.request_url = Some(WireText::new(invalid).unwrap());

			assert!(
				recovery_actions::destination(
					&context,
					&banner,
					AccountRecoveryAction::RequestIncrease
				)
				.is_none()
			);
		}
	}
}
