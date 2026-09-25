//! Connector exposure preferences, independent of connected-account approval policy.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};

/// A native model-facing tool surface that a connector preference can omit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefToolExposureSurface {
	/// Tools callable from Code Mode scripts.
	CodeMode,
	/// Tools discovered through tool search.
	Deferred,
	/// Tools in the model's initial tool list.
	Direct,
}
impl ChiefToolExposureSurface {
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefAppExposureResult {
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
	use super::*;
	use crate::ChiefActionDto;
	use serde_json::json;

	#[test]
	fn exposure_wire_preserves_inheritance_and_rejects_unknown_write_surfaces() {
		for omit in [None, Some(vec![]), Some(vec![ChiefToolExposureSurface::Deferred])] {
			let action = ChiefActionDto::SetAppToolExposure {
				work_id: EntityId::new("root").unwrap(),
				connector_id: WireText::new("calendar").unwrap(),
				review_token: WireText::new("a".repeat(64)).unwrap(),
				omit: omit.clone(),
			};
			let mut value = serde_json::to_value(&action).unwrap();
			assert_eq!(value["data"]["omit"], json!(omit));
			let decoded: ChiefActionDto = serde_json::from_value(value.clone()).unwrap();
			assert!(
				matches!(decoded, ChiefActionDto::SetAppToolExposure { omit: actual, .. } if actual == omit)
			);
			value["data"]["omit"] = json!(["future-surface"]);
			assert!(serde_json::from_value::<ChiefActionDto>(value).is_err());
		}
		let state = ChiefAppExposureResult::Available {
			work_id: EntityId::new("root").unwrap(),
			connector_id: WireText::new("calendar").unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			effective: Some(vec!["future-surface".into()]),
			preference: None,
			can_update: false,
			last_outcome: Some("unknown".into()),
		};
		assert_eq!(serde_json::from_value::<ChiefAppExposureResult>(json!(state)).unwrap(), state);
	}
}
