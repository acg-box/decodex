//! Account-bound backend recovery copy, independent of profile and reset-card availability.

use crate::{EntityId, EntityRevision, WireText};
use serde::{Deserialize, Serialize};

/// One account's daemon-owned recovery observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRecoveryResult {
	/// Requested account; never the backend's nested untrusted identity.
	pub account_id: EntityId,
	/// Exact account revision requested by the client.
	pub account_revision: EntityRevision,
	/// Time of the last accepted full usage response, if known.
	pub observed_at_unix_micros: Option<i64>,
	/// Freshness and supported content are separate from account permission.
	pub state: AccountRecoveryState,
}

/// Banner availability; only Current carries actionable fresh copy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "banner", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountRecoveryState {
	/// No matching observation is available.
	Unavailable,
	/// A fresh matching full response explicitly has no banner.
	Absent,
	/// A fresh response has a banner with an unsupported shape.
	Unsupported,
	/// Fresh matching content; individual actions still require source revalidation.
	Current(Box<AccountRecoveryBanner>),
	/// Retained copy after a failed or expired read; its actions must not execute.
	Stale(Box<AccountRecoveryBanner>),
}

/// Bounded backend copy and model scope. Suggestions never change models by themselves.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRecoveryBanner {
	/// Backend occurrence category.
	pub banner_type: WireText,
	/// Display title.
	pub title: WireText,
	/// Display description.
	pub description: WireText,
	/// Unix reset time in seconds; not permission to resume.
	pub reset_at: Option<i64>,
	/// Model described by this copy.
	pub model_slug: Option<WireText>,
	/// Exact blocked model; does not block other models or accounts.
	pub blocked_model_slug: Option<WireText>,
	/// Ordered ordinary fallback instructions requiring task-bound automatic recovery.
	pub fallback_model_slugs: Vec<WireText>,
	/// Whether the user can dismiss this occurrence.
	pub dismissible: bool,
	/// Ordered known actions with backend labels.
	pub actions: Vec<AccountRecoveryCta>,
	/// Validated optional request-increase URL; never an executable command.
	pub request_url: Option<WireText>,
}

/// One explicitly named backend action; effects are owned by the account service.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRecoveryCta {
	/// Known action category.
	pub action: AccountRecoveryAction,
	/// Backend display label.
	pub label: WireText,
}

/// Closed action vocabulary; receiving a result executes none of these actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRecoveryAction {
	/// Open credits settings.
	AddCredits,
	/// Open reset purchase information.
	BuyReset,
	/// Open the existing reset picker.
	ResetUsage,
	/// Open personal usage settings.
	ViewUsage,
	/// Open this workspace's usage settings.
	ViewWorkspaceUsage,
	/// Ask the workspace owner for credits after explicit action.
	NotifyOwner,
	/// Request a usage-limit increase.
	RequestIncrease,
	/// Open Plus information.
	PlusPricing,
	/// Open Pro information.
	ProPricing,
	/// Open pricing for the current plan.
	Pricing,
}

