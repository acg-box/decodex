//! Explicit continuing duties use the existing durable Agent inbox and host tick.
use serde::{Deserialize, Serialize};

/// One independent user-delegated wake. Missed occurrences coalesce into one input.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentWake {
	/// Stable identity within the owning Agent.
	pub id: String,
	/// Continuing user instruction, separate from the Agent's other duties.
	pub instruction: String,
	/// Original user decision. Direct user commands can use zero.
	pub authority_event_id: i64,
	/// Next Unix timestamp in microseconds; None means complete or cancelled.
	pub due_at_micros: Option<i64>,
	/// Optional interval in seconds. The service and machine must be available.
	pub interval_seconds: Option<u32>,
}
