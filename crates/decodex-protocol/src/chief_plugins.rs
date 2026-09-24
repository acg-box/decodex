//! Native task exclusions, separate from shared plugin installation and configuration.
use crate::{ChiefPluginInventory, EntityId, WireText};
use serde::{Deserialize, Serialize};

/// Durable request outcome, not proof that an active turn changed its capabilities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefPluginOutcome {
	/// Reserved before the native write.
	Reserved,
	/// Accepted for processing; await a native settings observation.
	Queued,
	/// Delivery is uncertain and must not be replayed.
	Unknown,
	/// Rejected before dispatch or by native validation.
	Rejected,
	/// Current native settings report the requested selection for subsequent turns.
	TargetObserved,
	/// A replacement native owner reports another selection after confirmed process death.
	Superseded,
}

/// Review facts for a task-local plugin selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefPluginSelectionState {
	/// Saved selection and independent shared installation metadata.
	Available {
		/// Exact local work.
		work_id: EntityId,
		/// Exact native thread.
		thread_id: EntityId,
		/// Opaque source, observation and catalog identity.
		review_token: WireText,
		/// Canonical plugin IDs excluded from subsequent turns.
		disabled_plugin_ids: Vec<WireText>,
		/// Shared discovery result; it does not report task eligibility.
		catalog: ChiefPluginInventory,
		/// False when work is not editable.
		can_update: bool,
		/// Last settled selection attempt, if any.
		last_outcome: Option<ChiefPluginOutcome>,
	},
	/// A durable request remains unconfirmed. Do not submit another selection.
	Pending {
		/// Requested complete selection, not an effective-runtime assertion.
		disabled_plugin_ids: Vec<WireText>,
		/// Current unresolved receipt state.
		state: ChiefPluginOutcome,
	},
	/// Current source-bound native facts cannot be established.
	Unavailable,
}
