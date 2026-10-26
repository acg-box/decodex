//! Exact local message delivery state, separate from the editable input.
use crate::{AgentActionDto, EntityId, WireText, desktop_drafts};
use serde::{Deserialize, Serialize};

/// Local knowledge of one input. Unconfirmed input never authorizes a retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopMessageState {
	/// Persisted before attempting service delivery.
	Sending,
	/// The service may have received this input.
	Unconfirmed,
	/// The service positively refused, or the transport did not submit this input.
	Failed,
}

/// One submitted input, without unrelated editors or profile snapshots.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopMessage {
	/// Exact original operation, including its attachments and execution choices.
	pub action: AgentActionDto,
	/// Original conversation, absent only before its creation.
	pub owner: Option<EntityId>,
	/// Native thread captured before submission, absent before conversation creation.
	pub thread_id: Option<WireText>,
	/// Complete original visible input.
	pub text: String,
	/// Current delivery knowledge.
	pub state: DesktopMessageState,
	/// The current failure reason, if available.
	pub error: Option<String>,
}
impl DesktopMessage {
	pub(super) fn validate(&self) -> Result<(), &'static str> {
		desktop_drafts::validate_text(&self.text)?;
		if let Some(error) = &self.error {
			desktop_drafts::validate_text(error)?;
		}
		if !matches!(
			&self.action,
			AgentActionDto::Start(_)
				| AgentActionDto::StartConfigured { .. }
				| AgentActionDto::NewConversation { .. }
				| AgentActionDto::Send { .. }
				| AgentActionDto::SendConfigured { .. }
				| AgentActionDto::Steer { .. }
				| AgentActionDto::NativeAgentInput { .. }
		) {
			return Err("Saved message is not an input operation");
		}
		Ok(())
	}
}
