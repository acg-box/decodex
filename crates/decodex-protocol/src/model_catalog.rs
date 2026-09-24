//! Account-scoped model discovery before a conversation exists.
use serde::{Deserialize, Serialize};

use crate::{
	ChiefModelDto, ConversationModel, ConversationReasoningEffort, ConversationWorkingDirectory,
	EntityId, ServiceTier,
};

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
		/// Native defaults, absent when an older service cannot provide them.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		defaults: Option<Box<InitialModelDefaults>>,
	},
	/// A complete, current observation could not be obtained.
	Unavailable,
}

/// Native defaults observed without creating a conversation or overriding user choices.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialExecutionDefaults {
	/// Model from the corresponding native configuration layer.
	pub model: Option<ConversationModel>,
	/// Reasoning effort from that layer, before model-specific resolution.
	pub reasoning_effort: Option<ConversationReasoningEffort>,
	/// Service tier from that layer; null means no configured override.
	pub service_tier: Option<ServiceTier>,
}

/// Separate sources of defaults; managed defaults do not enforce user selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialModelDefaults {
	/// Effective account and directory configuration.
	pub configured: InitialExecutionDefaults,
	/// Managed new-thread defaults; explicit model or effort opts out of that pair.
	pub managed: InitialExecutionDefaults,
	/// Provider catalog default, used only when configuration does not select a model.
	pub catalog_model: Option<ConversationModel>,
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn old_catalogs_remain_readable_and_default_sources_round_trip() {
		let mut wire = json!({"outcome":"available","account_id":"account","account_revision":1,"working_directory":"/tmp","models":[]});
		let old: InitialModelCatalogResult = serde_json::from_value(wire.clone()).unwrap();
		assert!(matches!(old, InitialModelCatalogResult::Available { defaults: None, .. }));
		assert_eq!(serde_json::to_value(old).unwrap(), wire);
		wire["defaults"] = json!({"configured":{"model":"project-model","reasoning_effort":"future-effort","service_tier":"flex"},"managed":{"model":"managed-model","reasoning_effort":null,"service_tier":null},"catalog_model":"catalog-model"});
		let projected: InitialModelCatalogResult = serde_json::from_value(wire.clone()).unwrap();
		assert_eq!(serde_json::to_value(projected).unwrap(), wire);
		wire["defaults"]["configured"]["reasoning_effort"] = json!(42);
		assert!(serde_json::from_value::<InitialModelCatalogResult>(wire).is_err());
	}
}
