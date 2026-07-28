//! Application-service seam used by the transport without exposing infrastructure.

use std::{
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
	ExecutionConsumer, HistoryItemKind, ItemStatus, PossibleSideEffects, ProductState,
	QuotaWindowClass, ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp,
	RoutingBlocker, RoutingDecisionKind, TurnRole,
};
use decodex_postgres::{
	AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
	AccountCommandReceiptLease, AccountLifecycleRejection, BootstrapFailure, CommandIdentity,
	ExecutionDecisionReadback, ExecutionQuotaExclusion, HistoryCursor, HistoryEntry, PostgresStore,
	ResetCardFailureCode, ResetCardOperationStatus, RoutingControlOutcome, StoreError,
};
use decodex_protocol::{
	AccountCommandRejectionDto, AccountCredentialBindingDto, AccountDto,
	AccountInitialSelectionResult, AccountInspectResult, AccountLifecycleReadinessDto,
	AccountManualRecoveryActionDto, AccountManualRecoveryOutcomeDto, AccountObservedStateDto,
	AccountOperationKindDto, AccountOperationPhaseDto, AccountProviderDto, AccountQuotaErrorDto,
	AccountQuotaStateDto, AccountQuotaWindowDto, AccountRoutingControlDto, AccountSelectionModeDto,
	AccountSelectionRecoveryDto, AccountUnsettledOperationDto, AccountsResult, Channel,
	CommandEnvelope, CommandError, CommandPayload, ConversationHistoryPage,
	ConversationHistoryResult, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, EntityId, EntityRevision, EventPayload, ExecutionConsumerDto,
	ExecutionDecisionDto, ExecutionDecisionQueryError, ExecutionDecisionResult,
	ExecutionQuotaExclusionDto, ExecutionQuotaWindowDto, ExecutionRouteBlockerDto,
	ExecutionRouteCauseDto, ExecutionRouteDto, HistoryArtifactId, HistoryArtifactReference,
	HistoryArtifactRevision, HistoryBlobLength, HistoryBlobReference, HistoryCursorToken,
	HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto, HistoryPayloadDto, HistoryQueryError,
	HistorySideEffectState, HistoryText, HistoryTurnRole, MAX_HISTORY_PAGE_SIZE, QueryEnvelope,
	QueryPayload, QueryResultPayload, ResetCardDescriptorDto, ResetCardError,
	ResetCardInventoryResult, ResetCardObservationDto, ResetCardOperationResult, ResetCardOutcome,
	ResultPayload, Sha256Digest, SnapshotItem, WireText,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	ProcessGenerationControl, ProviderAttemptControl,
	account_launch::{ResetCardInventoryObservation, ResetCardRuntime, ResetCardServiceError},
	account_service::{
		AccountLifecycleError, AccountManualRecoveryAction, AccountManualRecoveryOutcome,
		AccountService,
	},
	managed_repository_runtime::{
		ManagedRepositoryReadiness, ManagedRepositoryRuntime, ManagedRepositoryStartupError,
	},
};

/// The only mutation/observation seam reachable from the WebSocket server.
///
/// PostgreSQL-backed services can implement this async owner in XY-1267 without moving
/// command execution into the transport.
pub trait Application: Send + Sync + 'static {
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

	/// Return a bounded small-state snapshot. Artifact bytes are not representable.
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send;

	/// Execute one typed command under the application's revision policy.
	fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> impl Future<Output = Result<ApplicationPublication, CommandError>> + Send + 'a;

	/// Execute one fresh observation without mutation receipts or replay semantics.
	fn query<'a>(
		&'a self,
		query: &'a QueryEnvelope,
	) -> impl Future<Output = QueryResultPayload> + Send + 'a;
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

const ACCOUNT_COMMAND_RECEIPT_SCHEMA: &str = "decodex/account-command-result/1";

#[derive(Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
enum StoredAccountCommandOutcome {
	Succeeded {
		schema: String,
		entity_id: EntityId,
		entity_revision: EntityRevision,
		result: ResultPayload,
		event: EventPayload,
	},
	Rejected {
		schema: String,
		error: CommandError,
	},
}

