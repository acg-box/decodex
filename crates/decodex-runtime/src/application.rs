//! Application-service seam used by the transport without exposing infrastructure.

use std::{
	collections::{HashMap, HashSet},
	future::{self, Future},
	pin::Pin,
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};

use decodex_codex::CodexAdapter;
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
	AccountOperationPhase, AccountQuotaDisposition, AccountQuotaObservationError,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountSelectionRecovery, AccountState, Availability, BlobStore, ConversationId,
	HistoryItemKind, ItemStatus, ObjectiveId, PossibleSideEffects, ProductState, ProgramClaimId,
	ProgramEvidenceId, ProgramId, ProgramObservationId, ProgramProposalId, ProgramReviewId,
	ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp, RuntimeSessionState, TurnId,
	TurnRole, WorkItemId, WorkItemState,
};
use decodex_database::{
	AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
	AccountCommandReceiptLease, AccountLifecycleRejection,
	BindProgramDomainPack as StoreBindProgramDomainPack, CommandIdentity,
	ContinueProgram as StoreContinueProgram, CreateProgramCycle as StoreCreateProgramCycle,
	DatabaseError, DomainPackIdentity, HistoryCursor, HistoryEntry, OrdinaryTaskConversationCursor,
	OrdinaryTaskConversationProjection, OrdinaryTaskConversationReadback,
	OrdinaryTaskPreSessionState, ProgramCycleRecord, ProgramEvidenceInput, ProgramSummaryRecord,
	RecordProgramReview, RoutingControlOutcome, SqliteStore, StoreError,
};
use decodex_protocol::{
	AccountCommandRejectionDto, AccountCredentialBindingDto, AccountDto,
	AccountInitialSelectionResult, AccountInspectResult, AccountLifecycleReadinessDto,
	AccountManualRecoveryActionDto, AccountManualRecoveryOutcomeDto, AccountObservedStateDto,
	AccountOperationKindDto, AccountOperationPhaseDto, AccountProfileDailyUsageDto,
	AccountProfileDto, AccountProfileEmailDto, AccountProfileErrorDto, AccountProfileResult,
	AccountProviderDto, AccountQuotaErrorDto, AccountQuotaStateDto, AccountQuotaWindowDto,
	AccountRoutingControlDto, AccountSelectionModeDto, AccountSelectionRecoveryDto,
	AccountUnsettledOperationDto, AccountsResult, CausationId, Channel, CodexAuthProjectionResult,
	CommandEnvelope, CommandError, CommandPayload, ConversationHistoryPage,
	ConversationHistoryResult, CorrelationId, DoctorCheck, DoctorComponent, DoctorIssue,
	DoctorReport, DoctorStatus, EntityId, EntityRevision, EventPayload,
	ExecutionDecisionQueryError, ExecutionDecisionResult, HistoryArtifactId,
	HistoryArtifactReference, HistoryArtifactRevision, HistoryBlobLength, HistoryBlobReference,
	HistoryCursorToken, HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto,
	HistoryPayloadDto, HistoryQueryError, HistorySideEffectState, HistoryText, HistoryTurnRole,
	MAX_HISTORY_PAGE_SIZE, ProgramContinuationDraftDto, ProgramCycleDraftDto, ProgramCycleDto,
	ProgramCycleResult, ProgramEdgeDto, ProgramListResult, ProgramNodeDto, ProgramNodeFieldDto,
	ProgramNodeKind, ProgramRelationKind, ProgramReviewDraftDto, ProgramSummaryDto,
	ProjectListResult, QueryEnvelope, QueryPayload, QueryResultPayload,
	QuickTaskExecutionSettings as QuickTaskExecutionSettingsDto, QuickTaskListCursor,
	QuickTaskListPage, QuickTaskListResult, QuickTaskReadError, QuickTaskRecoveryAction,
	QuickTaskResult, QuickTaskState, QuickTaskSummary, QuickTaskTurnOutcome,
	ResetCardDescriptorDto, ResetCardError, ResetCardInventoryResult, ResetCardObservationDto,
	ResetCardOperationResult, ResetCardOutcome, ResultPayload, Sha256Digest, SnapshotItem,
	WireText, WorkItemBoardPageSize, WorkItemBoardProjectId, WorkItemBoardQueryError,
	WorkItemBoardResult, WorkItemBoardWorkItemId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	ProcessGenerationControl, ProviderAttemptControl,
	account_launch::{
		ApiResetCardRuntime, ResetCardFailureCode, ResetCardInventoryObservation,
		ResetCardOperationStatus, ResetCardServiceError,
	},
	account_observation::AccountObservationService,
	account_profile::{
		AccountProfileClaimsView, AccountProfileRuntimeError, AccountProfileRuntimeResult,
		AccountProfileView,
	},
	account_service::{
		AccountLifecycleError, AccountManualRecoveryAction, AccountManualRecoveryOutcome,
		AccountService, CodexAuthProjectionInspection, stable_account_alias,
	},
	domain_packs::{self, DomainPackError, QUICK_TASK_CAPABILITY},
	managed_repository_runtime::ManagedRepositoryCapability,
	quick_task::{
		ControlQuickTask, CreateQuickTask, QuickTaskCapability, QuickTaskControlOutcome,
		QuickTaskExecutionSettings as RuntimeQuickTaskExecutionSettings, QuickTaskLocalState,
		QuickTaskManualRecovery, QuickTaskOutcome, QuickTaskProjection, QuickTaskReadback,
		QuickTaskRuntime, QuickTaskTerminalState, RecoverQuickTask, SubmitQuickTaskTurn,
	},
	routing_orchestration::{ExecutionCoordinator, RoutingSuccessorExecutionCommand},
};

/// The only mutation/observation seam reachable from the WebSocket server.
///
/// Product services implement this async owner without moving command execution into transport.
pub trait Application: Send + Sync + 'static {
	/// Maximum application publications that the transport may defer behind one command result.
	const EVENT_CAPACITY: usize = 64;

	/// Synchronously close application work admission at the start of server shutdown.
	fn begin_shutdown(&self) {}

	/// Wait until cancellable wrappers and non-cancellable application work have both settled.
	fn wait_for_shutdown(&self) -> impl Future<Output = ()> + Send {
		future::ready(())
	}

	/// Return daemon-local background services for direct ownership by the server lifecycle.
	///
	/// Each future must finish after `stop` changes to `true`. The lifecycle drains all returned
	/// futures before it drops the application or releases local transport authority.
	fn daemon_service_tasks(
		&self,
		_stop: watch::Receiver<bool>,
	) -> Vec<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> {
		Vec::new()
	}

	/// Report whether this application owns one lifetime-stable publication source.
	fn has_publication_source(&self) -> bool {
		false
	}

	/// Return a bounded, read-only small-state snapshot. Artifact bytes are not representable.
	///
	/// The future performs no mutation and is cancellation-safe before the transport commits the
	/// returned snapshot to a session.
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send;

	/// Execute one typed command under the application's revision policy.
	fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> impl Future<Output = Result<ApplicationPublication, CommandError>> + Send + 'a;

	/// Execute one Query capability under the explicit authority of its owning domain.
	///
	/// Generic transport grants no Query effect authority, retry, receipt, replay, event, or
	/// command promotion. Each payload defines whether its value is freshly computed or
	/// daemon-observed, follows its explicit owning-domain authority, and must tolerate session
	/// cancellation or response loss under that authority.
	/// `GetConversationHistory` may leave its bounded authorized cursor residue.
	/// `GetAccountProfile` and `GetResetCards` may leave authorized provider observations.
	fn query<'a>(
		&'a self,
		query: &'a QueryEnvelope,
	) -> impl Future<Output = QueryResultPayload> + Send + 'a;

	/// Wait for one application-owned publication that completes after its initiating command.
	fn next_publication(&self) -> impl Future<Output = Option<ApplicationEventPublication>> + Send {
		future::ready(None)
	}
}

/// A successful application execution ready for result and event publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPublication {
	/// Logical channel for the resulting event.
	pub channel: Channel,
	/// Stable identity of the changed entity.
	pub entity_id: EntityId,
	/// Entity revision after execution.
	pub entity_revision: EntityRevision,
	/// Typed success result returned to the caller.
	pub result: ResultPayload,
	/// Typed event published to connected sessions.
	pub event: EventPayload,
}

impl ApplicationPublication {
	/// Whether this successful command also produces an asynchronous publication.
	pub(crate) fn publishes_event(&self) -> bool {
		!matches!(
			&self.result,
			ResultPayload::QuickTaskConversationAccepted { .. }
				| ResultPayload::QuickTaskInterruptAccepted { .. }
		)
	}
}

/// One asynchronous application event ready for ordered WebSocket publication.
pub struct ApplicationEventPublication {
	/// Correlation identity retained from the initiating command.
	pub correlation_id: decodex_protocol::CorrelationId,
	/// Optional direct cause retained from the initiating command.
	pub causation_id: Option<decodex_protocol::CausationId>,
	/// Logical channel for this event.
	pub channel: Channel,
	/// Stable identity of the changed or appended entity.
	pub entity_id: EntityId,
	/// Positive entity revision after persistence.
	pub entity_revision: EntityRevision,
	/// Typed event payload.
	pub event: EventPayload,
}

const ACCOUNT_COMMAND_RECEIPT_SCHEMA: &str = "decodex/account-command-result/1";

#[derive(Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
enum StoredAccountCommandOutcome {
	Succeeded {
		schema: String,
		entity_id: EntityId,
		entity_revision: EntityRevision,
		result: Box<ResultPayload>,
		event: Box<EventPayload>,
	},
	Rejected {
		schema: String,
		error: CommandError,
	},
}

enum ReservedAccountCommand {
	Owned(AccountCommandReceiptLease),
	Replayed(Box<Result<ApplicationPublication, CommandError>>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProductStoreUnavailableReason {
	Configuration,
	Unreachable,
	Incompatible,
	UnsafeAuthority,
	UnsafeHostPath,
}

impl ProductStoreUnavailableReason {
	const fn description(self) -> &'static str {
		match self {
			Self::Configuration => "local product database configuration is unavailable",
			Self::Unreachable => "local product database is unavailable",
			Self::Incompatible => "local product database schema is incompatible",
			Self::UnsafeAuthority => "local product database authority is unsafe",
			Self::UnsafeHostPath => "local product database path is unsafe",
		}
	}
}

#[derive(Clone)]
pub(crate) enum ProductStore {
	Available(SqliteStore),
	Unavailable(ProductStoreUnavailableReason),
}
impl ProductStore {
	async fn database_status(&self, unavailable: DoctorStatus) -> DoctorStatus {
		let Self::Available(store) = self else {
			return unavailable;
		};

		match store.revalidate().await {
			Ok(()) => DoctorStatus::Ready,
			Err(error) => DoctorStatus::Unavailable(match error {
				DatabaseError::Incompatible | DatabaseError::Corrupt => {
					DoctorIssue::DatabaseIncompatible
				},
				DatabaseError::UnsafePath => DoctorIssue::UnsafeHostPath,
				DatabaseError::Unavailable | DatabaseError::Closed => {
					DoctorIssue::DatabaseUnreachable
				},
				DatabaseError::Conflict
				| DatabaseError::NotFound
				| DatabaseError::AlreadyExists => DoctorIssue::Integrity,
			}),
		}
	}
}
impl ProductState for ProductStore {
	fn availability(&self) -> Availability {
		match self {
			Self::Available(store) => store.availability(),
			Self::Unavailable(reason) => Availability::Unavailable { reason: reason.description() },
		}
	}
}

/// Runtime-owned application service retaining the selected adapter and doctor report.
pub(crate) struct ServiceApplication {
	store: ProductStore,
	_managed_repositories: ManagedRepositoryCapability,
	process_generations: Option<ProcessGenerationControl>,
	provider_attempts: Option<ProviderAttemptControl>,
	_codex: CodexAdapter,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	reset_cards: Option<ApiResetCardRuntime>,
	account_observations: Option<AccountObservationService>,
	quick_tasks: QuickTaskCapability,
	doctor: DoctorReport,
}
impl ServiceApplication {
	#[allow(clippy::too_many_arguments)] // Composition keeps each independently owned runtime capability explicit.
	pub(crate) fn new(
		store: ProductStore,
		managed_repositories: ManagedRepositoryCapability,
		process_generations: Option<ProcessGenerationControl>,
		provider_attempts: Option<ProviderAttemptControl>,
		codex: CodexAdapter,
		blob_store: Option<BlobStore>,
		quick_tasks: QuickTaskCapability,
		doctor: DoctorReport,
	) -> Self {
		Self {
			store,
			_managed_repositories: managed_repositories,
			process_generations,
			provider_attempts,
			_codex: codex,
			blob_store,
			accounts: None,
			reset_cards: None,
			account_observations: None,
			quick_tasks,
			doctor,
		}
	}

	pub(crate) fn with_accounts(mut self, accounts: Option<Arc<AccountService>>) -> Self {
		self.accounts = accounts;

		self
	}

	pub(crate) fn with_reset_cards(mut self, reset_cards: Option<ApiResetCardRuntime>) -> Self {
		self.reset_cards = reset_cards;

		self
	}

	pub(crate) fn with_account_observations(
		mut self,
		account_observations: Option<AccountObservationService>,
	) -> Self {
		self.account_observations = account_observations;

		self
	}

	fn request_account_observation_refresh(&self) {
		if let Some(observations) = &self.account_observations {
			observations.request_refresh();
		}
	}

	async fn invalidate_account_observation(&self, entity_id: &EntityId) {
		let Some(observations) = &self.account_observations else {
			return;
		};
		let Ok(account_id) = AccountId::new(entity_id.as_str()) else {
			return;
		};
		observations.invalidate_account(&account_id).await;
	}

	async fn refreshed_doctor(&self) -> DoctorReport {
		let previous_database = self
			.doctor
			.check(DoctorComponent::ProductStore)
			.expect("the closed doctor report includes the product store")
			.status;
		let database = self.store.database_status(previous_database).await;
		let checks = self
			.doctor
			.checks()
			.iter()
			.map(|check| {
				if check.component == DoctorComponent::ProductStore {
					DoctorCheck::new(DoctorComponent::ProductStore, database)
				} else {
					*check
				}
			})
			.collect();

		DoctorReport::new(self.doctor.server_id().clone(), self.doctor.version(), checks)
			.expect("refresh preserves the bounded closed doctor shape")
	}
}

impl ServiceApplication {
	async fn account_list(&self) -> AccountsResult {
		let Some(service) = &self.accounts else {
			return AccountsResult::Unavailable;
		};
		// Account rows and routing are independent capabilities. The row read must not acquire
		// routing's all-account lock, and a transient routing read conflict must not erase fresh
		// account observations from the panel.
		let Ok(accounts) = service.list().await else {
			return AccountsResult::Unavailable;
		};
		let accounts = accounts
			.into_iter()
			.map(|inspection| account_dto(inspection.account))
			.collect::<Result<Vec<_>, _>>();
		let routing =
			service.routing_control().await.ok().and_then(|routing| routing_dto(routing).ok());
		match accounts {
			Ok(accounts) => AccountsResult::Available { accounts, routing },
			Err(_) => AccountsResult::Unavailable,
		}
	}

	async fn account_inspect(&self, account_id: &EntityId) -> AccountInspectResult {
		let Some(service) = &self.accounts else {
			return AccountInspectResult::Unavailable;
		};
		let Ok(account_id) = AccountId::new(account_id.as_str()) else {
			return AccountInspectResult::NotFound;
		};
		match service.inspect(&account_id).await {
			Ok(inspection) => account_dto(inspection.account)
				.map(Box::new)
				.map(AccountInspectResult::Available)
				.unwrap_or(AccountInspectResult::Unavailable),
			Err(AccountLifecycleError::AccountMissing) => AccountInspectResult::NotFound,
			Err(_) => AccountInspectResult::Unavailable,
		}
	}

	async fn codex_auth_projection(&self) -> CodexAuthProjectionResult {
		let Some(service) = &self.accounts else {
			return CodexAuthProjectionResult::Unavailable;
		};
		match service.codex_auth_projection().await {
			CodexAuthProjectionInspection::Current {
				account_id,
				account_revision,
				projection_digest,
			} => {
				let result = (
					EntityId::new(account_id.as_str().to_owned()),
					u64::try_from(account_revision).map(EntityRevision),
					Sha256Digest::new(projection_digest),
				);
				match result {
					(Ok(account_id), Ok(account_revision), Ok(projection_digest))
						if account_revision.0 > 0 =>
					{
						CodexAuthProjectionResult::Current {
							account_id,
							account_revision,
							projection_digest,
						}
					},
					_ => CodexAuthProjectionResult::Unavailable,
				}
			},
			CodexAuthProjectionInspection::Unmanaged => CodexAuthProjectionResult::Unmanaged,
			CodexAuthProjectionInspection::Unavailable => CodexAuthProjectionResult::Unavailable,
		}
	}

