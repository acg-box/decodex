use std::fmt::{Debug, Formatter};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::StoreError;
use decodex_core::{
	AccountId, AccountState, Agent, ObservationConfidence, Project, QuotaWindowClass,
	RemainingPercent,
};

/// Transactional creation input for one Project and its canonical Lead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProject {
	/// Revision-one active Project authority.
	pub project: Project,
	/// Matching revision-one active canonical Lead.
	pub lead: Agent,
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

/// Exact UTC Unix-microsecond timestamp accepted by quota storage.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct QuotaTimestampMicros(i64);
impl QuotaTimestampMicros {
	/// Unix epoch through the final microsecond of year 9999.
	pub const MAX: i64 = 253_402_300_799_999_999;

	/// Construct an exact product-valid timestamp without normalization.
	pub const fn new(value: i64) -> Result<Self, StoreError> {
		if value < 0 || value > Self::MAX {
			Err(StoreError::InvalidInput("quota timestamp is outside the product range"))
		} else {
			Ok(Self(value))
		}
	}

	/// Return the canonical UTC Unix-microsecond value.
	pub const fn get(self) -> i64 {
		self.0
	}

	/// Compute a nonnegative elapsed duration without wrapping.
	pub const fn checked_duration_since(self, earlier: Self) -> Result<u64, StoreError> {
		match self.0.checked_sub(earlier.0) {
			Some(value) if value >= 0 => Ok(value as u64),
			_ => Err(StoreError::InvalidInput(
				"quota timestamp chronology is reversed or unrepresentable",
			)),
		}
	}
}

/// Idempotent optimistic mutation of one duration-typed quota observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaWindowMutation {
	/// Owning account.
	pub account_id: AccountId,
	/// Exact five-hour or seven-day duration identity.
	pub window: QuotaWindowClass,
	/// Provider-reported remaining percentage, when known.
	pub remaining_percent: Option<RemainingPercent>,
	/// Exact reset timestamp, when known.
	pub resets_at: Option<QuotaTimestampMicros>,
	/// Exact observation timestamp.
	pub observed_at: QuotaTimestampMicros,
	/// Closed confidence classification.
	pub confidence: ObservationConfidence,
	/// Ordinary credential-negative metadata.
	pub metadata: Value,
	/// `None` creates revision 1; `Some` updates only that exact revision.
	pub expected_revision: Option<i64>,
}

/// Stored inert quota-window observation readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaWindow {
	/// Owning account.
	pub account_id: AccountId,
	/// Exact duration-owned window identity.
	pub window: QuotaWindowClass,
	/// Provider-reported remaining percentage.
	pub remaining_percent: Option<RemainingPercent>,
	/// Exact reset timestamp.
	pub resets_at: Option<QuotaTimestampMicros>,
	/// Exact observation timestamp.
	pub observed_at: QuotaTimestampMicros,
	/// Observation confidence.
	pub confidence: ObservationConfidence,
	/// Ordinary metadata.
	pub metadata: Value,
	/// Monotonic optimistic revision.
	pub revision: i64,
}

/// One exact depleted observation to exclude inertly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaExclusionMutation {
	/// Observation committed by the same transaction as the exclusion.
	pub observation: QuotaWindowMutation,
	/// Exact command time used for freshness and reset checks.
	pub excluded_at: QuotaTimestampMicros,
}

/// Mechanically inert hypothetical fallback evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HypotheticalFallbackFact;
impl HypotheticalFallbackFact {
	/// This persistence slice can never authorize dispatch.
	pub const fn dispatch_enabled(self) -> bool {
		false
	}
}

/// Exact committed result of an inert exclusion transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaExclusionReceipt {
	/// Owning account.
	pub account_id: AccountId,
	/// Exact duration-owned window identity.
	pub window: QuotaWindowClass,
	/// Revision of the committed observation evidence.
	pub observation_revision: i64,
	/// Exact depleted amount.
	pub remaining_percent: RemainingPercent,
	/// Exact reset timestamp.
	pub resets_at: QuotaTimestampMicros,
	/// Exact observation timestamp.
	pub observed_at: QuotaTimestampMicros,
	/// Exact exclusion timestamp.
	pub excluded_at: QuotaTimestampMicros,
	/// Closed confidence classification.
	pub confidence: ObservationConfidence,
	/// Immutable credential-negative observation metadata.
	pub metadata: Value,
	/// Canonical `/2` mutation digest.
	pub mutation_sha256: String,
	/// Canonical `/2` mutation byte length.
	pub mutation_length: i64,
	/// Persisted fact that cannot authorize routing.
	pub hypothetical_fallback: HypotheticalFallbackFact,
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
	use crate::types::AccountMetadata;
	use decodex_core::{AccountId, AccountState};

	#[test]
	fn account_debug_output_omits_all_caller_controlled_metadata() {
		let account_id = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let marker = "caller-controlled-private-marker";
		let stored = AccountMetadata {
			account_id,
			display_label: marker.into(),
			state: AccountState::Unavailable,
			metadata: serde_json::json!({"nested": [marker]}),
			revision: 1,
		};

		assert!(!format!("{stored:?}").contains(marker));
	}
}
