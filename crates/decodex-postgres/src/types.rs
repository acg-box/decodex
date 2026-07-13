use std::fmt::{Display, Formatter};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::StoreError;

/// Stable UUID-shaped account identity. The database validates its exact UUID syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountId(String);
impl AccountId {
	/// Construct an account identity without exposing a credential-bearing account object.
	pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
		let value = value.into();

		if value.is_empty() || value.len() > 64 {
			return Err(StoreError::InvalidInput("account id must be a UUID string"));
		}

		Ok(Self(value))
	}

	pub(crate) fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for AccountId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Inert account health metadata. This type has no eligibility or routing operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountState {
	/// No fresh evidence establishes account availability.
	Unknown,
	/// Fresh metadata reports availability; live selection remains owned elsewhere.
	Available,
	/// A known quota window is depleted.
	Depleted,
	/// Authentication failed.
	AuthFailed,
	/// Required plugin readiness was not established.
	PluginUnready,
	/// The account was administratively disabled.
	Disabled,
}
impl AccountState {
	pub(crate) const fn as_sql(self) -> &'static str {
		match self {
			Self::Unknown => "unknown",
			Self::Available => "available",
			Self::Depleted => "depleted",
			Self::AuthFailed => "auth_failed",
			Self::PluginUnready => "plugin_unready",
			Self::Disabled => "disabled",
		}
	}

	pub(crate) fn from_sql(value: &str) -> Result<Self, StoreError> {
		match value {
			"unknown" => Ok(Self::Unknown),
			"available" => Ok(Self::Available),
			"depleted" => Ok(Self::Depleted),
			"auth_failed" => Ok(Self::AuthFailed),
			"plugin_unready" => Ok(Self::PluginUnready),
			"disabled" => Ok(Self::Disabled),
			_ => Err(StoreError::Incompatible(format!("unknown account state {value}"))),
		}
	}
}

/// Idempotent optimistic account metadata mutation.
#[derive(Clone, Debug)]
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

/// Stored account metadata readback.
#[derive(Clone, Debug, PartialEq)]
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