	async fn account_profile(
		&self,
		account_id: &EntityId,
		include_email: bool,
	) -> AccountProfileResult {
		let Ok(account_id) = AccountId::new(account_id.as_str()) else {
			return unavailable_account_profile(AccountProfileErrorDto::InvalidRequest);
		};
		let Some(observations) = &self.account_observations else {
			return unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable);
		};
		let Some(profile) = observations.account_profile(&account_id, include_email).await else {
			return unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable);
		};
		match profile {
			AccountProfileRuntimeResult::Current(profile) => account_profile_dto(profile)
				.map(Box::new)
				.map(AccountProfileResult::Current)
				.unwrap_or_else(|()| {
					unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable)
				}),
			AccountProfileRuntimeResult::Cached { profile, refresh_error } => {
				match account_profile_dto(profile) {
					Ok(profile) => AccountProfileResult::Cached {
						profile: Box::new(profile),
						refresh_error: account_profile_error_dto(refresh_error),
					},
					Err(()) => {
						unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable)
					},
				}
			},
			AccountProfileRuntimeResult::Unavailable { claims, error } => {
				account_profile_unavailable_dto(claims, error).unwrap_or_else(|()| {
					unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable)
				})
			},
		}
	}

	async fn initial_account_selection(&self) -> AccountInitialSelectionResult {
		let Some(service) = &self.accounts else {
			return AccountInitialSelectionResult::Unavailable;
		};
		let Some(now_unix_micros) = application_unix_micros() else {
			return AccountInitialSelectionResult::Unavailable;
		};
		match service.select_initial(now_unix_micros).await {
			Ok(selected) => match (
				EntityId::new(selected.account.account_id.as_str().to_owned()),
				u64::try_from(selected.account.revision).map(EntityRevision),
			) {
				(Ok(account_id), Ok(account_revision)) => {
					AccountInitialSelectionResult::Selected { account_id, account_revision }
				},
				_ => AccountInitialSelectionResult::Unavailable,
			},
			Err(failure) => {
				let account_id = failure
					.account_id
					.map(|account_id| EntityId::new(account_id.as_str().to_owned()))
					.transpose();
				match account_id {
					Ok(account_id) => AccountInitialSelectionResult::RecoveryRequired {
						account_id,
						action: selection_recovery_dto(failure.recovery),
					},
					Err(_) => AccountInitialSelectionResult::Unavailable,
				}
			},
		}
	}

	async fn execute_account_command(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		validate_account_command_envelope(command)?;
		if self.accounts.is_none() {
			return Err(application_unavailable("account service is unavailable"));
		}
		let (kind, entity_id, expected_revision) = account_command_descriptor(command)?;
		let ProductStore::Available(store) = &self.store else {
			return Err(application_unavailable("account product state is unavailable"));
		};
		let request = serde_json::to_vec(&command.payload)
			.map_err(|_| account_rejection(AccountCommandRejectionDto::InvalidRequest, None))?;
		let identity = CommandIdentity::new(command.idempotency_key.as_str(), &request)
			.map_err(map_account_store_command_error)?;
		let reserved = match store
			.reserve_account_command(&identity, kind, &entity_id, expected_revision)
			.await
			.map_err(map_account_store_command_error)?
		{
			AccountCommandReceiptClaim::Owned(lease) => ReservedAccountCommand::Owned(lease),
			AccountCommandReceiptClaim::Replayed(value) => ReservedAccountCommand::Replayed(
				Box::new(decode_account_command_receipt(value).map_err(|_| {
					application_unavailable("account command receipt is incompatible")
				})?),
			),
		};
		let lease = match reserved {
			ReservedAccountCommand::Owned(lease) => lease,
			ReservedAccountCommand::Replayed(result) => return *result,
		};
		self.execute_atomic_account_command(command, lease).await
	}

	#[allow(clippy::too_many_lines)] // One closed dispatch maps all account commands into the same receipt boundary.
	async fn execute_atomic_account_command(
		&self,
		command: &CommandEnvelope,
		lease: AccountCommandReceiptLease,
	) -> Result<ApplicationPublication, CommandError> {
		let Some(service) = &self.accounts else {
			return Err(CommandError::AcceptanceUnknown);
		};
		let value = match &command.payload {
			CommandPayload::EnrollAccountFromSharedCodex { operation_id, account_id, enabled } => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				service
					.enroll_from_shared_codex_command(
						lease,
						operation_id,
						account_id,
						*enabled,
						|result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| account_changed_publication(account.clone()),
								),
							)
						},
					)
					.await
			},
			CommandPayload::ImportAccountCredentialFile {
				operation_id,
				account_id,
				enabled,
				source_descriptor,
			} => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				service
					.import_credential_file_command(
						lease,
						operation_id,
						account_id,
						*enabled,
						source_descriptor.as_str(),
						|result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| account_changed_publication(account.clone()),
								),
							)
						},
					)
					.await
			},
			CommandPayload::LogoutAccount { operation_id, account_id } => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.logout_command(lease, operation_id, &account_id, expected, |result| {
						encode_account_command_receipt(
							&result
								.map_err(account_lifecycle_command_error)
								.and_then(|account| account_logout_publication(account.clone())),
						)
					})
					.await
			},
			CommandPayload::RefreshAccount { operation_id, account_id } => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.refresh_command(lease, operation_id, &account_id, expected, |result| {
						encode_account_command_receipt(
							&result
								.map_err(account_lifecycle_command_error)
								.and_then(|account| account_changed_publication(account.clone())),
						)
					})
					.await
			},
			CommandPayload::ReauthenticateAccountFromCredentialFile {
				operation_id,
				account_id,
				source_descriptor,
			} => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.reauthenticate_from_credential_file_command(
						lease,
						operation_id,
						&account_id,
						expected,
						source_descriptor.as_str(),
						|result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| account_changed_publication(account.clone()),
								),
							)
						},
					)
					.await
			},
			CommandPayload::RecoverAccountOperation { operation_id, action } => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let expected = required_expected_revision(command)?;
				let action = match action {
					AccountManualRecoveryActionDto::ReconcileExactStoreState => {
						AccountManualRecoveryAction::ReconcileExactStoreState
					},
					AccountManualRecoveryActionDto::CancelBeforeEffect => {
						AccountManualRecoveryAction::CancelBeforeEffect
					},
				};
				let publication_operation_id = operation_id.clone();
				service
					.recover_operation_command(
						lease,
						&operation_id,
						expected,
						action,
						move |result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|(outcome, account)| {
										account_recovery_publication(
											publication_operation_id,
											outcome,
											account.clone(),
										)
									},
								),
							)
						},
					)
					.await
			},
			CommandPayload::SetAccountEnabled { account_id, enabled } => {
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.set_account_enabled_command(
						lease,
						&account_id,
						expected,
						*enabled,
						|outcome, account| {
							let result = match outcome {
								AccountAdministrationOutcome::Updated { .. } => account
									.cloned()
									.ok_or_else(|| {
										application_unavailable(
											"account command result is unavailable",
										)
									})
									.and_then(account_changed_publication),
								AccountAdministrationOutcome::Rejected { rejection, revision } => {
									Err(lifecycle_rejection(*rejection, *revision))
								},
							};
							encode_account_command_receipt(&result)
						},
					)
					.await
			},
			CommandPayload::UseAccountInCodex { account_id } => {
				let account_id = account_id_from_wire(account_id)?;
				let publication_account_id = account_id.clone();
				let expected = required_expected_revision(command)?;
				service
					.use_account_in_codex_command(lease, &account_id, expected, move |result| {
						encode_account_command_receipt(
							&result.map_err(account_lifecycle_command_error).and_then(
								|(revision, digest)| {
									codex_auth_projection_publication(
										publication_account_id,
										revision,
										digest,
									)
								},
							),
						)
					})
					.await
			},
			CommandPayload::SetFixedAccountSelection { account_id, expected_account_revision } => {
				let expected_routing_revision = required_expected_revision(command)?;
				let account_id = account_id_from_wire(account_id)?;
				let expected_account_revision = i64::try_from(expected_account_revision.0)
					.map_err(|_| {
						account_rejection(AccountCommandRejectionDto::InvalidRequest, None)
					})?;
				service
					.set_fixed_selection_command(
						lease,
						expected_routing_revision,
						&account_id,
						expected_account_revision,
						|outcome| encode_account_command_receipt(&routing_command_result(outcome)),
					)
					.await
			},
			CommandPayload::SetBalancedAccountSelection => {
				let expected_routing_revision = required_expected_revision(command)?;
				service
					.set_balanced_selection_command(lease, expected_routing_revision, |outcome| {
						encode_account_command_receipt(&routing_command_result(outcome))
					})
					.await
			},
			CommandPayload::SetAccountOrder { order } => {
				let expected_routing_revision = required_expected_revision(command)?;
				let order =
					order.iter().map(account_id_from_wire).collect::<Result<Vec<_>, _>>()?;
				service
					.set_account_order_command(
						lease,
						expected_routing_revision,
						&order,
						|outcome| encode_account_command_receipt(&routing_command_result(outcome)),
					)
					.await
			},
			_ => unreachable!("account command validation accepts only account mutations"),
		}
		.map_err(account_operation_command_error)?;

		decode_account_command_receipt(value)
			.map_err(|_| application_unavailable("account command receipt is incompatible"))?
	}

	async fn execution_decision(&self, decision_id: &EntityId) -> ExecutionDecisionResult {
		let _ = decision_id;
		ExecutionDecisionResult::Unavailable {
			error: ExecutionDecisionQueryError::ProductStateUnavailable,
		}
	}

	async fn reset_card_inventory(&self, account_id: &EntityId) -> ResetCardInventoryResult {
		let Some(observations) = &self.account_observations else {
			return ResetCardInventoryResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			};
		};
		let Ok(account_id) = AccountId::new(account_id.as_str()) else {
			return ResetCardInventoryResult::Unavailable { error: ResetCardError::InvalidRequest };
		};

		match observations.reset_card_inventory(&account_id).await {
			Ok(ResetCardInventoryObservation::Available(inventory)) => {
				let account_id =
					EntityId::new(inventory.account_id.as_str().to_owned()).map_err(|_| ());
				let account_revision =
					u64::try_from(inventory.account_revision).map(EntityRevision).map_err(|_| ());
				let cards = inventory
					.cards
					.into_iter()
					.map(|descriptor| {
						ResetCardDescriptorDto::new(
							descriptor.granted_at().unix_seconds(),
							descriptor.expires_at().unix_seconds(),
						)
						.map(|descriptor| ResetCardObservationDto { descriptor })
						.map_err(|_| ())
					})
					.collect::<Result<Vec<_>, _>>();

				let five_hour_quota = quota_dto(inventory.five_hour_quota);
				let seven_day_quota = quota_dto(inventory.seven_day_quota);

				match (account_id, account_revision, cards, five_hour_quota, seven_day_quota) {
					(
						Ok(account_id),
						Ok(account_revision),
						Ok(cards),
						Ok(five_hour_quota),
						Ok(seven_day_quota),
					) => ResetCardInventoryResult::Available {
						account_id,
						account_revision,
						reported_available_count: inventory.reported_available_count,
						details_complete: inventory.details_complete,
						cards,
						five_hour_quota,
						seven_day_quota,
					},
					_ => ResetCardInventoryResult::Unavailable {
						error: ResetCardError::InventoryIncomplete,
					},
				}
			},
			Ok(ResetCardInventoryObservation::ObservationFailed(failure)) => {
				let account_id = EntityId::new(failure.account_id.as_str().to_owned());
				let account_revision = u64::try_from(failure.account_revision).map(EntityRevision);
				let five_hour_quota = quota_dto(failure.five_hour_quota);
				let seven_day_quota = quota_dto(failure.seven_day_quota);
				match (account_id, account_revision, five_hour_quota, seven_day_quota) {
					(
						Ok(account_id),
						Ok(account_revision),
						Ok(five_hour_quota),
						Ok(seven_day_quota),
					) => ResetCardInventoryResult::ObservationFailed {
						account_id,
						account_revision,
						five_hour_quota,
						seven_day_quota,
						error: protocol_reset_error(failure.error),
					},
					_ => ResetCardInventoryResult::Unavailable {
						error: ResetCardError::InventoryIncomplete,
					},
				}
			},
			Err(error) => {
				ResetCardInventoryResult::Unavailable { error: protocol_reset_error(error) }
			},
		}
	}

	async fn reset_card_operation(&self, key: &str) -> ResetCardOperationResult {
		let Some(runtime) = &self.reset_cards else {
			return ResetCardOperationResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			};
		};

		operation_query_result(runtime.operation_status(key).await)
	}

	async fn conversation_history(
		&self,
		conversation_id: &EntityId,
		after: Option<&HistoryCursorToken>,
		page_size: u16,
	) -> ConversationHistoryResult {
		if page_size == 0 || page_size > MAX_HISTORY_PAGE_SIZE {
			return ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::InvalidRequest,
			};
		}

		let Ok(conversation_id) = ConversationId::new(conversation_id.as_str()) else {
			return ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::InvalidRequest,
			};
		};
		let after = match after.map(|cursor| HistoryCursor::parse(cursor.as_str())).transpose() {
			Ok(cursor) => cursor,
			Err(_) => {
				return ConversationHistoryResult::Unavailable {
					error: HistoryQueryError::InvalidRequest,
				};
			},
		};
		let (ProductStore::Available(store), Some(blob_store)) = (&self.store, &self.blob_store)
		else {
			return ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::ProductStateUnavailable,
			};
		};

		match store
			.conversation_history(blob_store, &conversation_id, after.as_ref(), page_size)
			.await
		{
			Ok(page) => {
				let items =
					page.entries.into_iter().map(history_dto).collect::<Result<Vec<_>, _>>();
				let next_cursor = page
					.next_cursor
					.map(|cursor| HistoryCursorToken::new(cursor.encode()))
					.transpose();

				match (items, next_cursor) {
					(Ok(items), Ok(next_cursor)) => {
						ConversationHistoryResult::Page(ConversationHistoryPage {
							items,
							next_cursor,
						})
					},
					_ => ConversationHistoryResult::Unavailable {
						error: HistoryQueryError::IntegrityUnavailable,
					},
				}
			},
			Err(StoreError::InvalidInput(_)) => {
				ConversationHistoryResult::Unavailable { error: HistoryQueryError::InvalidRequest }
			},
			Err(StoreError::CapacityExhausted(_)) => ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::ResourceExhausted,
			},
			Err(StoreError::Blob(_) | StoreError::Incompatible(_)) => {
				ConversationHistoryResult::Unavailable {
					error: HistoryQueryError::IntegrityUnavailable,
				}
			},
			Err(_) => ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::ProductStateUnavailable,
			},
		}
	}

	async fn work_item_board_page(
		&self,
		project_id: &WorkItemBoardProjectId,
		state: Option<WorkItemState>,
		after: Option<&WorkItemBoardWorkItemId>,
		page_size: WorkItemBoardPageSize,
	) -> WorkItemBoardResult {
		let _ = (project_id, state, after, page_size);
		WorkItemBoardResult::Unavailable { error: WorkItemBoardQueryError::ProductStateUnavailable }
	}

	async fn program_list(&self) -> ProgramListResult {
		let ProductStore::Available(store) = &self.store else {
			return ProgramListResult::Unavailable;
		};
		match store.list_programs(64).await {
			Ok(programs) => programs
				.into_iter()
				.map(program_summary_dto)
				.collect::<Result<Vec<_>, _>>()
				.map_or(ProgramListResult::Unavailable, ProgramListResult::Available),
			Err(_) => ProgramListResult::Unavailable,
		}
	}

	async fn program_cycle(&self, program_id: &EntityId) -> ProgramCycleResult {
		let ProductStore::Available(store) = &self.store else {
			return ProgramCycleResult::Unavailable;
		};
		let Ok(program_id) = ProgramId::new(program_id.as_str()) else {
			return ProgramCycleResult::Unavailable;
		};
		match store.program_cycle(&program_id).await {
			Ok(Some(record)) => self
				.program_cycle_dto(record)
				.await
				.map_or(ProgramCycleResult::Unavailable, |cycle| {
					ProgramCycleResult::Available(Box::new(cycle))
				}),
			Ok(None) => ProgramCycleResult::NotFound,
			Err(_) => ProgramCycleResult::Unavailable,
		}
	}

	async fn program_cycle_dto(&self, record: ProgramCycleRecord) -> Result<ProgramCycleDto, ()> {
		let domain_pack = domain_packs::projection(&record).map_err(|_| ())?;
		let mut run_states = Vec::new();
		for work_item in &record.work_items {
			let Some(conversation_id) = work_item.conversation_id.as_ref() else {
				continue;
			};
			let entity_id = EntityId::new(conversation_id.as_str()).map_err(|_| ())?;
			let state = match self.quick_task_get(&entity_id).await {
				QuickTaskResult::Available(summary) => quick_task_state_text(summary.state),
				QuickTaskResult::RoutingSuccessorRedirect { .. } => "routing_successor",
				QuickTaskResult::NotFound => "archived",
				QuickTaskResult::Unavailable { .. } => "unavailable",
			};
			run_states.push((conversation_id.clone(), state));
		}
		let cycle = program_cycle_dto(record, &run_states)?;
		match domain_pack {
			Some(domain_pack) => cycle.with_domain_pack(domain_pack).map_err(|_| ()),
			None => Ok(cycle),
		}
	}

	async fn execute_program_command(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let ProductStore::Available(store) = &self.store else {
			return Err(application_unavailable("Program storage is unavailable"));
		};
		let request = serde_json::to_vec(&command.payload)
			.map_err(|_| application_unavailable("Program command is invalid"))?;
		let identity = CommandIdentity::new(command.idempotency_key.as_str(), &request)
			.map_err(program_command_error)?;
		let record = match &command.payload {
			CommandPayload::CreateProgramCycle { draft } => {
				let domain_pack = domain_packs::resolve_identity(draft.domain_pack_id.as_str())
					.map_err(domain_pack_command_error)?;
				store
					.create_program_cycle(&identity, &store_program_create(draft, domain_pack)?)
					.await
					.map_err(program_command_error)?
			},
			CommandPayload::BindProgramDomainPack { program_id, domain_pack_id } => {
				let expected_revision = command
					.expected_revision
					.ok_or_else(|| application_unavailable("Program revision is required"))?;
				let binding = StoreBindProgramDomainPack {
					program_id: ProgramId::new(program_id.as_str())
						.map_err(|_| application_unavailable("Program identity is invalid"))?,
					expected_revision: expected_revision.0,
					domain_pack: domain_packs::resolve_identity(domain_pack_id.as_str())
						.map_err(domain_pack_command_error)?,
				};
				store
					.bind_program_domain_pack(&identity, &binding)
					.await
					.map_err(program_command_error)?
			},
			CommandPayload::ContinueProgram { continuation } => {
				let expected_revision = command
					.expected_revision
					.ok_or_else(|| application_unavailable("Program revision is required"))?;
				let continuation = store_program_continuation(continuation, expected_revision)?;
				store
					.continue_program(&identity, &continuation)
					.await
					.map_err(program_command_error)?
			},
			CommandPayload::RecordProgramReview { review } => store
				.record_program_review(&identity, &store_program_review(review)?)
				.await
				.map_err(program_command_error)?,
			_ => return Err(application_unavailable("Program command is invalid")),
		};
		let cycle = self
			.program_cycle_dto(record)
			.await
			.map_err(|_| application_unavailable("Program projection is unavailable"))?;
		let entity_id = cycle.program.program_id.clone();
		let entity_revision = cycle.program.revision;
		Ok(ApplicationPublication {
			channel: Channel::ProjectWork,
			entity_id,
			entity_revision,
			result: ResultPayload::ProgramCycleChanged { cycle: Box::new(cycle.clone()) },
			event: EventPayload::ProgramCycleChanged { cycle: Box::new(cycle) },
		})
	}

	async fn quick_task_row(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<OrdinaryTaskConversationProjection>, QuickTaskReadError> {
		let ProductStore::Available(store) = &self.store else {
			return Err(QuickTaskReadError::ProductStateUnavailable);
		};
		let mut rows = store
			.read_ordinary_task_conversations(Some(conversation_id), None, 2)
			.await
			.map_err(|error| quick_task_read_error(&error))?;
		if rows.len() > 1 {
			return Err(QuickTaskReadError::IntegrityUnavailable);
		}
		Ok(rows.pop())
	}

	async fn quick_task_list(
		&self,
		after: Option<&QuickTaskListCursor>,
		page_size: u16,
	) -> QuickTaskListResult {
		let ProductStore::Available(store) = &self.store else {
			return QuickTaskListResult::Unavailable {
				error: QuickTaskReadError::ProductStateUnavailable,
			};
		};
		let after = match after
			.map(|cursor| {
				Ok(OrdinaryTaskConversationCursor {
					updated_at_micros: cursor.updated_at_micros(),
					conversation_id: ConversationId::new(cursor.conversation_id().as_str())
						.map_err(|_| ())?,
				})
			})
			.transpose()
		{
			Ok(after) => after,
			Err(()) => {
				return QuickTaskListResult::Unavailable {
					error: QuickTaskReadError::InvalidRequest,
				};
			},
		};
		let requested = usize::from(page_size);
		let Some(limit) = requested.checked_add(1) else {
			return QuickTaskListResult::Unavailable { error: QuickTaskReadError::InvalidRequest };
		};
		let rows = match store.read_ordinary_task_conversations(None, after.as_ref(), limit).await {
			Ok(rows) => rows,
			Err(error) => {
				return QuickTaskListResult::Unavailable { error: quick_task_read_error(&error) };
			},
		};
		let mut rows = match rows
			.into_iter()
			.map(|projection| match projection {
				OrdinaryTaskConversationProjection::Current(row) => Ok(row),
				OrdinaryTaskConversationProjection::Archived { .. }
				| OrdinaryTaskConversationProjection::RoutingSuccessorRedirect { .. } => Err(()),
			})
			.collect::<Result<Vec<_>, _>>()
		{
			Ok(rows) => rows,
			Err(()) => {
				return QuickTaskListResult::Unavailable {
					error: QuickTaskReadError::IntegrityUnavailable,
				};
			},
		};
		let has_more = rows.len() > requested;
		if has_more {
			rows.pop();
		}
		let next_cursor = if has_more {
			rows.last().and_then(|row| {
				QuickTaskListCursor::new(
					row.updated_at_micros,
					EntityId::new(row.conversation_id.as_str()).ok()?,
				)
				.ok()
			})
		} else {
			None
		};
		if has_more && next_cursor.is_none() {
			return QuickTaskListResult::Unavailable {
				error: QuickTaskReadError::IntegrityUnavailable,
			};
		}
		let conversations = rows
			.into_iter()
			.map(|row| {
				let projection = self
					.quick_tasks
					.runtime()
					.and_then(|runtime| runtime.projection(&row.conversation_id));
				quick_task_summary_from_row(row, projection)
			})
			.collect::<Result<Vec<_>, _>>();
		match conversations.and_then(|conversations| {
			QuickTaskListPage::new(conversations, next_cursor).map_err(|_| ())
		}) {
			Ok(page) => QuickTaskListResult::Available(page),
			Err(()) => {
				QuickTaskListResult::Unavailable { error: QuickTaskReadError::IntegrityUnavailable }
			},
		}
	}

	async fn quick_task_get(&self, conversation_id: &EntityId) -> QuickTaskResult {
		let Ok(conversation_id) = ConversationId::new(conversation_id.as_str()) else {
			return QuickTaskResult::Unavailable { error: QuickTaskReadError::InvalidRequest };
		};
		let projection = match self.quick_task_row(&conversation_id).await {
			Ok(Some(projection)) => projection,
			Ok(None) => return QuickTaskResult::NotFound,
			Err(error) => return QuickTaskResult::Unavailable { error },
		};
		match projection {
			OrdinaryTaskConversationProjection::Current(row) => {
				let local = self
					.quick_tasks
					.runtime()
					.and_then(|runtime| runtime.projection(&conversation_id));
				match quick_task_summary_from_row(row, local) {
					Ok(summary) => QuickTaskResult::Available(summary),
					Err(()) => QuickTaskResult::Unavailable {
						error: QuickTaskReadError::IntegrityUnavailable,
					},
				}
			},
			OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
				source_conversation_id,
				source_revision,
				successor_conversation_id,
				successor_conversation_revision,
			} => match (
				EntityId::new(source_conversation_id.as_str()),
				u64::try_from(source_revision),
				EntityId::new(successor_conversation_id.as_str()),
				u64::try_from(successor_conversation_revision),
			) {
				(Ok(source), Ok(source_revision), Ok(successor), Ok(successor_revision)) => {
					QuickTaskResult::RoutingSuccessorRedirect {
						source_conversation_id: source,
						source_conversation_revision: EntityRevision(source_revision),
						successor_conversation_id: successor,
						successor_conversation_revision: EntityRevision(successor_revision),
					}
				},
				_ => {
					QuickTaskResult::Unavailable { error: QuickTaskReadError::IntegrityUnavailable }
				},
			},
			OrdinaryTaskConversationProjection::Archived { .. } => QuickTaskResult::NotFound,
		}
	}

	async fn publish_quick_task_routing_successor(
		&self,
		source_conversation_id: &ConversationId,
		source_revision: i64,
		successor_conversation_id: &ConversationId,
		successor_revision: i64,
	) -> Result<ApplicationPublication, CommandError> {
		let successor_id =
			EntityId::new(successor_conversation_id.as_str().to_owned()).map_err(|_| {
				application_unavailable("Quick Task successor identity is incompatible")
			})?;
		let QuickTaskResult::Available(successor) = self.quick_task_get(&successor_id).await else {
			return Err(application_unavailable("Quick Task successor readback is unavailable"));
		};
		quick_task_routing_successor_publication(
			source_conversation_id,
			source_revision,
			successor_conversation_id,
			successor_revision,
			successor,
		)
	}

	async fn quick_task_command_row(
		&self,
		conversation_id: &ConversationId,
		expected: Option<EntityRevision>,
	) -> Result<OrdinaryTaskConversationReadback, CommandError> {
		let projection = self
			.quick_task_row(conversation_id)
			.await
			.map_err(|_| application_unavailable("Quick Task readback is unavailable"))?
			.ok_or_else(quick_task_conflict)?;
		let OrdinaryTaskConversationProjection::Current(row) = projection else {
			return Err(quick_task_conflict());
		};
		let actual = EntityRevision(
			u64::try_from(row.conversation_revision).map_err(|_| quick_task_conflict())?,
		);
		let expected = expected.ok_or_else(quick_task_conflict)?;
		if expected != actual {
			return Err(CommandError::ExpectedRevisionMismatch { expected, actual });
		}
		Ok(row)
	}

	async fn execute_create_quick_task(
		&self,
		runtime: &QuickTaskRuntime,
		command: &CommandEnvelope,
	) -> Result<QuickTaskOutcome, CommandError> {
		let CommandPayload::CreateQuickTask {
			conversation_id,
			work_item_id,
			message,
			working_directory,
			execution,
		} = &command.payload
		else {
			return Err(quick_task_conflict());
		};
		if command.expected_revision.is_some() || message.as_str().trim().is_empty() {
			return Err(quick_task_conflict());
		}
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let work_item_id = work_item_id
			.as_ref()
			.map(|work_item_id| WorkItemId::new(work_item_id.as_str()))
			.transpose()
			.map_err(|_| quick_task_conflict())?;
		Ok(runtime
			.create(CreateQuickTask {
				operation_key: command.idempotency_key.as_str().to_owned(),
				correlation_id: command.correlation_id.as_str().to_owned(),
				causation_id: command.causation_id.as_ref().map(|id| id.as_str().to_owned()),
				conversation_id,
				work_item_id,
				message: message.as_str().to_owned(),
				working_directory: working_directory.as_str().to_owned(),
				execution: runtime_execution_settings(execution),
			})
			.await)
	}

	async fn execute_quick_task_recovery(
		&self,
		runtime: &QuickTaskRuntime,
		command: &CommandEnvelope,
	) -> Result<QuickTaskOutcome, CommandError> {
		let conversation_id = match &command.payload {
			CommandPayload::ResumeQuickTaskRouting { conversation_id } => conversation_id,
			CommandPayload::ResumeQuickTaskEstablishment { conversation_id } => conversation_id,
			_ => return Err(quick_task_conflict()),
		};
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let row = self.quick_task_command_row(&conversation_id, command.expected_revision).await?;
		let recoverable = match &command.payload {
			CommandPayload::ResumeQuickTaskRouting { .. } => {
				row.runtime_session_id.is_none()
					&& row.pre_session_state == Some(OrdinaryTaskPreSessionState::RoutingPending)
			},
			CommandPayload::ResumeQuickTaskEstablishment { .. } => {
				(row.runtime_session_id.is_none()
					&& row.pre_session_state
						== Some(OrdinaryTaskPreSessionState::EstablishmentPending))
					|| (row.runtime_session_id.is_some()
						&& row.pre_session_state.is_none()
						&& row.runtime_session_state == Some(RuntimeSessionState::Starting)
						&& !row.has_acknowledged_turn
						&& !row.has_active_provider_attempt
						&& !row.has_unknown_provider_attempt
						&& (!row.has_admitted_user_turn || row.active_turn_id.is_some()))
			},
			_ => false,
		};
		if !recoverable {
			return Err(quick_task_conflict());
		}
		let recovery = RecoverQuickTask {
			operation_key: command.idempotency_key.as_str().to_owned(),
			correlation_id: command.correlation_id.as_str().to_owned(),
			causation_id: command.causation_id.as_ref().map(|id| id.as_str().to_owned()),
			conversation_id,
			expected_conversation_revision: row.conversation_revision,
		};
		Ok(match &command.payload {
			CommandPayload::ResumeQuickTaskRouting { .. } => runtime.resume_routing(recovery).await,
			CommandPayload::ResumeQuickTaskEstablishment { .. } => {
				runtime.resume_establishment(recovery).await
			},
			_ => return Err(quick_task_conflict()),
		})
	}

	async fn execute_quick_task_routing_successor(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id } = &command.payload
		else {
			return Err(quick_task_conflict());
		};
		let source_conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let projection = self
			.quick_task_row(&source_conversation_id)
			.await
			.map_err(|_| application_unavailable("Quick Task readback is unavailable"))?
			.ok_or_else(quick_task_conflict)?;
		match projection {
			OrdinaryTaskConversationProjection::Archived { .. } => Err(quick_task_conflict()),
			OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
				source_conversation_id: redirected_source,
				source_revision,
				successor_conversation_id,
				successor_conversation_revision,
			} => {
				let expected = command.expected_revision.ok_or_else(quick_task_conflict)?;
				let source_revision_wire =
					u64::try_from(source_revision).map_err(|_| quick_task_conflict())?;
				if redirected_source != source_conversation_id
					|| expected.0.checked_add(1) != Some(source_revision_wire)
				{
					return Err(quick_task_conflict());
				}
				self.publish_quick_task_routing_successor(
					&redirected_source,
					source_revision,
					&successor_conversation_id,
					successor_conversation_revision,
				)
				.await
			},
			OrdinaryTaskConversationProjection::Current(row) => {
				let actual = EntityRevision(
					u64::try_from(row.conversation_revision).map_err(|_| quick_task_conflict())?,
				);
				let expected = command.expected_revision.ok_or_else(quick_task_conflict)?;
				if expected != actual {
					return Err(CommandError::ExpectedRevisionMismatch { expected, actual });
				}
				if row.runtime_session_id.is_some()
					|| !matches!(
						row.pre_session_state,
						Some(
							OrdinaryTaskPreSessionState::QuotaExhausted
								| OrdinaryTaskPreSessionState::NoRoute
						)
					) {
					return Err(quick_task_conflict());
				}
				let ProductStore::Available(store) = &self.store else {
					return Err(application_unavailable(
						"Quick Task successor persistence is unavailable",
					));
				};
				let operation_key = command.idempotency_key.as_str().to_owned();
				let execution = RoutingSuccessorExecutionCommand::new(
					&operation_key,
					source_conversation_id,
					row.conversation_revision,
				);
				let result = ExecutionCoordinator
					.successor_to_route(store, &execution)
					.await
					.map_err(|_| quick_task_conflict())?;
				let relation = result.successor;
				if let QuickTaskCapability::Ready(runtime) = &self.quick_tasks {
					runtime
						.start_preplanned_initial(
							RecoverQuickTask {
								operation_key,
								correlation_id: command.correlation_id.as_str().to_owned(),
								causation_id: command
									.causation_id
									.as_ref()
									.map(|id| id.as_str().to_owned()),
								conversation_id: relation.successor_conversation_id.clone(),
								expected_conversation_revision: relation.successor_revision,
							},
							result.routing,
						)
						.await;
				}
				self.publish_quick_task_routing_successor(
					&relation.source_conversation_id,
					relation.source_revision,
					&relation.successor_conversation_id,
					relation.successor_revision,
				)
				.await
			},
		}
	}

	async fn execute_submit_quick_task_turn(
		&self,
		runtime: &QuickTaskRuntime,
		command: &CommandEnvelope,
	) -> Result<QuickTaskOutcome, CommandError> {
		let CommandPayload::SubmitQuickTaskTurn {
			conversation_id,
			turn_id,
			message,
			working_directory,
			execution,
		} = &command.payload
		else {
			return Err(quick_task_conflict());
		};
		if message.as_str().trim().is_empty() {
			return Err(quick_task_conflict());
		}
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let turn_id = TurnId::new(turn_id.as_str()).map_err(|_| quick_task_conflict())?;
		self.quick_task_command_row(&conversation_id, command.expected_revision).await?;
		Ok(runtime
			.submit_turn(SubmitQuickTaskTurn {
				operation_key: command.idempotency_key.as_str().to_owned(),
				correlation_id: command.correlation_id.as_str().to_owned(),
				causation_id: command.causation_id.as_ref().map(|id| id.as_str().to_owned()),
				conversation_id,
				turn_id,
				message: message.as_str().to_owned(),
				working_directory: working_directory.as_str().to_owned(),
				execution: runtime_execution_settings(execution),
			})
			.await)
	}

	async fn execute_interrupt_quick_task(
		&self,
		runtime: &QuickTaskRuntime,
		command: &CommandEnvelope,
	) -> Result<QuickTaskOutcome, CommandError> {
		let CommandPayload::InterruptQuickTask { conversation_id, turn_id } = &command.payload
		else {
			return Err(quick_task_conflict());
		};
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let turn_id = TurnId::new(turn_id.as_str()).map_err(|_| quick_task_conflict())?;
		let row = self.quick_task_command_row(&conversation_id, command.expected_revision).await?;
		let Some(projection) = runtime.projection(&conversation_id) else {
			return Err(CommandError::QuickTaskRecoveryRequired {
				action: if row.active_turn_id.as_ref() == Some(&turn_id) {
					QuickTaskRecoveryAction::ResolvePriorActiveTurn
				} else {
					QuickTaskRecoveryAction::StartNewConversation
				},
			});
		};
		if projection.readback.active_turn_id.as_ref() != Some(&turn_id) {
			return Err(quick_task_conflict());
		}
		Ok(runtime.interrupt(&conversation_id))
	}

	async fn execute_control_quick_task(
		&self,
		runtime: &QuickTaskRuntime,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let (conversation_id, archive) = match &command.payload {
			CommandPayload::RefreshQuickTask { conversation_id } => (conversation_id, false),
			CommandPayload::ArchiveQuickTask { conversation_id } => (conversation_id, true),
			_ => return Err(quick_task_conflict()),
		};
		let core_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| quick_task_conflict())?;
		let row = self.quick_task_command_row(&core_id, command.expected_revision).await?;
		if row.has_active_provider_attempt {
			return Err(quick_task_busy());
		}
		if row.active_turn_id.is_some() != row.active_turn_revision.is_some() {
			return Err(quick_task_conflict());
		}
		let runtime_session_id = row.runtime_session_id.ok_or_else(quick_task_conflict)?;
		let runtime_session_revision =
			row.runtime_session_revision.ok_or_else(quick_task_conflict)?;
		match runtime
			.control_thread(ControlQuickTask {
				operation_key: command.idempotency_key.as_str().to_owned(),
				conversation_id: core_id,
				expected_conversation_revision: row.conversation_revision,
				runtime_session_id,
				expected_runtime_session_revision: runtime_session_revision,
				active_turn_id: row.active_turn_id,
				active_turn_revision: row.active_turn_revision,
				archive,
			})
			.await
		{
			QuickTaskControlOutcome::Current => {
				let QuickTaskResult::Available(conversation) =
					self.quick_task_get(conversation_id).await
				else {
					return Err(application_unavailable(
						"Quick Task refresh readback is unavailable",
					));
				};
				quick_task_command_publication(conversation, false)
			},
			QuickTaskControlOutcome::Archived { conversation_revision } => {
				let revision = EntityRevision(
					u64::try_from(conversation_revision).map_err(|_| quick_task_conflict())?,
				);
				Ok(ApplicationPublication {
					channel: Channel::ConversationStream,
					entity_id: conversation_id.clone(),
					entity_revision: revision,
					result: ResultPayload::QuickTaskArchived {
						conversation_id: conversation_id.clone(),
						conversation_revision: revision,
					},
					event: EventPayload::QuickTaskArchived {
						conversation_id: conversation_id.clone(),
						conversation_revision: revision,
					},
				})
			},
			QuickTaskControlOutcome::Busy => Err(quick_task_busy()),
			QuickTaskControlOutcome::Conflict => Err(quick_task_conflict()),
			QuickTaskControlOutcome::OutcomeUnknown => Err(CommandError::AcceptanceUnknown),
			QuickTaskControlOutcome::Unavailable => {
				Err(application_unavailable("Quick Task thread control is unavailable"))
			},
		}
	}

	async fn execute_quick_task(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		if matches!(&command.payload, CommandPayload::CreateQuickTaskRoutingSuccessor { .. }) {
			return self.execute_quick_task_routing_successor(command).await;
		}
		if let CommandPayload::CreateQuickTask { work_item_id: Some(work_item_id), .. } =
			&command.payload
		{
			let work_item_id =
				WorkItemId::new(work_item_id.as_str()).map_err(|_| quick_task_conflict())?;
			authorize_program_capability(&self.store, &work_item_id, QUICK_TASK_CAPABILITY).await?;
		}
		let runtime = match &self.quick_tasks {
			QuickTaskCapability::Ready(runtime) => runtime,
			QuickTaskCapability::Unavailable(reason) => {
				return Err(CommandError::QuickTaskUnavailable { unavailable_reason: *reason });
			},
		};
		if matches!(
			&command.payload,
			CommandPayload::RefreshQuickTask { .. } | CommandPayload::ArchiveQuickTask { .. }
		) {
			return self.execute_control_quick_task(runtime, command).await;
		}
		let outcome = match &command.payload {
			CommandPayload::CreateQuickTask { .. } => {
				self.execute_create_quick_task(runtime, command).await?
			},
			CommandPayload::ResumeQuickTaskRouting { .. }
			| CommandPayload::ResumeQuickTaskEstablishment { .. } => {
				self.execute_quick_task_recovery(runtime, command).await?
			},
			CommandPayload::SubmitQuickTaskTurn { .. } => {
				self.execute_submit_quick_task_turn(runtime, command).await?
			},
			CommandPayload::InterruptQuickTask { .. } => {
				self.execute_interrupt_quick_task(runtime, command).await?
			},
			_ => return Err(quick_task_conflict()),
		};
		let (conversation_id, interrupt) = quick_task_command_projection(outcome)?;
		let conversation_id = EntityId::new(conversation_id.as_str().to_owned())
			.map_err(|_| application_unavailable("Quick Task projection is unavailable"))?;
		let QuickTaskResult::Available(conversation) = self.quick_task_get(&conversation_id).await
		else {
			return Err(application_unavailable("Quick Task projection is unavailable"));
		};
		quick_task_command_publication(conversation, interrupt)
	}
}

