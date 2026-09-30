//! Native skill discovery for an existing Agent or a new conversation's selected account.
use serde::{Deserialize, Serialize};

use crate::{ConversationWorkingDirectory, EntityId, InitialModelCatalogRequest, WireText};

/// One enabled native skill; this is a usage reference, not a plugin management entry.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSkillDto {
	/// Exact native skill name, including its namespace when supplied.
	pub name: WireText,
	/// Native description shortened for the picker.
	pub description: WireText,
	/// Exact native skill document path.
	pub path: ConversationWorkingDirectory,
}

/// Bind discovery to the same account policy and project as the intended input.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentSkillsTarget {
	/// Read through the current owner of an existing native conversation.
	Existing {
		/// Exact Agent work identity.
		work_id: EntityId,
	},
	/// Inspect the account and directory selected for a new Agent without creating a thread.
	New {
		/// Same account selection and directory used by initial model discovery.
		request: InitialModelCatalogRequest,
	},
}

/// Bounded searchable projection of the native skills inventory.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSkillsPage {
	/// Matching enabled candidates, ordered by name and path.
	pub skills: Vec<AgentSkillDto>,
	/// More matches exist; refine the filter to find them.
	pub truncated: bool,
	/// Native discovery failures, without copying private diagnostics into the picker.
	pub errors: u32,
}

/// A source-checked skill inventory. Reading it never installs or enables a skill.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentSkillsResult {
	/// Current source could not be read completely.
	Unavailable,
	/// A complete source observation, with a bounded result page.
	Available {
		/// Echoed source for client-side comparison.
		target: AgentSkillsTarget,
		/// Public skill metadata only.
		page: AgentSkillsPage,
	},
}
