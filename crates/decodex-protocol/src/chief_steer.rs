//! Exact steering acceptance evidence, separate from pending-input pagination.
use serde::{Deserialize, Serialize};

/// Identity captured before one steering command is sent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefSteerIdentity {
	/// Local task that owns the submission.
	pub work_id: crate::EntityId,
	/// Native thread bound at submission time.
	pub thread_id: crate::WireText,
	/// Native turn selected for steering.
	pub turn_id: crate::WireText,
	/// Original command and native client-message identity.
	pub submission_id: crate::IdempotencyKey,
}

/// Read-only evidence; an absent receipt never authorizes a retry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefSteerReceiptResult {
	/// The saved receipt confirms this exact submission.
	Confirmed {
		/// Echoed identity for source validation.
		identity: ChiefSteerIdentity,
	},
	/// No matching positive receipt is available.
	Unconfirmed,
	/// The evidence store cannot be read.
	Unavailable,
}
