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
	HistoryItemKind, ItemStatus, PossibleSideEffects, ProductState, ProgramId,
	ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp, RuntimeSessionState, TurnId,
	TurnRole,
};
use decodex_database::{
	AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
	AccountCommandReceiptLease, AccountLifecycleRejection, CommandIdentity, DatabaseError,
	DesktopSettings as StoreDesktopSettings, HistoryCursor, HistoryEntry,
	OrdinaryTaskConversationCursor, OrdinaryTaskConversationProjection,
	OrdinaryTaskConversationReadback, OrdinaryTaskPreSessionState, ProgramCycleRecord,
	ProgramSummaryRecord, RoutingControlOutcome, SqliteStore, StoreError,
};
use decodex_protocol::{
	AccountCommandRejectionDto, AccountCredentialBindingDto, AccountDto,
	AccountInitialSelectionResult, AccountInspectResult, AccountLifecycleReadinessDto,
	AccountLoginRequest, AccountLoginStatus, AccountManualRecoveryActionDto,
	AccountManualRecoveryOutcomeDto, AccountObservedStateDto, AccountOperationKindDto,
	AccountOperationPhaseDto, AccountProfileDailyUsageDto, AccountProfileDto,
	AccountProfileEmailDto, AccountProfileErrorDto, AccountProfileResult, AccountProviderDto,
	AccountQuotaErrorDto, AccountQuotaStateDto, AccountQuotaWindowDto, AccountRoutingControlDto,
	AccountSelectionModeDto, AccountSelectionRecoveryDto, AccountUnsettledOperationDto,
	AccountsResult, CausationId, Channel, CodexAuthProjectionResult, CommandEnvelope, CommandError,
	CommandPayload, ConversationExecutionSettings as ConversationExecutionSettingsDto,
	ConversationHistoryPage, ConversationHistoryResult, ConversationListCursor,
	ConversationListPage, ConversationListResult, ConversationProgramContext,
	ConversationReadError, ConversationRecoveryAction, ConversationResult, ConversationState,
	ConversationSummary, ConversationTitle, ConversationTurnOutcome, CorrelationId,
	DESKTOP_SETTINGS_ENTITY_ID, DesktopSettingsDto, DesktopSettingsResult, DoctorCheck,
	DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus, EntityId, EntityRevision,
	EventPayload, HistoryArtifactId, HistoryArtifactReference, HistoryArtifactRevision,
	HistoryBlobLength, HistoryBlobReference, HistoryCursorToken, HistoryItemDto,
	HistoryItemKindDto, HistoryItemStatusDto, HistoryPayloadDto, HistoryQueryError,
	HistorySideEffectState, HistoryText, HistoryTurnRole, MAX_HISTORY_PAGE_SIZE, ProgramCycleDto,
	ProgramCycleResult, ProgramEdgeDto, ProgramListResult, ProgramNodeDto, ProgramNodeFieldDto,
	ProgramNodeKind, ProgramRelationKind, ProgramSummaryDto, ProviderThreadId, QueryEnvelope,
	QueryPayload, QueryResultPayload, ResetCardDescriptorDto, ResetCardError,
	ResetCardInventoryResult, ResetCardObservationDto, ResetCardOperationResult, ResetCardOutcome,
	ResultPayload, Sha256Digest, SnapshotItem, WireText,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	CredentialStoreError, ProcessGenerationControl, ProviderAttemptControl,
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
		AccountRouteCommit, AccountRouteFailure, AccountRouteResult, AccountService,
		CodexAuthProjectionInspection, stable_account_alias,
	},
	conversation::{
		ControlConversation, ConversationCapability, ConversationControlOutcome,
		ConversationExecutionSettings as RuntimeConversationExecutionSettings,
		ConversationLocalState, ConversationManualRecovery, ConversationOutcome,
		ConversationProjection, ConversationReadback, ConversationRuntime,
		ConversationTerminalState, CreateConversation, RecoverConversation, SubmitConversationTurn,
	},
	domain_packs,
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

	/// Return a static snapshot that does not depend on mutable command state.
	/// Only applications with command-independent snapshot contents may opt in.
	fn command_independent_snapshot(&self) -> Option<Vec<SnapshotItem>> {
		None
	}

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

	/// Execute one transient account-login operation without publication or retained state.
	fn account_login<'a>(
		&'a self,
		request: &'a AccountLoginRequest,
	) -> impl Future<Output = AccountLoginStatus> + Send + 'a {
		future::ready(crate::account_login::unavailable_status(request))
	}

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
			ResultPayload::ConversationAccepted { .. }
				| ResultPayload::ConversationInterruptAccepted { .. }
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
				DatabaseError::Incompatible | DatabaseError::Corrupt =>
					DoctorIssue::DatabaseIncompatible,
				DatabaseError::UnsafePath => DoctorIssue::UnsafeHostPath,
				DatabaseError::Unavailable | DatabaseError::Closed =>
					DoctorIssue::DatabaseUnreachable,
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
	chief: Option<crate::chief_host::ChiefHost>,
	store: ProductStore,
	process_generations: Option<ProcessGenerationControl>,
	provider_attempts: Option<ProviderAttemptControl>,
	_codex: CodexAdapter,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	publication_stop: watch::Sender<bool>,
	reset_cards: Option<ApiResetCardRuntime>,
	account_observations: Option<AccountObservationService>,
	account_login: Option<Arc<crate::account_login::AccountLoginManager>>,
	conversations: ConversationCapability,
	doctor: DoctorReport,
}
impl ServiceApplication {
	async fn query_guardian_review_page(
		&self,
		work: &str,
		before: Option<i64>,
	) -> QueryResultPayload {
		QueryResultPayload::ChiefGuardianReviews(
			query_guardian_reviews(&self.store, self.chief.as_ref(), work, before).await,
		)
	}

	async fn query_native_goal(&self, work: &str, thread: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefNativeGoal(match &self.chief {
			Some(chief) => chief.native_goal(work, thread).await,
			None => decodex_protocol::ChiefNativeGoalResult::Unavailable,
		})
	}

	async fn query_integrations(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefIntegrations(match &self.chief {
			Some(chief) => chief.integrations(work).await,
			None => decodex_protocol::ChiefIntegrationsResult::Unavailable,
		})
	}

	async fn query_app_settings(&self, work: &str, event: i64) -> QueryResultPayload {
		QueryResultPayload::ChiefAppSettings(match &self.chief {
			Some(chief) => chief.app_settings(work, event).await,
			None => decodex_protocol::ChiefAppSettingsResult::Unavailable,
		})
	}

	async fn query_saved_app_settings(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefSavedAppSettings(match &self.chief {
			Some(chief) => chief.saved_app_settings(work).await,
			None => decodex_protocol::ChiefSavedAppSettingsResult::Unavailable,
		})
	}

	async fn query_hook_settings(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefHookSettings(match &self.chief {
			Some(chief) => chief.hook_settings(work).await,
			None => decodex_protocol::ChiefHookSettingsState::Unavailable,
		})
	}

	async fn query_plugin_selection(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefPluginSelection(match &self.chief {
			Some(chief) => chief.plugin_selection(work).await,
			None => decodex_protocol::ChiefPluginSelectionState::Unavailable,
		})
	}

	async fn query_model_selection(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefModelSelection(match &self.chief {
			Some(chief) => chief.model_selection(work).await,
			None => decodex_protocol::ChiefModelSelectionState::Unavailable,
		})
	}

	async fn query_permission_profiles(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefPermissionProfiles(match &self.chief {
			Some(chief) => chief.permission_profiles(work).await,
			None => decodex_protocol::ChiefPermissionState::Unavailable,
		})
	}

	async fn query_live_reviewer(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefLiveReviewer(match &self.chief {
			Some(chief) => chief.live_reviewer(work).await,
			None => decodex_protocol::ChiefLiveReviewerState::Unavailable,
		})
	}

	async fn query_dictation(
		&self,
		request: &decodex_protocol::DictationRequest,
	) -> QueryResultPayload {
		QueryResultPayload::Dictation(match &self.chief {
			Some(chief) => chief.dictation(request).await,
			None =>
				crate::dictation::failed(request.session_id().clone(), "Chief is not connected."),
		})
	}

	fn query_voice(&self, request: &decodex_protocol::ChiefVoiceRequest) -> QueryResultPayload {
		QueryResultPayload::ChiefVoice(match &self.chief {
			Some(chief) => chief.voice(request),
			None =>
				crate::chief_voice::failed(request.session_id().clone(), "Chief is not connected."),
		})
	}

	async fn query_output(
		&self,
		work_id: &EntityId,
		after_revision: Option<u64>,
	) -> QueryResultPayload {
		let result = match &self.store {
			ProductStore::Available(store) =>
				match store.wait_chief_output(work_id.as_str().into(), after_revision).await {
					Ok((revision, output)) => {
						let messages = query_chief_live(output);
						decodex_protocol::ChiefOutputResult::Available {
							revision,
							work_id: work_id.clone(),
							messages,
						}
					},
					Err(_) => decodex_protocol::ChiefOutputResult::Unavailable,
				},
			_ => decodex_protocol::ChiefOutputResult::Unavailable,
		};
		QueryResultPayload::ChiefOutput(result)
	}

	async fn query_native_agents(
		&self,
		work: &EntityId,
		thread: Option<&WireText>,
		cursor: Option<&WireText>,
	) -> QueryResultPayload {
		QueryResultPayload::NativeAgents(match &self.chief {
			Some(chief) =>
				chief
					.native_agents(
						work.as_str(),
						thread.map(WireText::as_str),
						cursor.map(WireText::as_str),
					)
					.await,
			None => decodex_protocol::NativeAgentsResult::Unavailable,
		})
	}

	async fn query_history(&self, work: &str, before: Option<i64>) -> QueryResultPayload {
		let mut history = query_chief_history_page(&self.store, work, before).await;
		if let Some(chief) = &self.chief {
			chief.enrich_weather(work, &mut history).await;
		}
		QueryResultPayload::ChiefHistory(history)
	}

	async fn query_activity_detail(
		&self,
		work: &decodex_protocol::EntityId,
		turn: &decodex_protocol::WireText,
		item: &decodex_protocol::WireText,
		cursor: Option<&decodex_protocol::ChiefActivityDetailCursor>,
	) -> QueryResultPayload {
		QueryResultPayload::ChiefActivityDetail(match &self.chief {
			Some(chief) =>
				chief.activity_detail(work.as_str(), turn.as_str(), item.as_str(), cursor).await,
			None => decodex_protocol::ChiefActivityDetailResult::Unavailable,
		})
	}

	async fn query_input_receipts(&self, work: &str, after: Option<i64>) -> QueryResultPayload {
		QueryResultPayload::ChiefInputReceipts(
			query_chief_input_receipts(&self.store, work, after).await,
		)
	}

	async fn query_chief_snapshot(&self) -> QueryResultPayload {
		let before = match &self.chief {
			Some(chief) => chief.runtime_source().await,
			None => None,
		};
		let mut result = query_chief_snapshot(&self.store).await;
		let after = match &self.chief {
			Some(chief) => chief.runtime_source().await,
			None => None,
		};
		if let decodex_protocol::ChiefSnapshotResult::Available(snapshot) = &mut result {
			snapshot.runtime_source = if before == after { after } else { None };
			if !snapshot.is_valid() {
				result = decodex_protocol::ChiefSnapshotResult::Unavailable;
			}
		}
		QueryResultPayload::ChiefSnapshot(result)
	}

	async fn query_model_settings(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefModelSettings(match &self.chief {
			Some(chief) => chief.model_settings(work).await,
			None => decodex_protocol::ChiefModelSettingsResult::Unavailable,
		})
	}

	async fn query_usage_estimate(&self, work: &str) -> QueryResultPayload {
		QueryResultPayload::ChiefUsageEstimate(match &self.chief {
			Some(chief) => chief.usage_estimate(work).await,
			None => decodex_protocol::ChiefUsageEstimateResult::Unavailable,
		})
	}

	async fn query_steer_receipt(
		&self,
		identity: &decodex_protocol::ChiefSteerIdentity,
	) -> QueryResultPayload {
		use decodex_protocol::ChiefSteerReceiptResult as Receipt;
		let result = match &self.store {
			ProductStore::Available(store) => match store
				.chief_steer_confirmed(
					identity.work_id.as_str().into(),
					identity.thread_id.as_str().into(),
					identity.turn_id.as_str().into(),
					identity.submission_id.as_str().into(),
				)
				.await
			{
				Ok(true) => Receipt::Confirmed { identity: identity.clone() },
				Ok(false) => Receipt::Unconfirmed,
				Err(_) => Receipt::Unavailable,
			},
			_ => Receipt::Unavailable,
		};
		QueryResultPayload::ChiefSteerReceipt(result)
	}

	async fn query_media(
		&self,
		request: &decodex_protocol::ChiefMediaRequest,
	) -> QueryResultPayload {
		QueryResultPayload::ChiefMedia(match &self.chief {
			Some(chief) => chief.media(request).await,
			None => decodex_protocol::ChiefMediaResult::Unavailable,
		})
	}

	async fn query_timeline(
		&self,
		work: &str,
		thread: &str,
		cursor: Option<&str>,
	) -> QueryResultPayload {
		QueryResultPayload::ChiefTimeline(match &self.chief {
			Some(chief) => chief.timeline(work, thread, cursor).await,
			None => decodex_protocol::ChiefTimelineResult::Unavailable,
		})
	}

