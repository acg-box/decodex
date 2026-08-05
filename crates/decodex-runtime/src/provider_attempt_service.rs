//! Sole runtime writer and positive-only reconciler for durable ProviderAttempt authority.
//!
//! This service consumes an accepted Continuation Plan and one live fenced ProcessGeneration. It
//! has no account selector, RuntimeSession constructor, provider request gateway, automatic retry,
//! or negative-evidence operation. A replacement service can reconcile an original attempt but
//! cannot replay it.

use std::{
	fmt::{Display, Formatter},
	future::{self, Future},
	pin::Pin,
	sync::Arc,
	time::Duration,
};

use decodex_core::{
	AccountId, ContinuationPlanKind, ExecutionConsumer, ProcessExecutionEpochId,
	ProcessGenerationId, ProviderAttempt, ProviderAttemptConsumer, ProviderAttemptId,
	ProviderAttemptPreparation, ProviderAttemptState, ProviderAttemptUnknownReason,
	ProviderEvidenceId, ProviderPositiveEvidence, ProviderRequestId, RuntimeSessionId,
};
use decodex_postgres::{
	AuthorizeProviderDispatchOutcome, ContinuationPlanEffect, FreshPreparedProviderAttempt,
	PostgresStore, PrepareProviderAttemptOutcome, ProviderAttemptMutationOutcome,
	RuntimeSessionBindingReceipt, RuntimeSessionThreadBindingReadback,
};

use crate::process_supervisor::FencedProcess;

const RECONCILIATION_PAGE_SIZE: u16 = 256;
const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(5);
const EVIDENCE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Authority-bound diagnostic and positive-reconciliation port.
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
	/// Exact Continuation Plan consumed by this attempt.
	pub continuation_plan_id: String,
	/// Exact Routing Decision consumed by the plan.
	pub routing_decision_id: String,
	/// Accepted RuntimeSession supplied by Continuation Plan.
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

/// Exact credential-negative request facts for one in-memory thread resume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionResumeRequest {
	/// Positive JSON-RPC request identity.
	pub request_id: i64,
	/// Lowercase SHA-256 of the exact resume request bytes.
	pub request_sha256: String,
}

/// Exact typed successful response facts for one in-memory thread resume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessfulRuntimeSessionResume {
	/// Positive response identity matching the request identity.
	pub response_id: i64,
	/// Lowercase SHA-256 of the exact typed response bytes.
	pub response_sha256: String,
	/// Exact thread returned by the successful response.
	pub codex_thread_id: String,
}

/// One-time positive resume proof for the exact current ready generation.
///
/// This type is intentionally not `Clone`; ProviderAttempt preparation consumes it.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshRuntimeSessionResume {
	runtime_session_id: RuntimeSessionId,
	runtime_session_revision: i64,
	process_generation_id: ProcessGenerationId,
	process_generation_revision: i64,
	process_execution_epoch_id: ProcessExecutionEpochId,
	request: RuntimeSessionResumeRequest,
	response: SuccessfulRuntimeSessionResume,
}

impl FreshRuntimeSessionResume {
	/// Construct one typed positive resume result at the future process-operation boundary.
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new(
		runtime_session_id: RuntimeSessionId,
		runtime_session_revision: i64,
		process: &FencedProcess,
		process_execution_epoch_id: ProcessExecutionEpochId,
		request: RuntimeSessionResumeRequest,
		response: SuccessfulRuntimeSessionResume,
	) -> Result<Self, ProviderAttemptServiceError> {
		if runtime_session_revision <= 0
			|| process.revision() <= 0
			|| request.request_id <= 0
			|| response.response_id != request.request_id
			|| !is_lower_sha256(&request.request_sha256)
			|| !is_lower_sha256(&response.response_sha256)
			|| !is_canonical_uuid(&response.codex_thread_id)
		{
			return Err(ProviderAttemptServiceError::AuthorityConflict);
		}
		Ok(Self {
			runtime_session_id,
			runtime_session_revision,
			process_generation_id: process.generation_id().clone(),
			process_generation_revision: process.revision(),
			process_execution_epoch_id,
			request,
			response,
		})
	}
}

