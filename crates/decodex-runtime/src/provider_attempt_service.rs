//! Sole runtime writer and positive-only reconciler for durable ProviderAttempt authority.
//!
//! This service consumes an accepted V17 plan and one live fenced ProcessGeneration. It has no
//! account selector, RuntimeSession constructor, provider request gateway, automatic retry, or
//! negative-evidence operation. A replacement service can reconcile an original attempt but
//! cannot replay it.

use std::{
	fmt::{Display, Formatter},
	future::{self, Future},
	pin::Pin,
	sync::Arc,
	time::Duration,
};

use decodex_core::{
	AccountId, ProcessExecutionEpochId, ProviderAttempt, ProviderAttemptConsumer,
	ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState,
	ProviderAttemptUnknownReason, ProviderEvidenceId, ProviderPositiveEvidence, ProviderRequestId,
	RuntimeSessionId,
};
use decodex_postgres::{
	AuthorizeProviderDispatchOutcome, ContinuationPlanEffect, FreshPreparedProviderAttempt,
	PostgresStore, PrepareProviderAttemptOutcome, ProviderAttemptMutationOutcome,
};

use crate::process_supervisor::FencedProcess;

const RECONCILIATION_PAGE_SIZE: u16 = 256;
const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(5);
const EVIDENCE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Cloneable diagnostic and positive-reconciliation port.
#[derive(Clone)]
pub struct ProviderAttemptControl {
	inner: Arc<ProviderAttemptService>,
}

/// Sole in-process owner of every durable ProviderAttempt mutation capability.
struct ProviderAttemptService {
	store: PostgresStore,
	evidence_source: Arc<dyn ProviderPositiveEvidenceSource>,
	reconciliation_cursor: tokio::sync::Mutex<ProviderAttemptReconciliationCursor>,
}

#[derive(Default)]
struct ProviderAttemptReconciliationCursor {
	dispatch_authorized: Option<ProviderAttemptId>,
	unknown: Option<ProviderAttemptId>,
}

/// Exact bounded diagnostic that cannot expose provider keys or request bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttemptDiagnostic {
	/// Stable original attempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Exact immutable consumer.
	pub consumer: ProviderAttemptConsumer,
	/// Exact V17 continuation plan consumed by this attempt.
	pub continuation_plan_id: String,
	/// Exact V16 routing decision consumed by the plan.
	pub routing_decision_id: String,
	/// Accepted RuntimeSession supplied by V17.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact accepted RuntimeSession revision.
	pub runtime_session_revision: i64,
	/// Selected account.
	pub account_id: AccountId,
	/// Bound ProcessGeneration identity.
	pub process_generation_id: decodex_core::ProcessGenerationId,
	/// Exact ready generation revision retained before authorization.
	pub process_generation_revision: i64,
	/// Exact external execution epoch of the bound generation.
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	/// Exact logical request identity.
	pub request_id: ProviderRequestId,
	/// True when an exact provider idempotency key is retained privately.
	pub has_idempotency_key: bool,
	/// True when an exact provider correlation key is retained privately.
	pub has_correlation_key: bool,
	/// Current durable state.
	pub state: ProviderAttemptState,
	/// Closed reason only for an unknown attempt.
	pub unknown_reason: Option<ProviderAttemptUnknownReason>,
	/// Positive terminal evidence, when one exists.
	pub terminal_evidence_id: Option<ProviderEvidenceId>,
	/// Current durable revision.
	pub revision: i64,
	/// PostgreSQL-authored last-transition instant in Unix microseconds.
	pub updated_at_micros: i64,
}

/// Result of one exact positive-only reconciliation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAttemptReconciliation {
	/// The attempt is already terminal and remains attributable to its original identity.
	AlreadyTerminal {
		/// Current terminal state.
		state: ProviderAttemptState,
	},
	/// Positive evidence committed now or was read back exactly.
	PositiveEvidenceRecorded {
		/// Positively established terminal state.
		state: ProviderAttemptState,
	},
	/// No positive result or positive non-submission evidence is currently available.
	AwaitingPositiveEvidence {
		/// Current nonterminal state.
		state: ProviderAttemptState,
	},
	/// The exact attempt does not exist.
	AttemptMissing,
	/// A supplied positive receipt contradicted durable original-attempt authority.
	EvidenceRejected,
}

