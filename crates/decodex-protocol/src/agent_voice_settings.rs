//! Voice settings for the connected ChatGPT account.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// An available account voice.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentVoiceOption {
	/// ID sent to the account when selected.
	pub voice: WireText,
	/// Name displayed in the voice picker.
	pub name: WireText,
}

/// Voice selection for the account connected to the current task.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentVoiceSettingsResult {
	/// The owning native source could not be read.
	Unavailable,
	/// The connected account supplied the voice selection and catalog.
	Available {
		/// Task used to route the account request.
		work_id: EntityId,
		/// Identity of the source and reviewed selection.
		review_token: WireText,
		/// Available account voices.
		voices: Vec<AgentVoiceOption>,
		/// Current account voice ID.
		selected: WireText,
	},
}
