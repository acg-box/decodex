//! Typed vNext wire contracts and same-UID local transport shared by clients and
//! `decodex serve`.

mod account_login;
mod account_recovery;
mod agent;
mod agent_app_exposure;
mod agent_app_settings;
mod agent_app_ui;
mod agent_app_ui_call;
mod agent_archive;
mod agent_execution;
mod agent_guardian;
mod agent_hooks;
mod agent_integrations;
mod agent_live_settings;
mod agent_media;
mod agent_model_settings;
mod agent_models;
mod agent_native_goal;
mod agent_permissions;
mod agent_plugins;
mod agent_prompt_draft;
mod agent_prompt_edit;
mod agent_prompt_send;
mod agent_prompt_upload;
mod agent_questions;
mod agent_recap;
mod agent_requested_decision;
mod agent_search_settings;
mod agent_skills;
mod agent_steer;
mod agent_timeline;
mod agent_transcript;
mod agent_usage_estimate;
mod agent_voice_settings;
mod client;
mod conversation;
mod conversation_native_settings;
mod conversation_receipts;
mod conversation_turn_outcomes;
mod desktop_drafts;
mod desktop_ordinary_drafts;
mod dictation;
mod doctor;
mod domain_pack;
mod local_transport;
mod mcp_elicitation;
mod mcp_install;
mod mcp_login;
mod model_catalog;
mod native_agents;
mod program_cycle;
mod reset_card_recovery;
mod retained_session;
mod voice;
mod weather;
mod wire;

