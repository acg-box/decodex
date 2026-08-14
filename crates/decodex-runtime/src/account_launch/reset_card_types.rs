//! Shared reset-card projections used by the daemon API and account observer.

use decodex_core::{
	AccountId, AccountQuotaWindowObservation, ResetCardConsumeOutcome, ResetCardDescriptor,
};

/// Inert compatibility readback for the deferred reset-card capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardPreparation {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub descriptor: ResetCardDescriptor,
}

/// Closed deferred-capability operation status retained by the public protocol mapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardOperationStatus {
	NotFound,
	Prepared,
	EffectAmbiguous,
	Completed(ResetCardConsumeOutcome),
	FailedBeforeEffect(ResetCardFailureCode),
}

/// Value-free reset failure codes retained by the protocol mapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardFailureCode {
	AccountChanged,
	VaultUnavailable,
	SchemaUnsupported,
	InventoryIncomplete,
	InventoryChanged,
	ProviderUnavailable,
	ResourceExhausted,
}

/// Public reset-card observation plus the exact account revision observed around provider work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardInventoryView {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub reported_available_count: Option<u64>,
	pub details_complete: bool,
	pub cards: Vec<ResetCardDescriptor>,
	pub five_hour_quota: AccountQuotaWindowObservation,
	pub seven_day_quota: AccountQuotaWindowObservation,
}

/// Row-scoped failed provider observation with the last retained quota facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardObservationFailure {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub five_hour_quota: AccountQuotaWindowObservation,
	pub seven_day_quota: AccountQuotaWindowObservation,
	pub error: ResetCardServiceError,
}

/// One bounded direct-provider observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardInventoryObservation {
	Available(ResetCardInventoryView),
	ObservationFailed(ResetCardObservationFailure),
}

/// Whether the daemon-owned account API composition is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardVaultStatus {
	NotConfigured,
	Ready,
	Unavailable,
}

/// Closed service failures safe to map to the public protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardServiceError {
	InvalidRequest,
	AccountNotFound,
	AccountStateRejected,
	AccountChanged,
	ExpectedRevisionMismatch {
		actual: i64,
	},
	VaultUnavailable,
	/// Retained for decoding older durable/public results; the direct API path never emits it.
	SchemaUnsupported,
	ProviderUnavailable,
	InventoryIncomplete,
	InventoryChanged,
	/// Retained for decoding older durable/public results; the direct API path uses bounded API
	/// provider failures instead.
	RequestTimedOut,
	/// Retained for decoding older durable/public results; direct API work is bounded by HTTP
	/// request limits and does not use process capacity admission.
	ResourceExhausted,
	ProductStateUnavailable,
	IdempotencyConflict,
	AcceptanceUnknown,
}
