//! Exact steering acceptance evidence, separate from pending-input pagination.
use serde::{Deserialize, Serialize};

use crate::{EntityId, IdempotencyKey, WireText};

/// Identity captured before one steering command is sent.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSteerIdentity {
	/// Local task that owns the submission.
	pub work_id: EntityId,
	/// Native thread bound at submission time.
	pub thread_id: WireText,
	/// Native turn selected for steering.
	pub turn_id: WireText,
	/// Original command and native client-message identity.
	pub submission_id: IdempotencyKey,
}

/// Read-only evidence; an absent receipt never authorizes a retry.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentSteerReceiptResult {
	/// The saved receipt confirms this exact submission.
	Confirmed {
		/// Echoed identity for source validation.
		identity: AgentSteerIdentity,
	},
	/// No matching positive receipt is available.
	Unconfirmed,
	/// The evidence store cannot be read.
	Unavailable,
}