/// Closed post-process RuntimeSession authority consumed by ProviderAttempt preparation.
pub(crate) enum ProviderAttemptRuntimeAuthority {
	/// Exact durable two-transition binding and current ready epoch.
	InitialSessionBinding {
		binding: RuntimeSessionThreadBindingReadback,
		process_execution_epoch_id: ProcessExecutionEpochId,
	},
	/// Exact Context-Pack successor binding and current ready epoch.
	FallbackSessionBinding {
		binding: RuntimeSessionThreadBindingReadback,
		process_execution_epoch_id: ProcessExecutionEpochId,
	},
	/// One-time positive in-memory resume proof.
	ExistingSessionResume(FreshRuntimeSessionResume),
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
	/// Restore fail closed and perform one positive-only reconciliation pass.
	///
	/// The server lifecycle separately owns continued background reconciliation. No provider
	/// dispatch source is constructed by this composition.
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

		Ok(control)
	}

	/// Build the periodic reconciler for direct ownership by the server lifecycle.
	pub(crate) fn reconciliation_task(
		&self,
		mut stop: tokio::sync::watch::Receiver<bool>,
	) -> impl Future<Output = ()> + Send + 'static {
		let weak = Arc::downgrade(&self.inner);

		async move {
			loop {
				tokio::select! {
					biased;

					changed = stop.changed() => {
						let stopping = changed.is_err() || *stop.borrow_and_update();
						if stopping {
							break;
						}
						continue;
					},
					_ = tokio::time::sleep(RECONCILIATION_INTERVAL) => {},
				}

				if *stop.borrow_and_update() {
					break;
				}
				let Some(inner) = weak.upgrade() else {
					break;
				};
				let control = Self { inner };
				let _ = control.reconcile_all().await;
			}
		}
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
		authority: ProviderAttemptRuntimeAuthority,
	) -> Result<PrepareProviderAttemptOutcome, ProviderAttemptServiceError> {
		self.inner.prepare(plan, process, preparation, authority).await
	}

	/// Authorize one freshly prepared attempt immediately before provider I/O.
	pub(crate) async fn authorize_dispatch(
		&self,
		prepared: FreshPreparedProviderAttempt,
		process: &FencedProcess,
	) -> Result<AuthorizeProviderDispatchOutcome, ProviderAttemptServiceError> {
		self.inner.authorize_dispatch(prepared, process).await
	}

	/// Cancel one attempt that provably did not receive dispatch authorization.
	pub(crate) async fn cancel_prepared(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
	) -> Result<ProviderAttemptMutationOutcome, ProviderAttemptServiceError> {
		self.inner.cancel_prepared(attempt_id, expected_revision).await
	}

	/// Preserve one authorized attempt as unknown after transport ambiguity.
	pub(crate) async fn mark_unknown(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
	) -> Result<ProviderAttemptMutationOutcome, ProviderAttemptServiceError> {
		self.inner
			.mark_unknown(
				attempt_id,
				expected_revision,
				ProviderAttemptUnknownReason::DispatchOutcomeUnavailable,
			)
			.await
	}

	async fn reconcile_loaded(
		&self,
		attempt: ProviderAttempt,
	) -> Result<ProviderAttemptReconciliation, ProviderAttemptServiceError> {
		if attempt.state.is_terminal() {
			return Ok(ProviderAttemptReconciliation::AlreadyTerminal { state: attempt.state });
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
				Ok(ProviderAttemptReconciliation::AlreadyTerminal { state: mutation.state }),
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
		self.inner
			.store
			.reconcile_quick_task_terminalizations(RECONCILIATION_PAGE_SIZE)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)?;
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
	/// Prepare one attempt from an accepted Continuation Plan effect and exact live process fence.
	async fn prepare(
		&self,
		plan: &ContinuationPlanEffect,
		process: &FencedProcess,
		preparation: &ProviderAttemptPreparation,
		authority: ProviderAttemptRuntimeAuthority,
	) -> Result<PrepareProviderAttemptOutcome, ProviderAttemptServiceError> {
		if plan.plan.plan_id != preparation.continuation_plan_id {
			return Err(ProviderAttemptServiceError::AuthorityConflict);
		}
		let (expected_conversation_revision, expected_turn_revision) = match &plan.plan.consumer {
			ExecutionConsumer::ConversationTurn {
				conversation_id,
				conversation_revision,
				turn_id,
				..
			} if matches!(
				&preparation.consumer,
				ProviderAttemptConsumer::ConversationTurn {
					conversation_id: attempt_conversation_id,
					turn_id: attempt_turn_id,
				} if attempt_conversation_id == conversation_id && attempt_turn_id == turn_id
			) =>
				(Some(*conversation_revision), Some(1)),
			ExecutionConsumer::ManagedRunExecution { .. }
				if matches!(
					&preparation.consumer,
					ProviderAttemptConsumer::ManagedRunExecution { .. }
				) =>
				(None, None),
			_ => return Err(ProviderAttemptServiceError::AuthorityConflict),
		};
		let (process_execution_epoch_id, binding_receipt) = match authority {
			ProviderAttemptRuntimeAuthority::InitialSessionBinding {
				binding,
				process_execution_epoch_id,
			} if plan.plan.kind == ContinuationPlanKind::InitialThread
				&& binding.continuation_plan_id == plan.plan.plan_id
				&& matches!(
					&plan.plan.consumer,
					ExecutionConsumer::ConversationTurn {
						conversation_id,
						conversation_revision,
						turn_id,
						..
					} if &binding.conversation_id == conversation_id
						&& binding.conversation_revision == *conversation_revision
						&& &binding.turn_id == turn_id
						&& binding.turn_revision == 1
				) && binding.runtime_session_id == plan.plan.source_runtime_session_id
				&& plan.plan.source_runtime_session_revision.checked_add(2)
					== Some(binding.revision)
				&& binding.fence_prior_revision == plan.plan.source_runtime_session_revision
				&& binding.fence_revision.checked_add(1) == Some(binding.revision) =>
				(
					process_execution_epoch_id,
					Some(RuntimeSessionBindingReceipt::from_binding(&binding)),
				),
			ProviderAttemptRuntimeAuthority::FallbackSessionBinding {
				binding,
				process_execution_epoch_id,
			} if plan.plan.kind == ContinuationPlanKind::ContextPackFallback
				&& binding.continuation_plan_id == plan.plan.plan_id
				&& matches!(
					&plan.plan.consumer,
					ExecutionConsumer::ConversationTurn {
						conversation_id,
						conversation_revision,
						turn_id,
						..
					} if &binding.conversation_id == conversation_id
						&& binding.conversation_revision == *conversation_revision
						&& &binding.turn_id == turn_id
						&& binding.turn_revision == 1
				) && plan.plan.fallback_runtime_session_id.as_ref()
				== Some(&binding.runtime_session_id)
				&& binding.fence_prior_revision == 1
				&& binding.fence_revision == 2
				&& binding.revision == 3 =>
				(
					process_execution_epoch_id,
					Some(RuntimeSessionBindingReceipt::from_binding(&binding)),
				),
			ProviderAttemptRuntimeAuthority::ExistingSessionResume(resume)
				if plan.plan.kind == ContinuationPlanKind::SameThread
					&& resume.runtime_session_id == plan.plan.source_runtime_session_id
					&& resume.runtime_session_revision
						== plan.plan.source_runtime_session_revision
					&& plan.plan.codex_thread_id.as_deref()
						== Some(resume.response.codex_thread_id.as_str())
					&& resume.process_generation_id == *process.generation_id()
					&& resume.process_generation_revision == process.revision() =>
				(resume.process_execution_epoch_id, None),
			_ => return Err(ProviderAttemptServiceError::AuthorityConflict),
		};
		self.store
			.prepare_provider_attempt(
				preparation,
				process.generation_id(),
				process.revision(),
				&process_execution_epoch_id,
				binding_receipt.as_ref(),
				(expected_conversation_revision, expected_turn_revision),
			)
			.await
			.map_err(|_| ProviderAttemptServiceError::ProductState)
	}

	/// Commit one fresh dispatch authorization for immediate in-process fence consumption.
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

fn is_lower_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}