enum ReservedAccountCommand {
	Owned(AccountCommandReceiptLease),
	Replayed(Result<ApplicationPublication, CommandError>),
}

#[derive(Clone)]
pub(crate) enum ProductStore {
	Available(PostgresStore),
	Unavailable { reason: &'static str },
}
impl ProductStore {
	async fn database_status(&self, unavailable: DoctorStatus) -> DoctorStatus {
		let Self::Available(store) = self else {
			return unavailable;
		};

		match store.revalidate().await {
			Ok(()) => DoctorStatus::Ready,
			Err(error) => DoctorStatus::Unavailable(match error.bootstrap_failure() {
				BootstrapFailure::Authentication => DoctorIssue::Authentication,
				BootstrapFailure::Unreachable => DoctorIssue::DatabaseUnreachable,
				BootstrapFailure::Incompatible => DoctorIssue::DatabaseIncompatible,
				BootstrapFailure::UnsafeAuthority => DoctorIssue::UnsafeDatabaseAuthority,
				BootstrapFailure::UnsafeHostPath => DoctorIssue::UnsafeHostPath,
			}),
		}
	}
}
impl ProductState for ProductStore {
	fn availability(&self) -> Availability {
		match self {
			Self::Available(store) => store.availability(),
			Self::Unavailable { reason } => Availability::Unavailable { reason },
		}
	}
}

/// Runtime-owned application service retaining the selected adapter and doctor report.
pub(crate) struct ServiceApplication {
	store: ProductStore,
	_managed_repositories: Option<ManagedRepositoryRuntime>,
	_managed_repository_readiness: ManagedRepositoryReadiness,
	_managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
	process_generations: Option<ProcessGenerationControl>,
	provider_attempts: Option<ProviderAttemptControl>,
	_codex: CodexAdapter,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	reset_cards: Option<ResetCardRuntime>,
	doctor: DoctorReport,
}
impl ServiceApplication {
	#[allow(clippy::too_many_arguments)] // Composition keeps each independently owned runtime capability explicit.
	pub(crate) const fn new(
		store: ProductStore,
		managed_repositories: Option<ManagedRepositoryRuntime>,
		managed_repository_readiness: ManagedRepositoryReadiness,
		managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
		process_generations: Option<ProcessGenerationControl>,
		provider_attempts: Option<ProviderAttemptControl>,
		codex: CodexAdapter,
		blob_store: Option<BlobStore>,
		doctor: DoctorReport,
	) -> Self {
		Self {
			store,
			_managed_repositories: managed_repositories,
			_managed_repository_readiness: managed_repository_readiness,
			_managed_repository_startup_error: managed_repository_startup_error,
			process_generations,
			provider_attempts,
			_codex: codex,
			blob_store,
			accounts: None,
			reset_cards: None,
			doctor,
		}
	}

	pub(crate) fn with_accounts(mut self, accounts: Option<Arc<AccountService>>) -> Self {
		self.accounts = accounts;

		self
	}

	pub(crate) fn with_reset_cards(mut self, reset_cards: Option<ResetCardRuntime>) -> Self {
		self.reset_cards = reset_cards;

		self
	}

