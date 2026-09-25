//! Bounded backend banner data. Reference: openai/codex 595cc91e,
//! tui/src/backend_banners.rs and backend_banners/{actions,render}.rs.

use serde::Deserialize;
use serde_json::Value;

/// Full-read banner presence, including an unsupported payload that must not imply recovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum AccountApiBannerState {
	/// The response is not bound to the current authenticated account and user.
	#[default]
	Unavailable,
	/// A matching full response contains no banner.
	Absent,
	/// A matching response contains a banner whose shape is not supported.
	Unsupported,
	/// Bounded backend copy and recognized actions from a matching response.
	Available(Box<AccountApiBanner>),
}

/// Backend-owned copy with exact model scope; this is not an account-wide admission decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiBanner {
	/// Exact backend occurrence category.
	pub banner_type: String,
	/// Bounded title with terminal control characters removed.
	pub title: String,
	/// Bounded description with terminal control characters removed.
	pub description: String,
	/// Recognized actions in backend order, with bounded labels.
	pub actions: Vec<AccountApiBannerCta>,
	/// Backend Unix reset time in seconds; never a recovery authorization.
	pub reset_at: Option<i64>,
	/// Model described by the banner, when supplied.
	pub model_slug: Option<String>,
	/// Exact model whose requests are blocked, independent of the displayed replacement.
	pub blocked_model_slug: Option<String>,
	/// Ordered suggestions; their presence does not change the active model.
	pub fallback_model_slugs: Vec<String>,
	/// Whether the user may dismiss this occurrence.
	pub dismissible: bool,
	/// Validated HTTP(S) request-increase destination, if supplied.
	pub request_url: Option<String>,
}

/// One known backend action and its presentation label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApiBannerCta {
	/// Known action; URLs and native effects are resolved by the account owner.
	pub action: AccountApiBannerAction,
	/// Backend label, at most 256 bytes with no control characters.
	pub label: String,
}

/// Supported CTA meanings. Decoding never executes these actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountApiBannerAction {
	/// Open the account-appropriate credits purchase surface.
	AddCredits,
	/// Open the reset purchase surface.
	BuyReset,
	/// Open the existing reset picker, retaining its explicit confirmation.
	ResetUsage,
	/// Open personal usage settings.
	ViewUsage,
	/// Open the selected workspace's usage settings.
	ViewWorkspaceUsage,
	/// Request credits from the workspace owner after explicit user action.
	NotifyOwner,
	/// Open a validated increase request URL, or request a usage-limit increase.
	RequestIncrease,
	/// Open Plus plan information.
	PlusPricing,
	/// Open Pro plan information.
	ProPricing,
	/// Open the plan-appropriate pricing dialog.
	Pricing,
}

#[derive(Deserialize)]
struct RawBanner {
	banner_type: String,
	title: String,
	description: String,
	ctas: Vec<RawCta>,
	reset_at: Option<i64>,
	model_slug: Option<String>,
	blocked_model_slug: Option<String>,
	#[serde(default)]
	fallback_model_slugs: Vec<String>,
	#[serde(default = "inline")]
	presentation: String,
	request_url: Option<String>,
}

#[derive(Deserialize)]
struct RawCta {
	action: String,
	label: String,
}

fn inline() -> String {
	"inline".into()
}

pub(crate) fn decode_banner(value: Option<&Value>) -> AccountApiBannerState {
	let Some(value) = value.filter(|value| !value.is_null()) else {
		return AccountApiBannerState::Absent;
	};
	parse_banner(value).map_or(AccountApiBannerState::Unsupported, |banner| {
		AccountApiBannerState::Available(Box::new(banner))
	})
}

fn parse_banner(value: &Value) -> Option<AccountApiBanner> {
	let raw: RawBanner = serde_json::from_value(value.clone()).ok()?;
	if !scalar(&raw.banner_type, 256)
		|| raw.title.trim().is_empty()
		|| raw.title.len() > 1024
		|| raw.title.lines().count() > 3
		|| raw.description.len() > 4096
		|| raw.description.lines().count() > 12
		|| raw.ctas.len() > 8
		|| raw.fallback_model_slugs.len() > 16
		|| !raw.fallback_model_slugs.iter().all(|s| scalar(s, 256))
		|| !raw.model_slug.as_deref().is_none_or(|s| scalar(s, 256))
		|| !raw.blocked_model_slug.as_deref().is_none_or(|s| scalar(s, 256))
		|| !matches!(raw.presentation.as_str(), "inline" | "dismissible")
	{
		return None;
	}
	let request_url = raw.request_url.as_deref().and_then(|value| {
		if value.len() > 4096 {
			return None;
		}
		let url = url::Url::parse(value).ok()?;
		(matches!(url.scheme(), "https" | "http")
			&& url.host_str().is_some()
			&& url.username().is_empty()
			&& url.password().is_none())
		.then(|| url.to_string())
	});
	let actions = raw
		.ctas
		.into_iter()
		.filter_map(|cta| {
			if !scalar(&cta.label, 256) {
				return None;
			}
			let action = action(&cta.action)?;
			if action == AccountApiBannerAction::RequestIncrease
				&& raw.request_url.is_some()
				&& request_url.is_none()
			{
				return None;
			}
			Some(AccountApiBannerCta { action, label: cta.label })
		})
		.collect();
	let copy = |s: String| s.chars().filter(|c| !c.is_control() || *c == '\n').collect();
	Some(AccountApiBanner {
		banner_type: raw.banner_type,
		title: copy(raw.title),
		description: copy(raw.description),
		actions,
		reset_at: raw.reset_at,
		model_slug: raw.model_slug,
		blocked_model_slug: raw.blocked_model_slug,
		fallback_model_slugs: raw.fallback_model_slugs,
		dismissible: raw.presentation == "dismissible",
		request_url,
	})
}