async fn authorize_program_capability(
	store: &ProductStore,
	work_item_id: &WorkItemId,
	capability: &str,
) -> Result<(), CommandError> {
	let ProductStore::Available(store) = store else {
		return Err(application_unavailable("Program storage is unavailable"));
	};
	let owner = store
		.program_domain_pack_for_work_item(work_item_id)
		.await
		.map_err(|_| application_unavailable("Program Domain Pack is unavailable"))?
		.ok_or_else(|| application_unavailable("Program WorkItem is unavailable"))?;
	domain_packs::authorize(owner.domain_pack.as_ref(), capability)
		.map_err(domain_pack_command_error)
}

impl Application for ServiceApplication {
	fn has_publication_source(&self) -> bool {
		self.quick_tasks.runtime().is_some()
	}

	fn begin_shutdown(&self) {
		if let Some(runtime) = self.quick_tasks.runtime() {
			runtime.begin_shutdown();
		}
		if let Some(runtime) = &self.reset_cards {
			runtime.begin_shutdown();
		}
	}

	async fn wait_for_shutdown(&self) {
		if let Some(runtime) = self.quick_tasks.runtime() {
			runtime.wait_for_shutdown().await;
		}
		if let Some(runtime) = &self.reset_cards {
			runtime.wait_for_shutdown().await;
		}
	}

	fn daemon_service_tasks(
		&self,
		stop: watch::Receiver<bool>,
	) -> Vec<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> {
		let mut tasks: Vec<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> = Vec::new();
		if let Some(control) = &self.process_generations {
			tasks.push(Box::pin(control.reconciliation_task(stop.clone())));
		}
		if let Some(control) = &self.provider_attempts {
			tasks.push(Box::pin(control.reconciliation_task(stop.clone())));
		}

		if let Some(runtime) = &self.reset_cards {
			tasks.push(Box::pin(runtime.clone().daemon_service(stop.clone())));
		}
		if let Some(observations) = &self.account_observations {
			tasks.push(Box::pin(observations.clone().daemon_service(stop.clone())));
		}

		tasks
	}

	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send {
		future::ready(vec![SnapshotItem::SystemState {
			entity_id: EntityId::new("decodexd").expect("service entity ID is bounded"),
			revision: EntityRevision(0),
			status: WireText::new("typed doctor/status is available through the daemon protocol")
				.expect("service status is bounded"),
		}])
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		match &command.payload {
			CommandPayload::CreateProgramCycle { .. }
			| CommandPayload::BindProgramDomainPack { .. }
			| CommandPayload::ContinueProgram { .. }
			| CommandPayload::RecordProgramReview { .. } => self.execute_program_command(command).await,
			CommandPayload::RegisterProject { .. }
			| CommandPayload::CreateWorkItem { .. }
			| CommandPayload::StartWorkItem { .. }
			| CommandPayload::AcceptWorkItem { .. } => Err(application_unavailable(
				"managed Factory commands are not available in Local Product V1",
			)),
			CommandPayload::CreateQuickTask { .. }
			| CommandPayload::ResumeQuickTaskRouting { .. }
			| CommandPayload::CreateQuickTaskRoutingSuccessor { .. }
			| CommandPayload::ResumeQuickTaskEstablishment { .. }
			| CommandPayload::SubmitQuickTaskTurn { .. }
			| CommandPayload::RefreshQuickTask { .. }
			| CommandPayload::ArchiveQuickTask { .. }
			| CommandPayload::InterruptQuickTask { .. } => self.execute_quick_task(command).await,
			CommandPayload::EnrollAccountFromSharedCodex { .. }
			| CommandPayload::ImportAccountCredentialFile { .. }
			| CommandPayload::SetAccountEnabled { .. }
			| CommandPayload::LogoutAccount { .. }
			| CommandPayload::SetFixedAccountSelection { .. }
			| CommandPayload::SetBalancedAccountSelection
			| CommandPayload::SetAccountOrder { .. }
			| CommandPayload::RefreshAccount { .. }
			| CommandPayload::ReauthenticateAccountFromCredentialFile { .. }
			| CommandPayload::RecoverAccountOperation { .. }
			| CommandPayload::UseAccountInCodex { .. } => {
				let publication = self.execute_account_command(command).await?;
				self.invalidate_account_observation(&publication.entity_id).await;
				self.request_account_observation_refresh();
				Ok(publication)
			},
			CommandPayload::RefreshSystemObservation { .. } => {
				Err(CommandError::ApplicationUnavailable {
					message: WireText::new(
						"foundation refresh is superseded by typed doctor/status",
					)
					.expect("service message is bounded"),
				})
			},
			CommandPayload::ConsumeResetCard { account_id, descriptor } => {
				let Some(runtime) = &self.reset_cards else {
					return Err(application_unavailable(
						"manual reset-card service is unavailable",
					));
				};
				let account_id = AccountId::new(account_id.as_str())
					.map_err(|_| application_unavailable("reset-card account is invalid"))?;
				let expected = command.expected_revision.ok_or_else(|| {
					application_unavailable("reset-card expected revision is required")
				})?;
				let expected_revision = i64::try_from(expected.0).map_err(|_| {
					application_unavailable("reset-card expected revision is invalid")
				})?;
				let descriptor = core_reset_descriptor(*descriptor)
					.map_err(|_| application_unavailable("reset-card descriptor is invalid"))?;
				let prepared = runtime
					.prepare(
						command.idempotency_key.as_str(),
						&account_id,
						expected_revision,
						descriptor,
					)
					.await
					.map_err(|error| command_reset_error(error, expected))?;
				let entity_id = EntityId::new(prepared.account_id.as_str().to_owned())
					.expect("canonical account UUID is bounded");
				let entity_revision = EntityRevision(
					u64::try_from(prepared.account_revision)
						.expect("stored account revision is positive"),
				);
				let descriptor = reset_descriptor_dto(prepared.descriptor);
				let state = ResetCardOperationResult::Prepared;
				self.invalidate_account_observation(&entity_id).await;
				self.request_account_observation_refresh();

				Ok(ApplicationPublication {
					channel: Channel::AccountsHealth,
					entity_id: entity_id.clone(),
					entity_revision,
					result: ResultPayload::ResetCardOperationAccepted {
						account_id: entity_id.clone(),
						descriptor,
						state,
					},
					event: EventPayload::ResetCardOperationAccepted {
						account_id: entity_id,
						descriptor,
						state,
					},
				})
			},
		}
	}

	async fn query<'a>(&'a self, query: &'a QueryEnvelope) -> QueryResultPayload {
		match &query.payload {
			QueryPayload::ListPrograms => QueryResultPayload::Programs(self.program_list().await),
			QueryPayload::GetProgramCycle { program_id } => {
				QueryResultPayload::ProgramCycle(self.program_cycle(program_id).await)
			},
			QueryPayload::ListProjects => {
				QueryResultPayload::Projects(ProjectListResult::Unavailable)
			},
			QueryPayload::ListQuickTasks { after, page_size } => QueryResultPayload::QuickTasks(
				self.quick_task_list(after.as_ref(), page_size.get()).await,
			),
			QueryPayload::GetQuickTask { conversation_id } => {
				QueryResultPayload::QuickTask(self.quick_task_get(conversation_id).await)
			},
			QueryPayload::GetDoctorStatus => {
				QueryResultPayload::DoctorStatus(self.refreshed_doctor().await)
			},
			QueryPayload::GetExecutionDecision { decision_id } => {
				QueryResultPayload::ExecutionDecision(self.execution_decision(decision_id).await)
			},
			QueryPayload::GetConversationHistory { conversation_id, after, page_size } => {
				QueryResultPayload::ConversationHistory(
					self.conversation_history(conversation_id, after.as_ref(), *page_size).await,
				)
			},
			QueryPayload::GetWorkItemBoardPage { project_id, state, after, page_size } => {
				QueryResultPayload::WorkItemBoard(
					self.work_item_board_page(project_id, *state, after.as_ref(), *page_size).await,
				)
			},
			QueryPayload::GetResetCards { account_id } => {
				QueryResultPayload::ResetCards(self.reset_card_inventory(account_id).await)
			},
			QueryPayload::GetResetCardOperation { idempotency_key } => {
				QueryResultPayload::ResetCardOperation(
					self.reset_card_operation(idempotency_key.as_str()).await,
				)
			},
			QueryPayload::ListAccounts => QueryResultPayload::Accounts(self.account_list().await),
			QueryPayload::InspectAccount { account_id } => {
				QueryResultPayload::Account(self.account_inspect(account_id).await)
			},
			QueryPayload::GetAccountProfile { account_id, include_email } => {
				QueryResultPayload::AccountProfile(
					self.account_profile(account_id, *include_email).await,
				)
			},
			QueryPayload::GetInitialAccountSelection => {
				QueryResultPayload::InitialAccountSelection(self.initial_account_selection().await)
			},
			QueryPayload::GetCodexAuthProjection => {
				QueryResultPayload::CodexAuthProjection(self.codex_auth_projection().await)
			},
			QueryPayload::WaitForAccountObservation { after_generation, request_refresh } => {
				if request_refresh == &Some(true) {
					self.request_account_observation_refresh();
				}
				QueryResultPayload::AccountObservation(match self.account_observations.as_ref() {
					Some(observations) => observations.wait_for_change(*after_generation).await,
					None => AccountObservationService::heartbeat(*after_generation).await,
				})
			},
		}
	}

	async fn next_publication(&self) -> Option<ApplicationEventPublication> {
		let runtime = self.quick_tasks.runtime()?;
		loop {
			let outcome = runtime.next_event().await?;
			if let Some(publication) = self.quick_task_event_publication(outcome).await {
				return Some(publication);
			}
		}
	}
}

fn quick_task_read_error(error: &StoreError) -> QuickTaskReadError {
	match error {
		StoreError::InvalidInput(_) => QuickTaskReadError::InvalidRequest,
		StoreError::Incompatible(_) | StoreError::CredentialRejected => {
			QuickTaskReadError::IntegrityUnavailable
		},
		_ => QuickTaskReadError::ProductStateUnavailable,
	}
}