	async fn refreshed_doctor(&self) -> DoctorReport {
		let previous_database = self
			.doctor
			.check(DoctorComponent::Database)
			.expect("the closed doctor report includes PostgreSQL")
			.status;
		let database = self.store.database_status(previous_database).await;
		let checks = self
			.doctor
			.checks()
			.iter()
			.map(|check| {
				if check.component == DoctorComponent::Database {
					DoctorCheck::new(DoctorComponent::Database, database)
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
		let Ok((accounts, routing)) = service.list_snapshot().await else {
			return AccountsResult::Unavailable;
		};
		let accounts = accounts
			.into_iter()
			.map(|inspection| account_dto(inspection.account))
			.collect::<Result<Vec<_>, _>>();
		let routing = routing_dto(routing);
		match (accounts, routing) {
			(Ok(accounts), Ok(routing)) => AccountsResult::Available { accounts, routing },
			_ => AccountsResult::Unavailable,
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
				(Ok(account_id), Ok(account_revision)) =>
					AccountInitialSelectionResult::Selected { account_id, account_revision },
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
			AccountCommandReceiptClaim::Replayed(value) =>
				ReservedAccountCommand::Replayed(decode_account_command_receipt(value).map_err(
					|_| application_unavailable("account command receipt is incompatible"),
				)?),
		};
		let lease = match reserved {
			ReservedAccountCommand::Owned(lease) => lease,
			ReservedAccountCommand::Replayed(result) => return result,
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
			CommandPayload::EnrollAccountFromSharedCodex {
				operation_id,
				account_id,
				display_label,
				enabled,
			} => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let account_id = account_id_from_wire(account_id)?;
				service
					.enroll_from_shared_codex_command(
						lease,
						operation_id,
						account_id,
						display_label.as_str().to_owned(),
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
				display_label,
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
						display_label.as_str().to_owned(),
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
			CommandPayload::RecoverAccountOperation { operation_id, action } => {
				let operation_id = operation_id_from_wire(operation_id)?;
				let expected = required_expected_revision(command)?;
				let action = match action {
					AccountManualRecoveryActionDto::ReconcileExactStoreState =>
						AccountManualRecoveryAction::ReconcileExactStoreState,
					AccountManualRecoveryActionDto::CancelBeforeEffect =>
						AccountManualRecoveryAction::CancelBeforeEffect,
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
			CommandPayload::RenameAccount { account_id, display_label } => {
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.update_administration_command(
						lease,
						&account_id,
						expected,
						Some(display_label.as_str()),
						None,
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
								AccountAdministrationOutcome::Rejected { rejection, revision } =>
									Err(lifecycle_rejection(*rejection, *revision)),
							};
							encode_account_command_receipt(&result)
						},
					)
					.await
			},
			CommandPayload::SetAccountEnabled { account_id, enabled } => {
				let account_id = account_id_from_wire(account_id)?;
				let expected = required_expected_revision(command)?;
				service
					.update_administration_command(
						lease,
						&account_id,
						expected,
						None,
						Some(*enabled),
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
								AccountAdministrationOutcome::Rejected { rejection, revision } =>
									Err(lifecycle_rejection(*rejection, *revision)),
							};
							encode_account_command_receipt(&result)
						},
					)
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
		let ProductStore::Available(store) = &self.store else {
			return ExecutionDecisionResult::Unavailable {
				error: ExecutionDecisionQueryError::ProductStateUnavailable,
			};
		};
		match store.execution_decision(decision_id.as_str()).await {
			Ok(Some(readback)) => match execution_decision_dto(readback) {
				Ok(decision) => ExecutionDecisionResult::Decision(decision),
				Err(()) => ExecutionDecisionResult::Unavailable {
					error: ExecutionDecisionQueryError::IntegrityUnavailable,
				},
			},
			Ok(None) | Err(StoreError::InvalidInput(_)) => ExecutionDecisionResult::Unavailable {
				error: ExecutionDecisionQueryError::InvalidRequest,
			},
			Err(StoreError::Incompatible(_)) => ExecutionDecisionResult::Unavailable {
				error: ExecutionDecisionQueryError::IntegrityUnavailable,
			},
			Err(_) => ExecutionDecisionResult::Unavailable {
				error: ExecutionDecisionQueryError::ProductStateUnavailable,
			},
		}
	}

	async fn reset_card_inventory(&self, account_id: &EntityId) -> ResetCardInventoryResult {
		let Some(runtime) = &self.reset_cards else {
			return ResetCardInventoryResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			};
		};
		let Ok(account_id) = AccountId::new(account_id.as_str()) else {
			return ResetCardInventoryResult::Unavailable { error: ResetCardError::InvalidRequest };
		};

		match runtime.inventory(&account_id).await {
			Ok(ResetCardInventoryObservation::Available(inventory)) => {
				let account_id =
					EntityId::new(inventory.account_id.as_str().to_owned()).map_err(|_| ());
				let account_revision =
					u64::try_from(inventory.account_revision).map(EntityRevision).map_err(|_| ());
				let available_count = u16::try_from(inventory.cards.len()).map_err(|_| ());
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

				match (
					account_id,
					account_revision,
					available_count,
					cards,
					five_hour_quota,
					seven_day_quota,
				) {
					(
						Ok(account_id),
						Ok(account_revision),
						Ok(available_count),
						Ok(cards),
						Ok(five_hour_quota),
						Ok(seven_day_quota),
					) => ResetCardInventoryResult::Available {
						account_id,
						account_revision,
						available_count,
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
			Err(error) =>
				ResetCardInventoryResult::Unavailable { error: protocol_reset_error(error) },
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
					(Ok(items), Ok(next_cursor)) =>
						ConversationHistoryResult::Page(ConversationHistoryPage {
							items,
							next_cursor,
						}),
					_ => ConversationHistoryResult::Unavailable {
						error: HistoryQueryError::IntegrityUnavailable,
					},
				}
			},
			Err(StoreError::InvalidInput(_)) =>
				ConversationHistoryResult::Unavailable { error: HistoryQueryError::InvalidRequest },
			Err(StoreError::CapacityExhausted(_)) => ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::ResourceExhausted,
			},
			Err(StoreError::Blob(_) | StoreError::Incompatible(_)) =>
				ConversationHistoryResult::Unavailable {
					error: HistoryQueryError::IntegrityUnavailable,
				},
			Err(_) => ConversationHistoryResult::Unavailable {
				error: HistoryQueryError::ProductStateUnavailable,
			},
		}
	}
}

impl Application for ServiceApplication {
	fn begin_shutdown(&self) {
		if let Some(runtime) = &self.reset_cards {
			runtime.begin_shutdown();
		}
	}

	async fn wait_for_shutdown(&self) {
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
			tasks.push(Box::pin(runtime.clone().daemon_service(stop)));
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
			CommandPayload::EnrollAccountFromSharedCodex { .. }
			| CommandPayload::ImportAccountCredentialFile { .. }
			| CommandPayload::RenameAccount { .. }
			| CommandPayload::SetAccountEnabled { .. }
			| CommandPayload::LogoutAccount { .. }
			| CommandPayload::SetFixedAccountSelection { .. }
			| CommandPayload::SetBalancedAccountSelection
			| CommandPayload::SetAccountOrder { .. }
			| CommandPayload::RefreshAccount { .. }
			| CommandPayload::RecoverAccountOperation { .. } => self.execute_account_command(command).await,
			CommandPayload::RefreshSystemObservation { .. } =>
				Err(CommandError::ApplicationUnavailable {
					message: WireText::new(
						"foundation refresh is superseded by typed doctor/status",
					)
					.expect("service message is bounded"),
				}),
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
			QueryPayload::GetDoctorStatus =>
				QueryResultPayload::DoctorStatus(self.refreshed_doctor().await),
			QueryPayload::GetExecutionDecision { decision_id } =>
				QueryResultPayload::ExecutionDecision(self.execution_decision(decision_id).await),
			QueryPayload::GetConversationHistory { conversation_id, after, page_size } =>
				QueryResultPayload::ConversationHistory(
					self.conversation_history(conversation_id, after.as_ref(), *page_size).await,
				),
			QueryPayload::GetResetCards { account_id } =>
				QueryResultPayload::ResetCards(self.reset_card_inventory(account_id).await),
			QueryPayload::GetResetCardOperation { idempotency_key } =>
				QueryResultPayload::ResetCardOperation(
					self.reset_card_operation(idempotency_key.as_str()).await,
				),
			QueryPayload::ListAccounts => QueryResultPayload::Accounts(self.account_list().await),
			QueryPayload::InspectAccount { account_id } =>
				QueryResultPayload::Account(self.account_inspect(account_id).await),
			QueryPayload::GetInitialAccountSelection =>
				QueryResultPayload::InitialAccountSelection(self.initial_account_selection().await),
		}
	}
}

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
			source_runtime_session_id: entity(source_runtime_session_id.as_str())?,
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
		RoutingDecisionKind::WaitingReconciliation =>
			ExecutionRouteDto::WaitingReconciliation { causes },
		RoutingDecisionKind::NoRoute if !causes.is_empty() => ExecutionRouteDto::NoRoute { causes },
		RoutingDecisionKind::NoRoute => return Err(()),
	};
	Ok(ExecutionDecisionDto { decision_id: entity(&readback.decision_id)?, consumer, route })
}

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

fn account_dto(account: AccountRecord) -> Result<AccountDto, ()> {
	if account.tombstoned {
		return Err(());
	}
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
					AccountOperationPhase::ProviderEffectPending =>
						AccountOperationPhaseDto::ProviderEffectPending,
					AccountOperationPhase::StoreApplied => AccountOperationPhaseDto::StoreApplied,
					AccountOperationPhase::RecoveryRequired =>
						AccountOperationPhaseDto::RecoveryRequired,
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
		display_label: WireText::new(account.label).map_err(|_| ())?,
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
		AccountLifecycleReadiness::CredentialAbsent =>
			AccountLifecycleReadinessDto::CredentialAbsent,
		AccountLifecycleReadiness::StoreUnavailable =>
			AccountLifecycleReadinessDto::StoreUnavailable,
		AccountLifecycleReadiness::StoreMismatch => AccountLifecycleReadinessDto::StoreMismatch,
		AccountLifecycleReadiness::ProviderMismatch =>
			AccountLifecycleReadinessDto::ProviderMismatch,
		AccountLifecycleReadiness::OperationUnsettled =>
			AccountLifecycleReadinessDto::OperationUnsettled,
		AccountLifecycleReadiness::CallbackCapabilityUnready =>
			AccountLifecycleReadinessDto::CallbackCapabilityUnready,
		AccountLifecycleReadiness::Tombstoned => AccountLifecycleReadinessDto::Tombstoned,
	}
}