fn scalar(s: &str, limit: usize) -> bool {
	!s.trim().is_empty() && s.len() <= limit && !s.chars().any(char::is_control)
}

fn action(value: &str) -> Option<AccountApiBannerAction> {
	use AccountApiBannerAction as A;
	Some(match value {
		"add_credits" | "buy_credits" => A::AddCredits,
		"buy_reset" => A::BuyReset,
		"reset_usage" => A::ResetUsage,
		"view_usage" | "request_increase_usage_settings" => A::ViewUsage,
		"view_workspace_usage" | "increase_spend_cap" => A::ViewWorkspaceUsage,
		"notify_owner" | "contact_owner" => A::NotifyOwner,
		"request_increase" => A::RequestIncrease,
		"open_plus_pricing_web" => A::PlusPricing,
		"open_pro_pricing_web" => A::ProPricing,
		"open_pricing_dialog" => A::Pricing,
		_ => return None,
	})
}

#[cfg(test)]
mod tests {
	use super::{AccountApiBannerAction as A, AccountApiBannerState as S};
	use serde_json::json;

	#[test]
	fn full_usage_read_binds_banner_and_preserves_exact_model_scope() {
		let mut body = json!({"account_id":"a","user_id":"u","rate_limit":{},"rate_limit_upsell":{
			"banner_type":"model_limit","title":"Model unavailable","description":"Choose another model.",
			"blocked_model_slug":"blocked","model_slug":"replacement","fallback_model_slugs":["second","first"],
			"ctas":[{"action":"view_usage","label":"View usage"},{"action":"future_action","label":"Unknown"}]}});
		let read = |v: &serde_json::Value| {
			crate::decode_account_api_usage(v.to_string().as_bytes()).unwrap()
		};
		let usage = read(&body);
		let S::Available(banner) = usage.banner_for("a", "u") else {
			panic!("bounded banner");
		};
		assert_eq!(banner.blocked_model_slug.as_deref(), Some("blocked"));
		assert_eq!(banner.model_slug.as_deref(), Some("replacement"));
		assert_eq!(banner.fallback_model_slugs, ["second", "first"]);
		assert_eq!(banner.actions.len(), 1);
		assert_eq!(banner.actions[0].action, A::ViewUsage);
		assert_eq!(usage.banner_for("b", "u"), S::Unavailable);
		assert_eq!(usage.banner_for("a", "other"), S::Unavailable);
		body["rate_limit_upsell"] = json!({"unsupported":true});
		assert_eq!(read(&body).banner_for("a", "u"), S::Unsupported);
		body["rate_limit_upsell"] = serde_json::Value::Null;
		assert_eq!(read(&body).banner_for("a", "u"), S::Absent);
	}

	#[test]
	fn invalid_actions_cannot_become_urls_or_owner_notifications() {
		let mut raw = json!({"banner_type":"usage_limit","title":"Title","description":"Description","ctas":[
			{"action":"request_increase","label":"Request increase"},{"action":"view_usage","label":"bad\nlabel"}],
			"request_url":"javascript:alert(1)"});
		let S::Available(banner) = super::decode_banner(Some(&raw)) else {
			panic!("copy remains valid");
		};
		assert!(banner.actions.is_empty());
		assert!(banner.request_url.is_none());
		raw["request_url"] = json!("https://user:password@example.test/");
		let S::Available(banner) = super::decode_banner(Some(&raw)) else {
			panic!("copy remains valid");
		};
		assert!(banner.actions.is_empty());
		raw["request_url"] = json!("https://example.test/request");
		let S::Available(banner) = super::decode_banner(Some(&raw)) else {
			panic!("copy remains valid");
		};
		assert_eq!(banner.actions.len(), 1);
		for (key, value) in [
			("title", json!("x".repeat(1025))),
			("presentation", json!("future")),
			("fallback_model_slugs", json!(vec!["x"; 17])),
		] {
			let mut invalid = raw.clone();
			invalid[key] = value;
			assert_eq!(super::decode_banner(Some(&invalid)), S::Unsupported);
		}
	}
}