fn quick_task_summary_from_row(
	row: OrdinaryTaskConversationReadback,
	projection: Option<QuickTaskProjection>,
) -> Result<QuickTaskSummary, ()> {
	let projection_updated_at_micros = row.updated_at_micros;
	if let Some(projection) = projection {
		let readback = &projection.readback;
		if readback.conversation_id != row.conversation_id
			|| readback.conversation_revision != Some(row.conversation_revision)
			|| readback.runtime_session_id.as_ref() != row.runtime_session_id.as_ref()
			|| readback.runtime_session_revision != row.runtime_session_revision
		{
			return Err(());
		}
		return quick_task_summary_from_readback(
			projection.readback,
			projection.recovery,
			projection_updated_at_micros,
		);
	}
	if let Some(pre_session_state) = row.pre_session_state {
		let (state, recovery_action) = match pre_session_state {
			OrdinaryTaskPreSessionState::RoutingPending => {
				(QuickTaskState::RoutingPending, QuickTaskRecoveryAction::ResumeRouting)
			},
			OrdinaryTaskPreSessionState::EstablishmentPending => {
				(QuickTaskState::EstablishmentPending, QuickTaskRecoveryAction::ResumeEstablishment)
			},
			OrdinaryTaskPreSessionState::QuotaExhausted => {
				(QuickTaskState::QuotaExhausted, QuickTaskRecoveryAction::CreateRoutingSuccessor)
			},
			OrdinaryTaskPreSessionState::NoRoute => {
				(QuickTaskState::NoRoute, QuickTaskRecoveryAction::CreateRoutingSuccessor)
			},
		};
		return QuickTaskSummary::new(
			EntityId::new(row.conversation_id.as_str().to_owned()).map_err(|_| ())?,
			EntityRevision(u64::try_from(row.conversation_revision).map_err(|_| ())?),
			projection_updated_at_micros,
			None,
			None,
			state,
			None,
			Some(recovery_action),
		)
		.map_err(|_| ());
	}
	let runtime_session_id = row.runtime_session_id.ok_or(())?;
	let runtime_session_revision = row.runtime_session_revision.ok_or(())?;
	let runtime_session_state = row.runtime_session_state.ok_or(())?;

	let (state, active_turn_id, recovery_action) = if runtime_session_state
		== RuntimeSessionState::Starting
		&& !row.has_active_provider_attempt
		&& !row.has_unknown_provider_attempt
		&& (!row.has_admitted_user_turn || row.active_turn_id.is_some())
	{
		(
			QuickTaskState::Establishing,
			row.active_turn_id,
			Some(QuickTaskRecoveryAction::ResumeEstablishment),
		)
	} else if row.has_unknown_provider_attempt {
		(QuickTaskState::OutcomeUnknown, row.active_turn_id, None)
	} else if row.has_active_provider_attempt {
		(
			QuickTaskState::ManualRecovery,
			row.active_turn_id,
			Some(QuickTaskRecoveryAction::ResolvePriorAttempt),
		)
	} else if row.active_turn_id.is_some() {
		(
			QuickTaskState::ManualRecovery,
			row.active_turn_id,
			Some(QuickTaskRecoveryAction::ResolvePriorActiveTurn),
		)
	} else {
		match runtime_session_state {
			RuntimeSessionState::Starting => (
				QuickTaskState::ManualRecovery,
				None,
				Some(QuickTaskRecoveryAction::StartNewConversation),
			),
			RuntimeSessionState::Active if row.has_acknowledged_turn => {
				(QuickTaskState::Ready, None, None)
			},
			RuntimeSessionState::Active => (
				QuickTaskState::ManualRecovery,
				None,
				Some(QuickTaskRecoveryAction::StartNewConversation),
			),
			RuntimeSessionState::Ended | RuntimeSessionState::Diverged => (
				QuickTaskState::ManualRecovery,
				None,
				Some(QuickTaskRecoveryAction::StartNewConversation),
			),
		}
	};
	QuickTaskSummary::new(
		EntityId::new(row.conversation_id.as_str().to_owned()).map_err(|_| ())?,
		EntityRevision(u64::try_from(row.conversation_revision).map_err(|_| ())?),
		projection_updated_at_micros,
		Some(EntityId::new(runtime_session_id.as_str().to_owned()).map_err(|_| ())?),
		Some(EntityRevision(u64::try_from(runtime_session_revision).map_err(|_| ())?)),
		state,
		active_turn_id
			.map(|turn_id| EntityId::new(turn_id.as_str().to_owned()))
			.transpose()
			.map_err(|_| ())?,
		recovery_action,
	)
	.map_err(|_| ())
}

fn quick_task_summary_from_readback(
	readback: QuickTaskReadback,
	recovery: Option<QuickTaskManualRecovery>,
	projection_updated_at_micros: i64,
) -> Result<QuickTaskSummary, ()> {
	let conversation_revision = readback.conversation_revision.ok_or(())?;
	let runtime_session_id = readback
		.runtime_session_id
		.map(|id| EntityId::new(id.as_str().to_owned()))
		.transpose()
		.map_err(|_| ())?;
	let runtime_session_revision = readback
		.runtime_session_revision
		.map(|revision| u64::try_from(revision).map(EntityRevision))
		.transpose()
		.map_err(|_| ())?;
	let state = match readback.state {
		QuickTaskLocalState::RoutingPending => QuickTaskState::RoutingPending,
		QuickTaskLocalState::EstablishmentPending => QuickTaskState::EstablishmentPending,
		QuickTaskLocalState::QuotaExhausted => QuickTaskState::QuotaExhausted,
		QuickTaskLocalState::NoRoute => QuickTaskState::NoRoute,
		QuickTaskLocalState::Establishing => QuickTaskState::Establishing,
		QuickTaskLocalState::Ready => QuickTaskState::Ready,
		QuickTaskLocalState::Running => QuickTaskState::Running,
		QuickTaskLocalState::ManualRecovery => QuickTaskState::ManualRecovery,
		QuickTaskLocalState::OutcomeUnknown => QuickTaskState::OutcomeUnknown,
	};
	QuickTaskSummary::new(
		EntityId::new(readback.conversation_id.as_str().to_owned()).map_err(|_| ())?,
		EntityRevision(u64::try_from(conversation_revision).map_err(|_| ())?),
		projection_updated_at_micros,
		runtime_session_id,
		runtime_session_revision,
		state,
		readback
			.active_turn_id
			.map(|turn_id| EntityId::new(turn_id.as_str().to_owned()))
			.transpose()
			.map_err(|_| ())?,
		match state {
			QuickTaskState::RoutingPending => Some(QuickTaskRecoveryAction::ResumeRouting),
			QuickTaskState::EstablishmentPending => {
				Some(QuickTaskRecoveryAction::ResumeEstablishment)
			},
			QuickTaskState::QuotaExhausted | QuickTaskState::NoRoute => {
				Some(QuickTaskRecoveryAction::CreateRoutingSuccessor)
			},
			_ => recovery.map(quick_task_recovery_action),
		},
	)
	.map_err(|_| ())
}

const fn quick_task_recovery_action(action: QuickTaskManualRecovery) -> QuickTaskRecoveryAction {
	match action {
		QuickTaskManualRecovery::EnableAccount => QuickTaskRecoveryAction::EnableAccount,
		QuickTaskManualRecovery::EnrollCredentials => QuickTaskRecoveryAction::EnrollCredentials,
		QuickTaskManualRecovery::ResolveAccountOperation => {
			QuickTaskRecoveryAction::ResolveAccountOperation
		},
		QuickTaskManualRecovery::RepairCredentialStore => {
			QuickTaskRecoveryAction::RepairCredentialStore
		},
		QuickTaskManualRecovery::RestoreProviderAgreement => {
			QuickTaskRecoveryAction::RestoreProviderAgreement
		},
		QuickTaskManualRecovery::RefreshQuota => QuickTaskRecoveryAction::RefreshQuota,
		QuickTaskManualRecovery::SelectedAccountDrift => {
			QuickTaskRecoveryAction::StartNewConversation
		},
		QuickTaskManualRecovery::SelectedAccountReadiness => {
			QuickTaskRecoveryAction::ConfigureAccount
		},
		QuickTaskManualRecovery::UpgradeCodex => QuickTaskRecoveryAction::UpgradeCodex,
		QuickTaskManualRecovery::SelectWorkingDirectory => {
			QuickTaskRecoveryAction::SelectWorkingDirectory
		},
		QuickTaskManualRecovery::PriorActiveTurn => QuickTaskRecoveryAction::ResolvePriorActiveTurn,
		QuickTaskManualRecovery::PriorAttemptUnresolved => {
			QuickTaskRecoveryAction::ResolvePriorAttempt
		},
		QuickTaskManualRecovery::ProcessUnavailable => {
			QuickTaskRecoveryAction::RestoreProcessReadiness
		},
		QuickTaskManualRecovery::MissingLocalProcess
		| QuickTaskManualRecovery::MissingThread
		| QuickTaskManualRecovery::IncompatibleThread => QuickTaskRecoveryAction::StartNewConversation,
	}
}

const fn quick_task_conflict() -> CommandError {
	CommandError::QuickTaskRecoveryRequired { action: QuickTaskRecoveryAction::RefreshConversation }
}

const fn quick_task_busy() -> CommandError {
	CommandError::QuickTaskRecoveryRequired {
		action: QuickTaskRecoveryAction::WaitForCurrentCommand,
	}
}

fn quick_task_command_projection(
	outcome: QuickTaskOutcome,
) -> Result<(ConversationId, bool), CommandError> {
	let (readback, interrupt) = match outcome {
		QuickTaskOutcome::PreSession(readback)
		| QuickTaskOutcome::Started { readback, .. }
		| QuickTaskOutcome::Terminal { readback, .. } => (readback, false),
		QuickTaskOutcome::InterruptRequested(readback) => (readback, true),
		QuickTaskOutcome::ManualRecovery { action, .. } => {
			return Err(CommandError::QuickTaskRecoveryRequired {
				action: quick_task_recovery_action(action),
			});
		},
		QuickTaskOutcome::Unknown { .. } => return Err(CommandError::AcceptanceUnknown),
		QuickTaskOutcome::Busy(_) => return Err(quick_task_busy()),
		QuickTaskOutcome::Conflict => return Err(quick_task_conflict()),
		QuickTaskOutcome::Streaming { .. } | QuickTaskOutcome::Unavailable => {
			return Err(application_unavailable("Quick Task execution is unavailable"));
		},
	};
	Ok((readback.conversation_id, interrupt))
}

fn quick_task_command_publication(
	conversation: QuickTaskSummary,
	interrupt: bool,
) -> Result<ApplicationPublication, CommandError> {
	let entity_id = conversation.conversation_id.clone();
	let entity_revision = conversation.conversation_revision;
	let result = if interrupt {
		ResultPayload::QuickTaskInterruptAccepted { conversation: conversation.clone() }
	} else {
		ResultPayload::QuickTaskConversationAccepted { conversation: conversation.clone() }
	};
	Ok(ApplicationPublication {
		channel: Channel::ConversationStream,
		entity_id,
		entity_revision,
		result,
		event: EventPayload::QuickTaskConversationChanged { conversation },
	})
}

fn runtime_execution_settings(
	settings: &QuickTaskExecutionSettingsDto,
) -> RuntimeQuickTaskExecutionSettings {
	RuntimeQuickTaskExecutionSettings {
		model: settings.model.as_str().to_owned(),
		reasoning_effort: settings.reasoning_effort.as_str().to_owned(),
		fast: settings.fast,
	}
}

fn store_program_create(
	draft: &ProgramCycleDraftDto,
	domain_pack: DomainPackIdentity,
) -> Result<StoreCreateProgramCycle, CommandError> {
	Ok(StoreCreateProgramCycle {
		program_id: ProgramId::new(draft.program_id.as_str())
			.map_err(|_| application_unavailable("Program identity is invalid"))?,
		domain_pack: Some(domain_pack),
		signal_id: ProgramObservationId::new(draft.signal_id.as_str())
			.map_err(|_| application_unavailable("Signal identity is invalid"))?,
		claim_id: ProgramClaimId::new(draft.claim_id.as_str())
			.map_err(|_| application_unavailable("Claim identity is invalid"))?,
		proposal_id: ProgramProposalId::new(draft.proposal_id.as_str())
			.map_err(|_| application_unavailable("Proposal identity is invalid"))?,
		objective_id: ObjectiveId::new(draft.objective_id.as_str())
			.map_err(|_| application_unavailable("Objective identity is invalid"))?,
		work_item_id: WorkItemId::new(draft.work_item_id.as_str())
			.map_err(|_| application_unavailable("WorkItem identity is invalid"))?,
		name: draft.name.as_str().to_owned(),
		purpose: draft.purpose.as_str().to_owned(),
		non_goals: draft.non_goals.iter().map(|value| value.as_str().to_owned()).collect(),
		review_policy: draft.review_policy.as_str().to_owned(),
		signal_source: draft.signal_source.as_str().to_owned(),
		signal_summary: draft.signal_summary.as_str().to_owned(),
		signal_observed_at_micros: draft.signal_observed_at_micros,
		claim_statement: draft.claim_statement.as_str().to_owned(),
		proposal_summary: draft.proposal_summary.as_str().to_owned(),
		proposal_expected_effect: draft.proposal_expected_effect.as_str().to_owned(),
		proposal_risk: draft.proposal_risk.as_str().to_owned(),
		proposal_evidence_need: draft.proposal_evidence_need.as_str().to_owned(),
		objective_outcome: draft.objective_outcome.as_str().to_owned(),
		acceptance_criteria: draft
			.acceptance_criteria
			.iter()
			.map(|value| value.as_str().to_owned())
			.collect(),
		validation_criteria: draft
			.validation_criteria
			.iter()
			.map(|value| value.as_str().to_owned())
			.collect(),
		work_item_title: draft.work_item_title.as_str().to_owned(),
		work_item_instructions: draft.work_item_instructions.as_str().to_owned(),
		working_directory: draft.working_directory.as_str().to_owned(),
	})
}

fn store_program_continuation(
	continuation: &ProgramContinuationDraftDto,
	expected_revision: EntityRevision,
) -> Result<StoreContinueProgram, CommandError> {
	Ok(StoreContinueProgram {
		program_id: ProgramId::new(continuation.program_id.as_str())
			.map_err(|_| application_unavailable("Program identity is invalid"))?,
		predecessor_review_id: ProgramReviewId::new(continuation.predecessor_review_id.as_str())
			.map_err(|_| application_unavailable("Review identity is invalid"))?,
		expected_revision: expected_revision.0,
		signal_id: ProgramObservationId::new(continuation.signal_id.as_str())
			.map_err(|_| application_unavailable("Signal identity is invalid"))?,
		claim_id: ProgramClaimId::new(continuation.claim_id.as_str())
			.map_err(|_| application_unavailable("Claim identity is invalid"))?,
		proposal_id: ProgramProposalId::new(continuation.proposal_id.as_str())
			.map_err(|_| application_unavailable("Proposal identity is invalid"))?,
		objective_id: ObjectiveId::new(continuation.objective_id.as_str())
			.map_err(|_| application_unavailable("Objective identity is invalid"))?,
		work_item_id: WorkItemId::new(continuation.work_item_id.as_str())
			.map_err(|_| application_unavailable("WorkItem identity is invalid"))?,
		signal_source: continuation.signal_source.as_str().to_owned(),
		signal_summary: continuation.signal_summary.as_str().to_owned(),
		signal_observed_at_micros: continuation.signal_observed_at_micros,
		claim_statement: continuation.claim_statement.as_str().to_owned(),
		proposal_summary: continuation.proposal_summary.as_str().to_owned(),
		proposal_expected_effect: continuation.proposal_expected_effect.as_str().to_owned(),
		proposal_risk: continuation.proposal_risk.as_str().to_owned(),
		proposal_evidence_need: continuation.proposal_evidence_need.as_str().to_owned(),
		objective_outcome: continuation.objective_outcome.as_str().to_owned(),
		acceptance_criteria: continuation
			.acceptance_criteria
			.iter()
			.map(|value| value.as_str().to_owned())
			.collect(),
		validation_criteria: continuation
			.validation_criteria
			.iter()
			.map(|value| value.as_str().to_owned())
			.collect(),
		work_item_title: continuation.work_item_title.as_str().to_owned(),
		work_item_instructions: continuation.work_item_instructions.as_str().to_owned(),
		working_directory: continuation.working_directory.as_str().to_owned(),
	})
}

fn store_program_review(
	review: &ProgramReviewDraftDto,
) -> Result<RecordProgramReview, CommandError> {
	let evidence = |draft: &decodex_protocol::ProgramEvidenceDraftDto| {
		Ok(ProgramEvidenceInput {
			evidence_id: ProgramEvidenceId::new(draft.evidence_id.as_str())
				.map_err(|_| application_unavailable("Evidence identity is invalid"))?,
			source: draft.source.as_str().to_owned(),
			summary: draft.summary.as_str().to_owned(),
			observed_at_micros: draft.observed_at_micros,
		})
	};
	Ok(RecordProgramReview {
		review_id: ProgramReviewId::new(review.review_id.as_str())
			.map_err(|_| application_unavailable("Review identity is invalid"))?,
		program_id: ProgramId::new(review.program_id.as_str())
			.map_err(|_| application_unavailable("Program identity is invalid"))?,
		work_item_id: WorkItemId::new(review.work_item_id.as_str())
			.map_err(|_| application_unavailable("WorkItem identity is invalid"))?,
		deterministic: evidence(&review.deterministic)?,
		external: evidence(&review.external)?,
		classification: review.classification,
		rationale: review.rationale.as_str().to_owned(),
	})
}

fn program_summary_dto(record: ProgramSummaryRecord) -> Result<ProgramSummaryDto, ()> {
	Ok(ProgramSummaryDto {
		program_id: entity(record.program_id.as_str())?,
		name: wire(record.name)?,
		purpose: wire(record.purpose)?,
		state: record.state,
		revision: EntityRevision(record.revision),
		updated_at_micros: record.updated_at_micros,
	})
}