const fn selection_recovery_dto(recovery: AccountSelectionRecovery) -> AccountSelectionRecoveryDto {
	match recovery {
		AccountSelectionRecovery::ConfigureFixedAccount =>
			AccountSelectionRecoveryDto::ConfigureFixedAccount,
		AccountSelectionRecovery::EnableAccount => AccountSelectionRecoveryDto::EnableAccount,
		AccountSelectionRecovery::EnrollCredentials =>
			AccountSelectionRecoveryDto::EnrollCredentials,
		AccountSelectionRecovery::ResolveCredentialOperation =>
			AccountSelectionRecoveryDto::ResolveCredentialOperation,
		AccountSelectionRecovery::RepairCredentialStore =>
			AccountSelectionRecoveryDto::RepairCredentialStore,
		AccountSelectionRecovery::RestoreProviderAgreement =>
			AccountSelectionRecoveryDto::RestoreProviderAgreement,
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
		CommandPayload::EnrollAccountFromSharedCodex { account_id, .. } =>
			(AccountCommandKind::Enroll, account_id.as_str()),
		CommandPayload::ImportAccountCredentialFile { account_id, .. } =>
			(AccountCommandKind::Import, account_id.as_str()),
		CommandPayload::RenameAccount { account_id, .. } =>
			(AccountCommandKind::Rename, account_id.as_str()),
		CommandPayload::SetAccountEnabled { account_id, .. } =>
			(AccountCommandKind::SetEnabled, account_id.as_str()),
		CommandPayload::LogoutAccount { account_id, .. } =>
			(AccountCommandKind::Logout, account_id.as_str()),
		CommandPayload::SetFixedAccountSelection { .. } =>
			(AccountCommandKind::SetFixedSelection, "account-routing"),
		CommandPayload::SetBalancedAccountSelection =>
			(AccountCommandKind::SetBalancedSelection, "account-routing"),
		CommandPayload::SetAccountOrder { .. } =>
			(AccountCommandKind::SetAccountOrder, "account-routing"),
		CommandPayload::RefreshAccount { account_id, .. } =>
			(AccountCommandKind::Refresh, account_id.as_str()),
		CommandPayload::RecoverAccountOperation { operation_id, .. } =>
			(AccountCommandKind::Recover, operation_id.as_str()),
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
		CommandPayload::RenameAccount { account_id, .. }
		| CommandPayload::SetAccountEnabled { account_id, .. } => {
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::LogoutAccount { operation_id, account_id }
		| CommandPayload::RefreshAccount { operation_id, account_id } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
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
		RoutingControlOutcome::AccountMissing =>
			Err(account_rejection(AccountCommandRejectionDto::AccountNotFound, None)),
		RoutingControlOutcome::InvalidOrder { revision } => Err(account_rejection(
			AccountCommandRejectionDto::RoutingOrderInvalid,
			u64::try_from(*revision).ok().map(EntityRevision),
		)),
		RoutingControlOutcome::InvalidRequest =>
			Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None)),
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
		AccountManualRecoveryOutcome::StillRequiresRecovery =>
			AccountManualRecoveryOutcomeDto::StillRequiresRecovery,
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
		AccountLifecycleRejection::OperationUnsettled =>
			AccountCommandRejectionDto::OperationUnsettled,
		AccountLifecycleRejection::InvalidRequest => AccountCommandRejectionDto::InvalidRequest,
		AccountLifecycleRejection::AccountMissing => AccountCommandRejectionDto::AccountNotFound,
		AccountLifecycleRejection::StaleAccount => AccountCommandRejectionDto::StaleAccount,
		AccountLifecycleRejection::AccountInUse => AccountCommandRejectionDto::AccountInUse,
		AccountLifecycleRejection::OperationMissing =>
			AccountCommandRejectionDto::OperationNotFound,
		AccountLifecycleRejection::StaleOperation =>
			AccountCommandRejectionDto::ManualRecoveryRequired,
	};
	account_rejection(
		reason,
		u64::try_from(revision).ok().filter(|value| *value > 0).map(EntityRevision),
	)
}

