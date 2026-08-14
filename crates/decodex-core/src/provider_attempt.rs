//! Generic authority values for one external provider turn attempt.
//!
//! These values carry exact consumer, request, RuntimeSession, and ProcessGeneration lineage.
//! They do not select an account, create a RuntimeSession, dispatch a request, retry an unknown
//! effect, or treat a negative observation as evidence.

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use crate::{
	AccountId, ConversationId, ManagedRunId, ProcessExecutionEpochId, ProcessGenerationId,
	RuntimeSessionId, TurnId,
};

/// Maximum UTF-8 bytes in one opaque provider idempotency or correlation key.
pub const MAX_PROVIDER_REQUEST_KEY_BYTES: usize = 512;
/// Maximum UTF-8 bytes in one positive provider receipt, thread, or turn identity.
pub const MAX_PROVIDER_EVIDENCE_IDENTITY_BYTES: usize = 512;

macro_rules! provider_uuid_v4 {
	($name:ident, $label:literal, $error:ident) => {
		#[doc = concat!("Canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);

		impl $name {
			#[doc = concat!("Parse one lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ProviderAttemptError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(ProviderAttemptError::$error);
				}
				Ok(Self(value))
			}

			/// Borrow the canonical identity text.
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}

		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

provider_uuid_v4!(ProviderAttemptId, "ProviderAttempt", InvalidAttemptId);
provider_uuid_v4!(ProviderEvidenceId, "provider evidence", InvalidEvidenceId);
provider_uuid_v4!(ProviderRequestId, "provider request", InvalidRequestId);
provider_uuid_v4!(ManagedExecutionId, "ManagedRun execution", InvalidManagedExecutionId);

/// Exact opaque provider key. Debug output is intentionally redacted.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProviderRequestKey(String);
impl ProviderRequestKey {
	/// Validate one bounded printable provider request key.
	pub fn new(value: impl Into<String>) -> Result<Self, ProviderAttemptError> {
		let value = value.into();
		if !is_bounded_provider_identity(&value, MAX_PROVIDER_REQUEST_KEY_BYTES) {
			return Err(ProviderAttemptError::InvalidProviderKey);
		}
		Ok(Self(value))
	}

	/// Borrow the exact key for provider authority use. Callers must not log this value.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for ProviderRequestKey {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ProviderRequestKey([REDACTED])")
	}
}

/// Exact request keys retained for future positive reconciliation.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderRequestKeys {
	idempotency: Option<ProviderRequestKey>,
	correlation: Option<ProviderRequestKey>,
}
impl ProviderRequestKeys {
	/// Require at least one exact idempotency or correlation key.
	pub fn new(
		idempotency: Option<ProviderRequestKey>,
		correlation: Option<ProviderRequestKey>,
	) -> Result<Self, ProviderAttemptError> {
		if idempotency.is_none() && correlation.is_none() {
			return Err(ProviderAttemptError::MissingProviderKey);
		}
		Ok(Self { idempotency, correlation })
	}

	/// Borrow the provider idempotency key, when supplied.
	pub fn idempotency(&self) -> Option<&ProviderRequestKey> {
		self.idempotency.as_ref()
	}

	/// Borrow the provider correlation key, when supplied.
	pub fn correlation(&self) -> Option<&ProviderRequestKey> {
		self.correlation.as_ref()
	}

	/// Test whether one exact key belongs to this request.
	pub fn contains(&self, key: &ProviderRequestKey) -> bool {
		self.idempotency.as_ref() == Some(key) || self.correlation.as_ref() == Some(key)
	}
}
impl Debug for ProviderRequestKeys {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ProviderRequestKeys")
			.field("has_idempotency", &self.idempotency.is_some())
			.field("has_correlation", &self.correlation.is_some())
			.finish()
	}
}

/// Exactly one consumer intent bound before dispatch authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAttemptConsumer {
	/// One ordinary logical Conversation turn.
	ConversationTurn {
		/// Owning Conversation.
		conversation_id: ConversationId,
		/// Exact reserved Turn intent, whether or not the Conversation owner has materialized it.
		turn_id: TurnId,
	},
	/// One execution-scoped ManagedRun intent.
	ManagedRunExecution {
		/// Owning ManagedRun.
		managed_run_id: ManagedRunId,
		/// Exact immutable revision accepted by V16 and V17.
		managed_run_revision: i64,
		/// Distinct execution intent within the ManagedRun.
		execution_id: ManagedExecutionId,
	},
}
impl ProviderAttemptConsumer {
	/// Return the canonical durable-store consumer label.
	pub const fn as_sql(&self) -> &'static str {
		match self {
			Self::ConversationTurn { .. } => "conversation_turn",
			Self::ManagedRunExecution { .. } => "managed_run_execution",
		}
	}
}

