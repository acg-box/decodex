//! Application-service seam used by the transport without exposing infrastructure.

use std::future::{self, Future};

use decodex_codex::CodexAdapter;
use decodex_core::{
	Availability, BlobStore, ConversationId, HistoryItemKind, ItemStatus, PossibleSideEffects,
	ProductState, TurnRole,
};
use decodex_postgres::{BootstrapFailure, HistoryCursor, HistoryEntry, PostgresStore, StoreError};
use decodex_protocol::{
	Channel, CommandEnvelope, CommandError, CommandPayload, ConversationHistoryPage,
	ConversationHistoryResult, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, EntityId, EntityRevision, EventPayload, HistoryArtifactId,
	HistoryArtifactReference, HistoryArtifactRevision, HistoryBlobLength, HistoryBlobReference,
	HistoryCursorToken, HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto,
	HistoryPayloadDto, HistoryQueryError, HistorySideEffectState, HistoryText, HistoryTurnRole,
	MAX_HISTORY_PAGE_SIZE, QueryEnvelope, QueryPayload, QueryResultPayload, ResultPayload,
	Sha256Digest, SnapshotItem, WireText,
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
	_codex: CodexAdapter,
	blob_store: Option<BlobStore>,
	doctor: DoctorReport,
}
impl ServiceApplication {
	pub(crate) const fn new(
		store: ProductStore,
		codex: CodexAdapter,
		blob_store: Option<BlobStore>,
		doctor: DoctorReport,
	) -> Self {
		Self { store, _codex: codex, blob_store, doctor }
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
		match command.payload {
			CommandPayload::RefreshSystemObservation { .. } =>
				Err(CommandError::ApplicationUnavailable {
					message: WireText::new(
						"foundation refresh is superseded by typed doctor/status",
					)
					.expect("service message is bounded"),
				}),
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
		}
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