	pub(crate) fn new(
		store: ProductStore,
		process_generations: Option<ProcessGenerationControl>,
		provider_attempts: Option<ProviderAttemptControl>,
		codex: CodexAdapter,
		blob_store: Option<BlobStore>,
		conversations: ConversationCapability,
		doctor: DoctorReport,
	) -> Self {
		let (publication_stop, _) = watch::channel(false);
		let chief = match (&store, conversations.runtime()) {
			(ProductStore::Available(store), Some(runtime)) =>
				Some(crate::chief_host::ChiefHost::new(store.clone(), runtime.clone())),
			_ => None,
		};
		Self {
			chief,
			store,
			process_generations,
			provider_attempts,
			_codex: codex,
			blob_store,
			accounts: None,
			publication_stop,
			reset_cards: None,
			account_observations: None,
			account_login: None,
			conversations,
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

	pub(crate) fn with_account_login(
		mut self,
		account_login: Option<Arc<crate::account_login::AccountLoginManager>>,
	) -> Self {
		self.account_login = account_login;

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
	async fn desktop_settings(&self) -> DesktopSettingsResult {
		let ProductStore::Available(store) = &self.store else {
			return DesktopSettingsResult::Unavailable;
		};
		store
			.read_desktop_settings()
			.await
			.ok()
			.and_then(|settings| desktop_settings_dto(settings).ok())
			.map_or(DesktopSettingsResult::Unavailable, DesktopSettingsResult::Available)
	}

	async fn execute_desktop_settings(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let CommandPayload::SetDesktopSettings { show_in_menu_bar, auto_activate_quota } =
			&command.payload
		else {
			return Err(application_unavailable("desktop settings command is invalid"));
		};
		let expected = command.expected_revision.ok_or_else(|| {
			application_unavailable("desktop settings expected revision is required")
		})?;
		let expected_revision = i64::try_from(expected.0)
			.map_err(|_| application_unavailable("desktop settings revision is invalid"))?;
		let ProductStore::Available(store) = &self.store else {
			return Err(application_unavailable("desktop settings store is unavailable"));
		};
		let settings = store
			.set_desktop_settings(expected_revision, *show_in_menu_bar, *auto_activate_quota)
			.await
			.map_err(desktop_settings_command_error)?;
		self.request_account_observation_refresh();
		let settings = desktop_settings_dto(settings)
			.map_err(|_| application_unavailable("desktop settings projection is invalid"))?;
		let entity_id = EntityId::new(DESKTOP_SETTINGS_ENTITY_ID)
			.expect("desktop settings entity identity is bounded");

		Ok(ApplicationPublication {
			channel: Channel::Control,
			entity_id,
			entity_revision: settings.revision,
			result: ResultPayload::DesktopSettingsChanged { settings },
			event: EventPayload::DesktopSettingsChanged { settings },
		})
	}

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
						CodexAuthProjectionResult::Current {
							account_id,
							account_revision,
							projection_digest,
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
			AccountProfileRuntimeResult::Cached { profile, refresh_error } =>
				match account_profile_dto(profile) {
					Ok(profile) => AccountProfileResult::Cached {
						profile: Box::new(profile),
						refresh_error: account_profile_error_dto(refresh_error),
					},
					Err(()) =>
						unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable),
				},
			AccountProfileRuntimeResult::Unavailable { claims, error } =>
				account_profile_unavailable_dto(claims, error).unwrap_or_else(|()| {
					unavailable_account_profile(AccountProfileErrorDto::ProductStateUnavailable)
				}),
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
		let claim =
			store.reserve_account_command(&identity, kind, &entity_id, expected_revision).await;
		let reserved = match claim.map_err(map_account_store_command_error)? {
			AccountCommandReceiptClaim::Owned(lease) => ReservedAccountCommand::Owned(lease),
			AccountCommandReceiptClaim::Pending(value)
			| AccountCommandReceiptClaim::Replayed(value) => ReservedAccountCommand::Replayed(Box::new(
				decode_account_command_receipt(value).map_err(|_| {
					application_unavailable("account command receipt is incompatible")
				})?,
			)),
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
				let requested_account_id = account_id.clone();
				service
					.enroll_from_shared_codex_command(
						lease,
						operation_id,
						account_id,
						*enabled,
						move |result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| {
										account_enrollment_publication(
											&requested_account_id,
											account.clone(),
										)
									},
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
				let requested_account_id = account_id.clone();
				service
					.import_credential_file_command(
						lease,
						operation_id,
						account_id,
						*enabled,
						source_descriptor.as_str(),
						move |result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| {
										account_enrollment_publication(
											&requested_account_id,
											account.clone(),
										)
									},
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
			CommandPayload::RouteAccount { account_id } => {
				let account_id = account_id_from_wire(account_id)?;
				service
					.route_account_command_sync(lease, &account_id, |result| {
						encode_account_command_receipt(&account_route_result(result))
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
								AccountAdministrationOutcome::Rejected { rejection, revision } =>
									Err(lifecycle_rejection(*rejection, *revision)),
							};
							encode_account_command_receipt(&result)
						},
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

	async fn account_reset_card_operation(
		&self,
		account_id: &EntityId,
	) -> decodex_protocol::AccountResetCardOperationResult {
		use decodex_protocol::{AccountResetCardOperationResult as Result, ResetCardOperationView};
		let Some(runtime) = &self.reset_cards else {
			return Result::Unavailable { error: ResetCardError::ProductStateUnavailable };
		};
		let Ok(account) = AccountId::new(account_id.as_str()) else {
			return Result::Unavailable { error: ResetCardError::InvalidRequest };
		};
		let operation = match runtime.latest_operation(&account).await {
			Ok(Some(operation)) => operation,
			Ok(None) => return Result::NotFound,
			Err(error) => return Result::Unavailable { error: protocol_reset_error(error) },
		};
		let (Ok(key), Ok(descriptor), Ok(revision)) = (
			decodex_protocol::IdempotencyKey::new(operation.key.clone()),
			decodex_protocol::ResetCardDescriptorDto::new(
				operation.granted_at,
				operation.expires_at,
			),
			u64::try_from(operation.account_revision),
		) else {
			return Result::Unavailable { error: ResetCardError::ProductStateUnavailable };
		};
		Result::Found(ResetCardOperationView {
			account_id: account_id.clone(),
			account_revision: EntityRevision(revision),
			idempotency_key: key,
			descriptor,
			state: operation_query_result(runtime.operation_status(&operation.key).await),
		})
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
		let mut run_states = Vec::new();
		let mut provider_threads = HashMap::new();
		for work_item in &record.work_items {
			let Some(conversation_id) = work_item.conversation_id.as_ref() else {
				continue;
			};
			let entity_id = EntityId::new(conversation_id.as_str()).map_err(|_| ())?;
			let state = match self.conversation_get(&entity_id).await {
				ConversationResult::Available(summary) => {
					if let Some(thread_id) = summary.codex_thread_id {
						provider_threads.insert(conversation_id.clone(), thread_id);
					}
					conversation_state_text(summary.state)
				},
				ConversationResult::RoutingSuccessorRedirect { .. } => "routing_successor",
				ConversationResult::NotFound => "archived",
				ConversationResult::Unavailable { .. } => "unavailable",
			};
			run_states.push((conversation_id.clone(), state));
		}
		let domain_pack = domain_packs::projection(&record, &provider_threads).map_err(|_| ())?;
		let cycle = program_cycle_dto(record, &run_states, &provider_threads)?;
		match domain_pack {
			Some(domain_pack) => cycle.with_domain_pack(domain_pack).map_err(|_| ()),
			None => Ok(cycle),
		}
	}

	async fn conversation_row(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<OrdinaryTaskConversationProjection>, ConversationReadError> {
		let ProductStore::Available(store) = &self.store else {
			return Err(ConversationReadError::ProductStateUnavailable);
		};
		let mut rows = store
			.read_ordinary_task_conversations(Some(conversation_id), None, 2)
			.await
			.map_err(|error| conversation_read_error(&error))?;
		if rows.len() > 1 {
			return Err(ConversationReadError::IntegrityUnavailable);
		}
		Ok(rows.pop())
	}

	async fn conversation_list(
		&self,
		after: Option<&ConversationListCursor>,
		page_size: u16,
	) -> ConversationListResult {
		let ProductStore::Available(store) = &self.store else {
			return ConversationListResult::Unavailable {
				error: ConversationReadError::ProductStateUnavailable,
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
				return ConversationListResult::Unavailable {
					error: ConversationReadError::InvalidRequest,
				};
			},
		};
		let requested = usize::from(page_size);
		let Some(limit) = requested.checked_add(1) else {
			return ConversationListResult::Unavailable {
				error: ConversationReadError::InvalidRequest,
			};
		};
		let rows = match store.read_ordinary_task_conversations(None, after.as_ref(), limit).await {
			Ok(rows) => rows,
			Err(error) => {
				return ConversationListResult::Unavailable {
					error: conversation_read_error(&error),
				};
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
				return ConversationListResult::Unavailable {
					error: ConversationReadError::IntegrityUnavailable,
				};
			},
		};
		let has_more = rows.len() > requested;
		if has_more {
			rows.pop();
		}
		let next_cursor = if has_more {
			rows.last().and_then(|row| {
				ConversationListCursor::new(
					row.updated_at_micros,
					EntityId::new(row.conversation_id.as_str()).ok()?,
				)
				.ok()
			})
		} else {
			None
		};
		if has_more && next_cursor.is_none() {
			return ConversationListResult::Unavailable {
				error: ConversationReadError::IntegrityUnavailable,
			};
		}
		let conversations = rows
			.into_iter()
			.map(|row| {
				let projection = self
					.conversations
					.runtime()
					.and_then(|runtime| runtime.projection(&row.conversation_id));
				conversation_summary_from_row(row, projection)
			})
			.collect::<Result<Vec<_>, _>>();
		match conversations.and_then(|conversations| {
			ConversationListPage::new(conversations, next_cursor).map_err(|_| ())
		}) {
			Ok(page) => ConversationListResult::Available(page),
			Err(()) => ConversationListResult::Unavailable {
				error: ConversationReadError::IntegrityUnavailable,
			},
		}
	}

	async fn conversation_get(&self, conversation_id: &EntityId) -> ConversationResult {
		let Ok(conversation_id) = ConversationId::new(conversation_id.as_str()) else {
			return ConversationResult::Unavailable {
				error: ConversationReadError::InvalidRequest,
			};
		};
		let projection = match self.conversation_row(&conversation_id).await {
			Ok(Some(projection)) => projection,
			Ok(None) => return ConversationResult::NotFound,
			Err(error) => return ConversationResult::Unavailable { error },
		};
		match projection {
			OrdinaryTaskConversationProjection::Current(row) => {
				let local = self
					.conversations
					.runtime()
					.and_then(|runtime| runtime.projection(&conversation_id));
				match conversation_summary_from_row(row, local) {
					Ok(summary) => ConversationResult::Available(summary),
					Err(()) => ConversationResult::Unavailable {
						error: ConversationReadError::IntegrityUnavailable,
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
				(Ok(source), Ok(source_revision), Ok(successor), Ok(successor_revision)) =>
					ConversationResult::RoutingSuccessorRedirect {
						source_conversation_id: source,
						source_conversation_revision: EntityRevision(source_revision),
						successor_conversation_id: successor,
						successor_conversation_revision: EntityRevision(successor_revision),
					},
				_ => ConversationResult::Unavailable {
					error: ConversationReadError::IntegrityUnavailable,
				},
			},
			OrdinaryTaskConversationProjection::Archived { .. } => ConversationResult::NotFound,
		}
	}

	async fn publish_conversation_routing_successor(
		&self,
		source_conversation_id: &ConversationId,
		source_revision: i64,
		successor_conversation_id: &ConversationId,
		successor_revision: i64,
	) -> Result<ApplicationPublication, CommandError> {
		let successor_id =
			EntityId::new(successor_conversation_id.as_str().to_owned()).map_err(|_| {
				application_unavailable("Conversation successor identity is incompatible")
			})?;
		let ConversationResult::Available(successor) = self.conversation_get(&successor_id).await
		else {
			return Err(application_unavailable("Conversation successor readback is unavailable"));
		};
		conversation_routing_successor_publication(
			source_conversation_id,
			source_revision,
			successor_conversation_id,
			successor_revision,
			successor,
		)
	}

	async fn conversation_command_row(
		&self,
		conversation_id: &ConversationId,
		expected: Option<EntityRevision>,
	) -> Result<OrdinaryTaskConversationReadback, CommandError> {
		let projection = self
			.conversation_row(conversation_id)
			.await
			.map_err(|_| application_unavailable("Conversation readback is unavailable"))?
			.ok_or_else(conversation_conflict)?;
		let OrdinaryTaskConversationProjection::Current(row) = projection else {
			return Err(conversation_conflict());
		};
		let actual = EntityRevision(
			u64::try_from(row.conversation_revision).map_err(|_| conversation_conflict())?,
		);
		let expected = expected.ok_or_else(conversation_conflict)?;
		if expected != actual {
			return Err(CommandError::ExpectedRevisionMismatch { expected, actual });
		}
		Ok(row)
	}

	async fn execute_create_conversation(
		&self,
		runtime: &ConversationRuntime,
		command: &CommandEnvelope,
	) -> Result<ConversationOutcome, CommandError> {
		let CommandPayload::CreateConversation {
			conversation_id,
			message,
			working_directory,
			execution,
		} = &command.payload
		else {
			return Err(conversation_conflict());
		};
		if command.expected_revision.is_some() || message.as_str().trim().is_empty() {
			return Err(conversation_conflict());
		}
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		Ok(runtime
			.create(CreateConversation {
				operation_key: command.idempotency_key.as_str().to_owned(),
				correlation_id: command.correlation_id.as_str().to_owned(),
				causation_id: command.causation_id.as_ref().map(|id| id.as_str().to_owned()),
				conversation_id,
				message: message.as_str().to_owned(),
				working_directory: working_directory.as_str().to_owned(),
				execution: runtime_execution_settings(execution),
			})
			.await)
	}

	async fn execute_conversation_recovery(
		&self,
		runtime: &ConversationRuntime,
		command: &CommandEnvelope,
	) -> Result<ConversationOutcome, CommandError> {
		let conversation_id = match &command.payload {
			CommandPayload::ResumeConversationRouting { conversation_id } => conversation_id,
			CommandPayload::ResumeConversationEstablishment { conversation_id } => conversation_id,
			_ => return Err(conversation_conflict()),
		};
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		let row =
			self.conversation_command_row(&conversation_id, command.expected_revision).await?;
		let recoverable = match &command.payload {
			CommandPayload::ResumeConversationRouting { .. } =>
				row.runtime_session_id.is_none()
					&& row.pre_session_state == Some(OrdinaryTaskPreSessionState::RoutingPending),
			CommandPayload::ResumeConversationEstablishment { .. } =>
				(row.runtime_session_id.is_none()
					&& row.pre_session_state
						== Some(OrdinaryTaskPreSessionState::EstablishmentPending))
					|| (row.runtime_session_id.is_some()
						&& row.pre_session_state.is_none()
						&& row.runtime_session_state == Some(RuntimeSessionState::Starting)
						&& !row.has_acknowledged_turn
						&& !row.has_active_provider_attempt
						&& !row.has_unknown_provider_attempt
						&& (!row.has_admitted_user_turn || row.active_turn_id.is_some())),
			_ => false,
		};
		if !recoverable {
			return Err(conversation_conflict());
		}
		let recovery = RecoverConversation {
			operation_key: command.idempotency_key.as_str().to_owned(),
			correlation_id: command.correlation_id.as_str().to_owned(),
			causation_id: command.causation_id.as_ref().map(|id| id.as_str().to_owned()),
			conversation_id,
			expected_conversation_revision: row.conversation_revision,
		};
		Ok(match &command.payload {
			CommandPayload::ResumeConversationRouting { .. } =>
				runtime.resume_routing(recovery).await,
			CommandPayload::ResumeConversationEstablishment { .. } =>
				runtime.resume_establishment(recovery).await,
			_ => return Err(conversation_conflict()),
		})
	}

	async fn execute_conversation_routing_successor(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let CommandPayload::CreateConversationRoutingSuccessor { conversation_id } =
			&command.payload
		else {
			return Err(conversation_conflict());
		};
		let source_conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		let projection = self
			.conversation_row(&source_conversation_id)
			.await
			.map_err(|_| application_unavailable("Conversation readback is unavailable"))?
			.ok_or_else(conversation_conflict)?;
		match projection {
			OrdinaryTaskConversationProjection::Archived { .. } => Err(conversation_conflict()),
			OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
				source_conversation_id: redirected_source,
				source_revision,
				successor_conversation_id,
				successor_conversation_revision,
			} => {
				let expected = command.expected_revision.ok_or_else(conversation_conflict)?;
				let source_revision_wire =
					u64::try_from(source_revision).map_err(|_| conversation_conflict())?;
				if redirected_source != source_conversation_id
					|| expected.0.checked_add(1) != Some(source_revision_wire)
				{
					return Err(conversation_conflict());
				}
				self.publish_conversation_routing_successor(
					&redirected_source,
					source_revision,
					&successor_conversation_id,
					successor_conversation_revision,
				)
				.await
			},
			OrdinaryTaskConversationProjection::Current(row) => {
				let actual = EntityRevision(
					u64::try_from(row.conversation_revision)
						.map_err(|_| conversation_conflict())?,
				);
				let expected = command.expected_revision.ok_or_else(conversation_conflict)?;
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
					return Err(conversation_conflict());
				}
				let ProductStore::Available(store) = &self.store else {
					return Err(application_unavailable(
						"Conversation successor persistence is unavailable",
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
					.map_err(|_| conversation_conflict())?;
				let relation = result.successor;
				if let ConversationCapability::Ready(runtime) = &self.conversations {
					runtime
						.start_preplanned_initial(
							RecoverConversation {
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
				self.publish_conversation_routing_successor(
					&relation.source_conversation_id,
					relation.source_revision,
					&relation.successor_conversation_id,
					relation.successor_revision,
				)
				.await
			},
		}
	}

	async fn execute_submit_conversation_turn(
		&self,
		runtime: &ConversationRuntime,
		command: &CommandEnvelope,
	) -> Result<ConversationOutcome, CommandError> {
		let CommandPayload::SubmitConversationTurn {
			conversation_id,
			turn_id,
			message,
			working_directory,
			execution,
		} = &command.payload
		else {
			return Err(conversation_conflict());
		};
		if message.as_str().trim().is_empty() {
			return Err(conversation_conflict());
		}
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		let turn_id = TurnId::new(turn_id.as_str()).map_err(|_| conversation_conflict())?;
		self.conversation_command_row(&conversation_id, command.expected_revision).await?;
		Ok(runtime
			.submit_turn(SubmitConversationTurn {
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

	async fn execute_interrupt_conversation(
		&self,
		runtime: &ConversationRuntime,
		command: &CommandEnvelope,
	) -> Result<ConversationOutcome, CommandError> {
		let CommandPayload::InterruptConversation { conversation_id, turn_id } = &command.payload
		else {
			return Err(conversation_conflict());
		};
		let conversation_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		let turn_id = TurnId::new(turn_id.as_str()).map_err(|_| conversation_conflict())?;
		let row =
			self.conversation_command_row(&conversation_id, command.expected_revision).await?;
		let Some(projection) = runtime.projection(&conversation_id) else {
			return Err(CommandError::ConversationRecoveryRequired {
				action: if row.active_turn_id.as_ref() == Some(&turn_id) {
					ConversationRecoveryAction::ResolvePriorActiveTurn
				} else {
					ConversationRecoveryAction::StartNewConversation
				},
			});
		};
		if projection.readback.active_turn_id.as_ref() != Some(&turn_id) {
			return Err(conversation_conflict());
		}
		Ok(runtime.interrupt(&conversation_id))
	}

	async fn execute_control_conversation(
		&self,
		runtime: &ConversationRuntime,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		let (conversation_id, archive) = match &command.payload {
			CommandPayload::RefreshConversation { conversation_id } => (conversation_id, false),
			CommandPayload::ArchiveConversation { conversation_id } => (conversation_id, true),
			_ => return Err(conversation_conflict()),
		};
		let core_id =
			ConversationId::new(conversation_id.as_str()).map_err(|_| conversation_conflict())?;
		let row = self.conversation_command_row(&core_id, command.expected_revision).await?;
		if row.has_active_provider_attempt {
			return Err(conversation_busy());
		}
		if row.active_turn_id.is_some() != row.active_turn_revision.is_some() {
			return Err(conversation_conflict());
		}
		let runtime_session_id = row.runtime_session_id.ok_or_else(conversation_conflict)?;
		let runtime_session_revision =
			row.runtime_session_revision.ok_or_else(conversation_conflict)?;
		match runtime
			.control_thread(ControlConversation {
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
			ConversationControlOutcome::Current => {
				let ConversationResult::Available(conversation) =
					self.conversation_get(conversation_id).await
				else {
					return Err(application_unavailable(
						"Conversation refresh readback is unavailable",
					));
				};
				conversation_command_publication(conversation, false)
			},
			ConversationControlOutcome::Archived { conversation_revision } => {
				let revision = EntityRevision(
					u64::try_from(conversation_revision).map_err(|_| conversation_conflict())?,
				);
				Ok(ApplicationPublication {
					channel: Channel::ConversationStream,
					entity_id: conversation_id.clone(),
					entity_revision: revision,
					result: ResultPayload::ConversationArchived {
						conversation_id: conversation_id.clone(),
						conversation_revision: revision,
					},
					event: EventPayload::ConversationArchived {
						conversation_id: conversation_id.clone(),
						conversation_revision: revision,
					},
				})
			},
			ConversationControlOutcome::Busy => Err(conversation_busy()),
			ConversationControlOutcome::Conflict => Err(conversation_conflict()),
			ConversationControlOutcome::OutcomeUnknown => Err(CommandError::AcceptanceUnknown),
			ConversationControlOutcome::Unavailable =>
				Err(application_unavailable("Conversation thread control is unavailable")),
		}
	}

	async fn execute_conversation(
		&self,
		command: &CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		if matches!(&command.payload, CommandPayload::CreateConversationRoutingSuccessor { .. }) {
			return self.execute_conversation_routing_successor(command).await;
		}
		let runtime = match &self.conversations {
			ConversationCapability::Ready(runtime) => runtime,
			ConversationCapability::Unavailable(reason) => {
				return Err(CommandError::ConversationUnavailable { unavailable_reason: *reason });
			},
		};
		if matches!(
			&command.payload,
			CommandPayload::RefreshConversation { .. } | CommandPayload::ArchiveConversation { .. }
		) {
			return self.execute_control_conversation(runtime, command).await;
		}
		let outcome = match &command.payload {
			CommandPayload::CreateConversation { .. } =>
				self.execute_create_conversation(runtime, command).await?,
			CommandPayload::ResumeConversationRouting { .. }
			| CommandPayload::ResumeConversationEstablishment { .. } =>
				self.execute_conversation_recovery(runtime, command).await?,
			CommandPayload::SubmitConversationTurn { .. } =>
				self.execute_submit_conversation_turn(runtime, command).await?,
			CommandPayload::InterruptConversation { .. } =>
				self.execute_interrupt_conversation(runtime, command).await?,
			_ => return Err(conversation_conflict()),
		};
		let (conversation_id, interrupt) = conversation_command_projection(outcome)?;
		let conversation_id = EntityId::new(conversation_id.as_str().to_owned())
			.map_err(|_| application_unavailable("Conversation projection is unavailable"))?;
		let ConversationResult::Available(conversation) =
			self.conversation_get(&conversation_id).await
		else {
			return Err(application_unavailable("Conversation projection is unavailable"));
		};
		conversation_command_publication(conversation, interrupt)
	}
}

impl Application for ServiceApplication {
	fn has_publication_source(&self) -> bool {
		self.conversations.runtime().is_some()
	}

	fn begin_shutdown(&self) {
		self.publication_stop.send_replace(true);
		if let Some(manager) = &self.account_login {
			manager.begin_shutdown();
		}
		if let Some(runtime) = self.conversations.runtime() {
			runtime.begin_shutdown();
		}
		if let Some(runtime) = &self.reset_cards {
			runtime.begin_shutdown();
		}
	}

	async fn wait_for_shutdown(&self) {
		if let Some(manager) = &self.account_login {
			manager.wait_for_shutdown().await;
		}
		if let Some(runtime) = self.conversations.runtime() {
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
		if let Some(chief) = &self.chief {
			tasks.push(Box::pin(chief.clone().serve(stop.clone())));
		}
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
		future::ready(self.command_independent_snapshot().unwrap_or_default())
	}

	fn command_independent_snapshot(&self) -> Option<Vec<SnapshotItem>> {
		Some(vec![SnapshotItem::SystemState {
			entity_id: EntityId::new("decodex-service").expect("service entity ID is bounded"),
			revision: EntityRevision(0),
			status: WireText::new("typed doctor/status is available through the daemon protocol")
				.expect("service status is bounded"),
		}])
	}

	async fn account_login<'a>(&'a self, request: &'a AccountLoginRequest) -> AccountLoginStatus {
		let Some(manager) = &self.account_login else {
			return crate::account_login::unavailable_status(request);
		};
		let Ok(runtime) = tokio::runtime::Handle::try_current() else {
			return crate::account_login::unavailable_status(request);
		};
		manager.handle(request, runtime).await
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		match &command.payload {
			CommandPayload::Chief { action } => {
				let chief = self
					.chief
					.as_ref()
					.ok_or_else(|| application_unavailable("Chief service is unavailable"))?;
				let id = chief
					.submit(command.idempotency_key.as_str().into(), *action.clone())
					.await
					.map_err(|error| match error {
						crate::chief_host::ChiefHostError::Rejected(reason) =>
							application_unavailable(reason),
						crate::chief_host::ChiefHostError::Unknown(_) =>
							CommandError::AcceptanceUnknown,
					})?;
				let work_id = EntityId::new(id)
					.map_err(|_| application_unavailable("invalid Chief identity"))?;
				Ok(ApplicationPublication {
					channel: Channel::ProjectWork,
					entity_id: work_id.clone(),
					entity_revision: EntityRevision(0),
					result: ResultPayload::ChiefAccepted { work_id: work_id.clone() },
					event: EventPayload::ChiefChanged { work_id },
				})
			},
			CommandPayload::SetDesktopSettings { .. } =>
				self.execute_desktop_settings(command).await,
			CommandPayload::CreateConversation { .. }
			| CommandPayload::ResumeConversationRouting { .. }
			| CommandPayload::CreateConversationRoutingSuccessor { .. }
			| CommandPayload::ResumeConversationEstablishment { .. }
			| CommandPayload::SubmitConversationTurn { .. }
			| CommandPayload::RefreshConversation { .. }
			| CommandPayload::ArchiveConversation { .. }
			| CommandPayload::InterruptConversation { .. } => self.execute_conversation(command).await,
			CommandPayload::EnrollAccountFromSharedCodex { .. }
			| CommandPayload::ImportAccountCredentialFile { .. }
			| CommandPayload::SetAccountEnabled { .. }
			| CommandPayload::LogoutAccount { .. }
			| CommandPayload::RouteAccount { .. }
			| CommandPayload::SetBalancedAccountSelection
			| CommandPayload::SetAccountOrder { .. }
			| CommandPayload::RefreshAccount { .. }
			| CommandPayload::RecoverAccountOperation { .. } => {
				let publication = self.execute_account_command(command).await?;
				self.invalidate_account_observation(&publication.entity_id).await;
				self.request_account_observation_refresh();
				Ok(publication)
			},
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
			QueryPayload::GetConversationCreationReceipt { request } =>
				QueryResultPayload::ConversationCreationReceipt(
					conversation_receipts::query_creation_receipt(&self.store, request).await,
				),
			QueryPayload::GetInitialModelCatalog { .. }
			| QueryPayload::GetChiefCapabilities
			| QueryPayload::GetConversationCapabilities { .. } => self.query_model_catalog(query).await,

			QueryPayload::ExchangeDictation { request } => self.query_dictation(request).await,
			QueryPayload::ExchangeChiefVoice { request } => self.query_voice(request),

			QueryPayload::GetChiefResources { work_id } =>
				QueryResultPayload::ChiefResources(match &self.chief {
					Some(chief) => chief.resources(work_id.as_str()).await,
					None => decodex_protocol::ChiefResourcesResult::Unavailable,
				}),

			QueryPayload::ExchangeMcpLogin { request } =>
				QueryResultPayload::McpLogin(query_mcp_login(self.chief.as_ref(), request).await),
			QueryPayload::GetChiefNativeGoal { work_id, thread_id } =>
				self.query_native_goal(work_id.as_str(), thread_id.as_str()).await,
			QueryPayload::GetChiefAppSettings { work_id, event_id } =>
				self.query_app_settings(work_id.as_str(), *event_id).await,
			QueryPayload::GetChiefSavedAppSettings { work_id } =>
				self.query_saved_app_settings(work_id.as_str()).await,
			QueryPayload::GetChiefHookSettings { work_id } =>
				self.query_hook_settings(work_id.as_str()).await,
			QueryPayload::GetChiefPluginSelection { work_id } =>
				self.query_plugin_selection(work_id.as_str()).await,
			QueryPayload::GetChiefModelSelection { work_id } =>
				self.query_model_selection(work_id.as_str()).await,
			QueryPayload::GetChiefPermissionProfiles { work_id } =>
				self.query_permission_profiles(work_id.as_str()).await,
			QueryPayload::GetChiefLiveReviewer { work_id } =>
				self.query_live_reviewer(work_id.as_str()).await,
			QueryPayload::GetChiefModelSettings { work_id } =>
				self.query_model_settings(work_id.as_str()).await,
			QueryPayload::GetChiefUsageEstimate { work_id } =>
				self.query_usage_estimate(work_id.as_str()).await,
			QueryPayload::GetChiefInputReceipts { work_id, after } =>
				self.query_input_receipts(work_id.as_str(), *after).await,
			QueryPayload::GetChiefSteerReceipt { identity } =>
				self.query_steer_receipt(identity).await,
			QueryPayload::GetChiefMedia { request } => self.query_media(request).await,
			QueryPayload::GetChiefTimeline { work_id, thread_id, cursor } =>
				self.query_timeline(
					work_id.as_str(),
					thread_id.as_str(),
					cursor.as_ref().map(|c| c.as_str()),
				)
				.await,
			QueryPayload::GetChiefIntegrations { work_id } =>
				self.query_integrations(work_id.as_str()).await,
			QueryPayload::GetChiefActivityDetail { work_id, turn_id, item_id, cursor } =>
				self.query_activity_detail(work_id, turn_id, item_id, cursor.as_ref()).await,
			QueryPayload::GetChiefRequest { event_id } => QueryResultPayload::ChiefRequest(
				query_chief_request_with_details(&self.store, *event_id, self.chief.as_ref()).await,
			),
			QueryPayload::WaitForChiefOutput { work_id, after_revision } =>
				self.query_output(work_id, *after_revision).await,
			QueryPayload::GetNativeAgents { work_id, thread_id, cursor } =>
				self.query_native_agents(work_id, thread_id.as_ref(), cursor.as_ref()).await,
			QueryPayload::GetChiefHistory { work_id, before } =>
				self.query_history(work_id.as_str(), *before).await,
			QueryPayload::GetChiefArchiveState { work_id } =>
				QueryResultPayload::ChiefArchiveState(match &self.chief {
					Some(chief) => chief.archive_state(work_id.as_str()).await,
					None => decodex_protocol::ChiefArchiveResult::Unavailable,
				}),
			QueryPayload::GetChiefInstallState { work_id, event_id } =>
				QueryResultPayload::ChiefInstallState(match &self.chief {
					Some(chief) => chief.install_state(work_id.as_str(), *event_id).await,
					None => decodex_protocol::ChiefInstallState::Unavailable,
				}),
			QueryPayload::GetChiefGuardianReviews { work_id, before } =>
				self.query_guardian_review_page(work_id.as_str(), *before).await,
			QueryPayload::GetChiefSnapshot => self.query_chief_snapshot().await,
			QueryPayload::GetDesktopSettings =>
				QueryResultPayload::DesktopSettings(self.desktop_settings().await),
			QueryPayload::ListPrograms => QueryResultPayload::Programs(self.program_list().await),
			QueryPayload::GetProgramCycle { program_id } =>
				QueryResultPayload::ProgramCycle(self.program_cycle(program_id).await),
			QueryPayload::ListConversations { after, page_size } =>
				QueryResultPayload::Conversations(
					self.conversation_list(after.as_ref(), page_size.get()).await,
				),
			QueryPayload::GetConversation { conversation_id } =>
				QueryResultPayload::Conversation(self.conversation_get(conversation_id).await),
			QueryPayload::GetDoctorStatus =>
				QueryResultPayload::DoctorStatus(self.refreshed_doctor().await),
			QueryPayload::GetConversationHistory { conversation_id, after, page_size } =>
				QueryResultPayload::ConversationHistory(
					self.conversation_history(conversation_id, after.as_ref(), *page_size).await,
				),
			QueryPayload::GetResetCards { account_id } =>
				QueryResultPayload::ResetCards(self.reset_card_inventory(account_id).await),
			QueryPayload::GetAccountResetCardOperation { account_id } =>
				QueryResultPayload::AccountResetCardOperation(
					self.account_reset_card_operation(account_id).await,
				),
			QueryPayload::GetResetCardOperation { idempotency_key } =>
				QueryResultPayload::ResetCardOperation(
					self.reset_card_operation(idempotency_key.as_str()).await,
				),
			QueryPayload::ListAccounts => QueryResultPayload::Accounts(self.account_list().await),
			QueryPayload::InspectAccount { account_id } =>
				QueryResultPayload::Account(self.account_inspect(account_id).await),
			QueryPayload::GetAccountProfile { account_id, include_email } =>
				QueryResultPayload::AccountProfile(
					self.account_profile(account_id, *include_email).await,
				),
			QueryPayload::GetInitialAccountSelection =>
				QueryResultPayload::InitialAccountSelection(self.initial_account_selection().await),
			QueryPayload::GetCodexAuthProjection =>
				QueryResultPayload::CodexAuthProjection(self.codex_auth_projection().await),
			QueryPayload::WaitForAccountObservation { after_generation, request_refresh } =>
				self.query_account_observation(*after_generation, *request_refresh == Some(true))
					.await,
		}
	}

	async fn next_publication(&self) -> Option<ApplicationEventPublication> {
		let runtime = self.conversations.runtime()?;
		loop {
			let outcome = runtime.next_event().await?;
			if let Some(publication) = self.conversation_event_publication(outcome).await {
				return Some(publication);
			}
		}
	}
}

fn conversation_read_error(error: &StoreError) -> ConversationReadError {
	match error {
		StoreError::InvalidInput(_) => ConversationReadError::InvalidRequest,
		StoreError::Incompatible(_) | StoreError::CredentialRejected =>
			ConversationReadError::IntegrityUnavailable,
		_ => ConversationReadError::ProductStateUnavailable,
	}
}

fn conversation_summary_from_row(
	row: OrdinaryTaskConversationReadback,
	projection: Option<ConversationProjection>,
) -> Result<ConversationSummary, ()> {
	let projection_updated_at_micros = row.updated_at_micros;
	let (title, codex_thread_id, program) = conversation_presentation(&row)?;
	if let Some(projection) = projection {
		let readback = &projection.readback;
		if readback.conversation_id != row.conversation_id
			|| readback.conversation_revision != Some(row.conversation_revision)
			|| readback.runtime_session_id.as_ref() != row.runtime_session_id.as_ref()
			|| readback.runtime_session_revision != row.runtime_session_revision
			|| readback.codex_thread_id.as_deref() != row.codex_thread_id.as_deref()
		{
			return Err(());
		}
		return conversation_summary_from_readback(
			projection.readback,
			projection.recovery,
			projection_updated_at_micros,
			title,
			codex_thread_id,
			program,
		);
	}
	if let Some(pre_session_state) = row.pre_session_state {
		let (state, recovery_action) = match pre_session_state {
			OrdinaryTaskPreSessionState::RoutingPending =>
				(ConversationState::RoutingPending, ConversationRecoveryAction::ResumeRouting),
			OrdinaryTaskPreSessionState::EstablishmentPending => (
				ConversationState::EstablishmentPending,
				ConversationRecoveryAction::ResumeEstablishment,
			),
			OrdinaryTaskPreSessionState::QuotaExhausted => (
				ConversationState::QuotaExhausted,
				ConversationRecoveryAction::CreateRoutingSuccessor,
			),
			OrdinaryTaskPreSessionState::NoRoute =>
				(ConversationState::NoRoute, ConversationRecoveryAction::CreateRoutingSuccessor),
		};
		return ConversationSummary::new(
			EntityId::new(row.conversation_id.as_str().to_owned()).map_err(|_| ())?,
			title,
			codex_thread_id,
			program,
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
			ConversationState::Establishing,
			row.active_turn_id,
			Some(ConversationRecoveryAction::ResumeEstablishment),
		)
	} else if row.has_unknown_provider_attempt {
		(ConversationState::OutcomeUnknown, row.active_turn_id, None)
	} else if row.has_active_provider_attempt {
		(
			ConversationState::ManualRecovery,
			row.active_turn_id,
			Some(ConversationRecoveryAction::ResolvePriorAttempt),
		)
	} else if row.active_turn_id.is_some() {
		(
			ConversationState::ManualRecovery,
			row.active_turn_id,
			Some(ConversationRecoveryAction::ResolvePriorActiveTurn),
		)
	} else {
		match runtime_session_state {
			RuntimeSessionState::Starting => (
				ConversationState::ManualRecovery,
				None,
				Some(ConversationRecoveryAction::StartNewConversation),
			),
			RuntimeSessionState::Active if row.has_acknowledged_turn =>
				(ConversationState::Ready, None, None),
			RuntimeSessionState::Active => (
				ConversationState::ManualRecovery,
				None,
				Some(ConversationRecoveryAction::StartNewConversation),
			),
			RuntimeSessionState::Ended | RuntimeSessionState::Diverged => (
				ConversationState::ManualRecovery,
				None,
				Some(ConversationRecoveryAction::StartNewConversation),
			),
		}
	};
	ConversationSummary::new(
		EntityId::new(row.conversation_id.as_str().to_owned()).map_err(|_| ())?,
		title,
		codex_thread_id,
		program,
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

fn conversation_summary_from_readback(
	readback: ConversationReadback,
	recovery: Option<ConversationManualRecovery>,
	projection_updated_at_micros: i64,
	title: ConversationTitle,
	codex_thread_id: Option<ProviderThreadId>,
	program: Option<ConversationProgramContext>,
) -> Result<ConversationSummary, ()> {
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
		ConversationLocalState::RoutingPending => ConversationState::RoutingPending,
		ConversationLocalState::EstablishmentPending => ConversationState::EstablishmentPending,
		ConversationLocalState::QuotaExhausted => ConversationState::QuotaExhausted,
		ConversationLocalState::NoRoute => ConversationState::NoRoute,
		ConversationLocalState::Establishing => ConversationState::Establishing,
		ConversationLocalState::Ready => ConversationState::Ready,
		ConversationLocalState::Running => ConversationState::Running,
		ConversationLocalState::ManualRecovery => ConversationState::ManualRecovery,
		ConversationLocalState::OutcomeUnknown => ConversationState::OutcomeUnknown,
	};
	ConversationSummary::new(
		EntityId::new(readback.conversation_id.as_str().to_owned()).map_err(|_| ())?,
		title,
		codex_thread_id,
		program,
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
			ConversationState::RoutingPending => Some(ConversationRecoveryAction::ResumeRouting),
			ConversationState::EstablishmentPending =>
				Some(ConversationRecoveryAction::ResumeEstablishment),
			ConversationState::QuotaExhausted | ConversationState::NoRoute =>
				Some(ConversationRecoveryAction::CreateRoutingSuccessor),
			_ => recovery.map(conversation_recovery_action),
		},
	)
	.map_err(|_| ())
}

fn conversation_presentation(
	row: &OrdinaryTaskConversationReadback,
) -> Result<(ConversationTitle, Option<ProviderThreadId>, Option<ConversationProgramContext>), ()> {
	let title = ConversationTitle::new(row.title.clone()).map_err(|_| ())?;
	let codex_thread_id =
		row.codex_thread_id.clone().map(ProviderThreadId::new).transpose().map_err(|_| ())?;
	let program = row
		.program_work_item
		.as_ref()
		.map(|context| {
			ConversationProgramContext::new(
				EntityId::new(context.program_id.as_str().to_owned()).map_err(|_| ())?,
				EntityId::new(context.work_item_id.as_str().to_owned()).map_err(|_| ())?,
				title.clone(),
				WireText::new(context.instructions.clone()).map_err(|_| ())?,
				context.state,
				EntityRevision(context.revision),
			)
			.map_err(|_| ())
		})
		.transpose()?;
	Ok((title, codex_thread_id, program))
}

const fn conversation_recovery_action(
	action: ConversationManualRecovery,
) -> ConversationRecoveryAction {
	match action {
		ConversationManualRecovery::WaitForThreadClose =>
			ConversationRecoveryAction::WaitForThreadClose,
		ConversationManualRecovery::RestoreArchivedThread =>
			ConversationRecoveryAction::RestoreArchivedThread,
		ConversationManualRecovery::ReviewSandboxConfiguration =>
			ConversationRecoveryAction::ReviewSandboxConfiguration,
		ConversationManualRecovery::ReviewCodexConfiguration =>
			ConversationRecoveryAction::ReviewCodexConfiguration,
		ConversationManualRecovery::EnableAccount => ConversationRecoveryAction::EnableAccount,
		ConversationManualRecovery::EnrollCredentials =>
			ConversationRecoveryAction::EnrollCredentials,
		ConversationManualRecovery::ResolveAccountOperation =>
			ConversationRecoveryAction::ResolveAccountOperation,
		ConversationManualRecovery::RepairCredentialStore =>
			ConversationRecoveryAction::RepairCredentialStore,
		ConversationManualRecovery::RestoreProviderAgreement =>
			ConversationRecoveryAction::RestoreProviderAgreement,
		ConversationManualRecovery::RefreshQuota => ConversationRecoveryAction::RefreshQuota,
		ConversationManualRecovery::SelectedAccountDrift =>
			ConversationRecoveryAction::StartNewConversation,
		ConversationManualRecovery::SelectedAccountReadiness =>
			ConversationRecoveryAction::ConfigureAccount,
		ConversationManualRecovery::UpgradeCodex => ConversationRecoveryAction::UpgradeCodex,
		ConversationManualRecovery::SelectWorkingDirectory =>
			ConversationRecoveryAction::SelectWorkingDirectory,
		ConversationManualRecovery::PriorActiveTurn =>
			ConversationRecoveryAction::ResolvePriorActiveTurn,
		ConversationManualRecovery::PriorAttemptUnresolved =>
			ConversationRecoveryAction::ResolvePriorAttempt,
		ConversationManualRecovery::ProcessUnavailable =>
			ConversationRecoveryAction::RestoreProcessReadiness,
		ConversationManualRecovery::MissingLocalProcess
		| ConversationManualRecovery::MissingThread
		| ConversationManualRecovery::IncompatibleThread =>
			ConversationRecoveryAction::StartNewConversation,
	}
}

const fn conversation_conflict() -> CommandError {
	CommandError::ConversationRecoveryRequired {
		action: ConversationRecoveryAction::RefreshConversation,
	}
}

const fn conversation_busy() -> CommandError {
	CommandError::ConversationRecoveryRequired {
		action: ConversationRecoveryAction::WaitForCurrentCommand,
	}
}

fn conversation_command_projection(
	outcome: ConversationOutcome,
) -> Result<(ConversationId, bool), CommandError> {
	let (readback, interrupt) = match outcome {
		ConversationOutcome::PreSession(readback)
		| ConversationOutcome::Started { readback, .. }
		| ConversationOutcome::Terminal { readback, .. } => (readback, false),
		ConversationOutcome::InterruptRequested(readback) => (readback, true),
		ConversationOutcome::ManualRecovery { action, .. } => {
			return Err(CommandError::ConversationRecoveryRequired {
				action: conversation_recovery_action(action),
			});
		},
		ConversationOutcome::Unknown { .. } => return Err(CommandError::AcceptanceUnknown),
		ConversationOutcome::Busy(_) => return Err(conversation_busy()),
		ConversationOutcome::Conflict => return Err(conversation_conflict()),
		ConversationOutcome::Streaming { .. } | ConversationOutcome::Unavailable => {
			return Err(application_unavailable("Conversation execution is unavailable"));
		},
	};
	Ok((readback.conversation_id, interrupt))
}

fn conversation_command_publication(
	conversation: ConversationSummary,
	interrupt: bool,
) -> Result<ApplicationPublication, CommandError> {
	let entity_id = conversation.conversation_id.clone();
	let entity_revision = conversation.conversation_revision;
	let result = if interrupt {
		ResultPayload::ConversationInterruptAccepted { conversation: conversation.clone() }
	} else {
		ResultPayload::ConversationAccepted { conversation: conversation.clone() }
	};
	Ok(ApplicationPublication {
		channel: Channel::ConversationStream,
		entity_id,
		entity_revision,
		result,
		event: EventPayload::ConversationChanged { conversation },
	})
}

fn runtime_execution_settings(
	settings: &ConversationExecutionSettingsDto,
) -> RuntimeConversationExecutionSettings {
	RuntimeConversationExecutionSettings {
		model: settings.model.as_str().to_owned(),
		reasoning_effort: settings
			.reasoning_effort
			.as_ref()
			.map(|effort| effort.as_str().to_owned()),
		fast: settings.fast,
		service_tier: settings.effective_service_tier(),
	}
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
	provider_threads: &HashMap<ConversationId, ProviderThreadId>,
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

	append_historical_semantics(
		record.signals,
		record.claims,
		record.proposals,
		record.objectives,
		&program.program_id,
		&mut nodes,
		&mut edges,
	)?;
	append_historical_executions(
		record.work_items,
		run_states,
		provider_threads,
		&mut nodes,
		&mut edges,
	)?;
	append_historical_evidence(
		record.evidence,
		record.reviews,
		&program.program_id,
		&mut nodes,
		&mut edges,
	)?;

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

fn append_historical_semantics(
	signals: Vec<decodex_database::ProgramSignalRecord>,
	claims: Vec<decodex_database::ProgramClaimRecord>,
	proposals: Vec<decodex_database::ProgramProposalRecord>,
	objectives: Vec<decodex_database::ProgramObjectiveRecord>,
	program_id: &EntityId,
	nodes: &mut Vec<ProgramNodeDto>,
	edges: &mut Vec<ProgramEdgeDto>,
) -> Result<(), ()> {
	for signal in signals {
		let signal_id = entity(signal.signal_id.as_str())?;
		let (from, kind) = match signal.predecessor_review_id {
			Some(review_id) => (entity(review_id.as_str())?, ProgramRelationKind::Continues),
			None => (program_id.clone(), ProgramRelationKind::Observes),
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
	for claim in claims {
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
	for proposal in proposals {
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
	for objective in objectives {
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

	Ok(())
}

fn append_historical_executions(
	work_items: Vec<decodex_database::ProgramWorkItemRecord>,
	run_states: &[(ConversationId, &'static str)],
	provider_threads: &HashMap<ConversationId, ProviderThreadId>,
	nodes: &mut Vec<ProgramNodeDto>,
	edges: &mut Vec<ProgramEdgeDto>,
) -> Result<(), ()> {
	for work_item in work_items {
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
				title: wire("Codex Conversation")?,
				summary: wire("Execution through the existing Codex app-server worker path")?,
				state: wire(state)?,
				source: provider_threads
					.get(&conversation_id)
					.map(|thread_id| {
						thread_id.codex_url().map_err(|_| ()).and_then(|url| wire(url.to_string()))
					})
					.transpose()?,
				observed_at_micros: None,
				conversation_id: Some(run_id),
				fields: Vec::new(),
			});
		}
	}

	Ok(())
}

fn append_historical_evidence(
	evidence: Vec<decodex_database::ProgramEvidenceRecord>,
	reviews: Vec<decodex_database::ProgramReviewRecord>,
	program_id: &EntityId,
	nodes: &mut Vec<ProgramNodeDto>,
	edges: &mut Vec<ProgramEdgeDto>,
) -> Result<(), ()> {
	for evidence in evidence {
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
				decodex_core::ProgramEvidenceKind::DeterministicValidation =>
					"Deterministic validation",
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
	for review in reviews {
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
			to: program_id.clone(),
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

	Ok(())
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

const fn conversation_state_text(state: ConversationState) -> &'static str {
	match state {
		ConversationState::RoutingPending => "routing_pending",
		ConversationState::EstablishmentPending => "establishment_pending",
		ConversationState::QuotaExhausted => "quota_exhausted",
		ConversationState::NoRoute => "no_route",
		ConversationState::Establishing => "establishing",
		ConversationState::Ready => "ready",
		ConversationState::Running => "running",
		ConversationState::ManualRecovery => "manual_recovery",
		ConversationState::OutcomeUnknown => "outcome_unknown",
	}
}

fn conversation_routing_successor_publication(
	source_conversation_id: &ConversationId,
	source_revision: i64,
	successor_conversation_id: &ConversationId,
	successor_revision: i64,
	successor: ConversationSummary,
) -> Result<ApplicationPublication, CommandError> {
	let successor_revision =
		EntityRevision(u64::try_from(successor_revision).map_err(|_| conversation_conflict())?);
	if successor.conversation_id.as_str() != successor_conversation_id.as_str()
		|| successor.conversation_revision != successor_revision
	{
		return Err(conversation_conflict());
	}
	let source_conversation_id = EntityId::new(source_conversation_id.as_str().to_owned())
		.map_err(|_| conversation_conflict())?;
	let source_conversation_revision =
		EntityRevision(u64::try_from(source_revision).map_err(|_| conversation_conflict())?);
	Ok(ApplicationPublication {
		channel: Channel::ConversationStream,
		entity_id: successor.conversation_id.clone(),
		entity_revision: successor.conversation_revision,
		result: ResultPayload::ConversationRoutingSuccessorAccepted {
			source_conversation_id,
			source_conversation_revision,
			successor: successor.clone(),
		},
		event: EventPayload::ConversationChanged { conversation: successor },
	})
}

impl ServiceApplication {
	async fn conversation_event_publication(
		&self,
		outcome: ConversationOutcome,
	) -> Option<ApplicationEventPublication> {
		match outcome {
			ConversationOutcome::Streaming { readback, history_item_id, text } => {
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
					event: EventPayload::ConversationMessageDelta {
						conversation_id,
						turn_id,
						delta: text,
					},
				})
			},
			ConversationOutcome::Terminal { readback, turn_id, state, .. } => {
				let conversation = self.conversation_event_summary(&readback).await?;
				conversation_terminal_publication(readback, turn_id, state, conversation)
			},
			ConversationOutcome::Unknown { readback, .. } => {
				let conversation = self.conversation_event_summary(&readback).await?;
				conversation_summary_publication(readback, conversation, "unknown")
			},
			ConversationOutcome::ManualRecovery { readback, .. } => {
				let conversation = self.conversation_event_summary(&readback).await?;
				conversation_summary_publication(readback, conversation, "recovery")
			},
			ConversationOutcome::PreSession(_)
			| ConversationOutcome::Started { .. }
			| ConversationOutcome::Busy(_)
			| ConversationOutcome::Conflict
			| ConversationOutcome::InterruptRequested(_)
			| ConversationOutcome::Unavailable => None,
		}
	}

	async fn conversation_event_summary(
		&self,
		readback: &ConversationReadback,
	) -> Option<ConversationSummary> {
		let conversation_id = EntityId::new(readback.conversation_id.as_str().to_owned()).ok()?;
		match self.conversation_get(&conversation_id).await {
			ConversationResult::Available(summary) => Some(summary),
			_ => None,
		}
	}
}

fn conversation_terminal_publication(
	readback: ConversationReadback,
	turn_id: TurnId,
	state: ConversationTerminalState,
	conversation: ConversationSummary,
) -> Option<ApplicationEventPublication> {
	let mut publication = conversation_summary_publication(readback, conversation, "terminal")?;
	let EventPayload::ConversationChanged { conversation } = publication.event else {
		return None;
	};
	publication.event = EventPayload::ConversationTurnFinished {
		conversation,
		turn_id: EntityId::new(turn_id.as_str().to_owned()).ok()?,
		outcome: match state {
			ConversationTerminalState::Succeeded => ConversationTurnOutcome::Succeeded,
			ConversationTerminalState::Failed => ConversationTurnOutcome::Failed,
		},
	};
	Some(publication)
}

fn conversation_summary_publication(
	readback: ConversationReadback,
	conversation: ConversationSummary,
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
		event: EventPayload::ConversationChanged { conversation },
	})
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
		AccountProfileRuntimeError::AccountUnavailable =>
			AccountProfileErrorDto::AccountUnavailable,
		AccountProfileRuntimeError::ProductStateUnavailable =>
			AccountProfileErrorDto::ProductStateUnavailable,
		AccountProfileRuntimeError::CredentialUnavailable =>
			AccountProfileErrorDto::CredentialUnavailable,
		AccountProfileRuntimeError::CredentialBusy => AccountProfileErrorDto::CredentialBusy,
		AccountProfileRuntimeError::RefreshRejected => AccountProfileErrorDto::RefreshRejected,
		AccountProfileRuntimeError::RefreshAmbiguous => AccountProfileErrorDto::RefreshAmbiguous,
		AccountProfileRuntimeError::AccessRejectedAfterRefresh =>
			AccountProfileErrorDto::AccessRejectedAfterRefresh,
		AccountProfileRuntimeError::Unauthorized => AccountProfileErrorDto::Unauthorized,
		AccountProfileRuntimeError::ProviderUnavailable =>
			AccountProfileErrorDto::ProviderUnavailable,
		AccountProfileRuntimeError::ProtocolUnavailable =>
			AccountProfileErrorDto::ProtocolUnavailable,
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
		CommandPayload::SetAccountEnabled { account_id, .. } =>
			(AccountCommandKind::SetEnabled, account_id.as_str()),
		CommandPayload::LogoutAccount { account_id, .. } =>
			(AccountCommandKind::Logout, account_id.as_str()),
		CommandPayload::RouteAccount { .. } => (AccountCommandKind::Route, "account-routing"),
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
		CommandPayload::EnrollAccountFromSharedCodex { operation_id, account_id, .. } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
		},
		CommandPayload::ImportAccountCredentialFile {
			operation_id,
			account_id,
			source_descriptor,
			..
		} => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
			let source = source_descriptor.as_str();
			if source.is_empty() || source.len() > 4096 || source.chars().any(char::is_control) {
				return Err(account_rejection(AccountCommandRejectionDto::InvalidRequest, None));
			}
		},
		CommandPayload::SetAccountEnabled { account_id, .. } => {
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::LogoutAccount { operation_id, account_id }
		| CommandPayload::RefreshAccount { operation_id, account_id } => {
			let _ = operation_id_from_wire(operation_id)?;
			let _ = account_id_from_wire(account_id)?;
			let _ = required_expected_revision(command)?;
		},
		CommandPayload::RouteAccount { account_id } => {
			let _ = account_id_from_wire(account_id)?;
			if command.expected_revision.is_some() {
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

pub(crate) fn account_changed_publication(
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

pub(crate) fn account_enrollment_publication(
	requested_account_id: &AccountId,
	account: AccountRecord,
) -> Result<ApplicationPublication, CommandError> {
	if &account.account_id == requested_account_id {
		return account_changed_publication(account);
	}
	let requested_account_id = EntityId::new(requested_account_id.as_str().to_owned())
		.map_err(|_| application_unavailable("account enrollment result is incompatible"))?;
	let account = account_dto(account)
		.map_err(|_| application_unavailable("account enrollment result is incompatible"))?;
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id: account.account_id.clone(),
		entity_revision: account.account_revision,
		result: ResultPayload::AccountRestored {
			requested_account_id,
			account: Box::new(account.clone()),
		},
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

fn account_routed_publication(
	commit: AccountRouteCommit,
) -> Result<ApplicationPublication, CommandError> {
	let account = account_dto(commit.account)
		.map_err(|_| application_unavailable("account Route result is incompatible"))?;
	let routing = routing_dto(commit.routing)
		.map_err(|_| application_unavailable("account Route result is incompatible"))?;
	if routing.mode != decodex_protocol::AccountSelectionModeDto::Fixed(account.account_id.clone())
	{
		return Err(application_unavailable("account Route result is incompatible"));
	}
	let projection_digest = Sha256Digest::new(commit.projection_digest)
		.map_err(|_| application_unavailable("account Route result is incompatible"))?;
	let entity_id = EntityId::new("account-routing").expect("account routing entity is bounded");
	Ok(ApplicationPublication {
		channel: Channel::AccountsHealth,
		entity_id,
		entity_revision: routing.revision,
		result: ResultPayload::AccountRouted {
			account: Box::new(account.clone()),
			routing: routing.clone(),
			projection_digest: projection_digest.clone(),
		},
		event: EventPayload::AccountRouted {
			account: Box::new(account),
			routing,
			projection_digest,
		},
	})
}

fn account_route_result(
	result: Result<AccountRouteResult, AccountRouteFailure>,
) -> Result<ApplicationPublication, CommandError> {
	match result {
		Ok(AccountRouteResult::Committed(commit)) => account_routed_publication(*commit),
		Err(AccountRouteFailure::Lifecycle(error)) => Err(account_route_command_error(error)),
		Err(AccountRouteFailure::Routing(outcome)) => match routing_command_result(&outcome) {
			Err(error) => Err(error),
			Ok(_) => Err(application_unavailable("account Route result is incompatible")),
		},
	}
}

fn account_route_command_error(error: AccountLifecycleError) -> CommandError {
	match error {
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned)
		| AccountLifecycleError::CredentialAbsent =>
			account_rejection(AccountCommandRejectionDto::CredentialMissing, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreMismatch)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled) =>
			account_rejection(AccountCommandRejectionDto::CredentialNeedsLogin, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreUnavailable)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::CallbackCapabilityUnready)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::Ready) =>
			account_rejection(AccountCommandRejectionDto::CredentialRefreshUnavailable, None),
		other => account_lifecycle_command_error(other),
	}
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

pub(crate) fn account_lifecycle_command_error(error: AccountLifecycleError) -> CommandError {
	match error {
		AccountLifecycleError::OperationRejected(rejection) => lifecycle_rejection(rejection, 0),
		AccountLifecycleError::AccountMissing =>
			account_rejection(AccountCommandRejectionDto::AccountNotFound, None),
		AccountLifecycleError::CredentialAbsent =>
			account_rejection(AccountCommandRejectionDto::CredentialMissing, None),
		AccountLifecycleError::ProviderMismatch =>
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None),
		AccountLifecycleError::StaleAccount =>
			account_rejection(AccountCommandRejectionDto::StaleAccount, None),
		AccountLifecycleError::InvalidOperation =>
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled) =>
			account_rejection(AccountCommandRejectionDto::OperationUnsettled, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent) =>
			account_rejection(AccountCommandRejectionDto::CredentialMissing, None),
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch) =>
			account_rejection(AccountCommandRejectionDto::ProviderMismatch, None),
		AccountLifecycleError::NotReady(_) =>
			account_rejection(AccountCommandRejectionDto::LifecycleUnready, None),
		AccountLifecycleError::AccountDisabled =>
			account_rejection(AccountCommandRejectionDto::AccountDisabled, None),
		AccountLifecycleError::CredentialStore(CredentialStoreError::DuplicateProvider) =>
			account_rejection(AccountCommandRejectionDto::ProviderAlreadyEnrolled, None),
		AccountLifecycleError::CredentialStore(_) =>
			account_rejection(AccountCommandRejectionDto::CredentialStoreUnavailable, None),
		AccountLifecycleError::CredentialImport =>
			account_rejection(AccountCommandRejectionDto::InvalidRequest, None),
		AccountLifecycleError::CodexIsRunning =>
			account_rejection(AccountCommandRejectionDto::CodexIsRunning, None),
		AccountLifecycleError::AuthFileUnreadable =>
			account_rejection(AccountCommandRejectionDto::AuthFileUnreadable, None),
		AccountLifecycleError::AuthFileChanged =>
			account_rejection(AccountCommandRejectionDto::AuthFileChanged, None),
		AccountLifecycleError::AuthWriteFailed =>
			account_rejection(AccountCommandRejectionDto::AuthWriteFailed, None),
		AccountLifecycleError::AuthReadbackMismatch =>
			account_rejection(AccountCommandRejectionDto::AuthReadbackMismatch, None),
		AccountLifecycleError::Refresh(crate::CredentialRefreshError::OwnerBusy) =>
			account_rejection(AccountCommandRejectionDto::CodexIsRunning, None),
		AccountLifecycleError::Refresh(crate::CredentialRefreshError::Rejected) =>
			account_rejection(AccountCommandRejectionDto::CredentialRefreshRejected, None),
		AccountLifecycleError::Refresh(crate::CredentialRefreshError::Unavailable) =>
			account_rejection(AccountCommandRejectionDto::CredentialRefreshUnavailable, None),
		AccountLifecycleError::Refresh(crate::CredentialRefreshError::Ambiguous) =>
			account_rejection(AccountCommandRejectionDto::CredentialNeedsLogin, None),
		AccountLifecycleError::Persistence(_) | AccountLifecycleError::CoordinatorUnavailable =>
			application_unavailable("account service is unavailable"),
	}
}

fn account_operation_command_error(_error: AccountLifecycleError) -> CommandError {
	// Every deterministic account rejection after reservation is encoded into the durable receipt.
	// An escaped error therefore means that the atomic operation/receipt boundary did not finish.
	CommandError::AcceptanceUnknown
}

pub(crate) fn map_account_store_command_error(error: StoreError) -> CommandError {
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
			result: Box::new(publication.result.clone()),
			event: Box::new(publication.event.clone()),
		},
		Err(error) => StoredAccountCommandOutcome::Rejected {
			schema: ACCOUNT_COMMAND_RECEIPT_SCHEMA.to_owned(),
			error: error.clone(),
		},
	}
}

pub(crate) fn encode_account_command_receipt(
	result: &Result<ApplicationPublication, CommandError>,
) -> Result<serde_json::Value, StoreError> {
	serde_json::to_value(stored_account_command_outcome(result))
		.map_err(|_| StoreError::Incompatible("account command result is incompatible".into()))
}

pub(crate) fn decode_account_command_receipt(
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
				result: *result,
				event: *event,
			})),
		StoredAccountCommandOutcome::Rejected { schema, error }
			if schema == ACCOUNT_COMMAND_RECEIPT_SCHEMA =>
			Ok(Err(error)),
		_ => Err(()),
	}
}

fn quota_dto(observation: AccountQuotaWindowObservation) -> Result<AccountQuotaWindowDto, ()> {
	let (observed_at_unix_micros, result) = match observation.disposition {
		AccountQuotaDisposition::Unknown => (None, AccountQuotaStateDto::Unknown),
		AccountQuotaDisposition::NotApplicable =>
			(observation.observed_at_unix_micros, AccountQuotaStateDto::NotApplicable),
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

async fn query_chief_request_with_details(
	store: &ProductStore,
	event_id: i64,
	chief: Option<&crate::chief_host::ChiefHost>,
) -> decodex_protocol::ChiefRequestResult {
	use decodex_protocol::ChiefRequestResult;
	let request = query_chief_request(store, event_id).await;
	if !matches!(&request, ChiefRequestResult::Available { method, .. } if method == "item/fileChange/requestApproval")
	{
		return request;
	}
	let (Some(chief), ProductStore::Available(database)) = (chief, store) else {
		return request;
	};
	let Ok(event) = database.get_chief_inbox_event(event_id).await else {
		return ChiefRequestResult::Unavailable;
	};
	let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) else {
		return ChiefRequestResult::Unavailable;
	};
	let params = &payload["params"];
	let (Some(thread), Some(turn), Some(item)) =
		(params["threadId"].as_str(), params["turnId"].as_str(), params["itemId"].as_str())
	else {
		return request;
	};
	let detail = chief.file_approval_detail(thread, turn, item).await;
	// The native read can overlap a completed turn or a resolved request.
	if query_chief_request(store, event_id).await != request {
		return ChiefRequestResult::Unavailable;
	}
	attach_file_approval_detail(request, detail)
}

fn attach_file_approval_detail(
	mut request: decodex_protocol::ChiefRequestResult,
	detail: decodex_protocol::ChiefActivityDetailResult,
) -> decodex_protocol::ChiefRequestResult {
	if let decodex_protocol::ChiefRequestResult::Available { method, request_json, .. } =
		&mut request
		&& method == "item/fileChange/requestApproval"
		&& let decodex_protocol::ChiefActivityDetailResult::Available { text, truncated, .. } =
			detail
		&& let Ok(mut fields) = serde_json::from_str::<serde_json::Value>(request_json.as_str())
	{
		fields["changeDetails"] = serde_json::json!(text);
		fields["changeDetailsTruncated"] = serde_json::json!(truncated);
		if let Ok(updated) = decodex_protocol::HistoryText::new(fields.to_string()) {
			*request_json = updated;
		}
	}
	request
}

impl ServiceApplication {
	async fn query_model_catalog(&self, query: &QueryEnvelope) -> QueryResultPayload {
		match &query.payload {
			QueryPayload::GetInitialModelCatalog { request } =>
				QueryResultPayload::InitialModelCatalog(match self.conversations.runtime() {
					Some(runtime) =>
						runtime
							.initial_model_catalog(query.query_id.as_str(), request.clone())
							.await,
					None => decodex_protocol::InitialModelCatalogResult::Unavailable,
				}),
			QueryPayload::GetChiefCapabilities =>
				QueryResultPayload::ChiefCapabilities(match &self.chief {
					Some(chief) => chief.capabilities().await,
					None => decodex_protocol::ChiefCapabilitiesResult::Unavailable,
				}),
			QueryPayload::GetConversationCapabilities { conversation_id } =>
				QueryResultPayload::ConversationCapabilities(match self.conversations.runtime() {
					Some(runtime) => runtime.model_capabilities(conversation_id.as_str()).await,
					None => decodex_protocol::ChiefCapabilitiesResult::Unavailable,
				}),
			_ => unreachable!("model catalog query dispatched above"),
		}
	}

	async fn query_account_observation(
		&self,
		after_generation: u64,
		request_refresh: bool,
	) -> QueryResultPayload {
		if request_refresh {
			self.request_account_observation_refresh();
		}
		QueryResultPayload::AccountObservation(match self.account_observations.as_ref() {
			Some(observations) => observations.wait_for_change(after_generation).await,
			None => AccountObservationService::heartbeat(after_generation).await,
		})
	}
}

async fn query_mcp_login(
	chief: Option<&crate::chief_host::ChiefHost>,
	request: &decodex_protocol::McpLoginRequest,
) -> decodex_protocol::McpLoginStatus {
	match chief {
		Some(chief) => chief.mcp_login(request).await,
		None => crate::mcp_login::status(
			request,
			decodex_protocol::McpLoginPhase::Disconnected,
			"Chief is not connected.",
		),
	}
}

async fn query_guardian_reviews(
	store: &ProductStore,
	chief: Option<&crate::chief_host::ChiefHost>,
	work: &str,
	before: Option<i64>,
) -> decodex_protocol::ChiefGuardianReviewsResult {
	match store {
		ProductStore::Available(store) =>
			crate::chief_guardian::read(
				store,
				work,
				before,
				chief.and_then(|host| host.guardian_generation()),
			)
			.await,
		ProductStore::Unavailable(_) => decodex_protocol::ChiefGuardianReviewsResult::Unavailable,
	}
}

fn chief_request_metadata(value: &serde_json::Value) -> Option<serde_json::Value> {
	let meta = value.as_object()?;
	let selected_meta: serde_json::Map<String, serde_json::Value> = meta
		.iter()
		.filter(|(key, _)| {
			[
				"codex_approval_kind",
				"persist",
				"connector_name",
				"tool_name",
				"tool_title",
				"tool_description",
				"tool_params",
				"tool_params_display",
				"tool_type",
				"suggest_type",
				"tool_id",
				"suggestion_id",
				"install_url",
				"remote_plugin_id",
				"app_connector_ids",
			]
			.contains(&key.as_str())
		})
		.map(|(key, value)| (key.clone(), value.clone()))
		.collect();
	Some(serde_json::Value::Object(selected_meta))
}

async fn query_chief_request(
	store: &ProductStore,
	event_id: i64,
) -> decodex_protocol::ChiefRequestResult {
	use decodex_protocol::ChiefRequestResult;
	let ProductStore::Available(store) = store else {
		return ChiefRequestResult::Unavailable;
	};
	let Ok(event) = store.get_chief_inbox_event(event_id).await else {
		return ChiefRequestResult::Unavailable;
	};
	if event.disposition.is_some()
		|| !matches!(
			event.event_kind.as_str(),
			"permission_pending" | "user_input_pending" | "server_request_pending"
		) {
		return ChiefRequestResult::Unavailable;
	}
	let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) else {
		return ChiefRequestResult::Unavailable;
	};
	let Some(params) = payload["params"].as_object() else {
		return ChiefRequestResult::Unavailable;
	};
	let Ok(work) = store.get_chief_work_item(event.work_item_id.clone()).await else {
		return ChiefRequestResult::Unavailable;
	};
	let standalone_elicitation = payload["method"] == "mcpServer/elicitation/request"
		&& params.get("turnId").is_none_or(serde_json::Value::is_null);
	if work.codex_thread_id.is_none()
		|| params.get("threadId").and_then(serde_json::Value::as_str)
			!= work.codex_thread_id.as_deref()
		|| (!standalone_elicitation
			&& (work.dispatch_state != decodex_database::ChiefDispatchState::Running
				|| work.active_turn_id.is_none()
				|| params.get("turnId").and_then(serde_json::Value::as_str)
					!= work.active_turn_id.as_deref()))
	{
		return ChiefRequestResult::Unavailable;
	}

	let Some(method) = payload["method"].as_str() else {
		return ChiefRequestResult::Unavailable;
	};
	let keys: &[&str] = match method {
		"item/commandExecution/requestApproval" => &[
			"kind",
			"command",
			"cwd",
			"reason",
			"availableDecisions",
			"additionalPermissions",
			"networkApprovalContext",
			"proposedExecpolicyAmendment",
			"proposedNetworkPolicyAmendments",
		],
		"item/fileChange/requestApproval" => &["reason", "grantRoot"],
		"item/permissions/requestApproval" => &["cwd", "reason", "permissions", "environmentId"],
		"item/tool/requestUserInput" => &["questions", "isBlocking"],
		"mcpServer/elicitation/request" => &[
			"serverName",
			"mode",
			"message",
			"requestedSchema",
			"url",
			"elicitationId",
			"title",
			"description",
			"_meta",
		],
		_ => return ChiefRequestResult::Unavailable,
	};
	let mut selected = serde_json::Map::new();
	for key in keys {
		if let Some(value) = params.get(*key) {
			if *key == "_meta" {
				if let Some(meta) = chief_request_metadata(value) {
					selected.insert("_meta".into(), meta);
				}
				continue;
			}
			let valid = match *key {
				"kind" => matches!(value.as_str(), Some("command" | "writeStdin")),
				"command" | "cwd" | "reason" | "grantRoot" | "environmentId" =>
					value.is_null() || value.is_string(),
				"serverName" | "mode" | "message" | "url" | "elicitationId" | "title"
				| "description" => value.is_string(),
				// OpenAI form schemas are opaque JSON. Preserve unsupported shapes so
				// the client can offer decline/cancel instead of hiding the request.
				"requestedSchema" => true,
				"questions" => value.is_array(),
				"isBlocking" => value.is_boolean(),
				"availableDecisions"
				| "proposedExecpolicyAmendment"
				| "proposedNetworkPolicyAmendments" => value.is_null() || value.is_array(),
				"additionalPermissions" | "networkApprovalContext" =>
					value.is_null() || value.is_object(),
				"permissions" => value.is_object(),
				_ => false,
			};
			if !valid {
				return ChiefRequestResult::Unavailable;
			}
			selected.insert((*key).into(), value.clone());
		}
	}
	if method == "item/commandExecution/requestApproval" && !selected.contains_key("kind") {
		selected.insert("kind".into(), serde_json::json!("command"));
	}
	let Ok(request_json) =
		decodex_protocol::HistoryText::new(serde_json::Value::Object(selected).to_string())
	else {
		return ChiefRequestResult::Unavailable;
	};
	ChiefRequestResult::Available {
		event_id,
		work_id: event.work_item_id,
		method: method.into(),
		request_json,
	}
}

fn chief_user_message_text(value: &serde_json::Value) -> String {
	let raw = value["text"].as_str().unwrap_or("");
	let mut text = decodex_protocol::render_chief_async_question_history(raw);
	if let Some(files) = value.pointer("/options/attachments").and_then(serde_json::Value::as_array)
	{
		for file in files.iter().take(16) {
			if let Some(path) = file["path"].as_str() {
				text.push_str("\n\nAttached: ");
				text.push_str(path);
			}
		}
	}
	text
}

#[cfg(test)]
async fn query_chief_history(
	store: &ProductStore,
	id: &str,
) -> decodex_protocol::ChiefHistoryResult {
	query_chief_history_page(store, id, None).await
}

fn chief_history_notice(kind: &str, value: &serde_json::Value) -> String {
	if kind == "steer_pending" {
		return "Steer delivery is unconfirmed. Inspect the current response before sending again."
			.into();
	}
	let detail = value["recovery"].as_str().unwrap_or("Inspect persisted work before retrying.");
	if kind == "reconnection_needs_attention" {
		detail.to_owned()
	} else {
		format!("{kind}: {detail}")
	}
}

fn chief_assistant_history(
	value: &serde_json::Value,
	has_more: &mut bool,
) -> (&'static str, String) {
	let messages = value.pointer("/threadReadback/assistantMessages");
	let parsed = messages
		.and_then(serde_json::Value::as_str)
		.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
	let messages = parsed.as_ref().or(messages);
	let text = messages
		.and_then(serde_json::Value::as_array)
		.map(|items| {
			items.iter().filter_map(|item| item["text"].as_str()).collect::<Vec<_>>().join("\n\n")
		})
		.unwrap_or_default();
	let status = value.pointer("/terminal/turn/status").and_then(serde_json::Value::as_str);
	if text.is_empty() {
		if status == Some("interrupted") {
			return ("stopped", "Stopped".into());
		}
		return (
			"execution_notice",
			match status {
				Some("failed") => "Execution failed before a response was produced.",
				_ => "Execution ended without a recoverable response.",
			}
			.into(),
		);
	}
	let mut text = text;
	if matches!(status, Some("interrupted" | "failed")) {
		text.push_str(if status == Some("interrupted") {
			"\n\n*Stopped.*"
		} else {
			"\n\n*Execution failed.*"
		});
	}
	if value.pointer("/threadReadback/truncated").and_then(serde_json::Value::as_bool) == Some(true)
	{
		*has_more = true;
		text.insert_str(0, "[Assistant output truncated in saved history.]\n\n");
	}
	("assistant", text)
}

fn completed_chief_history(
	value: &serde_json::Value,
	has_more: &mut bool,
	pending_retry: Option<&decodex_database::ChiefCapacityRetry>,
	event_id: i64,
	completed_message_ids: &mut Vec<(String, String)>,
) -> (&'static str, String) {
	let messages = value.pointer("/threadReadback/assistantMessages");
	let parsed = messages
		.and_then(serde_json::Value::as_str)
		.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
	let messages = parsed.as_ref().or(messages);
	let turn =
		value.pointer("/threadReadback/turnId").and_then(serde_json::Value::as_str).unwrap_or("");
	if let Some(items) = messages.and_then(serde_json::Value::as_array) {
		completed_message_ids.extend(
			items
				.iter()
				.filter_map(|item| Some((turn.to_owned(), item["id"].as_str()?.to_owned()))),
		);
	}
	let (message_kind, mut text) = chief_assistant_history(value, has_more);
	// Capacity handling is process state, not an assistant response. Keep the
	// original provider error in the persisted event rather than stacking it
	// with a contradictory instruction to change models during an active retry.
	if message_kind == "execution_notice" && value.get("capacityRetry").is_some() {
		if let Some(retry) = pending_retry.filter(|retry| retry.event_id == event_id) {
			return (
				"capacity_retry_pending",
				format!("Model busy · retry {}/3 scheduled automatically.", retry.attempt),
			);
		}
		let notice = if value.pointer("/capacityRetry/cancelled") == Some(&serde_json::json!(true))
		{
			"Model busy · automatic retry cancelled."
		} else if value.pointer("/capacityRetry/exhausted") == Some(&serde_json::json!(true)) {
			"Model still busy after 3 retries. Try again later or choose another model."
		} else {
			"Model was busy · automatic retry requested."
		};
		return ("execution_notice", notice.into());
	}

	for path in
		["/terminal/turn/error/message", "/terminal/turn/error/misalignment/detailedExplanation"]
	{
		if let Some(detail) = value
			.pointer(path)
			.and_then(serde_json::Value::as_str)
			.filter(|text| !text.trim().is_empty())
		{
			text.push_str("\n\n");
			text.push_str(detail);
		}
	}
	if value.pointer("/capacityRetry/cancelled") == Some(&serde_json::json!(true)) {
		text.insert_str(0, "Automatic capacity retry cancelled.\n\n");
	}
	if value.pointer("/capacityRetry/exhausted") == Some(&serde_json::json!(true)) {
		text.insert_str(0, "Automatic capacity retries exhausted (3/3).\n\n");
	}
	if let Some(retry) = pending_retry.filter(|retry| retry.event_id == event_id) {
		text.insert_str(
			0,
			&format!("Model capacity retry {}/3 is pending on the same model.\n\n", retry.attempt),
		);
		("capacity_retry_pending", text)
	} else {
		(message_kind, text)
	}
}

async fn query_chief_input_receipts(
	store: &ProductStore,
	work: &str,
	after: Option<i64>,
) -> decodex_protocol::ChiefInputReceiptsResult {
	use decodex_protocol::ChiefInputReceiptsResult as Result;
	let ProductStore::Available(store) = store else {
		return Result::Unavailable;
	};
	let Ok(events) = store.read_chief_unconfirmed_inputs(work.into(), after, 33).await else {
		return Result::Unavailable;
	};
	let Ok(work_id) = decodex_protocol::EntityId::new(work) else {
		return Result::Unavailable;
	};
	let total = events.len();
	let mut entries = Vec::new();
	let mut remaining = 60 * 1024usize;
	let mut shortened = false;
	for event in events.into_iter().take(32) {
		let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.payload) else {
			return Result::Unavailable;
		};
		let (kind, text) = if event.event_kind == "work_instruction" {
			("instruction", value["text"].as_str().unwrap_or_default().to_owned())
		} else {
			("user", chief_user_message_text(&value))
		};
		let mut entry = chief_history_entry(&event, &value, kind, String::new());
		let Ok(metadata) = serde_json::to_vec(&entry) else {
			return Result::Unavailable;
		};
		if metadata.len() + 4 >= remaining {
			break;
		}
		let (text, trimmed) = bound_chief_text(text, (remaining - metadata.len() - 4).min(8192));
		shortened |= trimmed;
		entry.text = text;
		let Ok(encoded) = serde_json::to_vec(&entry) else {
			return Result::Unavailable;
		};
		remaining = remaining.saturating_sub(encoded.len() + 1);
		entries.push(entry);
	}
	let next_after =
		(entries.len() < total).then(|| entries.last().map(|entry| entry.id)).flatten();
	Result::Available { work_id, entries, next_after, shortened }
}

async fn query_chief_history_page(
	store: &ProductStore,
	id: &str,
	before: Option<i64>,
) -> decodex_protocol::ChiefHistoryResult {
	use decodex_protocol::ChiefHistoryResult;
	let ProductStore::Available(store) = store else {
		return ChiefHistoryResult::Unavailable;
	};
	let Ok((events, partial)) = store.read_chief_transcript(id.into(), before, 33).await else {
		return ChiefHistoryResult::Unavailable;
	};
	let Ok(precaution) = store.chief_misalignment(id.into()).await else {
		return ChiefHistoryResult::Unavailable;
	};
	let misalignment = precaution
		.map(|saved| {
			let details: serde_json::Value = saved
				.details_json
				.as_deref()
				.and_then(|value| serde_json::from_str(value).ok())
				.unwrap_or_default();
			decodex_protocol::ChiefMisalignmentDto {
				review_id: saved.review_id(),
				explanation: details["detailedExplanation"]
					.as_str()
					.filter(|text| !text.trim().is_empty() && text.len() <= 65536)
					.map(str::to_owned),
				continuation: details
					.pointer("/steer/message")
					.and_then(serde_json::Value::as_str)
					.filter(|text| !text.trim().is_empty() && text.len() <= 1024)
					.map(str::to_owned),
			}
		})
		.map(Box::new);
	let Ok(questions_recovering) = store.chief_async_questions_recovering(id.into()).await else {
		return ChiefHistoryResult::Unavailable;
	};
	let Ok(pending_questions) = store.read_chief_async_questions(id.into()).await else {
		return ChiefHistoryResult::Unavailable;
	};
	let mut questions = Vec::new();
	let mut questions_truncated = false;
	let mut question_bytes = 2;
	for pending in pending_questions {
		let Ok(mut question) =
			serde_json::from_str::<decodex_protocol::ChiefAsyncQuestionDto>(&pending.question_json)
		else {
			return ChiefHistoryResult::Unavailable;
		};
		question.arrived_live = pending.arrived_live;
		let cost = serde_json::to_vec(&question).expect("serializable question").len() + 1;
		if questions.len() >= 32
			|| question_bytes + cost > decodex_protocol::MAX_HISTORY_INLINE_BYTES
		{
			questions_truncated = true;
			break;
		}
		question_bytes += cost;
		questions.push(question);
	}
	let pending_retry = store.pending_chief_capacity_retry(id.into()).await.ok().flatten();
	let RenderedChiefHistory { entries, has_more, next_before } =
		render_chief_history(events, question_bytes, pending_retry);
	let live = query_chief_live(partial);
	ChiefHistoryResult::Available {
		questions,
		questions_truncated,
		questions_recovering,
		misalignment,
		entries,
		has_more,
		next_before,
		live,
		usage: store
			.read_chief_usage(id.into())
			.await
			.ok()
			.flatten()
			.and_then(|json| serde_json::from_str(&json).ok()),
	}
}

struct RenderedChiefHistory {
	entries: Vec<decodex_protocol::ChiefHistoryEntryDto>,
	has_more: bool,
	next_before: Option<i64>,
}

fn render_chief_history(
	events: Vec<decodex_database::ChiefInboxEvent>,
	question_bytes: usize,
	pending_retry: Option<decodex_database::ChiefCapacityRetry>,
) -> RenderedChiefHistory {
	let older_available = events.len() > 32;
	let mut has_more = older_available;
	let mut rendered_messages = std::collections::HashSet::<(String, String)>::new();
	let mut entries = Vec::new();
	let mut remaining = 64 * 1024 - question_bytes;
	let mut page_full = false;
	for event in events.into_iter().rev().take(32) {
		let value: serde_json::Value = serde_json::from_str(&event.payload).unwrap_or_default();
		let mut completed_message_ids = Vec::new();
		let (kind, mut text) = match event.event_kind.as_str() {
			"config_warning" => ("execution_notice", value["text"].as_str().unwrap_or("Codex reported a configuration warning.").to_owned()),
			"strict_review_notice" => ("execution_notice", "Codex requested additional safety checks for this turn. Tool calls may take longer; no action is required for this notice.".into()),
			"activity_started" | "activity_completed" => ("activity", String::new()),
            "user_message" if event.disposition == Some(decodex_database::ChiefDisposition::UserDecision) && event.delivered_turn_id.is_none() =>
                ("unsent_input", chief_user_message_text(&value)),
			"user_message" | "async_question_answer" | "voice_user" =>
				("user", chief_user_message_text(&value)),
			"voice_assistant" => ("assistant", chief_user_message_text(&value)),

			"work_instruction" => ("instruction", value["text"].as_str().unwrap_or("").to_owned()),
			"assistant_message" => {
				if let (Some(turn), Some(item)) =
					(value["turnId"].as_str(), value["item"]["id"].as_str())
					&& rendered_messages.contains(&(turn.to_owned(), item.to_owned()))
				{
					continue;
				}
				has_more |= value["truncated"] == true;
				("assistant", value["item"]["text"].as_str().unwrap_or("").to_owned())
			},
			"context_compacted" => ("system", "Codex compacted the thread context.".into()),
			"chief_turn_completed" | "worker_turn_completed" | "capacity_retry" =>
				completed_chief_history(
					&value,
					&mut has_more,
					pending_retry.as_ref(),
					event.id,
					&mut completed_message_ids,
				),
			"automation_result" =>
				("automation", value.as_str().unwrap_or(&event.payload).to_owned()),
			"steer_pending"
			| "configuration_needs_attention"
			| "reconnection_needs_attention"
			| "recovery_needs_attention"
			| "wake_failed"
			| "event_processing_failed"
			| "followup_processing_failed"
			| "connection_needs_attention" => ("system", chief_history_notice(&event.event_kind, &value)),
			_ => ("system", event.event_kind.clone()),
		};
		if let Some(note) =
			event.disposition_note.as_ref().filter(|_| {
				kind == "unsent_input"
					|| !matches!(
						event.event_kind.as_str(),
						"chief_turn_completed"
							| "worker_turn_completed"
							| "user_message" | "voice_user"
							| "voice_assistant" | "activity_started"
							| "activity_completed"
							| "assistant_message" | "context_compacted"
							| "strict_review_notice"
							| "config_warning"
					)
			}) {
			text.push_str(if kind == "unsent_input" { "\n\n" } else { "\n\nDisposition: " });
			text.push_str(note);
		}
		if event.event_kind == "user_message" {
			append_task_reference_labels(&mut text, &value);
		}
		let activity_cost = if kind == "activity" { event.payload.len() } else { 0 }
			+ serde_json::to_vec(&chief_history_receipt(&event)).map_or(remaining, |v| v.len());
		if serde_json::to_vec(&text).map_or(usize::MAX, |encoded| encoded.len())
			+ 160 + activity_cost
			> remaining
			&& !entries.is_empty()
		{
			page_full = true;
			has_more = true;
			break;
		}
		let (bounded, shortened) =
			bound_chief_text(text, remaining.saturating_sub(160 + activity_cost));
		has_more |= shortened;
		text = bounded;
		remaining = remaining.saturating_sub(
			serde_json::to_vec(&text).map_or(remaining, |encoded| encoded.len())
				+ 160 + activity_cost,
		);

		if !shortened
			&& value.pointer("/threadReadback/truncated") != Some(&serde_json::json!(true))
		{
			rendered_messages.extend(completed_message_ids);
		}
		entries.push(chief_history_entry(&event, &value, kind, text));
		if remaining == 0 {
			has_more = true;
			break;
		}
	}
	entries.reverse();
	let next_before = if older_available || remaining == 0 || page_full {
		entries.first().map(|entry| entry.id)
	} else {
		None
	};
	RenderedChiefHistory { entries, has_more, next_before }
}

fn append_task_reference_labels(text: &mut String, value: &serde_json::Value) {
	let Ok(references) = serde_json::from_value::<Vec<decodex_protocol::ChiefTaskReferenceDto>>(
		value.pointer("/options/taskReferences").cloned().unwrap_or(serde_json::Value::Null),
	) else {
		return;
	};
	if references.is_empty() {
		return;
	}
	text.push_str("\n\nReferenced tasks: ");
	for (index, reference) in references.iter().take(16).enumerate() {
		if index > 0 {
			text.push_str(", ");
		}
		text.push('@');
		for character in reference.title.as_str().chars().take(160) {
			if character.is_ascii_punctuation() {
				text.push('\\');
			}
			text.push(if character.is_control() { ' ' } else { character });
		}
	}
}

#[cfg(test)]
mod task_reference_display_tests {
	#[test]
	fn referenced_titles_are_literal_history_data_and_empty_options_leave_text_alone() {
		let mut text = "Prompt".to_owned();
		super::append_task_reference_labels(
			&mut text,
			&serde_json::json!({"options":{"taskReferences":[
				{"workId":"target","threadId":"thread","title":"[Title](https://example.test)\nnext"}
			]}}),
		);
		assert!(text.starts_with("Prompt\n\nReferenced tasks: @"));
		assert!(text.contains("\\[Title\\]"));
		assert!(!text.contains(")\nnext"));
		let before = text.clone();
		super::append_task_reference_labels(&mut text, &serde_json::json!({}));
		assert_eq!(text, before);
	}
}

fn chief_history_entry(
	event: &decodex_database::ChiefInboxEvent,
	value: &serde_json::Value,
	kind: &str,
	text: String,
) -> decodex_protocol::ChiefHistoryEntryDto {
	decodex_protocol::ChiefHistoryEntryDto {
		turn_id: value
			.pointer("/threadReadback/turnId")
			.and_then(serde_json::Value::as_str)
			.map(str::to_owned),
		weather: Vec::new(),
		receipt: chief_history_receipt(event),
		activity: if event.event_kind.starts_with("activity_") {
			serde_json::from_value(value.clone()).ok()
		} else {
			None
		},
		usage: serde_json::from_value(value["usage"].clone()).ok().or_else(|| {
			let usage: decodex_codex::ThreadTokenUsage =
				serde_json::from_value(value.pointer("/threadReadback/tokenUsage")?.clone())
					.ok()?;
			usage.is_valid().then_some(decodex_protocol::ChiefTurnUsageDto {
				input_tokens: usage.last.input_tokens,
				output_tokens: usage.last.output_tokens,
			})
		}),
		duration_ms: value.pointer("/terminal/turn/durationMs").and_then(serde_json::Value::as_u64),
		id: event.id,
		kind: kind.into(),
		text,
		created_at_micros: event.created_at_micros,
	}
}

fn chief_history_receipt(
	event: &decodex_database::ChiefInboxEvent,
) -> Option<decodex_protocol::ChiefHistoryReceiptDto> {
	if event.event_kind.is_empty()
		|| event.event_kind.len() > 80
		|| event.delivered_turn_id.as_ref().is_some_and(|id| id.len() > 512)
	{
		return None;
	}
	Some(decodex_protocol::ChiefHistoryReceiptDto {
		event_kind: event.event_kind.clone(),
		delivered_turn_id: event.delivered_turn_id.clone().filter(|id| !id.is_empty()),
		disposed: event.disposition.is_some(),
	})
}

#[cfg(test)]
mod history_receipt_tests {
	#[test]
	fn refused_input_stays_visible_with_attachments_without_calling_unknown_input_unsent() {
		let mut event = decodex_database::ChiefInboxEvent {
            id: 1, source_event_id: "input".into(), work_item_id: "work".into(), event_kind: "user_message".into(),
            payload: serde_json::json!({"text":"Keep this input","options":{"attachments":[{"path":"/tmp/retained.txt","image":false}]}}).to_string(),
            created_at_micros: 1, disposition: Some(decodex_database::ChiefDisposition::UserDecision),
            disposition_note: Some("Not sent: the local connection queue is full.".into()), disposed_at_micros: Some(2), delivered_turn_id: None,
        };
		let rows = super::render_chief_history(vec![event.clone()], 0, None).entries;
		assert_eq!(rows[0].kind, "unsent_input");
		assert!(rows[0].text.contains("Keep this input"));
		assert!(rows[0].text.contains("Attached: /tmp/retained.txt"));
		assert!(rows[0].text.contains("Not sent: the local connection queue is full."));
		for turn in ["", "acknowledged"] {
			event.delivered_turn_id = Some(turn.into());
			let rows = super::render_chief_history(vec![event.clone()], 0, None).entries;
			assert_eq!(rows[0].kind, "user", "a claimed or acknowledged turn is not known-unsent");
			assert!(!rows[0].text.contains("Not sent:"));
		}
	}

	#[test]
	fn disposition_does_not_imply_native_delivery_and_opaque_ids_are_not_truncated() {
		let mut event = decodex_database::ChiefInboxEvent {
			id: 1,
			source_event_id: "source".into(),
			work_item_id: "work".into(),
			event_kind: "async_question_answer".into(),
			payload: "{}".into(),
			created_at_micros: 1,
			disposition: None,
			disposition_note: None,
			disposed_at_micros: None,
			delivered_turn_id: None,
		};
		let entry =
			super::chief_history_entry(&event, &serde_json::json!({}), "user", "Answer".into());
		let receipt = entry.receipt.unwrap();
		assert_eq!(receipt.event_kind, "async_question_answer");
		assert!(!receipt.disposed);
		assert!(receipt.delivered_turn_id.is_none());
		// A claimed or uncertain dispatch uses an empty native turn fence in the store.
		event.delivered_turn_id = Some(String::new());
		let claimed =
			super::chief_history_entry(&event, &serde_json::json!({}), "user", "Answer".into())
				.receipt
				.expect("uncertain input stays visible");
		assert!(claimed.delivered_turn_id.is_none() && !claimed.disposed);
		event.disposition = Some(decodex_database::ChiefDisposition::Resolved);
		let receipt = super::chief_history_receipt(&event).unwrap();
		assert!(receipt.disposed);
		assert!(receipt.delivered_turn_id.is_none());
		event.delivered_turn_id = Some("native-turn".into());
		let receipt = super::chief_history_receipt(&event).unwrap();
		assert_eq!(receipt.delivered_turn_id.as_deref(), Some("native-turn"));
		event.delivered_turn_id = Some("x".repeat(513));
		assert!(super::chief_history_receipt(&event).is_none());
	}
}

fn bound_chief_text(mut text: String, encoded_budget: usize) -> (String, bool) {
	if serde_json::to_vec(&text).is_ok_and(|encoded| encoded.len() <= encoded_budget) {
		return (text, false);
	}
	let mut low = 0;
	let mut high = text.len();
	while low < high {
		let middle = low + (high - low).div_ceil(2);
		let mut end = middle;
		while !text.is_char_boundary(end) {
			end -= 1;
		}
		if serde_json::to_vec(&text[..end]).is_ok_and(|encoded| encoded.len() <= encoded_budget) {
			low = middle;
		} else {
			high = middle - 1;
		}
	}
	while !text.is_char_boundary(low) {
		low -= 1;
	}
	text.truncate(low);
	(text, true)
}

fn query_chief_live(
	partial: Vec<decodex_database::ChiefLiveOutput>,
) -> Vec<decodex_protocol::ChiefLiveMessageDto> {
	let mut live = Vec::new();
	let mut budget = 65536usize;
	for output in partial {
		if budget == 0 {
			break;
		}
		let metadata = serde_json::to_vec(&(&output.turn_id, &output.item_id))
			.map_or(budget, |encoded| encoded.len())
			+ 160;
		if metadata >= budget {
			break;
		}
		let (text, shortened) = bound_chief_text(output.text, budget - metadata);
		let truncated = output.truncated || shortened;
		budget = budget.saturating_sub(
			serde_json::to_vec(&text).map_or(budget, |encoded| encoded.len()) + metadata,
		);

		live.push(decodex_protocol::ChiefLiveMessageDto {
			turn_id: output.turn_id,
			item_id: output.item_id,
			text,
			truncated,
		});
	}
	live
}

async fn query_chief_snapshot(store: &ProductStore) -> decodex_protocol::ChiefSnapshotResult {
	use decodex_database::{
		ChiefDispatchState, ChiefStoreSnapshot, ChiefWorkKind, ChiefWorkStatus,
	};
	use decodex_protocol::{
		ChiefDependencyDto, ChiefDispatchStateDto, ChiefPendingEventDto, ChiefSnapshotDto,
		ChiefSnapshotResult, ChiefWorkItemDto, ChiefWorkKindDto, ChiefWorkStatusDto,
		MAX_CHIEF_DEPENDENCIES, MAX_CHIEF_PENDING_EVENTS, MAX_CHIEF_WORK_ITEMS,
	};
	let ProductStore::Available(store) = store else {
		return ChiefSnapshotResult::Unavailable;
	};
	let records = match store
		.read_chief_snapshot(MAX_CHIEF_WORK_ITEMS, MAX_CHIEF_DEPENDENCIES, MAX_CHIEF_PENDING_EVENTS)
		.await
	{
		Ok(records) => records,
		Err(_) => return ChiefSnapshotResult::Unavailable,
	};
	let (work_items, dependencies, pending_events, managers, workspaces) = match records {
		ChiefStoreSnapshot::CapacityExceeded { work_items, dependencies, pending_events } => {
			return ChiefSnapshotResult::CapacityExceeded {
				work_items,
				dependencies,
				pending_events,
			};
		},
		ChiefStoreSnapshot::Complete {
			work_items,
			dependencies,
			pending_events,
			managers,
			workspaces,
		} => (work_items, dependencies, pending_events, managers, workspaces),
	};
	let counts = (work_items.len() as u64, dependencies.len() as u64, pending_events.len() as u64);
	let snapshot = ChiefSnapshotDto {
		runtime_source: None,
		workspaces: workspaces
			.into_iter()
			.map(|(chief_id, name, directory)| decodex_protocol::ChiefWorkspaceDto {
				chief_id,
				name,
				directory,
			})
			.collect(),
		work_items: work_items
			.into_iter()
			.map(|item| ChiefWorkItemDto {
				id: item.id.clone(),
				parent_goal_id: item.parent_goal_id.clone(),
				kind: match item.kind {
					ChiefWorkKind::Goal =>
						if item.parent_goal_id.is_some() && managers.contains(&item.id) {
							ChiefWorkKindDto::Manager
						} else {
							ChiefWorkKindDto::Goal
						},
					ChiefWorkKind::Task => ChiefWorkKindDto::Task,
				},
				title: item.title,
				codex_thread_id: item.codex_thread_id,
				active_turn_id: item.active_turn_id,
				dispatch_state: match item.dispatch_state {
					ChiefDispatchState::Idle => ChiefDispatchStateDto::Idle,
					ChiefDispatchState::Dispatching => ChiefDispatchStateDto::Dispatching,
					ChiefDispatchState::Running => ChiefDispatchStateDto::Running,
					ChiefDispatchState::Unknown => ChiefDispatchStateDto::Unknown,
				},
				status: match item.status {
					ChiefWorkStatus::Open => ChiefWorkStatusDto::Open,
					ChiefWorkStatus::Resolved => ChiefWorkStatusDto::Resolved,
					ChiefWorkStatus::FollowUp => ChiefWorkStatusDto::FollowUp,
					ChiefWorkStatus::Wait => ChiefWorkStatusDto::Wait,
					ChiefWorkStatus::UserDecision => ChiefWorkStatusDto::UserDecision,
				},
				next_check_at_micros: item.next_check_at_micros,
				created_at_micros: item.created_at_micros,
				updated_at_micros: item.updated_at_micros,
			})
			.collect(),
		dependencies: dependencies
			.into_iter()
			.map(|edge| ChiefDependencyDto {
				work_item_id: edge.work_item_id,
				depends_on_id: edge.depends_on_id,
			})
			.collect(),
		pending_events: pending_events
			.into_iter()
			.map(|event| ChiefPendingEventDto {
				id: event.id,
				source_event_id: event.source_event_id,
				work_item_id: event.work_item_id,
				event_kind: event.event_kind,
				created_at_micros: event.created_at_micros,
				delivery_claimed: event.delivered_turn_id.is_some(),
			})
			.collect(),
	};
	if serde_json::to_vec(&snapshot)
		.is_ok_and(|bytes| bytes.len() > decodex_protocol::MAX_CHIEF_SNAPSHOT_BYTES)
	{
		return ChiefSnapshotResult::CapacityExceeded {
			work_items: counts.0,
			dependencies: counts.1,
			pending_events: counts.2,
		};
	}
	if snapshot.is_valid() {
		ChiefSnapshotResult::Available(snapshot)
	} else {
		ChiefSnapshotResult::Unavailable
	}
}

fn desktop_settings_dto(settings: StoreDesktopSettings) -> Result<DesktopSettingsDto, ()> {
	let revision = u64::try_from(settings.revision).map(EntityRevision).map_err(|_| ())?;
	let mut dto = DesktopSettingsDto::new(settings.show_in_menu_bar, revision).map_err(|_| ())?;
	dto.auto_activate_quota = settings.auto_activate_quota;
	Ok(dto)
}

fn desktop_settings_command_error(error: StoreError) -> CommandError {
	match error {
		StoreError::RevisionConflict { expected: Some(expected), actual: Some(actual), .. } =>
			match (u64::try_from(expected), u64::try_from(actual)) {
				(Ok(expected), Ok(actual)) => CommandError::ExpectedRevisionMismatch {
					expected: EntityRevision(expected),
					actual: EntityRevision(actual),
				},
				_ => application_unavailable("desktop settings revision is invalid"),
			},
		_ => application_unavailable("desktop settings store is unavailable"),
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
		ResetCardServiceError::SchemaUnsupported =>
			"stored reset-card result is incompatible with the current provider API",
		ResetCardServiceError::ProviderUnavailable => "reset-card provider is unavailable",
		ResetCardServiceError::InventoryIncomplete => "reset-card inventory is incomplete",
		ResetCardServiceError::InventoryChanged => "selected reset card changed",
		ResetCardServiceError::RequestTimedOut => "reset-card provider observation timed out",
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
	#[test]
	fn async_reply_history_is_readable_without_interpreting_partial_envelopes() {
		let question = decodex_protocol::ChiefAsyncQuestionDto {
			arrived_live: false,
			id: "question-1".into(),
			title: "Which region?".into(),
			options: vec![],
		};
		let reply = decodex_protocol::chief_async_question_reply(&question, "Europe").unwrap();
		assert_eq!(
			super::chief_user_message_text(&serde_json::json!({"text": reply.as_str()})),
			"> Which region?\n\nEurope"
		);
		let quoted = format!("An example: {}", reply.as_str());
		assert_eq!(super::chief_user_message_text(&serde_json::json!({"text": quoted})), quoted);
	}
	use crate::account_launch::{ResetCardFailureCode, ResetCardOperationStatus};
	use decodex_core::{
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
		AccountOperationPhase, AccountOperationStatus, AccountProvider, AccountQuotaDisposition,
		AccountQuotaWindow, AccountQuotaWindowObservation, AccountRecord, AccountState,
		ConversationId, DecodexRoot, ProgramObservationId, ProgramReviewId, ProviderIdentity,
		RuntimeSessionState,
	};
	use decodex_database::{
		AccountLifecycleRejection, AccountProfileDailyUsage, AccountProfileSnapshot,
		OrdinaryTaskConversationReadback, OrdinaryTaskPreSessionState, ProgramCycleRecord,
		SqliteStore,
	};
	use decodex_protocol::{
		AccountCommandRejectionDto, AccountProfileEmailDto, AccountQuotaStateDto, CommandError,
		ConversationRecoveryAction, ConversationState, ProgramNodeKind, ProgramRelationKind,
		ResetCardError, ResetCardOperationResult,
	};
	use std::collections::HashMap;

	use super::{
		ACCOUNT_COMMAND_RECEIPT_SCHEMA, AccountLifecycleError, AccountProfileClaimsView,
		AccountProfileRuntimeError, AccountProfileView, ProductStore, ResetCardServiceError,
		StoredAccountCommandOutcome, account_dto, account_lifecycle_command_error,
		account_profile_dto, account_profile_unavailable_dto, conversation_summary_from_row,
		decode_account_command_receipt, encode_account_command_receipt, lifecycle_rejection,
		operation_query_result, program_cycle_dto, protocol_reset_error, quota_dto,
	};

	#[tokio::test]
	async fn chief_read_projection_uses_store_and_excludes_private_content() {
		use decodex_database::{
			ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus, EnqueueChiefEvent,
		};
		use decodex_protocol::ChiefSnapshotResult;
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		let ChiefSnapshotResult::Available(empty) = super::query_chief_snapshot(&owner).await
		else {
			panic!("empty store must be available");
		};
		assert!(empty.work_items.is_empty());
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "goal".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Inspect real work".into(),
				instructions: "private instructions marker".into(),
				codex_thread_id: None,
				dispatch_state: ChiefDispatchState::Idle,
				active_turn_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "source:1".into(),
				work_item_id: "goal".into(),
				event_kind: "automation_result".into(),
				payload: "private provider payload marker".into(),
			})
			.await
			.unwrap();
		let ChiefSnapshotResult::Available(snapshot) = super::query_chief_snapshot(&owner).await
		else {
			panic!("real work must be available");
		};
		assert_eq!(snapshot.work_items.len(), 1);
		assert_eq!(snapshot.pending_events.len(), 1);
		assert_eq!(snapshot.pending_events[0].source_event_id, "source:1");
		assert!(snapshot.is_valid());
		let encoded = serde_json::to_string(&snapshot).unwrap();
		assert!(!encoded.contains("private instructions"));
		assert!(!encoded.contains("private provider"));
		assert!(!encoded.contains("payload"));
		store.close();
		assert_eq!(super::query_chief_snapshot(&owner).await, ChiefSnapshotResult::Unavailable);
	}

	async fn chief_query_work(store: &SqliteStore, id: &str) {
		use decodex_database::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};
		store
			.create_chief_work_item(ChiefWorkItem {
				id: id.into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: id.into(),
				instructions: "private instructions".into(),
				codex_thread_id: None,
				dispatch_state: ChiefDispatchState::Idle,
				active_turn_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn unconfirmed_input_pages_survive_history_eviction_restart_and_delivery_changes() {
		use decodex_database::{ChiefDisposition, EnqueueChiefEvent};
		use decodex_protocol::{ChiefHistoryResult, ChiefInputReceiptsResult as Receipts};
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		chief_query_work(&store, "chosen").await;
		chief_query_work(&store, "peer").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		let mut ids = Vec::new();
		for n in 0..40 {
			let text = if n == 39 { "界".repeat(10000) } else { format!("Input {n}") };
			ids.push(
				store
					.enqueue_chief_event(EnqueueChiefEvent {
						source_event_id: format!("input-{n}"),
						work_item_id: "chosen".into(),
						event_kind: ["user_message", "async_question_answer", "work_instruction"]
							[n % 3]
							.into(),
						payload: serde_json::json!({"text":text}).to_string(),
					})
					.await
					.unwrap()
					.id,
			);
		}
		store.begin_chief_dispatch_with_events("chosen".into(), vec![ids[0]]).await.unwrap();
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "peer-input".into(),
				work_item_id: "peer".into(),
				event_kind: "user_message".into(),
				payload: serde_json::json!({"text":"Peer input"}).to_string(),
			})
			.await
			.unwrap();
		for n in 0..50 {
			store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("output-{n}"),
					work_item_id: "chosen".into(),
					event_kind: "assistant_message".into(),
					payload: serde_json::json!({"item":{"text":"Newer output"}}).to_string(),
				})
				.await
				.unwrap();
		}
		drop(store);
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(reopened.clone());
		let ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history")
		};
		assert!(entries.iter().all(|entry| entry.kind != "user"));
		let Receipts::Available { entries, next_after, shortened, .. } =
			super::query_chief_input_receipts(&owner, "chosen", None).await
		else {
			panic!("pending inputs")
		};
		assert_eq!(entries.iter().map(|entry| entry.id).collect::<Vec<_>>(), ids[..32]);
		assert_eq!(next_after, Some(ids[31]));
		assert!(!shortened);
		assert!(
			entries[0]
				.receipt
				.as_ref()
				.is_some_and(|receipt| receipt.delivered_turn_id.is_none() && !receipt.disposed)
		);
		let second = super::query_chief_input_receipts(&owner, "chosen", next_after).await;
		assert!(serde_json::to_vec(&second).unwrap().len() < 64 * 1024);
		let Receipts::Available { entries, next_after, shortened, .. } = second else {
			panic!("second page")
		};
		assert_eq!(entries.iter().map(|entry| entry.id).collect::<Vec<_>>(), ids[32..]);
		assert!(next_after.is_none() && shortened);
		assert!(entries.last().unwrap().text.ends_with('界'));
		// A separate client acknowledges/disposes records; a fresh query must remove both.
		let other = reopened.clone();
		other.acknowledge_chief_dispatch("chosen".into(), "accepted-turn".into()).await.unwrap();
		other
			.dispose_chief_event(ids[1], ChiefDisposition::Resolved, "Handled".into(), None)
			.await
			.unwrap();
		let Receipts::Available { entries, .. } =
			super::query_chief_input_receipts(&owner, "chosen", None).await
		else {
			panic!("fresh inputs")
		};
		assert!(entries.iter().all(|entry| entry.id != ids[0] && entry.id != ids[1]));
		assert_eq!(entries[0].id, ids[2]);
		assert_eq!(
			super::query_chief_input_receipts(&owner, "chosen", Some(0)).await,
			Receipts::Unavailable
		);
		assert_eq!(
			super::query_chief_input_receipts(&owner, "missing", None).await,
			Receipts::Unavailable
		);
	}

	#[tokio::test]
	async fn capacity_retry_history_exposes_only_the_current_cancellable_event() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "chosen").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chosen".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chosen".into(), "turn".into()).await.unwrap();
		let event=store.complete_chief_turn_with_event("chosen".into(),"turn".into(),decodex_database::EnqueueChiefEvent {
			source_event_id:"capacity".into(),work_item_id:"chosen".into(),event_kind:"chief_turn_completed".into(),
			payload:serde_json::json!({"terminal":{"turn":{"status":"failed","error":{"message":"Selected model is at capacity.","codexErrorInfo":"serverOverloaded"}}},"threadReadback":{"capacityRetryEligible":true}}).to_string()
		}).await.unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history");
		};
		assert_eq!(entries[0].kind, "capacity_retry_pending");
		assert_eq!(entries[0].id, event.id);
		assert_eq!(entries[0].text, "Model busy · retry 1/3 scheduled automatically.");
		assert!(!entries[0].text.contains("Execution failed"));
		let (kind, text) = super::completed_chief_history(
			&serde_json::json!({"terminal":{"turn":{"status":"failed","error":{"message":"Selected model is at capacity."}}},"capacityRetry":{"attempt":1}}),
			&mut false,
			None,
			event.id,
			&mut Vec::new(),
		);
		assert_eq!(kind, "execution_notice");
		assert_eq!(text, "Model was busy · automatic retry requested.");

		store.cancel_chief_capacity_retry("chosen".into(), event.id).await.unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history");
		};
		assert!(entries.iter().all(|entry| entry.kind != "capacity_retry_pending"));
		assert!(entries[0].text.contains("cancelled"));
	}
	#[tokio::test]
	async fn history_projects_live_question_provenance_and_other_client_resolution() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		chief_query_work(&store, "chosen").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		let record = |id: &str| {
			vec![(
				id.to_owned(),
				serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
			)]
		};
		store
			.record_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"old".into(),
				record("old"),
			)
			.await
			.unwrap();
		store
			.record_live_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"live".into(),
				record("live"),
			)
			.await
			.unwrap();
		let owner = ProductStore::Available(store.clone());
		let decodex_protocol::ChiefHistoryResult::Available { questions, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history")
		};
		assert_eq!(questions.len(), 2);
		assert!(!questions[0].arrived_live);
		assert!(questions[1].arrived_live);
		let other = SqliteStore::open(&root.paths()).unwrap();
		other.resolve_chief_async_questions("thread".into(), vec!["live".into()]).await.unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { questions, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history")
		};
		assert_eq!(questions.len(), 1);
		assert_eq!(questions[0].id, "old");
		assert!(!questions[0].arrived_live);
	}

	#[tokio::test]
	async fn chief_history_deduplicates_async_questions_against_terminal_readback() {
		use decodex_database::EnqueueChiefEvent;
		use decodex_protocol::ChiefHistoryResult;
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "chosen").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chosen".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chosen".into(), "turn".into()).await.unwrap();
		let question = serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","text":"Which format?\n- PDF\n- Markdown"});
		store
			.record_chief_observation(EnqueueChiefEvent {
				source_event_id: "question".into(),
				work_item_id: "chosen".into(),
				event_kind: "assistant_message".into(),
				payload: serde_json::json!({"threadId":"thread","turnId":"turn","item":question})
					.to_string(),
			})
			.await
			.unwrap();
		let counts = serde_json::json!({"totalTokens":1200,"inputTokens":1000,"cachedInputTokens":500,"outputTokens":200,"reasoningOutputTokens":100});
		let usage = serde_json::json!({"total":counts,"last":counts,"modelContextWindow":128000});
		store
			.record_chief_observation(EnqueueChiefEvent {
				source_event_id: "usage".into(),
				work_item_id: "chosen".into(),
				event_kind: "token_usage".into(),
				payload:
					serde_json::json!({"threadId":"thread","turnId":"turn","tokenUsage":usage})
						.to_string(),
			})
			.await
			.unwrap();
		let ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history");
		};
		assert!(entries.iter().any(|entry| entry.text.contains("Which format?")));
		store.complete_chief_turn_with_event("chosen".into(), "turn".into(), EnqueueChiefEvent {
			source_event_id:"completed".into(),work_item_id:"chosen".into(),event_kind:"chief_turn_completed".into(),
			payload:serde_json::json!({"threadReadback":{"turnId":"turn","assistantMessages":[question],"tokenUsage":usage}}).to_string()
		}).await.unwrap();
		let ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history");
		};
		assert_eq!(entries.iter().filter(|entry| entry.text.contains("Which format?")).count(), 1);
		assert_eq!(
			entries.iter().filter(|entry| entry.text.contains("Thread total tokens: 1200")).count(),
			0
		);
		assert!(entries.iter().all(|entry| !entry.text.contains("Provider observation recorded")));
		assert!(entries.iter().any(|entry| {
			entry
				.usage
				.as_ref()
				.is_some_and(|usage| usage.input_tokens == 1000 && usage.output_tokens == 200)
		}));
	}

	#[tokio::test]
	async fn strict_review_history_survives_restart_without_claiming_a_review_result() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		chief_query_work(&store, "chosen").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chosen".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chosen".into(), "turn".into()).await.unwrap();
		store.record_chief_strict_review("thread".into(), "turn".into(), 10).await.unwrap();
		store.mark_chief_dispatch_unknown("chosen".into()).await.unwrap();
		store.record_chief_strict_review("thread".into(), "turn".into(), 20).await.unwrap();
		drop(store);
		let store = SqliteStore::open(&root.paths()).unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&ProductStore::Available(store.clone()), "chosen").await
		else {
			panic!("history")
		};
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].kind, "execution_notice");
		assert!(entries[0].text.starts_with("Codex requested additional safety checks"));
		assert!(!entries[0].text.contains("Disposition:"));
		assert!(!entries[0].text.contains("approved"));
		assert!(store.list_chief_wake_events("chosen".into(), 32).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn chief_activity_history_projects_receipts_without_disposition_prose() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		chief_query_work(&store, "chosen").await;
		store.bind_chief_thread("chosen".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chosen".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chosen".into(), "turn".into()).await.unwrap();
		let activity = decodex_protocol::ChiefActivityDto {
			turn_id: "turn".into(),
			item_id: "item".into(),
			kind: "contextCompaction".into(),
			status: "completed".into(),
			label: "Compacting context".into(),
			detail: String::new(),
			duration_ms: None,
		};
		store
			.record_chief_activity(
				"thread".into(),
				"turn".into(),
				"item".into(),
				true,
				serde_json::to_string(&activity).unwrap(),
			)
			.await
			.unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&ProductStore::Available(store), "chosen").await
		else {
			panic!("history");
		};
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].activity.as_ref(), Some(&activity));
		assert!(entries[0].text.is_empty());
	}

	#[tokio::test]
	async fn chief_history_query_selects_latest_work_and_bounds_utf8_content() {
		use decodex_database::EnqueueChiefEvent;
		use decodex_protocol::ChiefHistoryResult;
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "chosen").await;
		chief_query_work(&store, "other").await;
		assert_eq!(
			super::query_chief_history(&owner, "missing").await,
			ChiefHistoryResult::Unavailable
		);
		for index in 0..35 {
			store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("chosen-{index}"),
					work_item_id: "chosen".into(),
					event_kind: "user_message".into(),
					payload: serde_json::json!({"text":format!("message-{index}")}).to_string(),
				})
				.await
				.unwrap();
		}
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "other-1".into(),
				work_item_id: "other".into(),
				event_kind: "user_message".into(),
				payload: serde_json::json!({"text":"other work private marker"}).to_string(),
			})
			.await
			.unwrap();
		let ChiefHistoryResult::Available { entries, has_more, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("selected history");
		};
		assert!(has_more);
		assert_eq!(entries.len(), 32);
		assert_eq!(entries.first().unwrap().text, "message-3");
		assert_eq!(entries.last().unwrap().text, "message-34");
		assert!(entries.windows(2).all(|pair| pair[0].id < pair[1].id));
		let before = entries.first().unwrap().id;
		let ChiefHistoryResult::Available { entries: older, next_before, live, .. } =
			super::query_chief_history_page(&owner, "chosen", Some(before)).await
		else {
			panic!("older page");
		};
		assert_eq!(
			older.iter().map(|entry| entry.text.as_str()).collect::<Vec<_>>(),
			vec!["message-0", "message-1", "message-2"]
		);
		assert!(older.iter().all(|entry| entry.id < before));
		assert!(next_before.is_none());
		assert!(live.is_empty());

		for index in 0..9 {
			store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("large-{index}"),
					work_item_id: "chosen".into(),
					event_kind: "user_message".into(),
					payload: serde_json::json!({"text":"界".repeat(4000)}).to_string(),
				})
				.await
				.unwrap();
		}
		let ChiefHistoryResult::Available { entries, has_more, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("bounded history");
		};
		assert!(has_more);
		assert!(entries.iter().all(|entry| entry.text.len() <= 65536));
		assert!(entries.iter().map(|entry| entry.text.len()).sum::<usize>() <= 65536);
		assert!(entries.last().unwrap().text.starts_with('界'));
	}

	#[test]
	fn empty_interrupted_turn_is_a_normal_stop_not_a_failure() {
		let value = serde_json::json!({"terminal":{"turn":{"status":"interrupted"}},"threadReadback":{"assistantMessages":[]}});
		let (kind, text) = super::chief_assistant_history(&value, &mut false);
		assert_eq!(kind, "stopped");
		assert_eq!(text, "Stopped");
	}

	#[tokio::test]
	async fn chief_history_reads_structured_and_legacy_assistant_results() {
		use decodex_database::EnqueueChiefEvent;
		use decodex_protocol::ChiefHistoryResult;
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "chosen").await;
		for (index, messages, truncated) in [
			(0, serde_json::json!([{ "text": "legacy result" }]).to_string().into(), false),
			(1, serde_json::json!([{ "text": "界🙂\"\\\n".repeat(4000) }]), true),
		] {
			store.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: format!("assistant-{index}"), work_item_id: "chosen".into(),
				event_kind: "chief_turn_completed".into(),
				payload: serde_json::json!({"threadReadback": {"assistantMessages":messages,"truncated":truncated}}).to_string(),
			}).await.unwrap();
		}
		let ChiefHistoryResult::Available { entries, has_more, .. } =
			super::query_chief_history(&owner, "chosen").await
		else {
			panic!("history available");
		};
		assert!(has_more);
		assert_eq!(entries.len(), 2);
		assert_eq!(entries[0].text, "legacy result");
		assert!(
			entries[1].text.starts_with("[Assistant output truncated in saved history.]\n\n界🙂")
		);
		assert!(entries[1].text.len() <= 65536);
	}

	#[tokio::test]
	async fn chief_history_explains_provider_failure_without_submitting_continuation() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		chief_query_work(&store, "chosen").await;
		store.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
			source_event_id:"failure".into(),work_item_id:"chosen".into(),event_kind:"chief_turn_completed".into(),
			payload:serde_json::json!({"terminal":{"turn":{"status":"failed","error":{"message":"Provider stopped the turn.","misalignment":{"detailedExplanation":"Please clarify the intended scope.","steer":{"message":"unconfirmed continuation"}}}}}}).to_string(),
		}).await.unwrap();
		let decodex_protocol::ChiefHistoryResult::Available { entries, .. } =
			super::query_chief_history(&ProductStore::Available(store.clone()), "chosen").await
		else {
			panic!("history");
		};
		assert!(entries[0].text.contains("Provider stopped the turn."));
		assert!(entries[0].text.contains("Please clarify the intended scope."));
		assert!(!entries[0].text.contains("unconfirmed continuation"));
		assert!(store.list_chief_wake_events("chosen".into(), 10).await.unwrap().is_empty());
	}

	#[test]
	fn file_approval_details_preserve_request_identity_and_do_not_enrich_other_methods() {
		use decodex_protocol::{ChiefActivityDetailResult, ChiefRequestResult, HistoryText};
		for method in ["item/fileChange/requestApproval", "item/tool/requestUserInput"] {
			let request = ChiefRequestResult::Available {
				event_id: 7,
				work_id: "work".into(),
				method: method.into(),
				request_json: HistoryText::new("{\"reason\":\"Review\"}").unwrap(),
			};
			assert_eq!(
				super::attach_file_approval_detail(
					request.clone(),
					ChiefActivityDetailResult::Unavailable
				),
				request
			);
			let enriched = super::attach_file_approval_detail(
				request.clone(),
				ChiefActivityDetailResult::Available {
					text: "Path: /tmp/file\n+new".into(),
					offset: 0,
					next: None,
					truncated: true,
				},
			);
			if method.ends_with("requestUserInput") {
				assert_eq!(enriched, request);
				continue;
			}
			let ChiefRequestResult::Available { event_id, work_id, request_json, .. } = enriched
			else {
				panic!("request");
			};
			assert_eq!(event_id, 7);
			assert_eq!(work_id, "work");
			let fields: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
			assert_eq!(fields["reason"], "Review");
			assert_eq!(fields["changeDetailsTruncated"], true);
			assert!(fields["changeDetails"].as_str().unwrap().contains("/tmp/file"));
		}
	}

	#[tokio::test]
	async fn installation_suggestion_projection_preserves_target_but_not_private_metadata() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "worker").await;
		store.bind_chief_thread("worker".into(), "thread".into()).await.unwrap();
		let meta = serde_json::json!({"codex_approval_kind":"tool_suggestion","tool_type":"plugin","suggest_type":"install","tool_id":"sample@market","tool_name":"Sample","suggestion_id":"suggestion-1","remote_plugin_id":"plugins~sample","app_connector_ids":["connector-1"],"install_url":"https://chatgpt.com/apps/sample","private_token":"PRIVATE_TOKEN"});
		let event = store.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
			source_event_id: "install-suggestion".into(), work_item_id: "worker".into(),
			event_kind: "server_request_pending".into(),
			payload: serde_json::json!({"method":"mcpServer/elicitation/request","id":"private-rpc-id","params":{"threadId":"thread","serverName":"codex_apps","mode":"form","message":"Install Sample","requestedSchema":{"type":"object","properties":{}},"_meta":meta}}).to_string(),
		}).await.unwrap();
		let decodex_protocol::ChiefRequestResult::Available { request_json, .. } =
			super::query_chief_request(&owner, event.id).await
		else {
			panic!("pending suggestion");
		};
		let value: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
		let suggestion =
			decodex_protocol::McpInstallSuggestion::from_request(&value).unwrap().unwrap();
		assert_eq!(suggestion.tool_id, "sample@market");
		assert_eq!(value["_meta"]["suggestion_id"], "suggestion-1");
		assert_eq!(value["_meta"]["remote_plugin_id"], "plugins~sample");
		assert_eq!(suggestion.install_url(), Some("https://chatgpt.com/apps/sample"));
		assert!(!request_json.as_str().contains("PRIVATE_TOKEN"));
		assert!(!request_json.as_str().contains("private-rpc-id"));
		store.acknowledge_chief_request_event(event.id).await.unwrap();
		assert_eq!(
			super::query_chief_request(&owner, event.id).await,
			decodex_protocol::ChiefRequestResult::Unavailable
		);
	}

	#[tokio::test]
	async fn standalone_mcp_elicitation_projects_only_owned_request_fields() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "worker").await;
		store.bind_chief_thread("worker".into(), "thread".into()).await.unwrap();
		for (index, thread, turn, available) in [
			(0, "thread", serde_json::Value::Null, true),
			(1, "other", serde_json::Value::Null, false),
			(2, "thread", serde_json::json!("stale"), false),
			(3, "thread", serde_json::Value::Null, true),
			(4, "thread", serde_json::Value::Null, true),
		] {
			let mode = if index >= 3 { "openaiForm" } else { "form" };
			let schema = match index {
				3 => serde_json::json!(true),
				4 => serde_json::Value::Null,
				_ =>
					serde_json::json!({"type":"object","properties":{"date":{"type":"string","format":"date"}}}),
			};
			let payload = serde_json::json!({"method":"mcpServer/elicitation/request","params":{"threadId":thread,"turnId":turn,"serverName":"calendar","mode":mode,"message":"Choose a date","requestedSchema":schema,"challenge":"PRIVATE_CHALLENGE","_meta":{"tool_name":"calendar.create","private_token":"PRIVATE_TOKEN"}}});
			let event = store
				.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
					source_event_id: format!("elicitation-{index}"),
					work_item_id: "worker".into(),
					event_kind: "server_request_pending".into(),
					payload: payload.to_string(),
				})
				.await
				.unwrap();
			let result = super::query_chief_request(&owner, event.id).await;
			assert_eq!(
				matches!(result, decodex_protocol::ChiefRequestResult::Available { .. }),
				available
			);
			if let decodex_protocol::ChiefRequestResult::Available { request_json, .. } = result {
				let value: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
				assert_eq!(value["mode"], mode);
				assert_eq!(value["requestedSchema"], schema);
				assert_eq!(value["_meta"]["tool_name"], "calendar.create");
				assert!(!request_json.as_str().contains("PRIVATE_"));
			}
		}
	}

	#[tokio::test]
	async fn chief_request_query_filters_private_fields_and_rejects_stale_malformed_or_resolved() {
		use decodex_database::EnqueueChiefEvent;
		use decodex_protocol::ChiefRequestResult;
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "worker").await;
		store.bind_chief_thread("worker".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("worker".into()).await.unwrap();
		store.acknowledge_chief_dispatch("worker".into(), "turn".into()).await.unwrap();
		let payload = serde_json::json!({"method":"item/commandExecution/requestApproval", "id":"private-request-id", "token":"private-top-level", "params": {
			"threadId":"thread", "turnId":"turn", "command":"pwd", "cwd":"/tmp", "reason":"inspect directory",
			"availableDecisions":["accept","decline"], "authorization":"private-credential", "env":{"SECRET":"private-env"}
		}});
		let event = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "request-1".into(),
				work_item_id: "worker".into(),
				event_kind: "permission_pending".into(),
				payload: payload.to_string(),
			})
			.await
			.unwrap();
		let ChiefRequestResult::Available { event_id, work_id, request_json, .. } =
			super::query_chief_request(&owner, event.id).await
		else {
			panic!("live request");
		};
		assert_eq!(event_id, event.id);
		assert_eq!(work_id, "worker");
		let selected: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
		assert_eq!(selected["command"], "pwd");
		assert_eq!(selected["kind"], "command");
		assert!(!request_json.as_str().contains("private"));
		assert!(selected.get("threadId").is_none());
		assert_question_metadata_projection(&store, &owner).await;
		let mut stdin = payload.clone();
		stdin["params"]["kind"] = serde_json::json!("writeStdin");
		stdin["params"]["command"] = serde_json::json!("yes\\n");
		stdin["params"]["availableDecisions"] = serde_json::Value::Null;
		stdin["params"]["additionalPermissions"] = serde_json::json!({"network":{"enabled":true}});
		let stdin = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "stdin-request".into(),
				work_item_id: "worker".into(),
				event_kind: "permission_pending".into(),
				payload: stdin.to_string(),
			})
			.await
			.unwrap();
		let ChiefRequestResult::Available { request_json, .. } =
			super::query_chief_request(&owner, stdin.id).await
		else {
			panic!("stdin request");
		};
		let selected: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
		assert_eq!(selected["kind"], "writeStdin");
		assert_eq!(selected["additionalPermissions"]["network"]["enabled"], true);
		assert!(!request_json.as_str().contains("private"));
		store.acknowledge_chief_request_event(event.id).await.unwrap();
		assert_eq!(
			super::query_chief_request(&owner, event.id).await,
			ChiefRequestResult::Unavailable
		);
		let mut invalids = Vec::new();
		let mut unknown_kind = payload.clone();
		unknown_kind["params"]["kind"] = serde_json::json!("unknown-action");
		invalids.push(unknown_kind);
		let mut stale = payload.clone();
		stale["params"]["turnId"] = serde_json::json!("old-turn");
		invalids.push(stale);
		let mut malformed = payload.clone();
		malformed["params"] = serde_json::json!("invalid");
		invalids.push(malformed);
		let mut malformed = payload.clone();
		malformed["params"]["command"] = serde_json::json!({"token":"private"});
		invalids.push(malformed);
		let mut unsupported = payload.clone();
		unsupported["method"] = serde_json::json!("account/login");
		invalids.push(unsupported);
		let mut oversized = payload;
		oversized["params"]["command"] =
			serde_json::json!("x".repeat(decodex_protocol::MAX_HISTORY_INLINE_BYTES + 1));
		invalids.push(oversized);
		invalids.push(serde_json::Value::Null);
		for (index, payload) in invalids.into_iter().enumerate() {
			let event = store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("invalid-{index}"),
					work_item_id: "worker".into(),
					event_kind: "permission_pending".into(),
					payload: payload.to_string(),
				})
				.await
				.unwrap();
			assert_eq!(
				super::query_chief_request(&owner, event.id).await,
				ChiefRequestResult::Unavailable,
				"case {index}"
			);
		}
		assert_eq!(
			super::query_chief_request(&owner, 99999).await,
			ChiefRequestResult::Unavailable
		);
	}

	#[tokio::test]
	async fn permission_request_projection_preserves_the_native_executor() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		chief_query_work(&store, "worker").await;
		store.bind_chief_thread("worker".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("worker".into()).await.unwrap();
		store.acknowledge_chief_dispatch("worker".into(), "turn".into()).await.unwrap();
		for (index, environment) in
			[serde_json::json!("remote/工作"), serde_json::Value::Null, serde_json::json!(42)]
				.into_iter()
				.enumerate()
		{
			let permissions = serde_json::json!({"fileSystem":{"entries":[{"path":{"type":"special","value":{"kind":"project_roots"}},"access":"write"}]}});
			let payload = serde_json::json!({"method":"item/permissions/requestApproval","params":{"threadId":"thread","turnId":"turn","environmentId":environment,"cwd":"C:\\workspace","permissions":permissions,"privateToken":"hidden"}});
			let event = store
				.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
					source_event_id: format!("executor-{index}"),
					work_item_id: "worker".into(),
					event_kind: "permission_pending".into(),
					payload: payload.to_string(),
				})
				.await
				.unwrap();
			let result = super::query_chief_request(&owner, event.id).await;
			if environment.is_number() {
				assert_eq!(result, decodex_protocol::ChiefRequestResult::Unavailable);
				continue;
			}
			let decodex_protocol::ChiefRequestResult::Available { request_json, .. } = result
			else {
				panic!("permission request")
			};
			let value: serde_json::Value = serde_json::from_str(request_json.as_str()).unwrap();
			assert_eq!(value["environmentId"], environment);
			assert_eq!(value["cwd"], "C:\\workspace");
			assert_eq!(value["permissions"], permissions);
			assert!(value.get("privateToken").is_none());
		}
	}

	async fn assert_question_metadata_projection(store: &SqliteStore, owner: &ProductStore) {
		use decodex_database::EnqueueChiefEvent;
		use decodex_protocol::ChiefRequestResult;
		for (index, blocking) in
			[serde_json::json!(false), serde_json::json!(true), serde_json::json!("false")]
				.into_iter()
				.enumerate()
		{
			let request = serde_json::json!({"method":"item/tool/requestUserInput","params":{"threadId":"thread","turnId":"turn","questions":[],"isBlocking":blocking,"autoResolutionMs":1}});
			let event = store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("question-{index}"),
					work_item_id: "worker".into(),
					event_kind: "user_input_pending".into(),
					payload: request.to_string(),
				})
				.await
				.unwrap();
			let result = super::query_chief_request(owner, event.id).await;
			if blocking.is_boolean() {
				let ChiefRequestResult::Available { request_json, .. } = result else {
					panic!("question metadata");
				};
				let fields: serde_json::Value =
					serde_json::from_str(request_json.as_str()).unwrap();
				assert_eq!(fields["isBlocking"], blocking);
				assert!(fields.get("autoResolutionMs").is_none());
			} else {
				assert_eq!(result, ChiefRequestResult::Unavailable);
			}
		}
	}

	#[test]
	fn repeatable_program_projection_follows_review_lineage() {
		let review_1 = ProgramReviewId::new("38000000-0000-4000-8000-000000000001")
			.expect("fixture review identity");
		let signal_2 = ProgramObservationId::new("41000000-0000-4000-8000-000000000001")
			.expect("fixture signal identity");
		let record: ProgramCycleRecord =
			serde_json::from_str(include_str!("../tests/fixtures/historical_program_cycle.json"))
				.expect("historical two-cycle fixture");

		let projection =
			program_cycle_dto(record, &[], &HashMap::new()).expect("two-cycle projection");
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

	fn pre_session_conversation(
		state: OrdinaryTaskPreSessionState,
		decision_id: Option<&str>,
	) -> OrdinaryTaskConversationReadback {
		OrdinaryTaskConversationReadback {
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000001276").unwrap(),
			title: "Conversation fixture".to_owned(),
			conversation_revision: 1,
			runtime_session_id: None,
			runtime_session_revision: None,
			runtime_session_state: None,
			codex_thread_id: None,
			program_work_item: None,
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
	fn conversation_projection_distinguishes_routing_and_establishment_recovery() {
		let routing = conversation_summary_from_row(
			pre_session_conversation(OrdinaryTaskPreSessionState::RoutingPending, None),
			None,
		)
		.unwrap();
		let establishment = conversation_summary_from_row(
			pre_session_conversation(
				OrdinaryTaskPreSessionState::EstablishmentPending,
				Some("41000000-0000-4000-8000-000000001276"),
			),
			None,
		)
		.unwrap();

		assert_eq!(routing.state, ConversationState::RoutingPending);
		assert_eq!(routing.recovery_action, Some(ConversationRecoveryAction::ResumeRouting));
		assert_eq!(establishment.state, ConversationState::EstablishmentPending);
		assert_eq!(
			establishment.recovery_action,
			Some(ConversationRecoveryAction::ResumeEstablishment),
		);
	}

	#[test]
	fn terminal_session_projection_never_reopens_routing_recovery() {
		let row = OrdinaryTaskConversationReadback {
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000001276").unwrap(),
			title: "Conversation fixture".to_owned(),
			conversation_revision: 1,
			runtime_session_id: Some(
				decodex_core::RuntimeSessionId::new("42000000-0000-4000-8000-000000001276")
					.unwrap(),
			),
			runtime_session_revision: Some(4),
			runtime_session_state: Some(RuntimeSessionState::Ended),
			codex_thread_id: None,
			program_work_item: None,
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
		let projection = conversation_summary_from_row(row, None).unwrap();
		assert_eq!(projection.state, ConversationState::ManualRecovery);
		assert_eq!(
			projection.recovery_action,
			Some(ConversationRecoveryAction::StartNewConversation),
		);
	}

	#[test]
	fn optional_quota_absence_is_publicly_distinct_from_unknown_or_zero_usage() {
		let dto = quota_dto(AccountQuotaWindowObservation {
			duration_minutes: AccountQuotaWindow::FIVE_HOURS_MINUTES,
			observed_at_unix_micros: Some(1_000_000),
			disposition: AccountQuotaDisposition::NotApplicable,
		})
		.expect("confirmed optional absence has a bounded projection");
		assert_eq!(dto.result, AccountQuotaStateDto::NotApplicable);
		assert_eq!(dto.observed_at_unix_micros, Some(1_000_000));
		let encoded = serde_json::to_string(&dto).expect("quota DTO serializes");
		assert!(!encoded.contains("used_percent"));
		assert!(!encoded.contains("resets_at_unix_micros"));
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
			usage_observation: None,
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
	fn duplicate_provider_is_not_reported_as_credential_store_unavailable() {
		let error = account_lifecycle_command_error(AccountLifecycleError::CredentialStore(
			crate::CredentialStoreError::DuplicateProvider,
		));

		assert_eq!(
			error.clone(),
			CommandError::AccountCommandRejected {
				rejection: AccountCommandRejectionDto::ProviderAlreadyEnrolled,
				actual_revision: None,
			}
		);
		let encoded = encode_account_command_receipt(&Err(error.clone()))
			.expect("typed duplicate-provider rejection must encode");
		assert_eq!(
			encoded,
			serde_json::json!({
				"outcome": "rejected",
				"data": {
					"schema": "decodex/account-command-result/1",
					"error": {
						"reason": "account_command_rejected",
						"rejection": "provider_already_enrolled",
					},
				},
			}),
		);
		assert_eq!(decode_account_command_receipt(encoded.clone()), Ok(Err(error.clone())));
		assert_eq!(decode_account_command_receipt(encoded), Ok(Err(error)));
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

#[path = "application_conversation_receipts.rs"] mod conversation_receipts;
