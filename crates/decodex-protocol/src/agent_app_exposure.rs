//! Connector exposure preferences, independent of connected-account approval policy.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// A native model-facing tool surface that a connector preference can omit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExposureSurface {
	/// Tools callable from Code Mode scripts.
	CodeMode,
	/// Tools discovered through tool search.
	Deferred,
	/// Tools in the model's initial tool list.
	Direct,
}
impl AgentToolExposureSurface {
	/// Native configuration value.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::CodeMode => "code_mode",
			Self::Deferred => "deferred",
			Self::Direct => "direct",
		}
	}
}

/// Configuration facts for one connector, not a claim about a live model step.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentAppExposureResult {
	/// Current source, native inventory and configuration were verified.
	Available {
		/// Owning local task.
		work_id: EntityId,
		/// Exact native connector identity.
		connector_id: WireText,
		/// Source-bound native configuration review.
		review_token: WireText,
		/// Effective connector omissions, preserving future values.
		effective: Option<Vec<String>>,
		/// Writable omissions. None inherits; an empty list explicitly clears them.
		preference: Option<Vec<String>>,
		/// This exact review has not already been submitted.
		can_update: bool,
		/// Last durable write state, independent of the current configuration.
		last_outcome: Option<String>,
	},
	/// Source, inventory or configuration could not be verified.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use crate::{
		AgentActionDto, AgentAppExposureResult, AgentToolExposureSurface, EntityId, WireText,
	};

	#[test]
	fn exposure_wire_preserves_inheritance_and_rejects_unknown_write_surfaces() {
		for omit in [None, Some(vec![]), Some(vec![AgentToolExposureSurface::Deferred])] {
			let action = AgentActionDto::SetAppToolExposure {
				work_id: EntityId::new("root").unwrap(),
				connector_id: WireText::new("calendar").unwrap(),
				review_token: WireText::new("a".repeat(64)).unwrap(),
				omit: omit.clone(),
			};
			let mut value = serde_json::to_value(&action).unwrap();

			assert_eq!(value["data"]["omit"], serde_json::json!(omit));

			let decoded: AgentActionDto = serde_json::from_value(value.clone()).unwrap();

			assert!(
				matches!(decoded, AgentActionDto::SetAppToolExposure { omit: actual, .. } if actual == omit)
			);

			value["data"]["omit"] = serde_json::json!(["future-surface"]);

			assert!(serde_json::from_value::<AgentActionDto>(value).is_err());
		}

		let state = AgentAppExposureResult::Available {
			work_id: EntityId::new("root").unwrap(),
			connector_id: WireText::new("calendar").unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			effective: Some(vec!["future-surface".into()]),
			preference: None,
			can_update: false,
			last_outcome: Some("unknown".into()),
		};

		assert_eq!(
			serde_json::from_value::<AgentAppExposureResult>(serde_json::json!(state)).unwrap(),
			state
		);
	}
}
