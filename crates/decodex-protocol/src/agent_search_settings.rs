//! Search preferences for new native conversations.
use serde::{Deserialize, Serialize};

/// Current project-scoped search default and configuration identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentSearchSettingsResult {
	/// The owning native source could not be read.
	Unavailable,
	/// Allowed modes and effective project default.
	Available {
		/// Task that owns these settings.
		work_id: crate::EntityId,
		/// Source and native configuration version identity.
		review_token: crate::WireText,
		/// Modes allowed by native requirements.
		modes: Vec<crate::WireText>,
		/// Effective selection for this project.
		effective: Option<crate::WireText>,
		/// Saved user preference, which a project can override.
		preference: Option<crate::WireText>,
	},
}