/// Daemon-local readiness for ProviderAttempt restore projection and reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptReadiness {
	/// Restore projection and the first positive-only reconciliation pass completed.
	Ready,
	/// PostgreSQL authority was unavailable or inconsistent.
	ProductStateUnavailable,
}

/// Closed lookup failure. Absence and provider errors grant no state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderEvidenceLookupError {
	/// The positive provider evidence source is unavailable.
	Unavailable,
	/// The source returned a malformed or cross-linked positive receipt.
	InvalidEvidence,
}
impl std::error::Error for ProviderEvidenceLookupError {}
impl Display for ProviderEvidenceLookupError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

/// Positive-evidence lookup seam for a provider adapter.
///
/// Returning `Ok(None)` or any error is explicitly inconclusive. It never proves non-submission.
pub trait ProviderPositiveEvidenceSource: Send + Sync {
	/// Seek one exact positive result for the original request and provider key.
	fn positive_evidence<'a>(
		&'a self,
		attempt: &'a ProviderAttempt,
	) -> Pin<
		Box<
			dyn Future<
					Output = Result<Option<ProviderPositiveEvidence>, ProviderEvidenceLookupError>,
				> + Send
				+ 'a,
		>,
	>;
}

/// Closed service failure without provider keys, credentials, or database detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptServiceError {
	/// PostgreSQL ProviderAttempt authority was unavailable or inconsistent.
	ProductState,
	/// A requested attempt or positive receipt contradicted durable authority.
	AuthorityConflict,
	/// The positive provider evidence source was unavailable or returned invalid evidence.
	EvidenceUnavailable,
}
impl std::error::Error for ProviderAttemptServiceError {}
impl Display for ProviderAttemptServiceError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

struct NoPositiveProviderEvidence;
impl ProviderPositiveEvidenceSource for NoPositiveProviderEvidence {
	fn positive_evidence<'a>(
		&'a self,
		_attempt: &'a ProviderAttempt,
	) -> Pin<
		Box<
			dyn Future<
					Output = Result<Option<ProviderPositiveEvidence>, ProviderEvidenceLookupError>,
				> + Send
				+ 'a,
		>,
	> {
		Box::pin(future::ready(Ok(None)))
	}
}

impl ProviderAttemptControl {
	/// Restore fail closed, perform one positive-only reconciliation pass, and continue in
	/// background. No provider dispatch source is constructed by this composition.
	pub(crate) async fn start(store: PostgresStore) -> Result<Self, ProviderAttemptServiceError> {
		Self::start_with_source(store, Arc::new(NoPositiveProviderEvidence)).await
	}

	pub(crate) async fn start_with_source(
		store: PostgresStore,
		evidence_source: Arc<dyn ProviderPositiveEvidenceSource>,
	) -> Result<Self, ProviderAttemptServiceError> {
		store
			.project_provider_attempts_after_supervisor_loss()
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)?;
		let control = Self {
			inner: Arc::new(ProviderAttemptService {
				store,
				evidence_source,
				reconciliation_cursor: tokio::sync::Mutex::new(
					ProviderAttemptReconciliationCursor::default(),
				),
			}),
		};
		control.reconcile_all().await?;

		let weak = Arc::downgrade(&control.inner);
		tokio::spawn(async move {
			loop {
				tokio::time::sleep(RECONCILIATION_INTERVAL).await;
				let Some(inner) = weak.upgrade() else {
					break;
				};
				let control = ProviderAttemptControl { inner };
				let _ = control.reconcile_all().await;
			}
		});