fn account_lifecycle_command_error(error: AccountLifecycleError) -> CommandError {
	match error {
		AccountLifecycleError::OperationRejected(rejection) => lifecycle_rejection(rejection, 0),
		AccountLifecycleError::AccountMissing =>
			account_rejection(AccountCommandRejectionDto::AccountNotFound, None),
		AccountLifecycleError::CredentialAbsent =>
			account_rejection(AccountCommandRejectionDto::CredentialAbsent, None),
		AccountLifecycleError::ProviderMismatch =>
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None),
		AccountLifecycleError::StaleAccount =>
			account_rejection(AccountCommandRejectionDto::StaleAccount, None),
		AccountLifecycleError::InvalidOperation =>
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled) =>
			account_rejection(AccountCommandRejectionDto::OperationUnsettled, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent) =>
			account_rejection(AccountCommandRejectionDto::CredentialAbsent, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch) =>
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None),
		AccountLifecycleError::NotReady(_) =>
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None),
		AccountLifecycleError::AccountDisabled =>
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None),
		AccountLifecycleError::CredentialStore(_) =>
			account_rejection(AccountCommandRejectionDto::CredentialStoreUnavailable, None),
		AccountLifecycleError::CredentialImport =>
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None),
		AccountLifecycleError::Refresh(_) =>
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None),
		AccountLifecycleError::Persistence(_) | AccountLifecycleError::CoordinatorUnavailable =>
			application_unavailable("account service is unavailable"),
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
		StoreError::InvalidInput(_) | StoreError::CredentialRejected =>
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None),
		StoreError::CapacityExhausted(_) =>
			application_unavailable("account command receipt capacity is unavailable"),
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
			result: publication.result.clone(),
			event: publication.event.clone(),
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
		} if schema == ACCOUNT_COMMAND_RECEIPT_SCHEMA && entity_revision.0 > 0 =>
			Ok(Ok(ApplicationPublication {
				channel: Channel::AccountsHealth,
				entity_id,
				entity_revision,
				result,
				event,
			})),
		StoredAccountCommandOutcome::Rejected { schema, error }
			if schema == ACCOUNT_COMMAND_RECEIPT_SCHEMA =>
			Ok(Err(error)),
		_ => Err(()),
	}
}