impl AccountRecoveryResult {
	/// Select the first different ordinary model in backend order for this exact account revision.
	/// `models` must be the complete visible native catalog for the same account/process.
	/// This does not authorize an effect: the owner must also check freshness, task ownership,
	/// native authentication, pending recovery, and concurrent manual settings before dispatch.
	pub fn ordinary_fallback_model<'a>(
		&self,
		account: &EntityId,
		revision: EntityRevision,
		current_model: &str,
		models: &'a [crate::ChiefModelDto],
	) -> Option<&'a crate::ChiefModelDto> {
		let AccountRecoveryState::Current(banner) = &self.state else {
			return None;
		};
		if !self.valid_for(account, revision)
			|| banner.banner_type.as_str() == "luna_reserve"
			|| current_model == "gpt-reserve"
			|| banner.blocked_model_slug.as_ref().map(WireText::as_str) != Some(current_model)
		{
			return None;
		}
		banner.fallback_model_slugs.iter().find_map(|candidate| {
			let candidate = candidate.as_str();
			if candidate == current_model || candidate == "gpt-reserve" {
				return None;
			}
			models.iter().find(|model| model.model.as_str() == candidate)
		})
	}

	/// Whether this bounded current source explicitly offers this native notification.
	pub fn allows_nudge(&self, action: AccountRecoveryAction) -> bool {
		let AccountRecoveryState::Current(banner) = &self.state else {
			return false;
		};
		self.valid_for(&self.account_id, self.account_revision)
			&& banner.actions.iter().any(|cta| cta.action == action)
			&& (action == AccountRecoveryAction::NotifyOwner
				|| (action == AccountRecoveryAction::RequestIncrease
					&& banner.request_url.is_none()))
	}

	/// Validate the exact requested source and bounded display content.
	/// Freshness and effect-time authorization require separate checks.
	pub fn valid_for(&self, account: &EntityId, revision: EntityRevision) -> bool {
		if &self.account_id != account
			|| self.account_revision != revision
			|| revision.0 == 0
			|| self.observed_at_unix_micros.is_some_and(|at| at <= 0)
		{
			return false;
		}
		match &self.state {
			AccountRecoveryState::Unavailable => true,
			AccountRecoveryState::Absent | AccountRecoveryState::Unsupported =>
				self.observed_at_unix_micros.is_some(),
			AccountRecoveryState::Current(banner) | AccountRecoveryState::Stale(banner) => {
				let scalar = |v: &WireText, max| {
					!v.as_str().trim().is_empty()
						&& v.as_str().len() <= max
						&& !v.as_str().chars().any(char::is_control)
				};
				self.observed_at_unix_micros.is_some()
					&& scalar(&banner.banner_type, 256)
					&& !banner.title.as_str().trim().is_empty()
					&& banner.title.as_str().len() <= 1024
					&& banner.title.as_str().lines().count() <= 3
					&& banner.title.as_str().chars().all(|c| !c.is_control() || c == '\n')
					&& banner.description.as_str().len() <= 4096
					&& banner.description.as_str().lines().count() <= 12
					&& banner.description.as_str().chars().all(|c| !c.is_control() || c == '\n')
					&& banner.actions.len() <= 8
					&& banner.actions.iter().all(|a| scalar(&a.label, 256))
					&& banner.fallback_model_slugs.len() <= 16
					&& banner.fallback_model_slugs.iter().all(|s| scalar(s, 256))
					&& banner.model_slug.as_ref().is_none_or(|s| scalar(s, 256))
					&& banner.blocked_model_slug.as_ref().is_none_or(|s| scalar(s, 256))
					&& banner.request_url.as_ref().is_none_or(|value| {
						scalar(value, 4096)
							&& url::Url::parse(value.as_str()).is_ok_and(|url| {
								matches!(url.scheme(), "https" | "http")
									&& url.host_str().is_some() && url.username().is_empty()
									&& url.password().is_none()
							})
					})
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn ordinary_fallback_uses_backend_order_and_exact_current_source() {
		let account = EntityId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let model = |name: &str| crate::ChiefModelDto {
			model: crate::ConversationModel::new(name).unwrap(),
			name: name.into(),
			efforts: vec![crate::ConversationReasoningEffort::Medium],
			default_effort: Some(crate::ConversationReasoningEffort::Medium),
			supports_fast: false,
			service_tiers: Vec::new(),
			default_service_tier: None,
			available_cyber_programs: None,
			supports_images: false,
			availability: None,
			upgrade: None,
		};
		// Catalog order must not replace backend order. Hidden models are omitted upstream.
		let catalog = vec![model("last"), model("first"), model("old"), model("gpt-reserve")];
		let banner = AccountRecoveryBanner {
			banner_type: WireText::new("model_recovery").unwrap(),
			title: WireText::new("Limit").unwrap(),
			description: WireText::new("Choose an available model").unwrap(),
			reset_at: None,
			model_slug: None,
			blocked_model_slug: Some(WireText::new("old").unwrap()),
			fallback_model_slugs: ["hidden", "missing", "old", "gpt-reserve", "first", "last"]
				.into_iter()
				.map(|m| WireText::new(m).unwrap())
				.collect(),
			dismissible: false,
			actions: Vec::new(),
			request_url: None,
		};
		let original = AccountRecoveryResult {
			account_id: account.clone(),
			account_revision: EntityRevision(7),
			observed_at_unix_micros: Some(1),
			state: AccountRecoveryState::Current(Box::new(banner.clone())),
		};
		let choose = |result: &AccountRecoveryResult, current: &str| {
			result
				.ordinary_fallback_model(&account, EntityRevision(7), current, &catalog)
				.map(|m| m.model.as_str())
		};
		assert_eq!(choose(&original, "old"), Some("first"));
		assert_eq!(
			choose(&original, "first"),
			None,
			"recovery must not switch an already selected replacement"
		);
		assert_eq!(choose(&original, "other"), None);
		assert_eq!(choose(&original, "gpt-reserve"), None);
		assert!(
			original
				.ordinary_fallback_model(&account, EntityRevision(8), "old", &catalog)
				.is_none()
		);
		let other = EntityId::new("10000000-0000-4000-8000-000000000002").unwrap();
		assert!(
			original.ordinary_fallback_model(&other, EntityRevision(7), "old", &catalog).is_none()
		);
		for state in [
			AccountRecoveryState::Stale(Box::new(banner.clone())),
			AccountRecoveryState::Absent,
			AccountRecoveryState::Unavailable,
			AccountRecoveryState::Unsupported,
		] {
			let mut result = original.clone();
			result.state = state;
			assert_eq!(choose(&result, "old"), None);
		}
		for change in ["reserve", "no_blocked", "no_candidates", "only_ineligible"] {
			let mut next = banner.clone();
			match change {
				"reserve" => next.banner_type = WireText::new("luna_reserve").unwrap(),
				"no_blocked" => next.blocked_model_slug = None,
				"no_candidates" => next.fallback_model_slugs.clear(),
				_ =>
					next.fallback_model_slugs = vec![
						WireText::new("missing").unwrap(),
						WireText::new("old").unwrap(),
						WireText::new("gpt-reserve").unwrap(),
					],
			}
			let mut result = original.clone();
			result.state = AccountRecoveryState::Current(Box::new(next));
			assert_eq!(choose(&result, "old"), None, "{change}");
		}
	}
	#[test]
	fn client_validation_rejects_foreign_sources_and_unsafe_actions() {
		let account = EntityId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let banner = AccountRecoveryBanner {
			banner_type: WireText::new("limit").unwrap(),
			title: WireText::new("Title").unwrap(),
			description: WireText::new("Description").unwrap(),
			reset_at: None,
			model_slug: None,
			blocked_model_slug: None,
			fallback_model_slugs: Vec::new(),
			dismissible: false,
			actions: Vec::new(),
			request_url: None,
		};
		let mut result = AccountRecoveryResult {
			account_id: account.clone(),
			account_revision: EntityRevision(1),
			observed_at_unix_micros: Some(100),
			state: AccountRecoveryState::Current(Box::new(banner)),
		};
		assert!(result.valid_for(&account, EntityRevision(1)));
		assert!(!result.valid_for(&account, EntityRevision(2)));
		assert!(!result.valid_for(
			&EntityId::new("10000000-0000-4000-8000-000000000002").unwrap(),
			EntityRevision(1)
		));
		let AccountRecoveryState::Current(banner) = &mut result.state else { panic!("current") };
		banner.request_url = Some(WireText::new("javascript:alert(1)").unwrap());
		assert!(!result.valid_for(&account, EntityRevision(1)));
	}
}

/// Prepared destination after the daemon revalidates a displayed recovery action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountRecoveryPreparation {
	/// Source is unavailable, stale, changed, or no longer offers this action.
	Unavailable,
	/// No side effect has occurred; the client may continue the explicit interaction.
	Ready {
		/// Fresh account-bound source used for preparation.
		source: Box<AccountRecoveryResult>,
		/// Exact requested action.
		action: AccountRecoveryAction,
		/// Native-equivalent destination, not a completed effect.
		destination: AccountRecoveryDestination,
	},
}

/// Finite account recovery destinations; none execute during a read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountRecoveryDestination {
	/// Open a validated HTTP(S) address after the user's action.
	OpenUrl(WireText),
	/// Open the existing inventory picker and explicit reset confirmation.
	ResetPicker,
	/// Explicitly request credits from the workspace owner through the account effect owner.
	RequestCredits,
	/// Explicitly request a workspace usage-limit increase through the account effect owner.
	RequestUsageIncrease,
}

