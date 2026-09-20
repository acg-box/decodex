//! Account-scoped model discovery before a conversation exists.
use serde::{Deserialize, Serialize};

use crate::{ChiefModelDto, ConversationWorkingDirectory, EntityId};

/// Select the same account policy that the intended conversation will use.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogPurpose {
	/// Ordinary conversation routing settings.
	Conversation,
	/// Chief account selection, optionally with an explicit account preference.
	Chief,
}

/// Metadata inspection without creating a thread or sending a turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialModelCatalogRequest {
	/// Directory whose native provider configuration applies.
	pub working_directory: ConversationWorkingDirectory,
	/// Account policy for the intended task.
	pub purpose: ModelCatalogPurpose,
	/// Explicit Chief account preference; ordinary routing uses its saved policy.
	pub account_id: Option<EntityId>,
}

/// A complete native catalog with its observed account and directory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum InitialModelCatalogResult {
	/// The native query and process cleanup completed with unchanged account identity.
	Available {
		/// Exact selected local account.
		account_id: EntityId,
		/// Account revision verified after the native process closed.
		account_revision: i64,
		/// Directory used for native configuration resolution.
		working_directory: ConversationWorkingDirectory,
		/// Visible models projected from the complete native catalog.
		models: Vec<ChiefModelDto>,
	},
	/// A complete, current observation could not be obtained.
	Unavailable,
}
