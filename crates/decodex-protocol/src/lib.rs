//! Typed vNext wire contracts and same-UID local transport shared by clients and
//! `decodex serve`.

mod agent_requested_decision;
pub use agent_requested_decision::{AgentRequestedDecision, requested_decision_response};

mod account_login;
mod agent;
mod agent_prompt_draft;
mod agent_prompt_edit;
mod agent_prompt_send;
mod agent_prompt_upload;
pub use agent_prompt_draft::{DesktopPromptEditDraft, PromptDraft};
pub use agent_prompt_send::{PromptInputSend, PromptInputSendIdentity, PromptInputSendStatus};
pub use agent_prompt_upload::{PromptInputUpload, PromptInputUploadStatus};
mod agent_recap;
pub use agent_prompt_edit::{PromptEditEvidence, PromptEditPhase, PromptEditStatus};
mod agent_voice_settings;
pub use agent_recap::{TaskRecap, TaskRecapPhase, TaskRecapStatus};
pub use agent_voice_settings::AgentVoiceSettingsResult;
mod agent_app_exposure;
pub use agent_app_exposure::{AgentAppExposureResult, AgentToolExposureSurface};
mod native_agents;
pub use native_agents::{NativeAgentDto, NativeAgentMessage, NativeAgentsResult};
mod agent_archive;
pub use agent_archive::AgentArchiveResult;
mod agent_guardian;
pub use agent_guardian::{
	AgentGuardianDetailResult, AgentGuardianReviewDto, AgentGuardianReviewsResult,
	AgentGuardianStatus, AgentGuardianSubmission, GUARDIAN_DETAIL_PAGE_BYTES,
};
mod agent_integrations;
pub use agent_integrations::{AgentAppInventory, AgentAppStatusDto};
mod agent_app_ui_call;
pub use agent_app_ui_call::{
	AGENT_APP_UI_RECEIPT_CHUNK_BYTES, AgentAppUiCall, AgentAppUiCallReview,
	AgentAppUiReceiptRequest, AgentAppUiReceiptResult, AgentPendingAppUiCall,
	MAX_AGENT_APP_UI_CALL_BYTES, MAX_AGENT_APP_UI_RECEIPT_BYTES,
};
mod agent_app_ui;
pub use agent_app_ui::{
	AGENT_APP_UI_CHUNK_BYTES, AgentAppUiRequest, AgentAppUiResult, MAX_AGENT_APP_UI_BYTES,
};
mod agent_media;
pub use agent_media::{
	AGENT_MEDIA_CHUNK_BYTES, AgentMediaRequest, AgentMediaResult, MAX_AGENT_MEDIA_BYTES,
};
mod agent_native_goal;
pub use agent_native_goal::{AgentNativeGoal, AgentNativeGoalResult, AgentNativeGoalStatus};
mod agent_app_settings;
pub use agent_app_settings::{
	AgentAppApprovalMode, AgentAppReviewer, AgentAppSettingEdit, AgentAppSettingsResult,
	AgentConfigEditReceipt, AgentSavedAppConnection, AgentSavedAppSettingsResult,
};
mod agent_hooks;
pub use agent_hooks::{
	AgentHookChange, AgentHookDto, AgentHookEditReceipt, AgentHookSettingsState,
};
mod agent_models;
pub use agent_models::{
	AgentModelOutcome, AgentModelResponse, AgentModelSelectionReceipt, AgentModelSelectionState,
};
mod agent_plugins;
pub use agent_plugins::{AgentPluginOutcome, AgentPluginSelectionState};
mod agent_permissions;
pub use agent_permissions::{AgentPermissionOutcome, AgentPermissionProfile, AgentPermissionState};
mod agent_live_settings;
pub use agent_live_settings::{
	AgentLiveModelSelection, AgentLiveReviewerOutcome, AgentLiveReviewerState, AgentReviewer,
};
mod agent_model_settings;
mod agent_timeline;
pub use agent_model_settings::AgentModelSettingsResult;
mod agent_usage_estimate;
pub use agent_timeline::{
	AgentTimelineAttachment, AgentTimelineAttachmentSource, AgentTimelineContent,
	AgentTimelineEntry, AgentTimelineError, AgentTimelinePage, AgentTimelinePromotedContent,
	AgentTimelineResult,
};
pub use agent_usage_estimate::{
	AgentUsageEstimateResult, ThreadUsageEstimate, ThreadUsageEstimateGroup,
};
mod conversation_receipts;
mod conversation_turn_outcomes;
pub use conversation_turn_outcomes::{
	ConversationTurnOutcomeRequest, ConversationTurnOutcomeResult, ConversationTurnOutcomeState,
};
mod mcp_install;
mod mcp_login;
mod model_catalog;
pub use agent_integrations::{
	AgentIntegrationsResult, AgentMcpInventory, AgentMcpStatusDto, AgentPluginInventory,
	AgentPluginStatusDto,
};
pub use conversation_receipts::{
	ConversationCreationReceiptRequest, ConversationCreationReceiptResult,
};
pub use mcp_install::{AgentInstallApp, AgentInstallState, McpInstallSuggestion, McpInstallTarget};
pub use mcp_login::{McpAuthorizationUrl, McpLoginPhase, McpLoginRequest, McpLoginStatus};
pub use model_catalog::{
	ConversationModelReview, ConversationModelReviewResult, InitialExecutionDefaults,
	InitialModelCatalogRequest, InitialModelCatalogResult, InitialModelDefaults,
	InitialModelSource, ModelCatalogPurpose,
};
mod agent_questions;
pub use agent::{
	AgentActionDto, AgentActivityDetailCursor, AgentActivityDetailResult, AgentActivityDto,
	AgentAttachmentDto, AgentCapabilitiesResult, AgentHistoryEntryDto, AgentHistoryReceiptDto,
	AgentHistoryResult, AgentHistorySourceDto, AgentInputReceiptsResult, AgentLiveMessageDto,
	AgentLiveMessageKind, AgentMisalignmentDto, AgentModelDto, AgentModelUpgradeDto,
	AgentOutputResult, AgentRequestResult, AgentRequestText, AgentResourceDto,
	AgentResourcesResult, AgentSandboxDto, AgentServiceTierDto, AgentStartDto,
	AgentTaskReferenceDto, AgentTurnUsageDto, AgentUsageDto, AgentWorkspaceDto,
};
pub use agent_questions::{
	AgentAsyncQuestionDto, AgentAsyncQuestionReply, agent_async_question_id,
	agent_async_question_reply, parse_agent_async_question_replies, project_agent_async_questions,
	render_agent_async_question_history,
};
mod client;
mod conversation;
mod conversation_native_settings;
pub use conversation_native_settings::ConversationNativeSettings;
mod dictation;
mod doctor;
mod domain_pack;
mod local_transport;
mod program_cycle;
mod retained_session;
mod voice;
pub use dictation::{DictationBuffer, DictationPhase, DictationRequest, DictationStatus};
mod account_recovery;
mod reset_card_recovery;
pub use account_recovery::{
	AccountRecoveryAction, AccountRecoveryBanner, AccountRecoveryCta, AccountRecoveryDestination,
	AccountRecoveryNudgeOperation, AccountRecoveryNudgeResult, AccountRecoveryNudgeStatus,
	AccountRecoveryPreparation, AccountRecoveryResult, AccountRecoveryState,
};
pub use reset_card_recovery::{AccountResetCardOperationResult, ResetCardOperationView};
mod wire;
pub use voice::{AgentVoicePhase, AgentVoiceRequest, AgentVoiceStatus, VoiceSdp};