impl AccountRecoveryPreparation {
	/// Validate the prepared source, action and destination against the explicit selection.
	pub fn valid_for(
		&self,
		expected: &AccountRecoveryResult,
		requested: AccountRecoveryAction,
	) -> bool {
		let Self::Ready { source, action, destination } = self else {
			return true;
		};
		if *action != requested
			|| !source.valid_for(&expected.account_id, expected.account_revision)
			|| source.observed_at_unix_micros < expected.observed_at_unix_micros
		{
			return false;
		}
		let (AccountRecoveryState::Current(before), AccountRecoveryState::Current(after)) =
			(&expected.state, &source.state)
		else {
			return false;
		};
		if before != after || !after.actions.iter().any(|cta| cta.action == requested) {
			return false;
		}
		match destination {
			AccountRecoveryDestination::ResetPicker =>
				requested == AccountRecoveryAction::ResetUsage,
			AccountRecoveryDestination::RequestCredits =>
				requested == AccountRecoveryAction::NotifyOwner,
			AccountRecoveryDestination::RequestUsageIncrease =>
				requested == AccountRecoveryAction::RequestIncrease && after.request_url.is_none(),
			AccountRecoveryDestination::OpenUrl(value) =>
				!matches!(
					requested,
					AccountRecoveryAction::ResetUsage | AccountRecoveryAction::NotifyOwner
				) && (requested != AccountRecoveryAction::RequestIncrease
					|| after.request_url.is_some())
					&& value.as_str().len() <= 4096
					&& !value.as_str().chars().any(char::is_control)
					&& url::Url::parse(value.as_str()).is_ok_and(|url| {
						matches!(url.scheme(), "https" | "http")
							&& url.host_str().is_some()
							&& url.username().is_empty()
							&& url.password().is_none()
					}),
		}
	}
}