fn quota_dto(observation: AccountQuotaWindowObservation) -> Result<AccountQuotaWindowDto, ()> {
	let result = match observation.disposition {
		AccountQuotaDisposition::Unknown => AccountQuotaStateDto::Unknown,
		AccountQuotaDisposition::Current(fact) => AccountQuotaStateDto::Current {
			used_percent: fact.used_percent,
			resets_at_unix_micros: fact.resets_at_unix_micros,
		},
		AccountQuotaDisposition::Stale(fact) => AccountQuotaStateDto::Stale {
			used_percent: fact.used_percent,
			resets_at_unix_micros: fact.resets_at_unix_micros,
		},
		AccountQuotaDisposition::Error(error) => AccountQuotaStateDto::Error {
			error: match error {
				AccountQuotaObservationError::ProviderUnavailable =>
					AccountQuotaErrorDto::ProviderUnavailable,
				AccountQuotaObservationError::ProtocolUnavailable =>
					AccountQuotaErrorDto::ProtocolUnavailable,
				AccountQuotaObservationError::AccountMismatch =>
					AccountQuotaErrorDto::AccountMismatch,
				AccountQuotaObservationError::UnsupportedWindow =>
					AccountQuotaErrorDto::UnsupportedWindow,
			},
		},
	};
	if !matches!(observation.duration_minutes, 300 | 10_080) {
		return Err(());
	}

	Ok(AccountQuotaWindowDto {
		duration_minutes: observation.duration_minutes,
		observed_at_unix_micros: observation.observed_at_unix_micros,
		result,
	})
}

