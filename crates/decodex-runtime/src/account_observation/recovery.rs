//! Fresh and stale account recovery projections from the existing observation owner.

use decodex_codex::{AccountApiBanner, AccountApiBannerAction as A, AccountApiBannerState};
use decodex_protocol::{
	AccountRecoveryAction as B, AccountRecoveryBanner, AccountRecoveryCta, AccountRecoveryResult,
	AccountRecoveryState, EntityId, EntityRevision, WireText,
};

#[derive(Clone)]
pub(super) struct CachedAccountBanner {
	pub account_revision: i64,
	pub observed_at: i64,
	pub current: bool,
	pub banner: AccountApiBannerState,
	pub context: Option<decodex_codex::AccountApiRecoveryContext>,
}

pub(super) fn unavailable(
	account_id: EntityId,
	account_revision: EntityRevision,
) -> AccountRecoveryResult {
	AccountRecoveryResult {
		account_id,
		account_revision,
		observed_at_unix_micros: None,
		state: AccountRecoveryState::Unavailable,
	}
}

pub(super) fn project(
	account_id: EntityId,
	account_revision: EntityRevision,
	cached: Option<CachedAccountBanner>,
	now: i64,
) -> AccountRecoveryResult {
	let mut result = unavailable(account_id, account_revision);
	let Some(cached) = cached.filter(|c| {
		u64::try_from(c.account_revision).ok() == Some(account_revision.0)
			&& c.observed_at > 0
			&& c.observed_at <= now
	}) else {
		return result;
	};
	let current = cached.current && now.saturating_sub(cached.observed_at) <= 300_000_000;
	result.observed_at_unix_micros = Some(cached.observed_at);
	result.state = match cached.banner {
		AccountApiBannerState::Available(banner) => match project_banner(*banner) {
			Some(banner) if current => AccountRecoveryState::Current(Box::new(banner)),
			Some(banner) => AccountRecoveryState::Stale(Box::new(banner)),
			None if current => AccountRecoveryState::Unsupported,
			None => AccountRecoveryState::Unavailable,
		},
		AccountApiBannerState::Absent if current => AccountRecoveryState::Absent,
		AccountApiBannerState::Unsupported if current => AccountRecoveryState::Unsupported,
		_ => AccountRecoveryState::Unavailable,
	};
	result
}

fn project_banner(banner: AccountApiBanner) -> Option<AccountRecoveryBanner> {
	let text = |s: String| WireText::new(s).ok();
	let optional = |s: Option<String>| s.map(WireText::new).transpose().ok();
	let actions = banner
		.actions
		.into_iter()
		.map(|cta| {
			Some(AccountRecoveryCta {
				action: match cta.action {
					A::AddCredits => B::AddCredits,
					A::BuyReset => B::BuyReset,
					A::ResetUsage => B::ResetUsage,
					A::ViewUsage => B::ViewUsage,
					A::ViewWorkspaceUsage => B::ViewWorkspaceUsage,
					A::NotifyOwner => B::NotifyOwner,
					A::RequestIncrease => B::RequestIncrease,
					A::PlusPricing => B::PlusPricing,
					A::ProPricing => B::ProPricing,
					A::Pricing => B::Pricing,
				},
				label: text(cta.label)?,
			})
		})
		.collect::<Option<Vec<_>>>()?;
	Some(AccountRecoveryBanner {
		banner_type: text(banner.banner_type)?,
		title: text(banner.title)?,
		description: text(banner.description)?,
		reset_at: banner.reset_at,
		model_slug: optional(banner.model_slug)?,
		blocked_model_slug: optional(banner.blocked_model_slug)?,
		fallback_model_slugs: banner
			.fallback_model_slugs
			.into_iter()
			.map(text)
			.collect::<Option<Vec<_>>>()?,
		dismissible: banner.dismissible,
		actions,
		request_url: optional(banner.request_url)?,
	})
}

#[cfg(test)]
mod tests {
	use super::{CachedAccountBanner, project};
	use decodex_protocol::{AccountRecoveryState as S, EntityId, EntityRevision};

	#[test]
	fn expired_failed_or_wrong_revision_observations_never_authorize_actions() {
		let account = EntityId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let usage=decodex_codex::decode_account_api_usage(br#"{"account_id":"a","user_id":"u","rate_limit":{},"rate_limit_upsell":{"banner_type":"limit","title":"Blocked model","description":"Choose a model","ctas":[],"blocked_model_slug":"model-a"}}"#).unwrap();
		let cached = CachedAccountBanner {
			account_revision: 1,
			context: None,
			observed_at: 100,
			current: true,
			banner: usage.banner_for("a", "u"),
		};
		let read = |value: CachedAccountBanner, revision, now| {
			project(account.clone(), EntityRevision(revision), Some(value), now).state
		};
		assert!(
			matches!(read(cached.clone(),1,100),S::Current(b) if b.blocked_model_slug.as_ref().unwrap().as_str()=="model-a")
		);
		assert!(matches!(read(cached.clone(), 1, 300_000_101), S::Stale(_)));
		assert!(matches!(
			read(CachedAccountBanner { current: false, ..cached.clone() }, 1, 101),
			S::Stale(_)
		));
		assert!(matches!(read(cached.clone(), 2, 101), S::Unavailable));
		assert!(matches!(read(cached, 1, 99), S::Unavailable));
	}
}