/// Explicit duplicate-risk disposition for one new consumer intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderDuplicateRisk {
	/// This request has no acknowledged predecessor attempt.
	OriginalIntent,
	/// A user authorized a distinct new effect despite one exact unknown predecessor.
	AcknowledgedSuccessor {
		/// Original attempt that remains unknown and remains unchanged.
		predecessor_attempt_id: ProviderAttemptId,
		/// Lowercase SHA-256 of the durable acknowledgement receipt.
		acknowledgement_digest: String,
	},
}

/// Complete caller-supplied input for the atomic preparation transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttemptPreparation {
	/// New attempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Exactly one domain consumer.
	pub consumer: ProviderAttemptConsumer,
	/// Exact immutable V17 plan consumed by this attempt.
	pub continuation_plan_id: String,
	/// Exact logical provider request identity.
	pub request_id: ProviderRequestId,
	/// Lowercase SHA-256 of canonical request bytes.
	pub request_digest: String,
	/// Exact provider idempotency or correlation keys.
	pub provider_keys: ProviderRequestKeys,
	/// Explicit original or acknowledged-successor disposition.
	pub duplicate_risk: ProviderDuplicateRisk,
}
impl ProviderAttemptPreparation {
	/// Validate one complete preparation without granting persistence or dispatch authority.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		attempt_id: ProviderAttemptId,
		consumer: ProviderAttemptConsumer,
		continuation_plan_id: impl Into<String>,
		request_id: ProviderRequestId,
		request_digest: impl Into<String>,
		provider_keys: ProviderRequestKeys,
		duplicate_risk: ProviderDuplicateRisk,
	) -> Result<Self, ProviderAttemptError> {
		let continuation_plan_id = continuation_plan_id.into();
		let request_digest = request_digest.into();
		if !is_canonical_uuid(&continuation_plan_id) {
			return Err(ProviderAttemptError::InvalidContinuationPlanId);
		}
		if !is_sha256(&request_digest) {
			return Err(ProviderAttemptError::InvalidRequestDigest);
		}
		if matches!(
			&consumer,
			ProviderAttemptConsumer::ManagedRunExecution {
				managed_run_revision,
				..
			} if *managed_run_revision <= 0
		) {
			return Err(ProviderAttemptError::InvalidManagedRunRevision);
		}
		if matches!(
			&duplicate_risk,
			ProviderDuplicateRisk::AcknowledgedSuccessor {
				acknowledgement_digest,
				..
			} if !is_sha256(acknowledgement_digest)
		) {
			return Err(ProviderAttemptError::InvalidAcknowledgement);
		}

		Ok(Self {
			attempt_id,
			consumer,
			continuation_plan_id,
			request_id,
			request_digest,
			provider_keys,
			duplicate_risk,
		})
	}
}

/// Durable ProviderAttempt state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptState {
	/// Immutable consumer and runtime authority are durable; no dispatch is authorized.
	Prepared,
	/// Prepared intent was canceled before dispatch authorization.
	Canceled,
	/// Exactly one dispatch was authorized; submission outcome is not yet terminal.
	DispatchAuthorized,
	/// Positive evidence proves provider success.
	Succeeded,
	/// Positive evidence proves a definitive provider failure.
	FailedDefinitive,
	/// Positive evidence proves the request was not submitted.
	NotSubmitted,
	/// Submission or terminal outcome remains unproved.
	Unknown,
}
impl ProviderAttemptState {
	/// Return the canonical durable-store state label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::Prepared => "prepared",
			Self::Canceled => "canceled",
			Self::DispatchAuthorized => "dispatch_authorized",
			Self::Succeeded => "succeeded",
			Self::FailedDefinitive => "failed_definitive",
			Self::NotSubmitted => "not_submitted",
			Self::Unknown => "unknown",
		}
	}

	/// True for a state that cannot transition again.
	pub const fn is_terminal(self) -> bool {
		matches!(
			self,
			Self::Canceled | Self::Succeeded | Self::FailedDefinitive | Self::NotSubmitted
		)
	}
}