fn program_cycle_dto(
	record: ProgramCycleRecord,
	run_states: &[(ConversationId, &'static str)],
) -> Result<ProgramCycleDto, ()> {
	let node_order = program_node_order(&record)?;
	let program = ProgramSummaryDto {
		program_id: entity(record.program.program_id.as_str())?,
		name: wire(record.program.name)?,
		purpose: wire(record.program.purpose)?,
		state: record.program.state,
		revision: EntityRevision(record.program.revision),
		updated_at_micros: record.program.updated_at_micros,
	};
	let non_goals =
		record.program.non_goals.into_iter().map(wire).collect::<Result<Vec<_>, _>>()?;
	let review_policy = wire(record.program.review_policy)?;
	let mut nodes = Vec::new();
	let mut edges = Vec::new();

	for signal in record.signals {
		let signal_id = entity(signal.signal_id.as_str())?;
		let (from, kind) = match signal.predecessor_review_id {
			Some(review_id) => (entity(review_id.as_str())?, ProgramRelationKind::Continues),
			None => (program.program_id.clone(), ProgramRelationKind::Observes),
		};
		edges.push(ProgramEdgeDto { from, to: signal_id.clone(), kind });
		nodes.push(ProgramNodeDto {
			id: signal_id,
			kind: ProgramNodeKind::Signal,
			title: wire("Signal")?,
			summary: wire(signal.summary)?,
			state: wire("observed")?,
			source: Some(wire(signal.source)?),
			observed_at_micros: Some(signal.observed_at_micros),
			conversation_id: None,
			fields: Vec::new(),
		});
	}
	for claim in record.claims {
		let claim_id = entity(claim.claim_id.as_str())?;
		edges.push(ProgramEdgeDto {
			from: entity(claim.signal_id.as_str())?,
			to: claim_id.clone(),
			kind: ProgramRelationKind::Supports,
		});
		nodes.push(ProgramNodeDto {
			id: claim_id,
			kind: ProgramNodeKind::Claim,
			title: wire("Claim")?,
			summary: wire(claim.statement)?,
			state: wire("current")?,
			source: None,
			observed_at_micros: Some(claim.updated_at_micros),
			conversation_id: None,
			fields: Vec::new(),
		});
	}
	for proposal in record.proposals {
		let proposal_id = entity(proposal.proposal_id.as_str())?;
		edges.push(ProgramEdgeDto {
			from: entity(proposal.claim_id.as_str())?,
			to: proposal_id.clone(),
			kind: ProgramRelationKind::Justifies,
		});
		nodes.push(ProgramNodeDto {
			id: proposal_id,
			kind: ProgramNodeKind::Proposal,
			title: wire("Proposal")?,
			summary: wire(proposal.summary)?,
			state: wire("non_executable")?,
			source: None,
			observed_at_micros: Some(proposal.updated_at_micros),
			conversation_id: None,
			fields: vec![
				field("Expected effect", proposal.expected_effect)?,
				field("Risk", proposal.risk)?,
				field("Evidence need", proposal.evidence_need)?,
			],
		});
	}
	for objective in record.objectives {
		let objective_id = entity(objective.objective_id.as_str())?;
		edges.push(ProgramEdgeDto {
			from: entity(objective.proposal_id.as_str())?,
			to: objective_id.clone(),
			kind: ProgramRelationKind::Proposes,
		});
		nodes.push(ProgramNodeDto {
			id: objective_id,
			kind: ProgramNodeKind::Objective,
			title: wire("Objective")?,
			summary: wire(objective.outcome)?,
			state: wire(objective.state.as_str())?,
			source: None,
			observed_at_micros: Some(objective.updated_at_micros),
			conversation_id: None,
			fields: vec![
				field("Acceptance criteria", objective.acceptance_criteria.join(" · "))?,
				field("Validation criteria", objective.validation_criteria.join(" · "))?,
			],
		});
	}
	for work_item in record.work_items {
		let work_item_id = entity(work_item.work_item_id.as_str())?;
		edges.push(ProgramEdgeDto {
			from: entity(work_item.objective_id.as_str())?,
			to: work_item_id.clone(),
			kind: ProgramRelationKind::DecomposesTo,
		});
		nodes.push(ProgramNodeDto {
			id: work_item_id.clone(),
			kind: ProgramNodeKind::WorkItem,
			title: wire(work_item.title)?,
			summary: wire(work_item.instructions)?,
			state: wire(work_item.state.as_str())?,
			source: None,
			observed_at_micros: Some(work_item.updated_at_micros),
			conversation_id: work_item
				.conversation_id
				.as_ref()
				.map(|id| entity(id.as_str()))
				.transpose()?,
			fields: vec![field("Working directory", work_item.working_directory)?],
		});
		if let Some(conversation_id) = work_item.conversation_id {
			let run_id = entity(conversation_id.as_str())?;
			let state = run_states
				.iter()
				.find(|(id, _)| id == &conversation_id)
				.map_or("unavailable", |(_, state)| *state);
			edges.push(ProgramEdgeDto {
				from: work_item_id,
				to: run_id.clone(),
				kind: ProgramRelationKind::Executes,
			});
			nodes.push(ProgramNodeDto {
				id: run_id.clone(),
				kind: ProgramNodeKind::Run,
				title: wire("Codex Quick Task")?,
				summary: wire("Execution through the existing Codex app-server worker path")?,
				state: wire(state)?,
				source: None,
				observed_at_micros: None,
				conversation_id: Some(run_id),
				fields: Vec::new(),
			});
		}
	}
	for evidence in record.evidence {
		let evidence_id = entity(evidence.evidence_id.as_str())?;
		edges.push(ProgramEdgeDto {
			from: entity(evidence.work_item_id.as_str())?,
			to: evidence_id.clone(),
			kind: ProgramRelationKind::Produces,
		});
		nodes.push(ProgramNodeDto {
			id: evidence_id,
			kind: ProgramNodeKind::Evidence,
			title: wire(match evidence.kind {
				decodex_core::ProgramEvidenceKind::DeterministicValidation => {
					"Deterministic validation"
				},
				decodex_core::ProgramEvidenceKind::External => "External evidence",
			})?,
			summary: wire(evidence.summary)?,
			state: wire(evidence.kind.as_str())?,
			source: Some(wire(evidence.source)?),
			observed_at_micros: Some(evidence.observed_at_micros),
			conversation_id: None,
			fields: Vec::new(),
		});
	}
	for review in record.reviews {
		let review_id = entity(review.review_id.as_str())?;
		for evidence_id in [&review.deterministic_evidence_id, &review.external_evidence_id] {
			edges.push(ProgramEdgeDto {
				from: entity(evidence_id.as_str())?,
				to: review_id.clone(),
				kind: ProgramRelationKind::Supports,
			});
		}
		edges.push(ProgramEdgeDto {
			from: review_id.clone(),
			to: program.program_id.clone(),
			kind: ProgramRelationKind::Validates,
		});
		nodes.push(ProgramNodeDto {
			id: review_id,
			kind: ProgramNodeKind::Review,
			title: wire("Program Review")?,
			summary: wire(review.rationale)?,
			state: wire(review.classification.as_str())?,
			source: None,
			observed_at_micros: Some(review.created_at_micros),
			conversation_id: None,
			fields: Vec::new(),
		});
	}

	let positions = node_order
		.iter()
		.enumerate()
		.map(|(index, id)| (id.as_str(), index))
		.collect::<HashMap<_, _>>();
	if positions.len() != nodes.len()
		|| nodes.iter().any(|node| !positions.contains_key(node.id.as_str()))
	{
		return Err(());
	}
	nodes.sort_by_key(|node| positions[node.id.as_str()]);
	ProgramCycleDto::new(program, non_goals, review_policy, nodes, edges).map_err(|_| ())
}

fn program_node_order(record: &ProgramCycleRecord) -> Result<Vec<String>, ()> {
	let roots = record
		.signals
		.iter()
		.filter(|signal| signal.predecessor_review_id.is_none())
		.collect::<Vec<_>>();
	if roots.len() != 1 {
		return Err(());
	}
	let mut successors = HashMap::new();
	for signal in &record.signals {
		if let Some(predecessor) = &signal.predecessor_review_id
			&& successors.insert(predecessor.as_str(), signal).is_some()
		{
			return Err(());
		}
	}
	let mut claims = HashMap::new();
	for claim in &record.claims {
		if claims.insert(claim.signal_id.as_str(), claim).is_some() {
			return Err(());
		}
	}
	let mut proposals = HashMap::new();
	for proposal in &record.proposals {
		if proposals.insert(proposal.claim_id.as_str(), proposal).is_some() {
			return Err(());
		}
	}
	let mut objectives = HashMap::new();
	for objective in &record.objectives {
		if objectives.insert(objective.proposal_id.as_str(), objective).is_some() {
			return Err(());
		}
	}
	let mut work_items = HashMap::new();
	for work_item in &record.work_items {
		if work_items.insert(work_item.objective_id.as_str(), work_item).is_some() {
			return Err(());
		}
	}
	let mut reviews = HashMap::new();
	for review in &record.reviews {
		if reviews.insert(review.work_item_id.as_str(), review).is_some() {
			return Err(());
		}
	}
	let mut evidence = HashMap::<&str, Vec<_>>::new();
	for item in &record.evidence {
		evidence.entry(item.work_item_id.as_str()).or_default().push(item);
	}

	let mut order = Vec::new();
	let mut visited_signals = HashSet::new();
	let mut signal = roots[0];
	loop {
		if !visited_signals.insert(signal.signal_id.as_str()) {
			return Err(());
		}
		order.push(signal.signal_id.as_str().to_owned());
		let claim = claims.get(signal.signal_id.as_str()).ok_or(())?;
		order.push(claim.claim_id.as_str().to_owned());
		let proposal = proposals.get(claim.claim_id.as_str()).ok_or(())?;
		order.push(proposal.proposal_id.as_str().to_owned());
		let objective = objectives.get(proposal.proposal_id.as_str()).ok_or(())?;
		order.push(objective.objective_id.as_str().to_owned());
		let work_item = work_items.get(objective.objective_id.as_str()).ok_or(())?;
		order.push(work_item.work_item_id.as_str().to_owned());
		if let Some(conversation_id) = &work_item.conversation_id {
			order.push(conversation_id.as_str().to_owned());
		}
		let item_evidence =
			evidence.get(work_item.work_item_id.as_str()).map_or(&[][..], Vec::as_slice);
		let Some(review) = reviews.get(work_item.work_item_id.as_str()).copied() else {
			if !item_evidence.is_empty() {
				return Err(());
			}
			break;
		};
		let evidence_ids =
			item_evidence.iter().map(|item| item.evidence_id.as_str()).collect::<HashSet<_>>();
		if item_evidence.len() != 2
			|| !evidence_ids.contains(review.deterministic_evidence_id.as_str())
			|| !evidence_ids.contains(review.external_evidence_id.as_str())
		{
			return Err(());
		}
		order.push(review.deterministic_evidence_id.as_str().to_owned());
		order.push(review.external_evidence_id.as_str().to_owned());
		order.push(review.review_id.as_str().to_owned());
		let Some(next) = successors.get(review.review_id.as_str()).copied() else {
			break;
		};
		signal = next;
	}

	let expected = record.signals.len()
		+ record.claims.len()
		+ record.proposals.len()
		+ record.objectives.len()
		+ record.work_items.len()
		+ record.work_items.iter().filter(|item| item.conversation_id.is_some()).count()
		+ record.evidence.len()
		+ record.reviews.len();
	if order.len() != expected
		|| order.iter().map(String::as_str).collect::<HashSet<_>>().len() != expected
	{
		return Err(());
	}
	Ok(order)
}

fn field(label: &str, value: impl Into<String>) -> Result<ProgramNodeFieldDto, ()> {
	Ok(ProgramNodeFieldDto { label: wire(label)?, value: wire(value)? })
}

fn entity(value: &str) -> Result<EntityId, ()> {
	EntityId::new(value.to_owned()).map_err(|_| ())
}

fn wire(value: impl Into<String>) -> Result<WireText, ()> {
	WireText::new(value).map_err(|_| ())
}

const fn quick_task_state_text(state: QuickTaskState) -> &'static str {
	match state {
		QuickTaskState::RoutingPending => "routing_pending",
		QuickTaskState::EstablishmentPending => "establishment_pending",
		QuickTaskState::QuotaExhausted => "quota_exhausted",
		QuickTaskState::NoRoute => "no_route",
		QuickTaskState::Establishing => "establishing",
		QuickTaskState::Ready => "ready",
		QuickTaskState::Running => "running",
		QuickTaskState::ManualRecovery => "manual_recovery",
		QuickTaskState::OutcomeUnknown => "outcome_unknown",
	}
}

fn program_command_error(error: StoreError) -> CommandError {
	match error {
		StoreError::IdempotencyConflict => CommandError::IdempotencyConflict,
		StoreError::RevisionConflict { expected: Some(expected), actual: Some(actual), .. } => {
			match (u64::try_from(expected), u64::try_from(actual)) {
				(Ok(expected), Ok(actual)) => CommandError::ExpectedRevisionMismatch {
					expected: EntityRevision(expected),
					actual: EntityRevision(actual),
				},
				_ => application_unavailable("Program command conflicts with current state"),
			}
		},
		StoreError::Database(_) | StoreError::Incompatible(_) => {
			application_unavailable("Program storage is unavailable")
		},
		_ => application_unavailable("Program command conflicts with current state"),
	}
}

fn domain_pack_command_error(error: DomainPackError) -> CommandError {
	application_unavailable(match error {
		DomainPackError::UnknownPack => "Domain Pack is not built in",
		DomainPackError::BindingMissing => "Program Domain Pack is not bound",
		DomainPackError::BindingMismatch => "Program Domain Pack binding is incompatible",
		DomainPackError::CapabilityDenied => "Domain Pack does not grant this capability",
		DomainPackError::RegistryInvalid | DomainPackError::ProjectionInvalid => {
			"Domain Pack registry is unavailable"
		},
	})
}

fn quick_task_routing_successor_publication(
	source_conversation_id: &ConversationId,
	source_revision: i64,
	successor_conversation_id: &ConversationId,
	successor_revision: i64,
	successor: QuickTaskSummary,
) -> Result<ApplicationPublication, CommandError> {
	let successor_revision =
		EntityRevision(u64::try_from(successor_revision).map_err(|_| quick_task_conflict())?);
	if successor.conversation_id.as_str() != successor_conversation_id.as_str()
		|| successor.conversation_revision != successor_revision
	{
		return Err(quick_task_conflict());
	}
	let source_conversation_id = EntityId::new(source_conversation_id.as_str().to_owned())
		.map_err(|_| quick_task_conflict())?;
	let source_conversation_revision =
		EntityRevision(u64::try_from(source_revision).map_err(|_| quick_task_conflict())?);
	Ok(ApplicationPublication {
		channel: Channel::ConversationStream,
		entity_id: successor.conversation_id.clone(),
		entity_revision: successor.conversation_revision,
		result: ResultPayload::QuickTaskRoutingSuccessorAccepted {
			source_conversation_id,
			source_conversation_revision,
			successor: successor.clone(),
		},
		event: EventPayload::QuickTaskConversationChanged { conversation: successor },
	})
}

impl ServiceApplication {
	async fn quick_task_event_publication(
		&self,
		outcome: QuickTaskOutcome,
	) -> Option<ApplicationEventPublication> {
		match outcome {
			QuickTaskOutcome::Streaming { readback, history_item_id, text } => {
				let correlation_id =
					CorrelationId::new(readback.correlation_id.as_deref()?).ok()?;
				let causation_id =
					readback.causation_id.as_deref().map(CausationId::new).transpose().ok()?;
				let conversation_id =
					EntityId::new(readback.conversation_id.as_str().to_owned()).ok()?;
				let turn_id = EntityId::new(readback.active_turn_id?.as_str().to_owned()).ok()?;
				let entity_id = EntityId::new(history_item_id.as_str().to_owned()).ok()?;
				Some(ApplicationEventPublication {
					correlation_id,
					causation_id,
					channel: Channel::ConversationStream,
					entity_id,
					entity_revision: EntityRevision(1),
					event: EventPayload::QuickTaskMessageDelta {
						conversation_id,
						turn_id,
						delta: text,
					},
				})
			},
			QuickTaskOutcome::Terminal { readback, turn_id, state, .. } => {
				let conversation = self.quick_task_event_summary(&readback).await?;
				quick_task_terminal_publication(readback, turn_id, state, conversation)
			},
			QuickTaskOutcome::Unknown { readback, .. } => {
				let conversation = self.quick_task_event_summary(&readback).await?;
				quick_task_summary_publication(readback, conversation, "unknown")
			},
			QuickTaskOutcome::ManualRecovery { readback, .. } => {
				let conversation = self.quick_task_event_summary(&readback).await?;
				quick_task_summary_publication(readback, conversation, "recovery")
			},
			QuickTaskOutcome::PreSession(_)
			| QuickTaskOutcome::Started { .. }
			| QuickTaskOutcome::Busy(_)
			| QuickTaskOutcome::Conflict
			| QuickTaskOutcome::InterruptRequested(_)
			| QuickTaskOutcome::Unavailable => None,
		}
	}

	async fn quick_task_event_summary(
		&self,
		readback: &QuickTaskReadback,
	) -> Option<QuickTaskSummary> {
		let conversation_id = EntityId::new(readback.conversation_id.as_str().to_owned()).ok()?;
		match self.quick_task_get(&conversation_id).await {
			QuickTaskResult::Available(summary) => Some(summary),
			_ => None,
		}
	}
}

fn quick_task_terminal_publication(
	readback: QuickTaskReadback,
	turn_id: TurnId,
	state: QuickTaskTerminalState,
	conversation: QuickTaskSummary,
) -> Option<ApplicationEventPublication> {
	let mut publication = quick_task_summary_publication(readback, conversation, "terminal")?;
	let EventPayload::QuickTaskConversationChanged { conversation } = publication.event else {
		return None;
	};
	publication.event = EventPayload::QuickTaskTurnFinished {
		conversation,
		turn_id: EntityId::new(turn_id.as_str().to_owned()).ok()?,
		outcome: match state {
			QuickTaskTerminalState::Succeeded => QuickTaskTurnOutcome::Succeeded,
			QuickTaskTerminalState::Failed => QuickTaskTurnOutcome::Failed,
		},
	};
	Some(publication)
}

fn quick_task_summary_publication(
	readback: QuickTaskReadback,
	conversation: QuickTaskSummary,
	phase: &'static str,
) -> Option<ApplicationEventPublication> {
	let correlation_id = CorrelationId::new(readback.correlation_id.as_deref()?).ok()?;
	let entity_id =
		EntityId::new(format!("conversation-event/{}/{phase}", readback.operation_key.as_deref()?))
			.ok()?;
	let causation_id = readback.causation_id.as_deref().map(CausationId::new).transpose().ok()?;
	Some(ApplicationEventPublication {
		correlation_id,
		causation_id,
		channel: Channel::ConversationStream,
		entity_id,
		entity_revision: EntityRevision(1),
		event: EventPayload::QuickTaskConversationChanged { conversation },
	})
}

#[cfg(any())]
fn work_item_board_page_dto(
	project_id: WorkItemBoardProjectId,
	state: Option<WorkItemState>,
	after: Option<WorkItemBoardWorkItemId>,
	page_size: WorkItemBoardPageSize,
	items: Vec<StoredWorkItem>,
) -> Result<WorkItemBoardPage, ()> {
	let requested = usize::from(page_size.get());
	let maximum_observation = requested.checked_add(1).ok_or(())?;
	if items.len() > maximum_observation {
		return Err(());
	}

	let mut cards =
		items.into_iter().map(work_item_board_card_dto).collect::<Result<Vec<_>, _>>()?;
	if cards.iter().any(|card| {
		card.project_id() != &project_id || state.is_some_and(|expected| card.state() != expected)
	}) || cards.windows(2).any(|pair| pair[0].work_item_id() >= pair[1].work_item_id())
		|| after
			.as_ref()
			.is_some_and(|cursor| cards.first().is_some_and(|card| card.work_item_id() <= cursor))
	{
		return Err(());
	}

	let has_more = cards.len() > requested;
	if has_more {
		cards.pop().ok_or(())?;
	}
	let next_cursor =
		if has_more { Some(cards.last().ok_or(())?.work_item_id().clone()) } else { None };

	WorkItemBoardPage::new(project_id, state, after, page_size, cards, next_cursor).map_err(|_| ())
}

#[cfg(any())]
fn work_item_board_card_dto(stored: StoredWorkItem) -> Result<WorkItemBoardCard, ()> {
	let StoredWorkItem { work_item, edges, accepted_revision } = stored;
	let work_item_id = WorkItemBoardWorkItemId::new(work_item.id().as_str()).map_err(|_| ())?;
	let project_id =
		WorkItemBoardProjectId::new(work_item.project_id().as_str()).map_err(|_| ())?;
	let lead_id =
		WorkItemBoardLeadId::new(work_item.declared_lead_id().as_str()).map_err(|_| ())?;
	let program_id = work_item
		.program()
		.map(|program| WorkItemBoardProgramId::new(program.program_id().as_str()))
		.transpose()
		.map_err(|_| ())?;
	let objective_ids = work_item
		.objectives()
		.iter()
		.map(|objective| WorkItemBoardObjectiveId::new(objective.objective_id().as_str()))
		.collect::<Result<Vec<_>, _>>()
		.map_err(|_| ())?;
	let mut depends_on_ids = Vec::new();
	let mut blocked_by_ids = Vec::new();

	for edge in edges {
		if edge.work_item_id() != work_item.id() || edge.project_id() != work_item.project_id() {
			return Err(());
		}
		let related =
			WorkItemBoardWorkItemId::new(edge.related_work_item_id().as_str()).map_err(|_| ())?;
		match edge.kind() {
			WorkItemEdgeKind::DependsOn => depends_on_ids.push(related),
			WorkItemEdgeKind::BlockedBy => blocked_by_ids.push(related),
		}
	}

	WorkItemBoardCard::new(
		work_item_id,
		project_id,
		lead_id,
		program_id,
		objective_ids,
		depends_on_ids,
		blocked_by_ids,
		WorkItemBoardTitle::new(work_item.title()).map_err(|_| ())?,
		work_item.priority(),
		work_item.state(),
		EntityRevision(work_item.revision()),
		accepted_revision.map(EntityRevision),
	)
	.map_err(|_| ())
}

#[cfg(any())]
fn execution_decision_dto(readback: ExecutionDecisionReadback) -> Result<ExecutionDecisionDto, ()> {
	let consumer = match readback.consumer {
		ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision,
			source_runtime_session_id,
			source_runtime_session_revision,
			turn_id,
		} => ExecutionConsumerDto::ConversationTurn {
			conversation_id: entity(conversation_id.as_str())?,
			conversation_revision,
			source_runtime_session_id: source_runtime_session_id
				.as_ref()
				.map(|id| entity(id.as_str()))
				.transpose()?,
			source_runtime_session_revision,
			turn_id: entity(turn_id.as_str())?,
		},
		ExecutionConsumer::ManagedRunExecution {
			managed_run_id,
			managed_run_revision,
			execution_id,
		} => ExecutionConsumerDto::ManagedRunExecution {
			managed_run_id: entity(managed_run_id.as_str())?,
			managed_run_revision,
			managed_execution_id: entity(execution_id.as_str())?,
		},
	};
	let causes = readback
		.causes
		.into_iter()
		.map(|cause| {
			Ok(ExecutionRouteCauseDto {
				account_id: entity(cause.account_id.as_str())?,
				blocker: blocker_dto(cause.blocker),
			})
		})
		.collect::<Result<Vec<_>, ()>>()?;
	let quota_exclusions = readback
		.quota_exclusions
		.into_iter()
		.map(quota_exclusion_dto)
		.collect::<Result<Vec<_>, ()>>()?;
	let route = match readback.kind {
		RoutingDecisionKind::Selected => ExecutionRouteDto::Selected {
			account_id: entity(readback.selected_account_id.as_ref().ok_or(())?.as_str())?,
			quota_exclusions,
		},
		RoutingDecisionKind::WaitingUsage => ExecutionRouteDto::WaitingUsage {
			ready_at_micros: readback.waiting_ready_at_micros.ok_or(())?,
			causes,
			quota_exclusions,
		},
		RoutingDecisionKind::WaitingReconciliation => {
			ExecutionRouteDto::WaitingReconciliation { causes }
		},
		RoutingDecisionKind::NoRoute if !causes.is_empty() => ExecutionRouteDto::NoRoute { causes },
		RoutingDecisionKind::NoRoute => return Err(()),
	};
	Ok(ExecutionDecisionDto { decision_id: entity(&readback.decision_id)?, consumer, route })
}

#[cfg(any())]
fn quota_exclusion_dto(
	exclusion: ExecutionQuotaExclusion,
) -> Result<ExecutionQuotaExclusionDto, ()> {
	Ok(ExecutionQuotaExclusionDto {
		account_id: entity(exclusion.account_id.as_str())?,
		window: match exclusion.window {
			QuotaWindowClass::FiveHour => ExecutionQuotaWindowDto::FiveHour,
			QuotaWindowClass::SevenDay => ExecutionQuotaWindowDto::SevenDay,
		},
		duration_minutes: exclusion.duration_minutes,
		observation_revision: exclusion.observation_revision,
		resets_at_micros: exclusion.resets_at_micros,
	})
}

