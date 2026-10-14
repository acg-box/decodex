//! Native context defaults for new conversations.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// Context features exposed in task preferences.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextFeature {
	/// Prepare a compaction result while a conversation continues.
	AsyncCompaction,
	/// Preserve selected client tool instructions across compaction.
	RetainClientToolOutputs,
}

/// One native preference and its effective project default.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContextPreference {
	/// Native feature to configure.
	pub feature: AgentContextFeature,
	/// Effective configuration for a new conversation, not a running task's state.
	pub effective: bool,
	/// Explicit user preference. A project or managed layer can override this value.
	pub preference: Option<bool>,
}

/// Context settings bound to the task's current native source.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentContextSettingsResult {
	/// The current native source or configuration could not be read.
	Unavailable,
	/// Reviewed native configuration and version identity.
	Available {
		/// Owning task.
		work_id: EntityId,
		/// Native source and configuration version identity.
		review_token: WireText,
		/// The supported context preferences.
		preferences: Vec<AgentContextPreference>,
	},
}