/// Closed reason for an unresolved attempt. It never claims non-submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptUnknownReason {
	/// The original in-process dispatch supervision was lost.
	SupervisionLost,
	/// A dispatch returned no positive terminal or positive non-submission result.
	DispatchOutcomeUnavailable,
	/// Restore projected a possibly stale nonterminal row fail closed.
	RestoreProjection,
}
impl ProviderAttemptUnknownReason {
	/// Return the canonical durable-store reason label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::SupervisionLost => "supervision_lost",
			Self::DispatchOutcomeUnavailable => "dispatch_outcome_unavailable",
			Self::RestoreProjection => "restore_projection",
		}
	}
}

/// Closed positive provider evidence source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderEvidenceSource {
	/// A provider returned an exact positive terminal receipt.
	ProviderReceipt,
	/// An exact idempotency-key lookup positively established an outcome.
	PositiveIdempotencyLookup,
	/// Exact turn readback positively established the result.
	ExactTurnReadback,
	/// Exact thread and turn readback positively established the result.
	ExactThreadReadback,
	/// The provider positively attested that it did not submit the request.
	PositiveNonSubmissionReceipt,
}
impl ProviderEvidenceSource {
	/// Return the canonical durable-store source label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::ProviderReceipt => "provider_receipt",
			Self::PositiveIdempotencyLookup => "positive_idempotency_lookup",
			Self::ExactTurnReadback => "exact_turn_readback",
			Self::ExactThreadReadback => "exact_thread_readback",
			Self::PositiveNonSubmissionReceipt => "positive_non_submission_receipt",
		}
	}
}

/// Positive terminal outcome represented by provider evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderTerminalOutcome {
	/// Positive evidence proves success.
	Succeeded,
	/// Positive evidence proves definitive failure.
	FailedDefinitive,
	/// Positive evidence proves non-submission.
	NotSubmitted,
}
impl ProviderTerminalOutcome {
	/// Return the canonical durable-store outcome label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::Succeeded => "succeeded",
			Self::FailedDefinitive => "failed_definitive",
			Self::NotSubmitted => "not_submitted",
		}
	}

	/// Return the corresponding terminal attempt state.
	pub const fn state(self) -> ProviderAttemptState {
		match self {
			Self::Succeeded => ProviderAttemptState::Succeeded,
			Self::FailedDefinitive => ProviderAttemptState::FailedDefinitive,
			Self::NotSubmitted => ProviderAttemptState::NotSubmitted,
		}
	}
}

/// One exact positive evidence receipt for an original attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderPositiveEvidence {
	/// Unique append-only evidence identity.
	pub evidence_id: ProviderEvidenceId,
	/// Original attempt that owns this result, even after process replacement.
	pub attempt_id: ProviderAttemptId,
	/// Exact request identity retained by the attempt.
	pub request_id: ProviderRequestId,
	/// Positive evidence mechanism.
	pub source: ProviderEvidenceSource,
	/// Positively established terminal outcome.
	pub outcome: ProviderTerminalOutcome,
	/// Exact provider key used to correlate the proof.
	pub provider_key: ProviderRequestKey,
	/// Positive provider receipt identity, when the evidence shape requires it.
	pub provider_receipt_id: Option<String>,
	/// Exact provider thread identity, when positively observed.
	pub provider_thread_id: Option<String>,
	/// Exact provider turn identity, when positively observed.
	pub provider_turn_id: Option<String>,
	/// Lowercase SHA-256 of the positive provider witness.
	pub witness_digest: String,
}
impl ProviderPositiveEvidence {
	/// Validate a closed positive shape. There is no timeout, absence, or negative-search shape.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		evidence_id: ProviderEvidenceId,
		attempt_id: ProviderAttemptId,
		request_id: ProviderRequestId,
		source: ProviderEvidenceSource,
		outcome: ProviderTerminalOutcome,
		provider_key: ProviderRequestKey,
		provider_receipt_id: Option<String>,
		provider_thread_id: Option<String>,
		provider_turn_id: Option<String>,
		witness_digest: impl Into<String>,
	) -> Result<Self, ProviderAttemptError> {
		let witness_digest = witness_digest.into();
		if !is_sha256(&witness_digest)
			|| [&provider_receipt_id, &provider_thread_id, &provider_turn_id]
				.into_iter()
				.flatten()
				.any(|value| {
					!is_bounded_provider_identity(value, MAX_PROVIDER_EVIDENCE_IDENTITY_BYTES)
				}) {
			return Err(ProviderAttemptError::InvalidPositiveEvidence);
		}

		let valid_shape = match source {
			ProviderEvidenceSource::ProviderReceipt =>
				outcome != ProviderTerminalOutcome::NotSubmitted && provider_receipt_id.is_some(),
			ProviderEvidenceSource::PositiveIdempotencyLookup => true,
			ProviderEvidenceSource::ExactTurnReadback =>
				outcome != ProviderTerminalOutcome::NotSubmitted
					&& provider_receipt_id.is_none()
					&& provider_thread_id.is_none()
					&& provider_turn_id.is_some(),
			ProviderEvidenceSource::ExactThreadReadback =>
				outcome != ProviderTerminalOutcome::NotSubmitted
					&& provider_receipt_id.is_none()
					&& provider_thread_id.is_some()
					&& provider_turn_id.is_some(),
			ProviderEvidenceSource::PositiveNonSubmissionReceipt =>
				outcome == ProviderTerminalOutcome::NotSubmitted
					&& provider_receipt_id.is_some()
					&& provider_turn_id.is_none(),
		};
		if !valid_shape {
			return Err(ProviderAttemptError::InvalidPositiveEvidence);
		}

		Ok(Self {
			evidence_id,
			attempt_id,
			request_id,
			source,
			outcome,
			provider_key,
			provider_receipt_id,
			provider_thread_id,
			provider_turn_id,
			witness_digest,
		})
	}
}