pub use self::{
	account_login::{
		AccountLoginContractError, AccountLoginFailure, AccountLoginInstallMode,
		AccountLoginMethod, AccountLoginPrompt, AccountLoginRequest, AccountLoginRequestEnvelope,
		AccountLoginResponseEnvelope, AccountLoginStart, AccountLoginState, AccountLoginStatus,
		AccountLoginUrl, MAX_ACCOUNT_LOGIN_URL_BYTES,
	},
	account_recovery::{
		AccountRecoveryAction, AccountRecoveryBanner, AccountRecoveryCta,
		AccountRecoveryDestination, AccountRecoveryNudgeOperation, AccountRecoveryNudgeResult,
		AccountRecoveryNudgeStatus, AccountRecoveryPreparation, AccountRecoveryResult,
		AccountRecoveryState,
	},
	agent::{
		AgentActionDto, AgentActivityDetailCursor, AgentActivityDetailResult, AgentActivityDto,
		AgentAttachmentDto, AgentCapabilitiesResult, AgentDependencyDto, AgentDispatchStateDto,
		AgentHistoryEntryDto, AgentHistoryReceiptDto, AgentHistoryResult, AgentHistorySourceDto,
		AgentInputReceiptsResult, AgentLiveMessageDto, AgentLiveMessageKind, AgentMisalignmentDto,
		AgentModelDto, AgentModelUpgradeDto, AgentOutputResult, AgentPendingEventDto,
		AgentRequestResult, AgentRequestText, AgentResourceDto, AgentResourcesResult,
		AgentSandboxDto, AgentServiceTierDto, AgentSnapshotDto, AgentSnapshotResult, AgentStartDto,
		AgentTaskReferenceDto, AgentTurnUsageDto, AgentUsageDetailsDto, AgentUsageDto,
		AgentWorkItemDto, AgentWorkKindDto, AgentWorkStatusDto, AgentWorkspaceDto,
		MAX_AGENT_DEPENDENCIES, MAX_AGENT_PENDING_EVENTS, MAX_AGENT_SNAPSHOT_BYTES,
		MAX_AGENT_WORK_ITEMS,
	},
	agent_app_exposure::{AgentAppExposureResult, AgentToolExposureSurface},
	agent_app_settings::{
		AgentAppApprovalMode, AgentAppReviewer, AgentAppSettingEdit, AgentAppSettingsResult,
		AgentConfigEditReceipt, AgentSavedAppConnection, AgentSavedAppSettingsResult,
	},
	agent_app_ui::{
		AGENT_APP_UI_CHUNK_BYTES, AgentAppUiRequest, AgentAppUiResult, MAX_AGENT_APP_UI_BYTES,
	},
	agent_app_ui_call::{
		AGENT_APP_UI_RECEIPT_CHUNK_BYTES, AgentAppUiCall, AgentAppUiCallReview,
		AgentAppUiReceiptRequest, AgentAppUiReceiptResult, AgentPendingAppUiCall,
		MAX_AGENT_APP_UI_CALL_BYTES, MAX_AGENT_APP_UI_RECEIPT_BYTES,
	},
	agent_archive::AgentArchiveResult,
	agent_execution::AgentExecutionOverrides,
	agent_guardian::{
		AgentGuardianDetailResult, AgentGuardianReviewDto, AgentGuardianReviewsResult,
		AgentGuardianStatus, AgentGuardianSubmission, GUARDIAN_DETAIL_PAGE_BYTES,
	},
	agent_hooks::{AgentHookChange, AgentHookDto, AgentHookEditReceipt, AgentHookSettingsState},
	agent_integrations::{
		AgentAppInventory, AgentAppStatusDto, AgentIntegrationsResult, AgentMcpInventory,
		AgentMcpStatusDto, AgentPluginInventory, AgentPluginStatusDto,
	},
	agent_live_settings::{
		AgentLiveModelSelection, AgentLiveReviewerOutcome, AgentLiveReviewerState, AgentReviewer,
	},
	agent_media::{
		AGENT_MEDIA_CHUNK_BYTES, AgentMediaRequest, AgentMediaResult, MAX_AGENT_MEDIA_BYTES,
	},
	agent_model_settings::AgentModelSettingsResult,
	agent_models::{
		AgentModelOutcome, AgentModelResponse, AgentModelSelectionReceipt, AgentModelSelectionState,
	},
	agent_native_goal::{
		AgentGoalBudgetEdit, AgentGoalEdit, AgentNativeGoal, AgentNativeGoalResult,
		AgentNativeGoalStatus,
	},
	agent_permissions::{AgentPermissionOutcome, AgentPermissionProfile, AgentPermissionState},
	agent_plugins::{AgentPluginOutcome, AgentPluginSelectionState},
	agent_prompt_draft::{DesktopPromptEditDraft, PromptDraft},
	agent_prompt_edit::{
		PromptEditEvidence, PromptEditPhase, PromptEditStatus, PromptForkBoundary,
		PromptForkIntent, PromptForkPhase, PromptForkResult, PromptForkStatus,
	},
	agent_prompt_send::{PromptInputSend, PromptInputSendIdentity, PromptInputSendStatus},
	agent_prompt_upload::{PromptInputUpload, PromptInputUploadStatus},
	agent_questions::{
		AgentAsyncQuestionDto, AgentAsyncQuestionReply, agent_async_question_id,
		agent_async_question_reply, parse_agent_async_question_replies,
		project_agent_async_questions, render_agent_async_question_history,
	},
	agent_recap::{TaskRecap, TaskRecapPhase, TaskRecapStatus},
	agent_requested_decision::{AgentRequestedDecision, requested_decision_response},
	agent_search_settings::AgentSearchSettingsResult,
	agent_skills::{AgentSkillDto, AgentSkillsPage, AgentSkillsResult, AgentSkillsTarget},
	agent_steer::{AgentSteerIdentity, AgentSteerReceiptResult},
	agent_timeline::{
		AgentTimelineAttachment, AgentTimelineAttachmentSource, AgentTimelineContent,
		AgentTimelineEntry, AgentTimelineError, AgentTimelinePage, AgentTimelinePromotedContent,
		AgentTimelineResult,
	},
	agent_transcript::{
		AgentTranscriptRequest, AgentTranscriptResult, MAX_TRANSCRIPT_BYTES, TRANSCRIPT_CHUNK_BYTES,
	},
	agent_usage_estimate::{
		AgentUsageEstimateResult, ThreadUsageEstimate, ThreadUsageEstimateGroup,
	},
	agent_voice_settings::AgentVoiceSettingsResult,
	client::{
		AccountClient, AccountCommandResponse, AccountLoginClient, AgentClient,
		AgentCommandResponse, ClientFailure, ClientProfile, DoctorClient, ProfileKind,
		ResetCardClient, ResetCardConsumeResponse,
	},
	conversation::{
		ConversationContractError, ConversationExecutionOverrides, ConversationExecutionSettings,
		ConversationListCursor, ConversationListPage, ConversationListResult, ConversationListSize,
		ConversationModel, ConversationModelSettingsResult, ConversationProgramContext,
		ConversationReadError, ConversationReasoningEffort, ConversationRecoveryAction,
		ConversationResult, ConversationState, ConversationSummary, ConversationTitle,
		ConversationTurnOutcome, ConversationUnavailableReason, ConversationWorkingDirectory,
		CustomReasoningEffort, MAX_CONVERSATION_LIST_SIZE, MAX_CONVERSATION_MODEL_BYTES,
		MAX_CONVERSATION_TITLE_BYTES, MAX_CONVERSATION_WORKING_DIRECTORY_BYTES,
		MAX_PROVIDER_THREAD_ID_BYTES, ProviderThreadId,
	},
	conversation_native_settings::ConversationNativeSettings,
	conversation_receipts::{
		ConversationCreationReceiptRequest, ConversationCreationReceiptResult,
	},
	conversation_turn_outcomes::{
		ConversationTurnOutcomeRequest, ConversationTurnOutcomeResult, ConversationTurnOutcomeState,
	},
	desktop_drafts::{
		DesktopComposerDraft, DesktopCreationIntent, DesktopCreationSetup, DesktopDraftDocument,
		DesktopPendingDraft, DesktopProfileDraft, DesktopQuestionDraft, DesktopRecoveredDraft,
	},
	desktop_ordinary_drafts::{DesktopOrdinaryComposerDraft, DesktopOrdinaryDraft},
	dictation::{DictationBuffer, DictationPhase, DictationRequest, DictationStatus},
	doctor::{
		AppServerCapability, DoctorCheck, DoctorComponent, DoctorContractError, DoctorIssue,
		DoctorReport, DoctorStatus, MAX_DOCTOR_CHECKS, NativeProcessDiagnostics,
	},
	domain_pack::{
		DEVELOPMENT_DOMAIN_PACK_ID, DomainEntityDto, DomainEntityFieldDto, DomainPackCapabilityDto,
		DomainPackCapabilityStatus, DomainPackContractError, DomainPackDescriptorDto,
		DomainPackProjectionDto, DomainPackViewKind, DomainRelationDto,
		MAX_DOMAIN_PACK_CAPABILITIES, MAX_DOMAIN_PACK_ENTITIES, MAX_DOMAIN_PACK_RELATIONS,
		PAPER_INVESTMENT_DOMAIN_PACK_ID,
	},
	local_transport::{
		LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
		LocalTransportStream,
	},
	mcp_elicitation::{
		McpFormChoice, McpFormField, mcp_form_content, mcp_form_fields, mcp_request_fields,
		validate_mcp_response,
	},
	mcp_install::{AgentInstallApp, AgentInstallState, McpInstallSuggestion, McpInstallTarget},
	mcp_login::{McpAuthorizationUrl, McpLoginPhase, McpLoginRequest, McpLoginStatus},
	model_catalog::{
		ConversationModelReview, ConversationModelReviewResult, InitialExecutionDefaults,
		InitialModelCatalogRequest, InitialModelCatalogResult, InitialModelDefaults,
		InitialModelSource, ModelCatalogPurpose,
	},
	native_agents::{NativeAgentDto, NativeAgentsResult},
	program_cycle::{
		MAX_PROGRAM_EDGES, MAX_PROGRAM_LIST_ITEMS, MAX_PROGRAM_LIST_VALUES, MAX_PROGRAM_NODES,
		ProgramCycleContractError, ProgramCycleDto, ProgramCycleResult, ProgramEdgeDto,
		ProgramListResult, ProgramNodeDto, ProgramNodeFieldDto, ProgramNodeKind,
		ProgramRelationKind, ProgramReviewClassification, ProgramState, ProgramSummaryDto,
	},
	reset_card_recovery::{AccountResetCardOperationResult, ResetCardOperationView},
	retained_session::{
		ApplicationConfirmation, RetainedSession, RetainedSessionConfig, RetainedSessionFailure,
		SessionCancellation, SessionCheckpoint, SessionDelivery,
	},
	voice::{AgentVoiceOptions, AgentVoicePhase, AgentVoiceRequest, AgentVoiceStatus, VoiceSdp},
	weather::WeatherForecast,
	wire::{
		AccountCommandRejectionDto, AccountCredentialBindingDto, AccountDto,
		AccountInitialSelectionResult, AccountInspectResult, AccountLifecycleReadinessDto,
		AccountManualRecoveryActionDto, AccountManualRecoveryOutcomeDto, AccountObservationSignal,
		AccountObservedStateDto, AccountOperationKindDto, AccountOperationPhaseDto,
		AccountProfileDailyUsageDto, AccountProfileDto, AccountProfileEmailDto,
		AccountProfileErrorDto, AccountProfileResult, AccountProviderDto, AccountQuotaErrorDto,
		AccountQuotaStateDto, AccountQuotaWindowDto, AccountRoutingControlDto,
		AccountSelectionModeDto, AccountSelectionRecoveryDto, AccountUnsettledOperationDto,
		AccountsResult, CausationId, Channel, ClientCommandId, ClientHello, ClientMessage,
		CodexAuthProjectionResult, CommandEnvelope, CommandError, CommandOutcome, CommandPayload,
		CommandReceipt, CommandResultEnvelope, ConversationHistoryPage, ConversationHistoryResult,
		CorrelationId, Cursor, DESKTOP_SETTINGS_ENTITY_ID, DesktopSettingsDto,
		DesktopSettingsResult, EntityId, EntityRevision, EventEnvelope, EventPayload,
		HistoryArtifactId, HistoryArtifactReference, HistoryArtifactRevision, HistoryBlobLength,
		HistoryBlobReference, HistoryCursorToken, HistoryItemDto, HistoryItemKindDto,
		HistoryItemStatusDto, HistoryMediaType, HistoryMetadata, HistoryMetadataValue,
		HistoryPayloadDto, HistoryQueryError, HistorySideEffectState, HistoryText, HistoryTurnRole,
		IdempotencyKey, IdempotencyKeyError, MAX_ACCOUNT_PROFILE_DAILY_USAGE,
		MAX_HISTORY_INLINE_BYTES, MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE, MAX_IDEMPOTENCY_KEY_BYTES,
		MAX_RESET_CARD_ITEMS, MAX_WIRE_TEXT_BYTES, QueryEnvelope, QueryId, QueryPayload,
		QueryResultEnvelope, QueryResultPayload, ReceiptDisposition, ReconnectMode, Refusal,
		RefusalEnvelope, ResetCardDescriptorDto, ResetCardDescriptorError, ResetCardError,
		ResetCardInventoryResult, ResetCardObservationDto, ResetCardOperationResult,
		ResetCardOutcome, ResultPayload, ResumeCursor, ServerId, ServerInstanceId, ServerMessage,
		ServerWelcome, Sha256Digest, SnapshotEnvelope, SnapshotItem, WireScalarTooLong, WireText,
		decode_client_message, encode_server_message,
	},
};
/// Exact service-tier identity shared with the provider and persistence boundaries.
pub use decodex_core::ServiceTier;
pub use decodex_core::{
	ClientDraftError, ClientDraftSnapshot, ClientDraftStore, MAX_CLIENT_DRAFT_BYTES,
};
/// Shared global client settings; these do not change thread execution settings.
pub use decodex_core::{FastModeFailure, global_fast_mode_enabled, set_global_fast_mode_enabled};