		Ok(control)
	}

	/// Read bounded diagnostics. Exact provider keys and request digests are not representable.
	pub async fn diagnostics(
		&self,
		account_id: Option<&AccountId>,
		state: Option<ProviderAttemptState>,
		limit: u16,
	) -> Result<Vec<ProviderAttemptDiagnostic>, ProviderAttemptServiceError> {
		self.inner
			.store
			.read_provider_attempt_page(account_id, state, None, limit)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
			.map(|attempts| attempts.into_iter().map(diagnostic).collect())
	}

	/// Reconcile one exact attempt through the configured positive-evidence source.
	pub async fn reconcile(
		&self,
		attempt_id: &ProviderAttemptId,
	) -> Result<ProviderAttemptReconciliation, ProviderAttemptServiceError> {
		let Some(attempt) = self
			.inner
			.store
			.read_provider_attempt(attempt_id)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)?
		else {
			return Ok(ProviderAttemptReconciliation::AttemptMissing);
		};
		self.reconcile_loaded(attempt).await
	}

	/// Record an externally obtained exact positive receipt against its original attempt.
	///
	/// This operation cannot authorize replay or create a successor intent.
	pub async fn record_positive_evidence(
		&self,
		evidence: &ProviderPositiveEvidence,
	) -> Result<ProviderAttemptReconciliation, ProviderAttemptServiceError> {
		let Some(attempt) = self
			.inner
			.store
			.read_provider_attempt(&evidence.attempt_id)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)?
		else {
			return Ok(ProviderAttemptReconciliation::AttemptMissing);
		};
		self.commit_positive_evidence(&attempt, evidence).await
	}

	/// Prepare one exact attempt through the sole writer.
	///
	/// Only the stateless coordinator can call this crate-private seam. The result carries no
	/// dispatch authorization.
	pub(crate) async fn prepare(
		&self,
		plan: &ContinuationPlanEffect,
		process: &FencedProcess,
		preparation: &ProviderAttemptPreparation,
	) -> Result<PrepareProviderAttemptOutcome, ProviderAttemptServiceError> {
		self.inner.prepare(plan, process, preparation).await
	}

	async fn reconcile_loaded(
		&self,
		attempt: ProviderAttempt,
	) -> Result<ProviderAttemptReconciliation, ProviderAttemptServiceError> {
		if attempt.state.is_terminal() {
			return Ok(ProviderAttemptReconciliation::AlreadyTerminal {
				state: attempt.state,
			});
		}
		if !matches!(
			attempt.state,
			ProviderAttemptState::DispatchAuthorized | ProviderAttemptState::Unknown
		) {
			return Ok(ProviderAttemptReconciliation::AwaitingPositiveEvidence {
				state: attempt.state,
			});
		}
		let evidence = tokio::time::timeout(
			EVIDENCE_LOOKUP_TIMEOUT,
			self.inner.evidence_source.positive_evidence(&attempt),
		)
		.await
		.map_err(|_| ProviderAttemptServiceError::EvidenceUnavailable)?
		.map_err(|_| ProviderAttemptServiceError::EvidenceUnavailable)?;
		let Some(evidence) = evidence else {
			return Ok(ProviderAttemptReconciliation::AwaitingPositiveEvidence {
				state: attempt.state,
			});
		};
		self.commit_positive_evidence(&attempt, &evidence).await
	}

	async fn commit_positive_evidence(
		&self,
		attempt: &ProviderAttempt,
		evidence: &ProviderPositiveEvidence,
	) -> Result<ProviderAttemptReconciliation, ProviderAttemptServiceError> {
		if evidence.attempt_id != attempt.attempt_id
			|| evidence.request_id != attempt.request_id
			|| !attempt.provider_keys.contains(&evidence.provider_key)
		{
			return Ok(ProviderAttemptReconciliation::EvidenceRejected);
		}
		match self
			.inner
			.store
			.record_provider_attempt_positive_evidence(attempt.revision, evidence)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)?
		{
			ProviderAttemptMutationOutcome::Applied(mutation) =>
				Ok(ProviderAttemptReconciliation::PositiveEvidenceRecorded {
					state: mutation.state,
				}),
			ProviderAttemptMutationOutcome::Replayed(mutation) =>
				Ok(ProviderAttemptReconciliation::AlreadyTerminal {
					state: mutation.state,
				}),
			ProviderAttemptMutationOutcome::Rejected { .. } =>
				Ok(ProviderAttemptReconciliation::EvidenceRejected),
		}
	}

	async fn reconcile_all(&self) -> Result<(), ProviderAttemptServiceError> {
		for state in [ProviderAttemptState::DispatchAuthorized, ProviderAttemptState::Unknown] {
			let after = self.inner.reconciliation_cursor.lock().await.after(state).cloned();
			let page = self
				.inner
				.store
				.read_provider_attempt_page(
					None,
					Some(state),
					after.as_ref(),
					RECONCILIATION_PAGE_SIZE,
				)
				.await
				.map_err(|_| ProviderAttemptServiceError::ProductState)?;
			let next_after = (page.len() == usize::from(RECONCILIATION_PAGE_SIZE))
				.then(|| page.last().expect("a full page is nonempty").attempt_id.clone());
			self.inner.reconciliation_cursor.lock().await.set(state, next_after);
			for attempt in page {
				match self.reconcile_loaded(attempt).await {
					Ok(_) | Err(ProviderAttemptServiceError::EvidenceUnavailable) => {},
					Err(error) => return Err(error),
				}
			}
		}
		Ok(())
	}
}

