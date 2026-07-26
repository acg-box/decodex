//! Application-service seam used by the transport without exposing infrastructure.

use std::{
	future::{self, Future},
	sync::Arc,
};

use decodex_codex::CodexAdapter;
use decodex_core::{
	AccountId, AccountState, Availability, BlobStore, ConversationId, HistoryItemKind, ItemStatus,
	PossibleSideEffects, ProductState, ResetCardConsumeOutcome, ResetCardDescriptor,
	ResetCardTimestamp, TurnRole,
};
use decodex_postgres::{
	BootstrapFailure, HistoryCursor, HistoryEntry, PostgresStore, ResetCardFailureCode,
	ResetCardOperationStatus, StoreError,
};
use decodex_protocol::{
	Channel, CommandEnvelope, CommandError, CommandPayload, ConversationHistoryPage,
	ConversationHistoryResult, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, EntityId, EntityRevision, EventPayload, HistoryArtifactId,
	HistoryArtifactReference, HistoryArtifactRevision, HistoryBlobLength, HistoryBlobReference,
	HistoryCursorToken, HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto,
	HistoryPayloadDto, HistoryQueryError, HistorySideEffectState, HistoryText, HistoryTurnRole,
	MAX_HISTORY_PAGE_SIZE, QueryEnvelope, QueryPayload, QueryResultPayload, ResetCardAccountDto,
	ResetCardAccountsResult, ResetCardAdmissionState, ResetCardDescriptorDto, ResetCardError,
	ResetCardInventoryResult, ResetCardObservationDto, ResetCardOperationResult, ResetCardOutcome,
	ResultPayload, Sha256Digest, SnapshotItem, WireText,
};

use crate::{
	account_launch::{ResetCardRuntime, ResetCardServiceError},
	managed_repository_runtime::{
		ManagedRepositoryReadiness, ManagedRepositoryRuntime, ManagedRepositoryStartupError,
	},
};

/// The only mutation/observation seam reachable from the WebSocket server.
///
/// PostgreSQL-backed services can implement this async owner in XY-1267 without moving
/// command execution into the transport.
pub trait Application: Send + Sync + 'static {
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
#[derive(Clone)]
pub(crate) struct ServiceApplication {
	store: ProductStore,
	_managed_repositories: Option<ManagedRepositoryRuntime>,
	_managed_repository_readiness: ManagedRepositoryReadiness,
	_managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
	_codex: CodexAdapter,
	blob_store: Option<BlobStore>,
	reset_cards: Option<ResetCardRuntime>,
	doctor: DoctorReport,
}
impl ServiceApplication {
	pub(crate) const fn new(
		store: ProductStore,
		managed_repositories: Option<ManagedRepositoryRuntime>,
		managed_repository_readiness: ManagedRepositoryReadiness,
		managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
		codex: CodexAdapter,
		blob_store: Option<BlobStore>,
		doctor: DoctorReport,
	) -> Self {
		Self {
			store,
			_managed_repositories: managed_repositories,
			_managed_repository_readiness: managed_repository_readiness,
			_managed_repository_startup_error: managed_repository_startup_error,
			_codex: codex,
			blob_store,
			reset_cards: None,
			doctor,
		}
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
	async fn reset_card_accounts(&self) -> ResetCardAccountsResult {
		let Some(runtime) = &self.reset_cards else {
			return ResetCardAccountsResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			};
		};

		match runtime.accounts().await {
			Ok(accounts) => {
				let accounts = accounts
					.into_iter()
					.map(|account| {
						Ok(ResetCardAccountDto {
							account_id: EntityId::new(account.account_id.as_str().to_owned())
								.map_err(|_| ())?,
							display_label: WireText::new(account.display_label).map_err(|_| ())?,
							account_revision: EntityRevision(
								u64::try_from(account.revision).map_err(|_| ())?,
							),
							admission_state: match account.state {
								AccountState::Available => ResetCardAdmissionState::Available,
								AccountState::Depleted => ResetCardAdmissionState::Depleted,
								_ => return Err(()),
							},
						})
					})
					.collect::<Result<Vec<_>, _>>();

				match accounts {
					Ok(accounts) => ResetCardAccountsResult::Available { accounts },
					Err(()) => ResetCardAccountsResult::Unavailable {
						error: ResetCardError::ProductStateUnavailable,
					},
				}
			},
			Err(error) =>
				ResetCardAccountsResult::Unavailable { error: protocol_reset_error(error) },
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
			Ok(inventory) => {
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

				match (account_id, account_revision, available_count, cards) {
					(Ok(account_id), Ok(account_revision), Ok(available_count), Ok(cards)) =>
						ResetCardInventoryResult::Available {
							account_id,
							account_revision,
							available_count,
							cards,
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
			QueryPayload::GetConversationHistory { conversation_id, after, page_size } =>
				QueryResultPayload::ConversationHistory(
					self.conversation_history(conversation_id, after.as_ref(), *page_size).await,
				),
			QueryPayload::ListResetCardAccounts =>
				QueryResultPayload::ResetCardAccounts(self.reset_card_accounts().await),
			QueryPayload::GetResetCards { account_id } =>
				QueryResultPayload::ResetCards(self.reset_card_inventory(account_id).await),
			QueryPayload::GetResetCardOperation { idempotency_key } =>
				QueryResultPayload::ResetCardOperation(
					self.reset_card_operation(idempotency_key.as_str()).await,
				),
		}
	}
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
			Ok(HistoryArtifactReference {
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
	use decodex_postgres::{ResetCardFailureCode, ResetCardOperationStatus};
	use decodex_protocol::{ResetCardError, ResetCardOperationResult};

	use super::{ResetCardServiceError, operation_query_result};

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
}