#[cfg(any())]
const fn blocker_dto(blocker: RoutingBlocker) -> ExecutionRouteBlockerDto {
	use ExecutionRouteBlockerDto as Dto;
	use RoutingBlocker as Core;
	match blocker {
		Core::ExcludedByPolicy => Dto::ExcludedByPolicy,
		Core::AccountFromFuture => Dto::AccountFromFuture,
		Core::AccountStale => Dto::AccountStale,
		Core::AccountUnavailable => Dto::AccountUnavailable,
		Core::AccountUnknown => Dto::AccountUnknown,
		Core::AccountDepleted => Dto::AccountDepleted,
		Core::AccountAuthFailed => Dto::AccountAuthFailed,
		Core::AccountPluginUnready => Dto::AccountPluginUnready,
		Core::AccountDisabled => Dto::AccountDisabled,
		Core::EvidenceMissing => Dto::EvidenceMissing,
		Core::EvidenceFromFuture => Dto::EvidenceFromFuture,
		Core::EvidenceStale => Dto::EvidenceStale,
		Core::EvidenceAccountMismatch => Dto::EvidenceAccountMismatch,
		Core::EvidenceProfileMismatch => Dto::EvidenceProfileMismatch,
		Core::EvidenceBuildMismatch => Dto::EvidenceBuildMismatch,
		Core::QuotaFiveHourMissing => Dto::QuotaFiveHourMissing,
		Core::QuotaFiveHourFromFuture => Dto::QuotaFiveHourFromFuture,
		Core::QuotaFiveHourStale => Dto::QuotaFiveHourStale,
		Core::QuotaFiveHourUnknown => Dto::QuotaFiveHourUnknown,
		Core::QuotaFiveHourResetElapsed => Dto::QuotaFiveHourResetElapsed,
		Core::QuotaFiveHourDepleted => Dto::QuotaFiveHourDepleted,
		Core::QuotaSevenDayMissing => Dto::QuotaSevenDayMissing,
		Core::QuotaSevenDayFromFuture => Dto::QuotaSevenDayFromFuture,
		Core::QuotaSevenDayStale => Dto::QuotaSevenDayStale,
		Core::QuotaSevenDayUnknown => Dto::QuotaSevenDayUnknown,
		Core::QuotaSevenDayResetElapsed => Dto::QuotaSevenDayResetElapsed,
		Core::QuotaSevenDayDepleted => Dto::QuotaSevenDayDepleted,
		Core::RequiredCapabilityUnsatisfied => Dto::RequiredCapabilityUnsatisfied,
		Core::AuthenticationRequired => Dto::AuthenticationRequired,
		Core::PluginUnready => Dto::PluginUnready,
		Core::DependencyBlocked => Dto::DependencyBlocked,
		Core::ApprovalRequired => Dto::ApprovalRequired,
		Core::UserRequired => Dto::UserRequired,
		Core::ExternalBlocked => Dto::ExternalBlocked,
		Core::UsageUnproven => Dto::UsageUnproven,
		Core::ReconciliationUnproven => Dto::ReconciliationUnproven,
		Core::ReviewerUnavailable => Dto::ReviewerUnavailable,
		Core::ReviewerFailed => Dto::ReviewerFailed,
		Core::ReviewerAmbiguous => Dto::ReviewerAmbiguous,
		Core::ProcessGenerationUnresolved => Dto::ProcessGenerationUnresolved,
		Core::ProcessGenerationUnavailable => Dto::ProcessGenerationUnavailable,
		Core::ProviderAttemptUnresolved => Dto::ProviderAttemptUnresolved,
		Core::ProviderAttemptCompleted => Dto::ProviderAttemptCompleted,
	}
}

#[cfg(any())]
fn entity(value: &str) -> Result<EntityId, ()> {
	EntityId::new(value.to_owned()).map_err(|_| ())
}

fn core_reset_descriptor(descriptor: ResetCardDescriptorDto) -> Result<ResetCardDescriptor, ()> {
	let granted = ResetCardTimestamp::from_unix_seconds(descriptor.granted_at_unix_seconds())
		.map_err(|_| ())?;
	let expires = ResetCardTimestamp::from_unix_seconds(descriptor.expires_at_unix_seconds())
		.map_err(|_| ())?;

	ResetCardDescriptor::new(granted, expires).map_err(|_| ())
}

fn reset_descriptor_dto(descriptor: ResetCardDescriptor) -> ResetCardDescriptorDto {
	ResetCardDescriptorDto::new(
		descriptor.granted_at().unix_seconds(),
		descriptor.expires_at().unix_seconds(),
	)
	.expect("validated core reset-card descriptor maps to the wire contract")
}

fn account_profile_dto(profile: AccountProfileView) -> Result<AccountProfileDto, ()> {
	let snapshot = profile.snapshot;
	let (email, plan_type) = account_profile_claims_fields(profile.email, profile.plan_type)?;
	let daily_usage = snapshot
		.daily_usage
		.into_iter()
		.map(|fact| {
			Ok(AccountProfileDailyUsageDto {
				start_date: profile_wire_text(fact.start_date, 10)?,
				tokens: u64::try_from(fact.tokens).map_err(|_| ())?,
			})
		})
		.collect::<Result<Vec<_>, ()>>()?;

	Ok(AccountProfileDto {
		account_id: EntityId::new(snapshot.account_id.as_str().to_owned()).map_err(|_| ())?,
		account_revision: EntityRevision(u64::try_from(snapshot.account_revision).map_err(|_| ())?),
		observed_at_unix_micros: snapshot.observed_at_unix_micros,
		email,
		plan_type,
		display_name: snapshot
			.display_name
			.map(|value| profile_wire_text(value, 256))
			.transpose()?,
		username: snapshot.username.map(|value| profile_wire_text(value, 256)).transpose()?,
		lifetime_tokens: snapshot.lifetime_tokens.map(u64::try_from).transpose().map_err(|_| ())?,
		peak_daily_tokens: snapshot
			.peak_daily_tokens
			.map(u64::try_from)
			.transpose()
			.map_err(|_| ())?,
		longest_task_seconds: snapshot
			.longest_task_seconds
			.map(u64::try_from)
			.transpose()
			.map_err(|_| ())?,
		current_streak_days: snapshot
			.current_streak_days
			.map(u32::try_from)
			.transpose()
			.map_err(|_| ())?,
		longest_streak_days: snapshot
			.longest_streak_days
			.map(u32::try_from)
			.transpose()
			.map_err(|_| ())?,
		daily_usage,
	})
}

fn account_profile_unavailable_dto(
	claims: AccountProfileClaimsView,
	error: AccountProfileRuntimeError,
) -> Result<AccountProfileResult, ()> {
	let (email, plan_type) = account_profile_claims_fields(claims.email, claims.plan_type)?;
	Ok(AccountProfileResult::Unavailable {
		error: account_profile_error_dto(error),
		email,
		plan_type,
	})
}

fn unavailable_account_profile(error: AccountProfileErrorDto) -> AccountProfileResult {
	AccountProfileResult::Unavailable {
		error,
		email: AccountProfileEmailDto::Redacted,
		plan_type: None,
	}
}

fn account_profile_claims_fields(
	email: Option<String>,
	plan_type: Option<String>,
) -> Result<(AccountProfileEmailDto, Option<WireText>), ()> {
	let email = match email {
		Some(value) => AccountProfileEmailDto::Visible(profile_wire_text(value, 320)?),
		None => AccountProfileEmailDto::Redacted,
	};
	let plan_type = plan_type.map(|value| profile_wire_text(value, 128)).transpose()?;
	Ok((email, plan_type))
}

fn profile_wire_text(value: String, maximum: usize) -> Result<WireText, ()> {
	if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
		return Err(());
	}
	WireText::new(value).map_err(|_| ())
}

const fn account_profile_error_dto(error: AccountProfileRuntimeError) -> AccountProfileErrorDto {
	match error {
		AccountProfileRuntimeError::AccountUnavailable => {
			AccountProfileErrorDto::AccountUnavailable
		},
		AccountProfileRuntimeError::ProductStateUnavailable => {
			AccountProfileErrorDto::ProductStateUnavailable
		},
		AccountProfileRuntimeError::CredentialUnavailable => {
			AccountProfileErrorDto::CredentialUnavailable
		},
		AccountProfileRuntimeError::Unauthorized => AccountProfileErrorDto::Unauthorized,
		AccountProfileRuntimeError::ProviderUnavailable => {
			AccountProfileErrorDto::ProviderUnavailable
		},
		AccountProfileRuntimeError::ProtocolUnavailable => {
			AccountProfileErrorDto::ProtocolUnavailable
		},
		AccountProfileRuntimeError::AccountChanged => AccountProfileErrorDto::AccountChanged,
	}
}

fn account_dto(account: AccountRecord) -> Result<AccountDto, ()> {
	if account.tombstoned {
		return Err(());
	}
	let alias = account
		.credential
		.as_ref()
		.map(|binding| stable_account_alias(&binding.provider))
		.unwrap_or_else(|| account.label.clone());
	let credential = account
		.credential
		.map(|binding| {
			Ok::<AccountCredentialBindingDto, ()>(AccountCredentialBindingDto {
				schema_version: binding.schema_version.get(),
				version: binding.version.get(),
				fingerprint_sha256: Sha256Digest::new(binding.fingerprint.as_str().to_owned())
					.map_err(|_| ())?,
				provider: AccountProviderDto::Chatgpt,
				provider_account_id: WireText::new(binding.provider.account_id().to_owned())
					.map_err(|_| ())?,
			})
		})
		.transpose()?;
	let unsettled_operation = account
		.unsettled_operation
		.map(|operation| {
			Ok(AccountUnsettledOperationDto {
				operation_id: EntityId::new(operation.operation_id.as_str().to_owned())
					.map_err(|_| ())?,
				kind: match operation.kind {
					AccountOperationKind::Enroll => AccountOperationKindDto::Enroll,
					AccountOperationKind::Import => AccountOperationKindDto::Import,
					AccountOperationKind::Refresh => AccountOperationKindDto::Refresh,
					AccountOperationKind::Logout => AccountOperationKindDto::Logout,
				},
				phase: match operation.phase {
					AccountOperationPhase::Prepared => AccountOperationPhaseDto::Prepared,
					AccountOperationPhase::ProviderEffectPending => {
						AccountOperationPhaseDto::ProviderEffectPending
					},
					AccountOperationPhase::StoreApplied => AccountOperationPhaseDto::StoreApplied,
					AccountOperationPhase::RecoveryRequired => {
						AccountOperationPhaseDto::RecoveryRequired
					},
					AccountOperationPhase::Committed | AccountOperationPhase::Cancelled => {
						return Err(());
					},
				},
				recovery_code: operation
					.recovery_code
					.map(WireText::new)
					.transpose()
					.map_err(|_| ())?,
			})
		})
		.transpose()?;

	Ok(AccountDto {
		account_id: EntityId::new(account.account_id.as_str().to_owned()).map_err(|_| ())?,
		alias: WireText::new(alias).map_err(|_| ())?,
		enabled: account.enabled,
		account_revision: EntityRevision(u64::try_from(account.revision).map_err(|_| ())?),
		observed_state: match account.observed_state {
			AccountState::Unavailable => AccountObservedStateDto::Unavailable,
			AccountState::Unknown => AccountObservedStateDto::Unknown,
			AccountState::Available => AccountObservedStateDto::Available,
			AccountState::Depleted => AccountObservedStateDto::Depleted,
			AccountState::AuthFailed => AccountObservedStateDto::AuthFailed,
			AccountState::PluginUnready => AccountObservedStateDto::PluginUnready,
		},
		lifecycle_readiness: lifecycle_readiness_dto(account.lifecycle_readiness),
		credential_binding: credential,
		unsettled_operation,
		five_hour_quota: quota_dto(account.five_hour_quota)?,
		seven_day_quota: quota_dto(account.seven_day_quota)?,
	})
}

fn routing_dto(routing: AccountRoutingControl) -> Result<AccountRoutingControlDto, ()> {
	Ok(AccountRoutingControlDto {
		revision: EntityRevision(u64::try_from(routing.revision).map_err(|_| ())?),
		mode: match routing.mode {
			AccountSelectionMode::Fixed(account_id) => AccountSelectionModeDto::Fixed(
				EntityId::new(account_id.as_str().to_owned()).map_err(|_| ())?,
			),
			AccountSelectionMode::Balanced => AccountSelectionModeDto::Balanced,
		},
		order: routing
			.order
			.into_iter()
			.map(|account_id| EntityId::new(account_id.as_str().to_owned()).map_err(|_| ()))
			.collect::<Result<Vec<_>, _>>()?,
	})
}

const fn lifecycle_readiness_dto(
	readiness: AccountLifecycleReadiness,
) -> AccountLifecycleReadinessDto {
	match readiness {
		AccountLifecycleReadiness::Ready => AccountLifecycleReadinessDto::Ready,
		AccountLifecycleReadiness::CredentialAbsent => {
			AccountLifecycleReadinessDto::CredentialAbsent
		},
		AccountLifecycleReadiness::StoreUnavailable => {
			AccountLifecycleReadinessDto::StoreUnavailable
		},
		AccountLifecycleReadiness::StoreMismatch => AccountLifecycleReadinessDto::StoreMismatch,
		AccountLifecycleReadiness::ProviderMismatch => {
			AccountLifecycleReadinessDto::ProviderMismatch
		},
		AccountLifecycleReadiness::OperationUnsettled => {
			AccountLifecycleReadinessDto::OperationUnsettled
		},
		AccountLifecycleReadiness::CallbackCapabilityUnready => {
			AccountLifecycleReadinessDto::CallbackCapabilityUnready
		},
		AccountLifecycleReadiness::Tombstoned => AccountLifecycleReadinessDto::Tombstoned,
	}
}

const fn selection_recovery_dto(recovery: AccountSelectionRecovery) -> AccountSelectionRecoveryDto {
	match recovery {
		AccountSelectionRecovery::ConfigureFixedAccount => {
			AccountSelectionRecoveryDto::ConfigureFixedAccount
		},
		AccountSelectionRecovery::EnableAccount => AccountSelectionRecoveryDto::EnableAccount,
		AccountSelectionRecovery::EnrollCredentials => {
			AccountSelectionRecoveryDto::EnrollCredentials
		},
		AccountSelectionRecovery::ResolveCredentialOperation => {
			AccountSelectionRecoveryDto::ResolveCredentialOperation
		},
		AccountSelectionRecovery::RepairCredentialStore => {
			AccountSelectionRecoveryDto::RepairCredentialStore
		},
		AccountSelectionRecovery::RestoreProviderAgreement => {
			AccountSelectionRecoveryDto::RestoreProviderAgreement
		},
		AccountSelectionRecovery::RefreshQuota => AccountSelectionRecoveryDto::RefreshQuota,
		AccountSelectionRecovery::UpgradeCodex => AccountSelectionRecoveryDto::UpgradeCodex,
	}
}

fn application_unix_micros() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
}

fn account_command_descriptor(
	command: &CommandEnvelope,
) -> Result<(AccountCommandKind, String, Option<i64>), CommandError> {
	let expected = command
		.expected_revision
		.map(|revision| {
			i64::try_from(revision.0)
				.map_err(|_| account_rejection(AccountCommandRejectionDto::InvalidRequest, None))
		})
		.transpose()?;
	let descriptor = match &command.payload {
		CommandPayload::EnrollAccountFromSharedCodex { account_id, .. } => {
			(AccountCommandKind::Enroll, account_id.as_str())
		},
		CommandPayload::ImportAccountCredentialFile { account_id, .. } => {
			(AccountCommandKind::Import, account_id.as_str())
		},
		CommandPayload::SetAccountEnabled { account_id, .. } => {
			(AccountCommandKind::SetEnabled, account_id.as_str())
		},
		CommandPayload::UseAccountInCodex { account_id } => {
			(AccountCommandKind::UseInCodex, account_id.as_str())
		},
		CommandPayload::LogoutAccount { account_id, .. } => {
			(AccountCommandKind::Logout, account_id.as_str())
		},
		CommandPayload::SetFixedAccountSelection { .. } => {
			(AccountCommandKind::SetFixedSelection, "account-routing")
		},
		CommandPayload::SetBalancedAccountSelection => {
			(AccountCommandKind::SetBalancedSelection, "account-routing")
		},
		CommandPayload::SetAccountOrder { .. } => {
			(AccountCommandKind::SetAccountOrder, "account-routing")
		},
		CommandPayload::RefreshAccount { account_id, .. } => {
			(AccountCommandKind::Refresh, account_id.as_str())
		},
		CommandPayload::ReauthenticateAccountFromCredentialFile { account_id, .. } => {
			(AccountCommandKind::Refresh, account_id.as_str())
		},
		CommandPayload::RecoverAccountOperation { operation_id, .. } => {
			(AccountCommandKind::Recover, operation_id.as_str())
		},
		_ => return Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None)),
	};
	Ok((descriptor.0, descriptor.1.to_owned(), expected))
}

fn validate_account_command_envelope(command: &CommandEnvelope) -> Result<(), CommandError> {
	match &command.payload {
		CommandPayload::EnrollAccountFromSharedCodex { operation_id, account_id, .. }
		| CommandPayload::ImportAccountCredentialFile { operation_id, account_id, .. } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
		},
		CommandPayload::SetAccountEnabled { account_id, .. }
		| CommandPayload::UseAccountInCodex { account_id } => {
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::LogoutAccount { operation_id, account_id }
		| CommandPayload::RefreshAccount { operation_id, account_id } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::ReauthenticateAccountFromCredentialFile {
			operation_id,
			account_id,
			source_descriptor,
		} => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
			let source = source_descriptor.as_str();
			if source.is_empty() || source.len() > 4096 || source.chars().any(char::is_control) {
				return Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None));
			}
		},
		CommandPayload::SetFixedAccountSelection { account_id, expected_account_revision } => {
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
			if expected_account_revision.0 == 0 {
				return Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None));
			}
		},
		CommandPayload::SetBalancedAccountSelection => {
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::SetAccountOrder { order } => {
			let _ = required_expected_revision(command)?;
			for account_id in order {
				let _ = account_id_from_wire(account_id)?;
			}
		},
		CommandPayload::RecoverAccountOperation { operation_id, .. } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = required_expected_revision(command)?;
		},
		_ => return Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None)),
	}
	Ok(())
}

fn account_id_from_wire(account_id: &EntityId) -> Result<AccountId, CommandError> {
	AccountId::new(account_id.as_str())
		.map_err(|_| account_rejection(AccountCommandRejectionDto::InvalidRequest, None))
}

fn operation_id_from_wire(operation_id: &EntityId) -> Result<AccountOperationId, CommandError> {
	AccountOperationId::new(operation_id.as_str())
		.map_err(|_| account_rejection(AccountCommandRejectionDto::InvalidRequest, None))
}

fn required_expected_revision(command: &CommandEnvelope) -> Result<i64, CommandError> {
	command
		.expected_revision
		.and_then(|revision| i64::try_from(revision.0).ok())
		.filter(|revision| *revision > 0)
		.ok_or_else(|| account_rejection(AccountCommandRejectionDto::InvalidRequest, None))
}

fn account_changed_publication(
	account: AccountRecord,
) -> Result<ApplicationPublication, CommandError> {
	let account = account_dto(account)
		.map_err(|_| application_unavailable("account result is incompatible"))?;
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id: account.account_id.clone(),
		entity_revision: account.account_revision,
		result: ResultPayload::AccountChanged { account: Box::new(account.clone()) },
		event: EventPayload::AccountChanged { account: Box::new(account) },
	})
}

fn codex_auth_projection_publication(
	account_id: AccountId,
	account_revision: i64,
	projection_digest: String,
) -> Result<ApplicationPublication, CommandError> {
	let account_revision = u64::try_from(account_revision)
		.ok()
		.filter(|revision| *revision > 0)
		.map(EntityRevision)
		.ok_or_else(|| application_unavailable("account result is incompatible"))?;
	let account_id = EntityId::new(account_id.as_str().to_owned())
		.map_err(|_| application_unavailable("account result is incompatible"))?;
	let projection_digest = Sha256Digest::new(projection_digest)
		.map_err(|_| application_unavailable("account result is incompatible"))?;

	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id: account_id.clone(),
		entity_revision: account_revision,
		result: ResultPayload::CodexAuthProjected {
			account_id: account_id.clone(),
			account_revision,
			projection_digest: projection_digest.clone(),
		},
		event: EventPayload::CodexAuthProjected { account_id, account_revision, projection_digest },
	})
}

fn account_logout_publication(
	account: AccountRecord,
) -> Result<ApplicationPublication, CommandError> {
	if !account.tombstoned {
		return Err(application_unavailable("account logout result is incompatible"));
	}
	let account_id = EntityId::new(account.account_id.as_str().to_owned())
		.map_err(|_| application_unavailable("account logout result is incompatible"))?;
	let tombstone_revision = EntityRevision(
		u64::try_from(account.revision)
			.map_err(|_| application_unavailable("account logout result is incompatible"))?,
	);
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id: account_id.clone(),
		entity_revision: tombstone_revision,
		result: ResultPayload::AccountLoggedOut {
			account_id: account_id.clone(),
			tombstone_revision,
		},
		event: EventPayload::AccountLoggedOut { account_id, tombstone_revision },
	})
}

fn account_routing_publication(
	routing: AccountRoutingControlDto,
) -> Result<ApplicationPublication, CommandError> {
	let entity_id = EntityId::new("account-routing").expect("account routing entity is bounded");
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id,
		entity_revision: routing.revision,
		result: ResultPayload::AccountRoutingChanged { routing: routing.clone() },
		event: EventPayload::AccountRoutingChanged { routing },
	})
}

fn routing_command_result(
	outcome: &RoutingControlOutcome,
) -> Result<ApplicationPublication, CommandError> {
	match outcome {
		RoutingControlOutcome::Updated { routing } => routing_dto(routing.clone())
			.map_err(|_| application_unavailable("account routing result is incompatible"))
			.and_then(account_routing_publication),
		RoutingControlOutcome::StaleRoutingControl { revision } => Err(account_rejection(
			AccountCommandRejectionDto::StaleRoutingControl,
			u64::try_from(*revision).ok().map(EntityRevision),
		)),
		RoutingControlOutcome::StaleAccount { revision } => Err(account_rejection(
			AccountCommandRejectionDto::StaleAccount,
			u64::try_from(*revision).ok().map(EntityRevision),
		)),
		RoutingControlOutcome::AccountMissing => {
			Err(account_rejection(AccountCommandRejectionDto::AccountNotFound, None))
		},
		RoutingControlOutcome::InvalidOrder { revision } => Err(account_rejection(
			AccountCommandRejectionDto::RoutingOrderInvalid,
			u64::try_from(*revision).ok().map(EntityRevision),
		)),
		RoutingControlOutcome::InvalidRequest => {
			Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None))
		},
	}
}

fn account_recovery_publication(
	operation_id: AccountOperationId,
	outcome: AccountManualRecoveryOutcome,
	account: AccountRecord,
) -> Result<ApplicationPublication, CommandError> {
	let operation_id = EntityId::new(operation_id.as_str().to_owned())
		.map_err(|_| application_unavailable("account recovery result is incompatible"))?;
	let entity_id = EntityId::new(account.account_id.as_str().to_owned())
		.map_err(|_| application_unavailable("account recovery result is incompatible"))?;
	let entity_revision = EntityRevision(
		u64::try_from(account.revision)
			.map_err(|_| application_unavailable("account recovery result is incompatible"))?,
	);
	let outcome = match outcome {
		AccountManualRecoveryOutcome::Committed => AccountManualRecoveryOutcomeDto::Committed,
		AccountManualRecoveryOutcome::Cancelled => AccountManualRecoveryOutcomeDto::Cancelled,
		AccountManualRecoveryOutcome::StillRequiresRecovery => {
			AccountManualRecoveryOutcomeDto::StillRequiresRecovery
		},
	};
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id,
		entity_revision,
		result: ResultPayload::AccountOperationRecovered {
			operation_id: operation_id.clone(),
			outcome,
		},
		event: EventPayload::AccountOperationRecovered { operation_id, outcome },
	})
}

