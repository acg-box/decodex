//! Read-only native configured model observations.
use serde::{Deserialize, Serialize};
/// A configured model observation, never per-turn execution telemetry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefModelSettingsResult {
	/// Native read completed under the same source ownership.
	Available {
		/// Exact local task.
		work_id: crate::EntityId,
		/// Exact native thread.
		thread_id: crate::EntityId,
		/// Account that owns the native process.
		account_id: crate::EntityId,
		/// Provider ID reported by this native thread; no local default is substituted.
		#[serde(default)]
		model_provider: Option<crate::WireText>,
		/// Configured model. Null means unavailable.
		model: Option<crate::WireText>,
		/// Configured effort. Null means unset or unavailable.
		reasoning_effort: Option<crate::WireText>,
	},
	/// The server does not expose these fields.
	NotReported,
	/// The source or response could not be verified.
	Unavailable,
}