fn command_reset_error(error: ResetCardServiceError, expected: EntityRevision) -> CommandError {
	match error {
		ResetCardServiceError::ExpectedRevisionMismatch { actual } if actual >= 0 =>
			CommandError::ExpectedRevisionMismatch {
				expected,
				actual: EntityRevision(u64::try_from(actual).unwrap_or(0)),
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
		ResetCardServiceError::AccountStateRejected =>
			"reset-card account state rejects manual use",
		ResetCardServiceError::AccountChanged
		| ResetCardServiceError::ExpectedRevisionMismatch { .. } => "reset-card account revision changed",
		ResetCardServiceError::VaultUnavailable => "reset-card credential vault is unavailable",
		ResetCardServiceError::SchemaUnsupported => "Codex app-server does not support reset cards",
		ResetCardServiceError::ProviderUnavailable => "reset-card provider is unavailable",
		ResetCardServiceError::InventoryIncomplete => "reset-card inventory is incomplete",
		ResetCardServiceError::InventoryChanged => "selected reset card changed",
		ResetCardServiceError::ResourceExhausted => "reset-card process capacity is exhausted",
		ResetCardServiceError::ProductStateUnavailable => "reset-card product state is unavailable",
		ResetCardServiceError::IdempotencyConflict => "reset-card idempotency key conflicts",
		ResetCardServiceError::AcceptanceUnknown =>
			"reset-card durable acceptance could not be established",
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
		ResetCardOperationStatus::Completed(outcome) =>
			ResetCardOperationResult::Completed { outcome: protocol_outcome(outcome) },
		ResetCardOperationStatus::FailedBeforeEffect(error) =>
			ResetCardOperationResult::FailedBeforeEffect { error: failure_reset_error(error) },
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
		(Some(text), None, None) =>
			HistoryPayloadDto::Inline { text: HistoryText::new(text).map_err(|_| ())? },
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
	use decodex_postgres::{
		AccountLifecycleRejection, ResetCardFailureCode, ResetCardOperationStatus,
	};
	use decodex_protocol::{
		AccountCommandRejectionDto, CommandError, ResetCardError, ResetCardOperationResult,
	};

	use super::{
		ACCOUNT_COMMAND_RECEIPT_SCHEMA, AccountLifecycleError, ResetCardServiceError,
		StoredAccountCommandOutcome, account_lifecycle_command_error,
		decode_account_command_receipt, encode_account_command_receipt, lifecycle_rejection,
		operation_query_result,
	};

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