/// Complete durable attempt projection used inside the sole writer and reconciler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttempt {
	/// Stable attempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Exact immutable consumer.
	pub consumer: ProviderAttemptConsumer,
	/// V17 continuation plan.
	pub continuation_plan_id: String,
	/// Consumed V16 routing decision.
	pub routing_decision_id: String,
	/// Accepted RuntimeSession supplied by V17.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact accepted RuntimeSession revision.
	pub runtime_session_revision: i64,
	/// Account selected by V16.
	pub account_id: AccountId,
	/// Live fenced generation accepted before dispatch authorization.
	pub process_generation_id: ProcessGenerationId,
	/// Exact ready generation revision retained at preparation.
	pub process_generation_revision: i64,
	/// External execution epoch of the bound generation.
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	/// Exact logical request identity.
	pub request_id: ProviderRequestId,
	/// Lowercase SHA-256 of canonical request bytes.
	pub request_digest: String,
	/// Exact keys retained for positive reconciliation. Debug output is redacted.
	pub provider_keys: ProviderRequestKeys,
	/// Original or explicitly acknowledged successor disposition.
	pub duplicate_risk: ProviderDuplicateRisk,
	/// Current durable state.
	pub state: ProviderAttemptState,
	/// Closed reason only while `state` is `unknown`.
	pub unknown_reason: Option<ProviderAttemptUnknownReason>,
	/// Positive terminal evidence identity, when terminal evidence exists.
	pub terminal_evidence_id: Option<ProviderEvidenceId>,
	/// Positive optimistic revision.
	pub revision: i64,
	/// durable-store-authored creation instant in Unix microseconds.
	pub created_at_micros: i64,
	/// durable-store-authored last-transition instant in Unix microseconds.
	pub updated_at_micros: i64,
}

/// Closed ProviderAttempt validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptError {
	/// ProviderAttempt identity was not canonical UUID-v4 text.
	InvalidAttemptId,
	/// Positive-evidence identity was not canonical UUID-v4 text.
	InvalidEvidenceId,
	/// Provider request identity was not canonical UUID-v4 text.
	InvalidRequestId,
	/// ManagedRun execution identity was not canonical UUID-v4 text.
	InvalidManagedExecutionId,
	/// V17 continuation-plan identity was not canonical UUID text.
	InvalidContinuationPlanId,
	/// ManagedRun revision was not positive.
	InvalidManagedRunRevision,
	/// Canonical request digest was malformed.
	InvalidRequestDigest,
	/// No idempotency or correlation key was supplied.
	MissingProviderKey,
	/// Provider request key was empty, too large, or contained control text.
	InvalidProviderKey,
	/// Duplicate-risk acknowledgement was malformed.
	InvalidAcknowledgement,
	/// Positive evidence did not match one of the closed supported shapes.
	InvalidPositiveEvidence,
}
impl Error for ProviderAttemptError {}
impl Display for ProviderAttemptError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();
	is_canonical_uuid(value) && bytes[14] == b'4' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_bounded_provider_identity(value: &str, max_bytes: usize) -> bool {
	!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}