use serde::{Deserialize, Serialize};

/// The only protocol generation and revision accepted by this build.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 2, minor: 111 };
/// A version of the Decodex application protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ProtocolVersion {
	/// Breaking protocol generation.
	pub major: u16,
	/// Compatible protocol revision within a generation.
	pub minor: u16,
}
impl ProtocolVersion {
	/// Negotiate this client version against the server's exact-current window.
	pub fn negotiate(self) -> Result<Self, Self> {
		if self != CURRENT_VERSION {
			return Err(CURRENT_VERSION);
		}

		Ok(self)
	}
}

#[cfg(test)]
mod tests {
	use crate::{CURRENT_VERSION, ProtocolVersion};
	#[cfg(any(target_os = "linux", target_os = "macos"))]
	use crate::{LocalTransportAuthority, LocalTransportRefusal};
	#[cfg(any(target_os = "linux", target_os = "macos"))]
	use decodex_core::{DecodexRoot, LocalTrustPolicy};

	#[test]
	fn negotiation_accepts_only_the_current_major_and_minor() {
		assert_eq!(CURRENT_VERSION.negotiate(), Ok(CURRENT_VERSION));

		for requested in [
			ProtocolVersion { major: CURRENT_VERSION.major ^ 1, ..CURRENT_VERSION },
			ProtocolVersion { minor: CURRENT_VERSION.minor ^ 1, ..CURRENT_VERSION },
		] {
			assert_eq!(requested.negotiate(), Err(CURRENT_VERSION));
		}
	}

	#[cfg(any(target_os = "linux", target_os = "macos"))]
	#[test]
	fn local_transport_authority_accepts_only_the_process_effective_uid() {
		let temp = tempfile::tempdir().expect("test operation must succeed");
		let root = DecodexRoot::new(
			temp.path().canonicalize().expect("test operation must succeed").join(".decodex"),
		)
		.expect("test operation must succeed");
		let paths = root.paths();

		paths.ensure_layout().expect("test operation must succeed");

		// SAFETY: `geteuid` has no arguments or failure return.
		let uid = unsafe { libc::geteuid() };

		assert!(
			LocalTransportAuthority::new(paths.clone(), LocalTrustPolicy::SameUid, Some(uid),)
				.is_ok()
		);
		assert_eq!(
			LocalTransportAuthority::new(paths.clone(), LocalTrustPolicy::Disabled, None,)
				.unwrap_err(),
			LocalTransportRefusal::Disabled,
		);
		assert_eq!(
			LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(uid ^ 1))
				.unwrap_err(),
			LocalTransportRefusal::EffectiveUidMismatch,
		);
	}
}
