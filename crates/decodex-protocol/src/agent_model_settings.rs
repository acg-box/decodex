//! Read-only native configured model observations.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// A configured model observation, never per-turn execution telemetry.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentModelSettingsResult {
	/// Native read completed under the same source ownership.
	Available {
		/// Exact local task.
		work_id: EntityId,
		/// Exact native thread.
		thread_id: EntityId,
		/// Account that owns the native process.
		account_id: EntityId,
		/// Provider ID reported by this native thread; no local default is substituted.
		model_provider: Option<WireText>,
		/// Configured model. Null means unavailable.
		model: Option<WireText>,
		/// Configured effort. Null means unset or unavailable.
		reasoning_effort: Option<WireText>,
	},
	/// The server does not expose these fields.
	NotReported,
	/// The source or response could not be verified.
	Unavailable,
}