fn account_rejection(
	reason: AccountCommandRejectionDto,
	actual_revision: Option<EntityRevision>,
) -> CommandError {
	CommandError::AccountCommandRejected { rejection: reason, actual_revision }
}

fn lifecycle_rejection(rejection: AccountLifecycleRejection, revision: i64) -> CommandError {
	let reason = match rejection {
		AccountLifecycleRejection::IdentityConflict => AccountCommandRejectionDto::ProviderMismatch,
		AccountLifecycleRejection::OperationUnsettled => {
			AccountCommandRejectionDto::OperationUnsettled
		},
		AccountLifecycleRejection::InvalidRequest => AccountCommandRejectionDto::InvalidRequest,
		AccountLifecycleRejection::AccountMissing => AccountCommandRejectionDto::AccountNotFound,
		AccountLifecycleRejection::StaleAccount => AccountCommandRejectionDto::StaleAccount,
		AccountLifecycleRejection::AccountInUse => AccountCommandRejectionDto::AccountInUse,
		AccountLifecycleRejection::OperationMissing => {
			AccountCommandRejectionDto::OperationNotFound
		},
		AccountLifecycleRejection::StaleOperation => {
			AccountCommandRejectionDto::ManualRecoveryRequired
		},
	};
	account_rejection(
		reason,
		u64::try_from(revision).ok().filter(|value| *value > 0).map(EntityRevision),
	)
}

fn account_lifecycle_command_error(error: AccountLifecycleError) -> CommandError {
	match error {
		AccountLifecycleError::OperationRejected(rejection) => lifecycle_rejection(rejection, 0),
		AccountLifecycleError::AccountMissing => {
			account_rejection(AccountCommandRejectionDto::AccountNotFound, None)
		},
		AccountLifecycleError::CredentialAbsent => {
			account_rejection(AccountCommandRejectionDto::CredentialAbsent, None)
		},
		AccountLifecycleError::ProviderMismatch => {
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None)
		},
		AccountLifecycleError::StaleAccount => {
			account_rejection(AccountCommandRejectionDto::StaleAccount, None)
		},
		AccountLifecycleError::InvalidOperation => {
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None)
		},
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled) => {
			account_rejection(AccountCommandRejectionDto::OperationUnsettled, None)
		},
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent) => {
			account_rejection(AccountCommandRejectionDto::CredentialAbsent, None)
		},
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch) => {
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None)
		},
		AccountLifecycleError::NotReady(_) => {
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None)
		},
		AccountLifecycleError::AccountDisabled => {
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None)
		},
		AccountLifecycleError::CredentialStore(_) => {
			account_rejection(AccountCommandRejectionDto::CredentialStoreUnavailable, None)
		},
		AccountLifecycleError::CredentialImport => {
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None)
		},
		AccountLifecycleError::Refresh(_) => {
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None)
		},
		AccountLifecycleError::Persistence(_) | AccountLifecycleError::CoordinatorUnavailable => {
			application_unavailable("account service is unavailable")
		},
	}
}

fn account_operation_command_error(_error: AccountLifecycleError) -> CommandError {
	// Every deterministic account rejection after reservation is encoded into the durable receipt.
	// An escaped error therefore means that the atomic operation/receipt boundary did not finish.
	CommandError::AcceptanceUnknown
}

fn map_account_store_command_error(error: StoreError) -> CommandError {
	match error {
		StoreError::IdempotencyConflict => CommandError::IdempotencyConflict,
		StoreError::InvalidInput(_) | StoreError::CredentialRejected => {
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None)
		},
		StoreError::CapacityExhausted(_) => {
			application_unavailable("account command receipt capacity is unavailable")
		},
		StoreError::Database(_) | StoreError::OwnershipLost(_) => CommandError::AcceptanceUnknown,
		_ => application_unavailable("account command receipt store is unavailable"),
	}
}

fn stored_account_command_outcome(
	result: &Result<ApplicationPublication, CommandError>,
) -> StoredAccountCommandOutcome {
	match result {
		Ok(publication) => StoredAccountCommandOutcome::Succeeded {
			schema: ACCOUNT_COMMAND_RECEIPT_SCHEMA.to_owned(),
			entity_id: publication.entity_id.clone(),
			entity_revision: publication.entity_revision,
			result: Box::new(publication.result.clone()),
			event: Box::new(publication.event.clone()),
		},
		Err(error) => StoredAccountCommandOutcome::Rejected {
			schema: ACCOUNT_COMMAND_RECEIPT_SCHEMA.to_owned(),
			error: error.clone(),
		},
	}
}

fn encode_account_command_receipt(
	result: &Result<ApplicationPublication, CommandError>,
) -> Result<serde_json::Value, StoreError> {
	serde_json::to_value(stored_account_command_outcome(result))
		.map_err(|_| StoreError::Incompatible("account command result is incompatible".into()))
}

fn decode_account_command_receipt(
	value: serde_json::Value,
) -> Result<Result<ApplicationPublication, CommandError>, ()> {
	match serde_json::from_value(value).map_err(|_| ())? {
		StoredAccountCommandOutcome::Succeeded {
			schema,
			entity_id,
			entity_revision,
			result,
			event,
		} if schema == ACCOUNT_COMMAND_RECEIPT_SCHEMA && entity_revision.0 > 0 => {
			Ok(Ok(ApplicationPublication {
				channel: Channel::AccountsHealth,
				entity_id,
				entity_revision,
				result: *result,
				event: *event,
			}))
		},
		StoredAccountCommandOutcome::Rejected { schema, error }
			if schema == ACCOUNT_COMMAND_RECEIPT_SCHEMA =>
		{
			Ok(Err(error))
		},
		_ => Err(()),
	}
}

fn quota_dto(observation: AccountQuotaWindowObservation) -> Result<AccountQuotaWindowDto, ()> {
	let (observed_at_unix_micros, result) = match observation.disposition {
		AccountQuotaDisposition::Unknown => (None, AccountQuotaStateDto::Unknown),
		AccountQuotaDisposition::Current(fact) => (
			observation.observed_at_unix_micros,
			AccountQuotaStateDto::Current {
				used_percent: fact.used_percent,
				resets_at_unix_micros: fact.resets_at_unix_micros,
			},
		),
		AccountQuotaDisposition::Stale(_) => (None, AccountQuotaStateDto::Unknown),
		AccountQuotaDisposition::Error(error) => (
			observation.observed_at_unix_micros,
			AccountQuotaStateDto::Error {
				error: match error {
					AccountQuotaObservationError::ProviderUnavailable => {
						AccountQuotaErrorDto::ProviderUnavailable
					},
					AccountQuotaObservationError::ProtocolUnavailable => {
						AccountQuotaErrorDto::ProtocolUnavailable
					},
					AccountQuotaObservationError::AccountMismatch => {
						AccountQuotaErrorDto::AccountMismatch
					},
					AccountQuotaObservationError::UnsupportedWindow => {
						AccountQuotaErrorDto::UnsupportedWindow
					},
				},
			},
		),
	};
	if !matches!(observation.duration_minutes, 300 | 10_080) {
		return Err(());
	}

	Ok(AccountQuotaWindowDto {
		duration_minutes: observation.duration_minutes,
		observed_at_unix_micros,
		result,
	})
}

fn command_reset_error(error: ResetCardServiceError, expected: EntityRevision) -> CommandError {
	match error {
		ResetCardServiceError::ExpectedRevisionMismatch { actual } if actual >= 0 => {
			CommandError::ExpectedRevisionMismatch {
				expected,
				actual: EntityRevision(u64::try_from(actual).unwrap_or(0)),
			}
		},
		ResetCardServiceError::IdempotencyConflict => CommandError::IdempotencyConflict,
		ResetCardServiceError::AcceptanceUnknown => CommandError::AcceptanceUnknown,
		_ => application_unavailable(reset_error_message(error)),
	}
}

fn application_unavailable(message: &'static str) -> CommandError {
	CommandError::ApplicationUnavailable {
		message: WireText::new(message).expect("static application message is bounded"),
	}
}

const fn reset_error_message(error: ResetCardServiceError) -> &'static str {
	match error {
		ResetCardServiceError::InvalidRequest => "reset-card request is invalid",
		ResetCardServiceError::AccountNotFound => "reset-card account is not configured",
		ResetCardServiceError::AccountStateRejected => {
			"reset-card account state rejects manual use"
		},
		ResetCardServiceError::AccountChanged
		| ResetCardServiceError::ExpectedRevisionMismatch { .. } => "reset-card account revision changed",
		ResetCardServiceError::VaultUnavailable => "reset-card credential vault is unavailable",
		ResetCardServiceError::SchemaUnsupported => {
			"stored reset-card result is incompatible with the current provider API"
		},
		ResetCardServiceError::ProviderUnavailable => "reset-card provider is unavailable",
		ResetCardServiceError::InventoryIncomplete => "reset-card inventory is incomplete",
		ResetCardServiceError::InventoryChanged => "selected reset card changed",
		ResetCardServiceError::RequestTimedOut => "reset-card provider observation timed out",
		ResetCardServiceError::ResourceExhausted => "reset-card process capacity is exhausted",
		ResetCardServiceError::ProductStateUnavailable => "reset-card product state is unavailable",
		ResetCardServiceError::IdempotencyConflict => "reset-card idempotency key conflicts",
		ResetCardServiceError::AcceptanceUnknown => {
			"reset-card durable acceptance could not be established"
		},
	}
}

const fn protocol_reset_error(error: ResetCardServiceError) -> ResetCardError {
	match error {
		ResetCardServiceError::InvalidRequest
		| ResetCardServiceError::IdempotencyConflict
		| ResetCardServiceError::ExpectedRevisionMismatch { .. } => ResetCardError::InvalidRequest,
		ResetCardServiceError::AccountNotFound => ResetCardError::AccountNotFound,
		ResetCardServiceError::AccountStateRejected => ResetCardError::AccountStateRejected,
		ResetCardServiceError::AccountChanged => ResetCardError::InventoryChanged,
		ResetCardServiceError::VaultUnavailable => ResetCardError::VaultUnavailable,
		ResetCardServiceError::SchemaUnsupported => ResetCardError::SchemaUnsupported,
		ResetCardServiceError::ProviderUnavailable => ResetCardError::ProviderUnavailable,
		ResetCardServiceError::InventoryIncomplete => ResetCardError::InventoryIncomplete,
		ResetCardServiceError::InventoryChanged => ResetCardError::InventoryChanged,
		ResetCardServiceError::RequestTimedOut => ResetCardError::RequestTimedOut,
		ResetCardServiceError::ResourceExhausted => ResetCardError::ResourceExhausted,
		ResetCardServiceError::ProductStateUnavailable => ResetCardError::ProductStateUnavailable,
		ResetCardServiceError::AcceptanceUnknown => ResetCardError::ProductStateUnavailable,
	}
}

fn operation_query_result(
	result: Result<ResetCardOperationStatus, ResetCardServiceError>,
) -> ResetCardOperationResult {
	match result {
		Ok(status) => operation_result(status),
		Err(error) => ResetCardOperationResult::Unavailable { error: protocol_reset_error(error) },
	}
}

const fn operation_result(status: ResetCardOperationStatus) -> ResetCardOperationResult {
	match status {
		ResetCardOperationStatus::NotFound => ResetCardOperationResult::NotFound,
		ResetCardOperationStatus::Prepared => ResetCardOperationResult::Prepared,
		ResetCardOperationStatus::EffectAmbiguous => ResetCardOperationResult::EffectAmbiguous,
		ResetCardOperationStatus::Completed(outcome) => {
			ResetCardOperationResult::Completed { outcome: protocol_outcome(outcome) }
		},
		ResetCardOperationStatus::FailedBeforeEffect(error) => {
			ResetCardOperationResult::FailedBeforeEffect { error: failure_reset_error(error) }
		},
	}
}

const fn protocol_outcome(outcome: ResetCardConsumeOutcome) -> ResetCardOutcome {
	match outcome {
		ResetCardConsumeOutcome::Reset => ResetCardOutcome::Reset,
		ResetCardConsumeOutcome::NothingToReset => ResetCardOutcome::NothingToReset,
		ResetCardConsumeOutcome::NoCredit => ResetCardOutcome::NoCredit,
		ResetCardConsumeOutcome::AlreadyRedeemed => ResetCardOutcome::AlreadyRedeemed,
	}
}

const fn failure_reset_error(failure: ResetCardFailureCode) -> ResetCardError {
	match failure {
		ResetCardFailureCode::AccountChanged => ResetCardError::InventoryChanged,
		ResetCardFailureCode::VaultUnavailable => ResetCardError::VaultUnavailable,
		ResetCardFailureCode::SchemaUnsupported => ResetCardError::SchemaUnsupported,
		ResetCardFailureCode::InventoryIncomplete => ResetCardError::InventoryIncomplete,
		ResetCardFailureCode::InventoryChanged => ResetCardError::InventoryChanged,
		ResetCardFailureCode::ProviderUnavailable => ResetCardError::ProviderUnavailable,
		ResetCardFailureCode::ResourceExhausted => ResetCardError::ResourceExhausted,
	}
}

fn history_dto(entry: HistoryEntry) -> Result<HistoryItemDto, ()> {
	let artifact = entry
		.artifact
		.map(|(id, revision)| {
			Ok::<HistoryArtifactReference, ()>(HistoryArtifactReference {
				artifact_id: HistoryArtifactId::new(id.as_str().to_owned()).ok_or(())?,
				revision: HistoryArtifactRevision::new(revision).ok_or(())?,
			})
		})
		.transpose()?;
	let payload = match (entry.inline_text, entry.blob_hash, entry.blob_byte_length) {
		(Some(text), None, None) => {
			HistoryPayloadDto::Inline { text: HistoryText::new(text).map_err(|_| ())? }
		},
		(None, Some(hash), Some(byte_length)) => HistoryPayloadDto::Blob(HistoryBlobReference {
			sha256: Sha256Digest::new(hash.to_hex()).map_err(|_| ())?,
			byte_length: HistoryBlobLength::new(byte_length).map_err(|_| ())?,
		}),
		_ => return Err(()),
	};

	Ok(HistoryItemDto {
		history_item_id: EntityId::new(entry.history_item_id).map_err(|_| ())?,
		turn_id: EntityId::new(entry.turn_id).map_err(|_| ())?,
		runtime_session_id: EntityId::new(entry.runtime_session_id).map_err(|_| ())?,
		turn_role: match entry.turn_role {
			TurnRole::User => HistoryTurnRole::User,
			TurnRole::Assistant => HistoryTurnRole::Assistant,
			TurnRole::System => HistoryTurnRole::System,
			TurnRole::Tool => HistoryTurnRole::Tool,
		},
		possible_side_effects: match entry.possible_side_effects {
			PossibleSideEffects::None => HistorySideEffectState::None,
			PossibleSideEffects::Possible => HistorySideEffectState::Possible,
			PossibleSideEffects::Unknown => HistorySideEffectState::Unknown,
		},
		kind: match entry.kind {
			HistoryItemKind::Message => HistoryItemKindDto::Message,
			HistoryItemKind::Reasoning => HistoryItemKindDto::Reasoning,
			HistoryItemKind::ToolCall => HistoryItemKindDto::ToolCall,
			HistoryItemKind::ToolResult => HistoryItemKindDto::ToolResult,
			HistoryItemKind::Artifact => HistoryItemKindDto::Artifact,
			HistoryItemKind::Status => HistoryItemKindDto::Status,
		},
		status: match entry.status {
			ItemStatus::Streaming => HistoryItemStatusDto::Streaming,
			ItemStatus::Completed => HistoryItemStatusDto::Completed,
			ItemStatus::Failed => HistoryItemStatusDto::Failed,
		},
		payload,
		media_type: entry.media_type,
		metadata: entry.metadata,
		artifact,
		revision: EntityRevision(u64::try_from(entry.revision).map_err(|_| ())?),
	})
}

#[cfg(test)]
mod tests {
	use crate::account_launch::{ResetCardFailureCode, ResetCardOperationStatus};
	use decodex_core::{
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
		AccountOperationPhase, AccountOperationStatus, AccountProvider, AccountQuotaDisposition,
		AccountQuotaWindow, AccountQuotaWindowObservation, AccountRecord, AccountState,
		ConversationId, DecodexRoot, ObjectiveId, ObjectiveState, ProgramClaimId,
		ProgramEvidenceId, ProgramEvidenceKind, ProgramId, ProgramObservationId, ProgramProposalId,
		ProgramReviewClassification, ProgramReviewId, ProgramState, ProviderIdentity,
		RuntimeSessionState, WorkItemId, WorkItemState,
	};
	use decodex_database::{
		AccountLifecycleRejection, AccountProfileDailyUsage, AccountProfileSnapshot,
		CommandIdentity, CreateProgramCycle, DomainPackIdentity, OrdinaryTaskConversationReadback,
		OrdinaryTaskPreSessionState, ProgramCharterRecord, ProgramClaimRecord, ProgramCycleRecord,
		ProgramEvidenceRecord, ProgramObjectiveRecord, ProgramProposalRecord, ProgramReviewRecord,
		ProgramSignalRecord, ProgramWorkItemRecord, SqliteStore,
	};
	use decodex_protocol::{
		AccountCommandRejectionDto, AccountProfileEmailDto, AccountQuotaStateDto, CommandError,
		ProgramNodeKind, ProgramRelationKind, QuickTaskRecoveryAction, QuickTaskState,
		ResetCardError, ResetCardOperationResult,
	};

	use super::{
		ACCOUNT_COMMAND_RECEIPT_SCHEMA, AccountLifecycleError, AccountProfileClaimsView,
		AccountProfileRuntimeError, AccountProfileView, ProductStore, ResetCardServiceError,
		StoredAccountCommandOutcome, account_dto, account_lifecycle_command_error,
		account_profile_dto, account_profile_unavailable_dto, authorize_program_capability,
		decode_account_command_receipt, encode_account_command_receipt, lifecycle_rejection,
		operation_query_result, program_cycle_dto, protocol_reset_error,
		quick_task_summary_from_row, quota_dto,
	};
	use crate::domain_packs::{QUICK_TASK_CAPABILITY, resolve_identity};

	fn program_preflight_fixture(
		sequence: u64,
		domain_pack: Option<DomainPackIdentity>,
		working_directory: &str,
	) -> CreateProgramCycle {
		let id = |prefix: u8| format!("{prefix:02x}000000-0000-4000-8000-{sequence:012x}");
		CreateProgramCycle {
			program_id: ProgramId::new(id(0x91)).expect("Program identity"),
			domain_pack,
			signal_id: ProgramObservationId::new(id(0x92)).expect("Signal identity"),
			claim_id: ProgramClaimId::new(id(0x93)).expect("Claim identity"),
			proposal_id: ProgramProposalId::new(id(0x94)).expect("Proposal identity"),
			objective_id: ObjectiveId::new(id(0x95)).expect("Objective identity"),
			work_item_id: WorkItemId::new(id(0x96)).expect("WorkItem identity"),
			name: format!("Pack preflight fixture {sequence}"),
			purpose: "Prove capability rejection before provider execution.".to_owned(),
			non_goals: vec!["Do not contact a provider.".to_owned()],
			review_policy: "Require deterministic and external evidence.".to_owned(),
			signal_source: "runtime test".to_owned(),
			signal_summary: "A Program requests one bounded Quick Task.".to_owned(),
			signal_observed_at_micros: 1,
			claim_statement: "Pack admission must precede worker admission.".to_owned(),
			proposal_summary: "Run the deny-by-default preflight.".to_owned(),
			proposal_expected_effect: "Rejected Packs create no provider attempt.".to_owned(),
			proposal_risk: "Late validation could dispatch unauthorized work.".to_owned(),
			proposal_evidence_need: "Inspect the provider-attempt store.".to_owned(),
			objective_outcome: "One exact admission result is returned.".to_owned(),
			acceptance_criteria: vec!["No ProviderAttempt row exists.".to_owned()],
			validation_criteria: vec!["SQLite readback remains empty.".to_owned()],
			work_item_title: "Exercise Pack preflight".to_owned(),
			work_item_instructions: "Reject invalid Pack authority without provider work."
				.to_owned(),
			working_directory: working_directory.to_owned(),
		}
	}

	#[tokio::test]
	async fn pack_preflight_rejections_happen_before_provider_attempt_creation() {
		let directory = tempfile::tempdir().expect("temporary Decodex root");
		let root =
			DecodexRoot::new(directory.path().canonicalize().expect("canonical temporary root"))
				.expect("typed Decodex root");
		let store = SqliteStore::open(&root.paths()).expect("SQLite product store");
		let product_store = ProductStore::Available(store.clone());
		let valid = resolve_identity(decodex_protocol::DEVELOPMENT_DOMAIN_PACK_ID)
			.expect("built-in Development Pack");
		let cases = [
			(None, "Program Domain Pack is not bound"),
			(
				Some(DomainPackIdentity {
					pack_id: "decodex.unknown".to_owned(),
					pack_version: "1.0.0".to_owned(),
					pack_digest: "1".repeat(64),
				}),
				"Domain Pack is not built in",
			),
			(
				Some(DomainPackIdentity { pack_digest: "0".repeat(64), ..valid.clone() }),
				"Program Domain Pack binding is incompatible",
			),
		];

		for (index, (binding, expected)) in cases.into_iter().enumerate() {
			let fixture = program_preflight_fixture(
				u64::try_from(index + 1).expect("bounded sequence"),
				binding,
				directory.path().to_str().expect("UTF-8 temporary path"),
			);
			store
				.create_program_cycle(
					&CommandIdentity::new(
						format!("pack-preflight-{index}"),
						&[u8::try_from(index).expect("bounded request")],
					)
					.expect("command identity"),
					&fixture,
				)
				.await
				.expect("persist Program fixture");
			let error = authorize_program_capability(
				&product_store,
				&fixture.work_item_id,
				QUICK_TASK_CAPABILITY,
			)
			.await
			.expect_err("Pack preflight must reject");
			assert!(matches!(
				error,
				CommandError::ApplicationUnavailable { message } if message.as_str() == expected
			));
			assert!(
				store
					.read_provider_attempt_page(None, None, None, 1)
					.await
					.expect("ProviderAttempt readback")
					.is_empty()
			);
		}

		let fixture = program_preflight_fixture(
			4,
			Some(valid),
			directory.path().to_str().expect("UTF-8 temporary path"),
		);
		store
			.create_program_cycle(
				&CommandIdentity::new("pack-preflight-undeclared", b"undeclared capability")
					.expect("command identity"),
				&fixture,
			)
			.await
			.expect("persist declared Pack fixture");
		let error = authorize_program_capability(
			&product_store,
			&fixture.work_item_id,
			"finance.place_order",
		)
		.await
		.expect_err("undeclared capability must be denied");
		assert!(matches!(
			error,
			CommandError::ApplicationUnavailable { message }
				if message.as_str() == "Domain Pack does not grant this capability"
		));
		assert!(
			store
				.read_provider_attempt_page(None, None, None, 1)
				.await
				.expect("ProviderAttempt readback")
				.is_empty()
		);
	}

