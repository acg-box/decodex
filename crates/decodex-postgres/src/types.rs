use std::fmt::{Debug, Formatter};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::StoreError;
use decodex_core::{AccountId, AccountState, Agent, Project};

/// Transactional creation input for one Project and its canonical Lead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProject {
	/// Revision-one active Project authority.
	pub project: Project,
	/// Matching revision-one active canonical Lead.
	pub lead: Agent,
}

/// Idempotent optimistic account metadata mutation.
#[derive(Clone)]
pub struct AccountMutation {
	/// Stable account identity.
	pub account_id: AccountId,
	/// Human-readable non-secret label.
	pub display_label: String,
	/// Inert observed health state.
	pub state: AccountState,
	/// Ordinary metadata. Credential-shaped keys are rejected recursively.
	pub metadata: Value,
	/// `None` creates revision 1; `Some` updates only that exact revision.
	pub expected_revision: Option<i64>,
}
impl Debug for AccountMutation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AccountMutation")
			.field("account_id", &self.account_id)
			.field("state", &self.state)
			.field("expected_revision", &self.expected_revision)
			.finish_non_exhaustive()
	}
}

/// Stored account metadata readback.
#[derive(Clone, PartialEq)]
pub struct AccountMetadata {
	/// Stable account identity.
	pub account_id: AccountId,
	/// Human-readable non-secret label.
	pub display_label: String,
	/// Inert observed health state.
	pub state: AccountState,
	/// Ordinary credential-negative metadata.
	pub metadata: Value,
	/// Monotonic optimistic revision.
	pub revision: i64,
}
impl Debug for AccountMetadata {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AccountMetadata")
			.field("account_id", &self.account_id)
			.field("state", &self.state)
			.field("revision", &self.revision)
			.finish_non_exhaustive()
	}
}

/// Idempotent optimistic quota-window metadata mutation.
#[derive(Clone, Debug)]
pub struct QuotaWindowMutation {
	/// Owning account.
	pub account_id: AccountId,
	/// Provider-independent window class such as `usage`.
	pub window_class: String,
	/// Exact duration, never inferred from primary/secondary position.
	pub duration_seconds: i64,
	/// Provider-reported remaining amount, when known.
	pub remaining_amount: Option<f64>,
	/// Provider reset timestamp as RFC 3339 text, when known.
	pub resets_at: Option<String>,
	/// Observation timestamp as RFC 3339 text.
	pub observed_at: String,
	/// Confidence in the observation in the closed range 0..=1.
	pub confidence: f64,
	/// Ordinary credential-negative metadata.
	pub metadata: Value,
	/// `None` creates revision 1; `Some` updates only that exact revision.
	pub expected_revision: Option<i64>,
}

/// Stored inert quota-window metadata readback.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaWindow {
	/// Owning account.
	pub account_id: AccountId,
	/// Provider-independent window class.
	pub window_class: String,
	/// Exact duration.
	pub duration_seconds: i64,
	/// Provider-reported remaining amount.
	pub remaining_amount: Option<f64>,
	/// Provider reset timestamp.
	pub resets_at: Option<String>,
	/// Observation timestamp.
	pub observed_at: String,
	/// Observation confidence.
	pub confidence: f64,
	/// Ordinary metadata.
	pub metadata: Value,
	/// Monotonic optimistic revision.
	pub revision: i64,
}

/// Caller-chosen idempotency key plus a stable hash of the logical request bytes.
#[derive(Clone, Debug)]
pub struct CommandIdentity {
	pub(crate) key: String,
	pub(crate) request_hash: String,
}
impl CommandIdentity {
	/// Hash canonical logical request bytes for receipt matching.
	pub fn new(key: impl Into<String>, request: &[u8]) -> Result<Self, StoreError> {
		let key = key.into();

		if key.is_empty() || key.len() > 256 {
			return Err(StoreError::InvalidInput("idempotency key must contain 1..=256 bytes"));
		}

		crate::ensure_credential_negative_text(&key)?;

		let request_hash =
			Sha256::digest(request).iter().map(|byte| format!("{byte:02x}")).collect();

		Ok(Self { key, request_hash })
	}
}

/// Result of an atomic lease acquisition attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseClaim {
	/// Whether this caller owns the lease.
	pub acquired: bool,
	/// Rotating fencing token, present only for the owner.
	pub token: Option<String>,
	/// Monotonic lease revision, present only for the owner.
	pub revision: Option<i64>,
}

/// Durable outbox lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxState {
	/// Ready for a bounded claim after `available_at`.
	Pending,
	/// Owned by one worker until its claim expires.
	InFlight,
	/// Authoritative receipt/readback confirmed the effect.
	Delivered,
	/// Retry policy was exhausted or an operator explicitly dead-lettered it.
	DeadLetter,
}

/// One worker-owned outbox claim.
#[derive(Clone, Debug, PartialEq)]
pub struct OutboxClaim {
	/// Monotonic outbox identity.
	pub id: i64,
	/// Per-claim fencing token, rotated on every claim or reclaim.
	pub claim_token: String,
	/// Stable logical effect identity.
	pub effect_key: String,
	/// Credential-negative event payload.
	pub payload: Value,
	/// Attempt number after this claim.
	pub attempt_count: i32,
	/// True when a prior attempt may have produced the external effect.
	pub requires_reconciliation: bool,
	/// Previously recorded provider receipt, if one exists.
	pub receipt: Option<Value>,
}

/// Authoritative readback used to reconcile an ambiguous outbox effect.
#[derive(Clone, Debug)]
pub struct OutboxReconciliation {
	/// Credential-negative receipt/readback evidence.
	pub readback: Value,
	/// Whether readback proves the effect exists or is absent.
	pub outcome: ReconciliationOutcome,
}

/// Result of authoritative effect readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationOutcome {
	/// The logical effect exists; delivery can complete without replay.
	EffectPresent,
	/// The logical effect is absent; retry may be scheduled.
	EffectAbsent,
}

/// Append-only activity projection record.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivityRecord {
	/// Monotonic activity sequence.
	pub sequence: i64,
	/// Aggregate type owned by the mutation.
	pub aggregate_kind: String,
	/// Aggregate identity.
	pub aggregate_id: String,
	/// Aggregate revision after mutation.
	pub revision: i64,
	/// Stable event kind.
	pub event_kind: String,
	/// Credential-negative activity payload.
	pub payload: Value,
}

#[cfg(test)]
mod tests {
	use crate::types::{AccountMetadata, AccountMutation};
	use decodex_core::{AccountId, AccountState};

	#[test]
	fn account_debug_output_omits_all_caller_controlled_metadata() {
		let account_id = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let marker = "caller-controlled-private-marker";
		let mutation = AccountMutation {
			account_id: account_id.clone(),
			display_label: marker.into(),
			state: AccountState::Unknown,
			metadata: serde_json::json!({"nested": [marker]}),
			expected_revision: None,
		};
		let stored = AccountMetadata {
			account_id,
			display_label: marker.into(),
			state: AccountState::Unavailable,
			metadata: serde_json::json!({"nested": [marker]}),
			revision: 1,
		};

		assert!(!format!("{mutation:?}").contains(marker));
		assert!(!format!("{stored:?}").contains(marker));
	}
}