#[cfg(test)]
mod preparation_tests {
	use super::*;
	#[test]
	fn preparation_rejects_changed_source_action_and_effect_kind() {
		let source = AccountRecoveryResult {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: EntityRevision(1),
			observed_at_unix_micros: Some(100),
			state: AccountRecoveryState::Current(Box::new(AccountRecoveryBanner {
				banner_type: WireText::new("limit").unwrap(),
				title: WireText::new("Limit").unwrap(),
				description: WireText::new("Description").unwrap(),
				reset_at: None,
				model_slug: None,
				blocked_model_slug: None,
				fallback_model_slugs: Vec::new(),
				dismissible: false,
				actions: vec![AccountRecoveryCta {
					action: AccountRecoveryAction::ResetUsage,
					label: WireText::new("Reset").unwrap(),
				}],
				request_url: None,
			})),
		};
		let mut prepared = AccountRecoveryPreparation::Ready {
			source: Box::new(source.clone()),
			action: AccountRecoveryAction::ResetUsage,
			destination: AccountRecoveryDestination::ResetPicker,
		};
		assert!(prepared.valid_for(&source, AccountRecoveryAction::ResetUsage));
		assert!(!prepared.valid_for(&source, AccountRecoveryAction::NotifyOwner));
		let mut changed = source.clone();
		changed.account_revision = EntityRevision(2);
		assert!(!prepared.valid_for(&changed, AccountRecoveryAction::ResetUsage));
		let AccountRecoveryPreparation::Ready { destination, .. } = &mut prepared else {
			unreachable!()
		};
		*destination = AccountRecoveryDestination::RequestCredits;
		assert!(!prepared.valid_for(&source, AccountRecoveryAction::ResetUsage));
		let AccountRecoveryPreparation::Ready { destination, .. } = &mut prepared else {
			unreachable!()
		};
		*destination =
			AccountRecoveryDestination::OpenUrl(WireText::new("https://chatgpt.com/").unwrap());
		assert!(!prepared.valid_for(&source, AccountRecoveryAction::ResetUsage));
	}
}

/// Durable outcome of one explicit workspace-owner notification attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRecoveryNudgeStatus {
	/// The native backend confirmed the notification.
	Sent,
	/// The native backend reports an existing cooldown.
	CooldownActive,
	/// The source or native account process was unavailable before submission.
	Unavailable,
	/// Native does not implement the operation; no alternate effect was sent.
	Unsupported,
	/// Delivery is uncertain. This operation must not be automatically resent.
	Uncertain,
}

/// Durable readback for the selected account and notification purpose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountRecoveryNudgeResult {
	/// Product state could not be read. This is not permission to resend.
	Unavailable,
	/// No matching receipt is known.
	NotFound,
	/// Exact matching latest or requested operation.
	Found(AccountRecoveryNudgeOperation),
}

/// Bounded credential-negative notification receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRecoveryNudgeOperation {
	/// Affected local account.
	pub account_id: EntityId,
	/// Account revision at the original attempt.
	pub account_revision: EntityRevision,
	/// Exact notification purpose.
	pub action: AccountRecoveryAction,
	/// Stable original operation identity.
	pub operation_key: crate::IdempotencyKey,
	/// Durable reservation time in Unix microseconds.
	pub reserved_at_unix_micros: i64,
	/// Last known delivery outcome, including unfinished uncertain claims.
	pub outcome: AccountRecoveryNudgeStatus,
}
