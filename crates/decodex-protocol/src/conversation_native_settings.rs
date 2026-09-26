//! Last observed native session configuration; never a next-turn command.
use crate::{
	ConversationContractError, ConversationModel, ConversationReasoningEffort, EntityId,
	EntityRevision,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Source-bound historical response facts. Presence does not mean the process is live.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationNativeSettings {
	/// Native current model at the time of the response.
	pub model: ConversationModel,
	/// Native provider identifier, not a display label or API URL.
	pub model_provider: String,
	/// Native server-host directory at the time of the response.
	pub cwd: String,
	/// Native configured effort, absent if unreported.
	pub reasoning_effort: Option<ConversationReasoningEffort>,
	/// Time the daemon saved the response, in Unix microseconds.
	pub observed_at_micros: i64,
	/// Local account that owned the native process.
	pub source_account_id: EntityId,
	/// Exact account revision used by the native process.
	pub source_account_revision: EntityRevision,
	/// Exact process that supplied the response; may have since exited.
	pub source_process_generation_id: EntityId,
}
impl ConversationNativeSettings {
	/// Validate bounded observation data without granting execution or filesystem authority.
	pub fn validate(self) -> Result<Self, ConversationContractError> {
		if self.model_provider.trim().is_empty()
			|| self.model_provider.len() > 512
			|| self.model_provider.chars().any(char::is_control)
			|| self.cwd.is_empty()
			|| self.cwd.len() > 4096
			|| self.cwd.chars().any(char::is_control)
			|| !self.cwd.starts_with('/')
			|| self.observed_at_micros <= 0
			|| self.source_account_revision.0 == 0
			|| !crate::conversation::is_canonical_uuid_v4(self.source_account_id.as_str())
			|| !crate::conversation::is_canonical_uuid_v4(
				self.source_process_generation_id.as_str(),
			) {
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(self)
	}
}
impl<'de> Deserialize<'de> for ConversationNativeSettings {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			model: ConversationModel,
			model_provider: String,
			cwd: String,
			reasoning_effort: Option<ConversationReasoningEffort>,
			observed_at_micros: i64,
			source_account_id: EntityId,
			source_account_revision: EntityRevision,
			source_process_generation_id: EntityId,
		}
		let raw = Raw::deserialize(deserializer)?;
		Self {
			model: raw.model,
			model_provider: raw.model_provider,
			cwd: raw.cwd,
			reasoning_effort: raw.reasoning_effort,
			observed_at_micros: raw.observed_at_micros,
			source_account_id: raw.source_account_id,
			source_account_revision: raw.source_account_revision,
			source_process_generation_id: raw.source_process_generation_id,
		}
		.validate()
		.map_err(D::Error::custom)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn observations_round_trip_but_do_not_attach_to_unbound_conversations() {
		let settings = json!({"model":"native-model","model_provider":"native-provider","cwd":"/native/project","reasoning_effort":"ultra","observed_at_micros":42,"source_account_id":"10000000-0000-4000-8000-000000000001","source_account_revision":3,"source_process_generation_id":"20000000-0000-4000-8000-000000000001"});
		let mut summary = json!({"conversation_id":"30000000-0000-4000-8000-000000000001","title":"Task","codex_thread_id":"native-thread","conversation_revision":1,"projection_updated_at_micros":43,"runtime_session_id":"40000000-0000-4000-8000-000000000001","runtime_session_revision":3,"state":"ready","native_settings":settings});
		let decoded: crate::ConversationSummary = serde_json::from_value(summary.clone()).unwrap();
		assert_eq!(serde_json::to_value(decoded).unwrap()["native_settings"], settings);
		summary["codex_thread_id"] = serde_json::Value::Null;
		assert!(serde_json::from_value::<crate::ConversationSummary>(summary).is_err());
		for (field, value) in [
			("model_provider", json!(" ")),
			("cwd", json!("relative")),
			("observed_at_micros", json!(0)),
			("source_account_revision", json!(0)),
			("source_process_generation_id", json!("foreign")),
		] {
			let mut invalid = settings.clone();
			invalid[field] = value;
			assert!(serde_json::from_value::<ConversationNativeSettings>(invalid).is_err());
		}
	}
}
