//! Explicit review of shared native hook configuration.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};
/// Explicit change to one reviewed hook.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "enabled", rename_all = "snake_case")]
pub enum ChiefHookChange {
	/// Trust the exact reviewed content hash.
	Trust,
	/// Save the shared enabled override.
	Enabled(bool),
}
/// Complete display facts for one native hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefHookDto {
	/// Native hook identity.
	pub key: WireText,
	/// Effective native trust status, including future unknown values.
	pub trust_status: String,
	/// Effective enabled state, distinct from saved override.
	pub enabled: bool,
	/// Native policy ownership.
	pub managed: bool,
	/// Hash whose full handler metadata is shown for consent.
	pub current_hash: WireText,
	/// Explicit saved override; None means inherited.
	pub saved_enabled: Option<bool>,
	/// Explicit saved hash; None does not mean trusted.
	pub saved_hash: Option<String>,
	/// Complete metadata JSON from a native inventory bounded to 256 KiB.
	pub details: String,
}
/// Last write to this shared config, possibly initiated from another task.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefHookEditReceipt {
	/// Reserved, saved, overridden, rejected, unknown, target_observed or superseded.
	pub outcome: String,
	/// Hook identity of the original request.
	pub hook: WireText,
	/// Original local task.
	pub work_id: EntityId,
	/// Original account.
	pub account_id: EntityId,
}
/// Current owner-bound hook review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefHookSettingsState {
	/// Effective hooks, raw overrides and independent durable request result.
	Available {
		/// Exact local work.
		work_id: EntityId,
		/// Exact native thread used for directory discovery.
		thread_id: EntityId,
		/// Consumed once when submitting an action.
		review_token: WireText,
		/// Native writable config shared by tasks that use this file.
		config_file: WireText,
		/// Native hook metadata without truncation.
		hooks: Vec<ChiefHookDto>,
		/// Native discovery warnings and errors, preserved for review.
		notices: Vec<String>,
		/// False while another edit is unresolved or this task cannot authorize writes.
		can_update: bool,
		/// Shared config's latest write, not an assertion of effective hook behavior.
		last_edit: Option<Box<ChiefHookEditReceipt>>,
	},
	/// Current native source or complete metadata cannot be established.
	Unavailable,
}
