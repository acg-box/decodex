//! Voice preferences for the next native conversation.
use serde::{Deserialize, Serialize};

/// Current task-scoped voice selection and configuration identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefVoiceSettingsResult {
	/// The owning native source could not be read.
	Unavailable,
	/// Native catalog and effective project selection.
	Available {
		/// Task that owns these settings.
		work_id: crate::EntityId,
		/// Source and native configuration version identity.
		review_token: crate::WireText,
		/// Native catalog or upstream fallback choices for V3 voice conversations.
		voices: Vec<crate::WireText>,
		/// Effective selection for this project.
		effective: Option<crate::WireText>,
		/// Saved user preference, which a project can override.
		preference: Option<crate::WireText>,
	},
}