	#[test]
	fn repeatable_program_projection_follows_review_lineage() {
		let program_id = ProgramId::new("30000000-0000-4000-8000-000000000001").unwrap();
		let signal_1 = ProgramObservationId::new("31000000-0000-4000-8000-000000000001").unwrap();
		let claim_1 = ProgramClaimId::new("32000000-0000-4000-8000-000000000001").unwrap();
		let proposal_1 = ProgramProposalId::new("33000000-0000-4000-8000-000000000001").unwrap();
		let objective_1 = ObjectiveId::new("34000000-0000-4000-8000-000000000001").unwrap();
		let work_1 = WorkItemId::new("35000000-0000-4000-8000-000000000001").unwrap();
		let deterministic = ProgramEvidenceId::new("36000000-0000-4000-8000-000000000001").unwrap();
		let external = ProgramEvidenceId::new("37000000-0000-4000-8000-000000000001").unwrap();
		let review_1 = ProgramReviewId::new("38000000-0000-4000-8000-000000000001").unwrap();
		let signal_2 = ProgramObservationId::new("41000000-0000-4000-8000-000000000001").unwrap();
		let claim_2 = ProgramClaimId::new("42000000-0000-4000-8000-000000000001").unwrap();
		let proposal_2 = ProgramProposalId::new("43000000-0000-4000-8000-000000000001").unwrap();
		let objective_2 = ObjectiveId::new("44000000-0000-4000-8000-000000000001").unwrap();
		let work_2 = WorkItemId::new("45000000-0000-4000-8000-000000000001").unwrap();
		let record = ProgramCycleRecord {
			program: ProgramCharterRecord {
				program_id: program_id.clone(),
				name: "Repeatable Program".into(),
				purpose: "Keep one causal identity".into(),
				non_goals: vec!["No scheduler".into()],
				review_policy: "Review each finite cycle".into(),
				state: ProgramState::Active,
				revision: 3,
				created_at_micros: 1,
				updated_at_micros: 3,
			},
			domain_pack: None,
			signals: vec![
				ProgramSignalRecord {
					signal_id: signal_1.clone(),
					program_id: program_id.clone(),
					predecessor_review_id: None,
					source: "operator".into(),
					summary: "first signal".into(),
					observed_at_micros: 1,
					created_at_micros: 1,
				},
				ProgramSignalRecord {
					signal_id: signal_2.clone(),
					program_id: program_id.clone(),
					predecessor_review_id: Some(review_1.clone()),
					source: "review".into(),
					summary: "second signal".into(),
					observed_at_micros: 2,
					created_at_micros: 2,
				},
			],
			claims: vec![
				ProgramClaimRecord {
					claim_id: claim_1.clone(),
					program_id: program_id.clone(),
					signal_id: signal_1,
					statement: "first claim".into(),
					revision: 1,
					created_at_micros: 1,
					updated_at_micros: 1,
				},
				ProgramClaimRecord {
					claim_id: claim_2.clone(),
					program_id: program_id.clone(),
					signal_id: signal_2.clone(),
					statement: "second claim".into(),
					revision: 1,
					created_at_micros: 2,
					updated_at_micros: 2,
				},
			],
			proposals: vec![
				ProgramProposalRecord {
					proposal_id: proposal_1.clone(),
					program_id: program_id.clone(),
					claim_id: claim_1,
					summary: "first proposal".into(),
					expected_effect: "first effect".into(),
					risk: "first risk".into(),
					evidence_need: "first evidence".into(),
					revision: 1,
					created_at_micros: 1,
					updated_at_micros: 1,
				},
				ProgramProposalRecord {
					proposal_id: proposal_2.clone(),
					program_id: program_id.clone(),
					claim_id: claim_2,
					summary: "second proposal".into(),
					expected_effect: "second effect".into(),
					risk: "second risk".into(),
					evidence_need: "second evidence".into(),
					revision: 1,
					created_at_micros: 2,
					updated_at_micros: 2,
				},
			],
			objectives: vec![
				ProgramObjectiveRecord {
					objective_id: objective_1.clone(),
					program_id: program_id.clone(),
					proposal_id: proposal_1,
					outcome: "first outcome".into(),
					acceptance_criteria: vec!["first acceptance".into()],
					validation_criteria: vec!["first validation".into()],
					state: ObjectiveState::Abandoned,
					revision: 2,
					created_at_micros: 1,
					updated_at_micros: 2,
				},
				ProgramObjectiveRecord {
					objective_id: objective_2.clone(),
					program_id: program_id.clone(),
					proposal_id: proposal_2,
					outcome: "second outcome".into(),
					acceptance_criteria: vec!["second acceptance".into()],
					validation_criteria: vec!["second validation".into()],
					state: ObjectiveState::Active,
					revision: 1,
					created_at_micros: 2,
					updated_at_micros: 2,
				},
			],
			work_items: vec![
				ProgramWorkItemRecord {
					work_item_id: work_1.clone(),
					program_id: program_id.clone(),
					objective_id: objective_1,
					title: "first work".into(),
					instructions: "complete first work".into(),
					working_directory: "/tmp/decodex".into(),
					state: WorkItemState::Done,
					revision: 3,
					conversation_id: None,
					created_at_micros: 1,
					updated_at_micros: 2,
				},
				ProgramWorkItemRecord {
					work_item_id: work_2,
					program_id: program_id.clone(),
					objective_id: objective_2,
					title: "second work".into(),
					instructions: "complete second work".into(),
					working_directory: "/tmp/decodex".into(),
					state: WorkItemState::Ready,
					revision: 1,
					conversation_id: None,
					created_at_micros: 2,
					updated_at_micros: 2,
				},
			],
			evidence: vec![
				ProgramEvidenceRecord {
					evidence_id: deterministic.clone(),
					program_id: program_id.clone(),
					work_item_id: work_1.clone(),
					kind: ProgramEvidenceKind::DeterministicValidation,
					source: "test".into(),
					summary: "checks passed".into(),
					observed_at_micros: 2,
					created_at_micros: 2,
				},
				ProgramEvidenceRecord {
					evidence_id: external.clone(),
					program_id: program_id.clone(),
					work_item_id: work_1.clone(),
					kind: ProgramEvidenceKind::External,
					source: "provider".into(),
					summary: "provider settled".into(),
					observed_at_micros: 2,
					created_at_micros: 2,
				},
			],
			reviews: vec![ProgramReviewRecord {
				review_id: review_1.clone(),
				program_id,
				work_item_id: work_1,
				deterministic_evidence_id: deterministic,
				external_evidence_id: external,
				classification: ProgramReviewClassification::KnowledgeProgress,
				rationale: "continue with the next bounded gap".into(),
				created_at_micros: 2,
			}],
		};

		let projection = program_cycle_dto(record, &[]).expect("two-cycle projection");
		assert_eq!(
			projection.nodes.iter().map(|node| node.kind).collect::<Vec<_>>(),
			vec![
				ProgramNodeKind::Signal,
				ProgramNodeKind::Claim,
				ProgramNodeKind::Proposal,
				ProgramNodeKind::Objective,
				ProgramNodeKind::WorkItem,
				ProgramNodeKind::Evidence,
				ProgramNodeKind::Evidence,
				ProgramNodeKind::Review,
				ProgramNodeKind::Signal,
				ProgramNodeKind::Claim,
				ProgramNodeKind::Proposal,
				ProgramNodeKind::Objective,
				ProgramNodeKind::WorkItem,
			]
		);
		assert!(projection.edges.iter().any(|edge| {
			edge.from.as_str() == review_1.as_str()
				&& edge.to.as_str() == signal_2.as_str()
				&& edge.kind == ProgramRelationKind::Continues
		}));
	}

	#[cfg(any())]
	const BOARD_PROJECT: &str = "10000000-0000-4000-8000-000000000001";
	#[cfg(any())]
	const OTHER_PROJECT: &str = "10000000-0000-4000-8000-000000000002";
	#[cfg(any())]
	const BOARD_LEAD: &str = "30000000-0000-4000-8000-000000000001";

	#[cfg(any())]
	fn stored_work_item(
		work_item_id: &str,
		project_id: &str,
		accepted_revision: Option<u64>,
	) -> StoredWorkItem {
		let project_id = ProjectId::new(project_id).expect("fixture Project ID must be valid");
		let lead_id = AgentId::new(BOARD_LEAD).expect("fixture Lead ID must be valid");
		let provenance = WorkItemProvenance::new(
			lead_id.clone(),
			WorkItemCorrelationId::new("60000000-0000-4000-8000-000000000001")
				.expect("fixture correlation ID must be valid"),
			"Board projection fixture",
		)
		.expect("fixture provenance must be valid");
		let work_item = WorkItem::new(
			WorkItemId::new(work_item_id).expect("fixture WorkItem ID must be valid"),
			project_id,
			lead_id,
			None,
			Vec::new(),
			"Board item",
			"Exercise bounded board projection.",
			WorkItemPriority::Medium,
			vec!["The projection is accepted.".to_owned()],
			vec!["The projection is validated.".to_owned()],
			WorkItemTimestamp::from_unix_microseconds(1).unwrap(),
			provenance,
		)
		.expect("fixture WorkItem must be valid");

		StoredWorkItem { work_item, edges: Vec::new(), accepted_revision }
	}

	#[cfg(any())]
	fn mapped_board_result(items: Vec<StoredWorkItem>) -> WorkItemBoardResult {
		match work_item_board_page_dto(
			WorkItemBoardProjectId::new(BOARD_PROJECT).unwrap(),
			None,
			None,
			WorkItemBoardPageSize::new(2).unwrap(),
			items,
		) {
			Ok(page) => WorkItemBoardResult::Page(page),
			Err(()) => WorkItemBoardResult::Unavailable {
				error: WorkItemBoardQueryError::IntegrityUnavailable,
			},
		}
	}

	fn pre_session_quick_task(
		state: OrdinaryTaskPreSessionState,
		decision_id: Option<&str>,
	) -> OrdinaryTaskConversationReadback {
		OrdinaryTaskConversationReadback {
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000001276").unwrap(),
			conversation_revision: 1,
			runtime_session_id: None,
			runtime_session_revision: None,
			runtime_session_state: None,
			has_acknowledged_turn: false,
			active_turn_id: None,
			active_turn_revision: None,
			has_admitted_user_turn: false,
			has_active_provider_attempt: false,
			has_unknown_provider_attempt: false,
			pre_session_state: Some(state),
			routing_decision_id: decision_id.map(str::to_owned),
			updated_at_micros: 1,
		}
	}

	#[test]
	fn quick_task_projection_distinguishes_routing_and_establishment_recovery() {
		let routing = quick_task_summary_from_row(
			pre_session_quick_task(OrdinaryTaskPreSessionState::RoutingPending, None),
			None,
		)
		.unwrap();
		let establishment = quick_task_summary_from_row(
			pre_session_quick_task(
				OrdinaryTaskPreSessionState::EstablishmentPending,
				Some("41000000-0000-4000-8000-000000001276"),
			),
			None,
		)
		.unwrap();

		assert_eq!(routing.state, QuickTaskState::RoutingPending);
		assert_eq!(routing.recovery_action, Some(QuickTaskRecoveryAction::ResumeRouting));
		assert_eq!(establishment.state, QuickTaskState::EstablishmentPending);
		assert_eq!(
			establishment.recovery_action,
			Some(QuickTaskRecoveryAction::ResumeEstablishment),
		);
	}

	#[test]
	fn terminal_session_projection_never_reopens_routing_recovery() {
		let row = OrdinaryTaskConversationReadback {
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000001276").unwrap(),
			conversation_revision: 1,
			runtime_session_id: Some(
				decodex_core::RuntimeSessionId::new("42000000-0000-4000-8000-000000001276")
					.unwrap(),
			),
			runtime_session_revision: Some(4),
			runtime_session_state: Some(RuntimeSessionState::Ended),
			has_acknowledged_turn: true,
			active_turn_id: None,
			active_turn_revision: None,
			has_admitted_user_turn: true,
			has_active_provider_attempt: false,
			has_unknown_provider_attempt: false,
			pre_session_state: None,
			routing_decision_id: Some("41000000-0000-4000-8000-000000001276".to_owned()),
			updated_at_micros: 1,
		};
		let projection = quick_task_summary_from_row(row, None).unwrap();
		assert_eq!(projection.state, QuickTaskState::ManualRecovery);
		assert_eq!(projection.recovery_action, Some(QuickTaskRecoveryAction::StartNewConversation),);
	}

	#[test]
	#[cfg(any())]
	fn board_lookahead_maps_one_extra_row_and_refuses_malformed_or_cross_project_data() {
		let result = mapped_board_result(vec![
			stored_work_item("20000000-0000-4000-8000-000000000001", BOARD_PROJECT, Some(1)),
			stored_work_item("20000000-0000-4000-8000-000000000002", BOARD_PROJECT, Some(1)),
			stored_work_item("20000000-0000-4000-8000-000000000003", BOARD_PROJECT, Some(1)),
		]);
		let WorkItemBoardResult::Page(page) = &result else {
			panic!("bounded lookahead must produce a page");
		};

		assert_eq!(
			page.cards().iter().map(|card| card.work_item_id().as_str()).collect::<Vec<_>>(),
			vec!["20000000-0000-4000-8000-000000000001", "20000000-0000-4000-8000-000000000002",]
		);
		assert_eq!(
			page.next_cursor().map(|cursor| cursor.as_str()),
			Some("20000000-0000-4000-8000-000000000002")
		);
		let encoded = serde_json::to_value(result).expect("valid page must serialize");
		assert!(encoded["data"].get("total").is_none());
		assert!(encoded["data"].get("total_count").is_none());
		assert!(encoded["data"].get("exhaustive").is_none());

		for (case, items) in [
			(
				"malformed accepted revision",
				vec![stored_work_item(
					"20000000-0000-4000-8000-000000000001",
					BOARD_PROJECT,
					Some(0),
				)],
			),
			(
				"cross-project card",
				vec![stored_work_item(
					"20000000-0000-4000-8000-000000000001",
					OTHER_PROJECT,
					Some(1),
				)],
			),
		] {
			assert_eq!(
				mapped_board_result(items),
				WorkItemBoardResult::Unavailable {
					error: WorkItemBoardQueryError::IntegrityUnavailable,
				},
				"{case}",
			);
		}
	}

	#[test]
	fn stale_internal_quota_is_publicly_unknown_without_old_values() {
		let dto = quota_dto(AccountQuotaWindowObservation {
			duration_minutes: AccountQuotaWindow::SEVEN_DAYS_MINUTES,
			observed_at_unix_micros: Some(1_000_000),
			disposition: AccountQuotaDisposition::Stale(
				AccountQuotaWindow::new(AccountQuotaWindow::SEVEN_DAYS_MINUTES, 42, 2_000_000)
					.unwrap(),
			),
		})
		.expect("supported stale quota should have a bounded public projection");

		assert_eq!(dto.observed_at_unix_micros, None);
		assert_eq!(dto.result, AccountQuotaStateDto::Unknown);
	}

	#[test]
	fn account_profile_projection_keeps_email_visibility_and_bounded_daily_facts_explicit() {
		let dto = account_profile_dto(AccountProfileView {
			snapshot: AccountProfileSnapshot {
				account_id: AccountId::new("40000000-0000-4000-8000-000000000001").unwrap(),
				account_revision: 4,
				provider: ProviderIdentity::new(AccountProvider::Chatgpt, "provider-1").unwrap(),
				observed_at_unix_micros: 1_700_000_000_000_000,
				display_name: Some("Iris".into()),
				username: None,
				lifetime_tokens: Some(12_345),
				peak_daily_tokens: Some(900),
				longest_task_seconds: Some(600),
				current_streak_days: Some(3),
				longest_streak_days: Some(8),
				daily_usage: vec![AccountProfileDailyUsage {
					start_date: "2026-07-28".into(),
					tokens: 900,
				}],
			},
			email: Some("iris@example.test".into()),
			plan_type: Some("pro".into()),
		})
		.expect("validated profile snapshot must map");

		assert!(matches!(
			dto.email,
			AccountProfileEmailDto::Visible(ref email)
				if email.as_str() == "iris@example.test"
		));
		assert_eq!(dto.daily_usage[0].start_date.as_str(), "2026-07-28");
		assert_eq!(dto.daily_usage[0].tokens, 900);
	}

	#[test]
	fn prepared_account_uses_its_immutable_derived_alias_without_a_current_binding() {
		let account = AccountRecord {
			account_id: AccountId::new("40000000-0000-4000-8000-000000000002").unwrap(),
			label: "Val".to_owned(),
			enabled: true,
			revision: 1,
			observed_state: AccountState::Unknown,
			lifecycle_readiness: AccountLifecycleReadiness::OperationUnsettled,
			credential: None,
			unsettled_operation: Some(AccountOperationStatus {
				operation_id: AccountOperationId::new("40000000-0000-4000-8000-000000000003")
					.unwrap(),
				kind: AccountOperationKind::Enroll,
				phase: AccountOperationPhase::Prepared,
				recovery_code: None,
			}),
			five_hour_quota: AccountQuotaWindowObservation::unknown(
				AccountQuotaWindow::FIVE_HOURS_MINUTES,
			)
			.unwrap(),
			seven_day_quota: AccountQuotaWindowObservation::unknown(
				AccountQuotaWindow::SEVEN_DAYS_MINUTES,
			)
			.unwrap(),
			tombstoned: false,
		};

		assert_eq!(
			account_dto(account).expect("prepared account must remain listable").alias.as_str(),
			"Val",
		);
	}

	#[test]
	fn unavailable_profile_keeps_current_claims_and_a_typed_error() {
		let result = account_profile_unavailable_dto(
			AccountProfileClaimsView {
				email: Some("iris@example.test".into()),
				plan_type: Some("pro".into()),
			},
			AccountProfileRuntimeError::ProviderUnavailable,
		)
		.expect("bounded credential claims must map");

		assert!(matches!(
			result,
			decodex_protocol::AccountProfileResult::Unavailable {
				error: decodex_protocol::AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Visible(ref email),
				plan_type: Some(ref plan_type),
			} if email.as_str() == "iris@example.test" && plan_type.as_str() == "pro"
		));
	}

	#[test]
	fn transient_status_failure_is_not_projected_as_durable_pre_effect_failure() {
		let result = operation_query_result(Err(ResetCardServiceError::ProductStateUnavailable));

		assert_eq!(
			result,
			ResetCardOperationResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			}
		);
		assert!(!matches!(result, ResetCardOperationResult::FailedBeforeEffect { .. }));
	}

	#[test]
	fn inventory_deadline_projects_as_a_typed_query_error() {
		assert_eq!(
			protocol_reset_error(ResetCardServiceError::RequestTimedOut),
			ResetCardError::RequestTimedOut,
		);
	}

	#[test]
	fn only_persisted_terminal_failure_projects_as_failed_before_effect() {
		assert_eq!(
			operation_query_result(Ok(ResetCardOperationStatus::FailedBeforeEffect(
				ResetCardFailureCode::InventoryChanged,
			))),
			ResetCardOperationResult::FailedBeforeEffect {
				error: ResetCardError::InventoryChanged,
			},
		);
	}

	#[test]
	fn account_command_receipt_decoder_rejects_unknown_fields() {
		let encoded = serde_json::to_value(StoredAccountCommandOutcome::Rejected {
			schema: ACCOUNT_COMMAND_RECEIPT_SCHEMA.to_owned(),
			error: decodex_protocol::CommandError::IdempotencyConflict,
		})
		.expect("typed receipt serialization must succeed");
		let mut unknown_envelope = encoded.clone();
		unknown_envelope
			.as_object_mut()
			.expect("the stored receipt is an object")
			.insert("unknown".to_owned(), serde_json::Value::Bool(true));
		let mut unknown_result = encoded;
		unknown_result["data"]
			.as_object_mut()
			.expect("the stored result is an object")
			.insert("unknown".to_owned(), serde_json::Value::Bool(true));

		assert!(decode_account_command_receipt(unknown_envelope).is_err());
		assert!(decode_account_command_receipt(unknown_result).is_err());
	}

	#[test]
	fn provider_identity_conflicts_complete_and_replay_typed_provider_mismatch() {
		let error = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::ProviderMismatch,
			actual_revision: None,
		};
		assert_eq!(lifecycle_rejection(AccountLifecycleRejection::IdentityConflict, 0), error,);
		assert_eq!(account_lifecycle_command_error(AccountLifecycleError::ProviderMismatch), error,);
		let encoded = encode_account_command_receipt(&Err(error.clone()))
			.expect("typed provider-mismatch rejection must encode");
		assert_eq!(
			encoded,
			serde_json::json!({
				"outcome": "rejected",
				"data": {
					"schema": "decodex/account-command-result/1",
					"error": {
						"reason": "account_command_rejected",
						"rejection": "provider_mismatch",
					},
				},
			}),
		);
		assert_eq!(decode_account_command_receipt(encoded.clone()), Ok(Err(error.clone())));
		assert_eq!(decode_account_command_receipt(encoded), Ok(Err(error)));
	}
}