impl ProviderAttemptReconciliationCursor {
	fn after(&self, state: ProviderAttemptState) -> Option<&ProviderAttemptId> {
		match state {
			ProviderAttemptState::DispatchAuthorized => self.dispatch_authorized.as_ref(),
			ProviderAttemptState::Unknown => self.unknown.as_ref(),
			_ => None,
		}
	}

	fn set(&mut self, state: ProviderAttemptState, after: Option<ProviderAttemptId>) {
		match state {
			ProviderAttemptState::DispatchAuthorized => self.dispatch_authorized = after,
			ProviderAttemptState::Unknown => self.unknown = after,
			_ => {},
		}
	}
}

impl ProviderAttemptService {
	/// Prepare one attempt from an accepted V17 effect and exact live process fence.
	///
	async fn prepare(
		&self,
		plan: &ContinuationPlanEffect,
		process: &FencedProcess,
		preparation: &ProviderAttemptPreparation,
	) -> Result<PrepareProviderAttemptOutcome, ProviderAttemptServiceError> {
		if plan.plan.plan_id != preparation.continuation_plan_id {
			return Err(ProviderAttemptServiceError::AuthorityConflict);
		}
		self.store
			.prepare_provider_attempt(
				preparation,
				process.generation_id(),
				process.revision(),
			)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
	}

	/// Commit one fresh dispatch authorization. No live gateway can consume the result yet.
	#[expect(dead_code, reason = "live provider dispatch remains structurally disabled")]
	async fn authorize_dispatch(
		&self,
		prepared: FreshPreparedProviderAttempt,
		process: &FencedProcess,
	) -> Result<AuthorizeProviderDispatchOutcome, ProviderAttemptServiceError> {
		self.store
			.authorize_provider_attempt_dispatch(
				prepared,
				process.generation_id(),
				process.revision(),
			)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
	}

	/// Cancel a prepared request. This operation cannot consume a dispatch fence.
	#[expect(dead_code, reason = "sealed until accepted Conversation/ManagedRun integration")]
	async fn cancel_prepared(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
	) -> Result<ProviderAttemptMutationOutcome, ProviderAttemptServiceError> {
		self.store
			.cancel_provider_attempt(attempt_id, expected_revision)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
	}

	/// Preserve a live authorized request as unknown after supervision is lost.
	#[expect(dead_code, reason = "live provider dispatch remains structurally disabled")]
	async fn mark_unknown(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
		reason: ProviderAttemptUnknownReason,
	) -> Result<ProviderAttemptMutationOutcome, ProviderAttemptServiceError> {
		self.store
			.mark_provider_attempt_unknown(attempt_id, expected_revision, reason)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
	}
}

fn diagnostic(attempt: ProviderAttempt) -> ProviderAttemptDiagnostic {
	ProviderAttemptDiagnostic {
		attempt_id: attempt.attempt_id,
		consumer: attempt.consumer,
		continuation_plan_id: attempt.continuation_plan_id,
		routing_decision_id: attempt.routing_decision_id,
		runtime_session_id: attempt.runtime_session_id,
		runtime_session_revision: attempt.runtime_session_revision,
		account_id: attempt.account_id,
		process_generation_id: attempt.process_generation_id,
		process_generation_revision: attempt.process_generation_revision,
		process_execution_epoch_id: attempt.process_execution_epoch_id,
		request_id: attempt.request_id,
		has_idempotency_key: attempt.provider_keys.idempotency().is_some(),
		has_correlation_key: attempt.provider_keys.correlation().is_some(),
		state: attempt.state,
		unknown_reason: attempt.unknown_reason,
		terminal_evidence_id: attempt.terminal_evidence_id,
		revision: attempt.revision,
		updated_at_micros: attempt.updated_at_micros,
	}
}
