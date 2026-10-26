//! Native shared configuration write receipts.
use serde::{Deserialize, Serialize};

/// Durable shared config result; it does not assert effective tool policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentConfigEditReceipt {
	/// Reserved, saved, overridden, rejected, unknown, target_observed or superseded.
	pub outcome: String,
	/// App connection or hook identity for display.
	pub target: String,
	/// Original local task.
	pub work_id: String,
	/// Original Codex account.
	pub account_id: String,
	/// Native acknowledgement version, if the app write was acknowledged.
	pub saved_version: Option<String>,
}