pub use self::{
	account_login::{
		AccountLoginContractError, AccountLoginFailure, AccountLoginInstallMode,
		AccountLoginMethod, AccountLoginPrompt, AccountLoginRequest, AccountLoginRequestEnvelope,
		AccountLoginResponseEnvelope, AccountLoginStart, AccountLoginState, AccountLoginStatus,
		AccountLoginUrl, MAX_ACCOUNT_LOGIN_URL_BYTES,
	},
	agent::{
		AgentDependencyDto, AgentDispatchStateDto, AgentPendingEventDto, AgentSnapshotDto,
		AgentSnapshotResult, AgentWorkItemDto, AgentWorkKindDto, AgentWorkStatusDto,
		MAX_AGENT_DEPENDENCIES, MAX_AGENT_PENDING_EVENTS, MAX_AGENT_SNAPSHOT_BYTES,
		MAX_AGENT_WORK_ITEMS,
	},
	client::{
		AccountClient, AccountCommandResponse, AccountLoginClient, AgentClient,
		AgentCommandResponse, ClientFailure, ClientProfile, DoctorClient, ProfileKind,
		ResetCardClient, ResetCardConsumeResponse,
	},
	conversation::{
		ConversationContractError, ConversationExecutionSettings, ConversationListCursor,
		ConversationListPage, ConversationListResult, ConversationListSize, ConversationModel,
		ConversationProgramContext, ConversationReadError, ConversationReasoningEffort,
		ConversationRecoveryAction, ConversationResult, ConversationState, ConversationSummary,
		ConversationTitle, ConversationTurnOutcome, ConversationUnavailableReason,
		ConversationWorkingDirectory, CustomReasoningEffort, MAX_CONVERSATION_LIST_SIZE,
		MAX_CONVERSATION_MODEL_BYTES, MAX_CONVERSATION_TITLE_BYTES,
		MAX_CONVERSATION_WORKING_DIRECTORY_BYTES, MAX_PROVIDER_THREAD_ID_BYTES, ProviderThreadId,
	},
	doctor::{
		AppServerCapability, DoctorCheck, DoctorComponent, DoctorContractError, DoctorIssue,
		DoctorReport, DoctorStatus, MAX_DOCTOR_CHECKS,
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
	program_cycle::{
		MAX_PROGRAM_EDGES, MAX_PROGRAM_LIST_ITEMS, MAX_PROGRAM_LIST_VALUES, MAX_PROGRAM_NODES,
		ProgramCycleContractError, ProgramCycleDto, ProgramCycleResult, ProgramEdgeDto,
		ProgramListResult, ProgramNodeDto, ProgramNodeFieldDto, ProgramNodeKind,
		ProgramRelationKind, ProgramReviewClassification, ProgramState, ProgramSummaryDto,
	},
	retained_session::{
		ApplicationConfirmation, RetainedSession, RetainedSessionConfig, RetainedSessionFailure,
		SessionCancellation, SessionCheckpoint, SessionDelivery,
	},
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

use serde::{Deserialize, Serialize};

use decodex_core::FoundationStatus;

/// Exact service-tier identity shared with the provider and persistence boundaries.
pub use decodex_core::ServiceTier;

/// The only protocol generation and revision accepted by this build.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 2, minor: 96 };
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

/// The compile-time service announcement used before a socket is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceAnnouncement {
	/// Application protocol version selected by the service.
	pub version: ProtocolVersion,
	/// Current authority-bearing adapter status.
	pub foundation: FoundationStatus,
}

#[cfg(test)]
mod tests {
	use crate::{CURRENT_VERSION, ProtocolVersion};

	#[test]
	fn only_the_exact_current_version_is_accepted() {
		assert_eq!(CURRENT_VERSION.negotiate(), Ok(CURRENT_VERSION));
		assert_eq!(ProtocolVersion { major: 2, minor: 42 }.negotiate(), Err(CURRENT_VERSION));
		assert_eq!(ProtocolVersion { major: 2, minor: 40 }.negotiate(), Err(CURRENT_VERSION));
		assert_eq!(ProtocolVersion { major: 2, minor: 16 }.negotiate(), Err(CURRENT_VERSION));
	}

	#[test]
	fn any_version_mismatch_requires_the_one_current_version() {
		let requested = ProtocolVersion { major: 1, minor: 5 };

		assert_eq!(requested.negotiate(), Err(CURRENT_VERSION));
	}

	#[cfg(any(target_os = "linux", target_os = "macos"))]
	#[test]
	fn local_transport_authority_accepts_only_the_process_effective_uid() {
		use crate::{LocalTransportAuthority, LocalTransportRefusal};
		use decodex_core::{DecodexRoot, LocalTrustPolicy};

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

mod mcp_elicitation;
pub use mcp_elicitation::{
	McpFormChoice, McpFormField, mcp_form_content, mcp_form_fields, mcp_request_fields,
	validate_mcp_response,
};

mod weather;
pub use weather::WeatherForecast;
mod agent_execution;
pub use agent_execution::AgentExecutionOverrides;
mod agent_steer;
pub use agent_steer::{AgentSteerIdentity, AgentSteerReceiptResult};
mod desktop_drafts;
mod desktop_ordinary_drafts;
pub use decodex_core::{
	ClientDraftError, ClientDraftSnapshot, ClientDraftStore, MAX_CLIENT_DRAFT_BYTES,
};
pub use desktop_drafts::{
	DesktopComposerDraft, DesktopCreationIntent, DesktopCreationSetup, DesktopDraftDocument,
	DesktopPendingDraft, DesktopProfileDraft, DesktopQuestionDraft, DesktopRecoveredDraft,
};
pub use desktop_ordinary_drafts::{DesktopOrdinaryComposerDraft, DesktopOrdinaryDraft};

pub use conversation::{ConversationExecutionOverrides, ConversationModelSettingsResult};
