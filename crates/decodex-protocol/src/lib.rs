//! Typed vNext wire contracts and same-UID local transport shared by clients and
//! `decodex serve`.

mod account_login;
mod client;
mod conversation;
mod doctor;
mod domain_pack;
mod local_transport;
mod program_cycle;
mod retained_session;
mod wire;

pub use self::{
	account_login::{
		AccountLoginContractError, AccountLoginFailure, AccountLoginInstallMode,
		AccountLoginMethod, AccountLoginPrompt, AccountLoginRequest, AccountLoginRequestEnvelope,
		AccountLoginResponseEnvelope, AccountLoginStart, AccountLoginState, AccountLoginStatus,
		AccountLoginUrl, MAX_ACCOUNT_LOGIN_URL_BYTES,
	},
	client::{
		AccountClient, AccountCommandResponse, AccountLoginClient, ClientFailure, ClientProfile,
		DoctorClient, ProfileKind, ResetCardClient, ResetCardConsumeResponse,
	},
	conversation::{
		ConversationContractError, ConversationExecutionSettings, ConversationListCursor,
		ConversationListPage, ConversationListResult, ConversationListSize, ConversationModel,
		ConversationProgramContext, ConversationReadError, ConversationReasoningEffort,
		ConversationRecoveryAction, ConversationResult, ConversationState, ConversationSummary,
		ConversationTitle, ConversationTurnOutcome, ConversationUnavailableReason,
		ConversationWorkingDirectory, MAX_CONVERSATION_LIST_SIZE, MAX_CONVERSATION_MODEL_BYTES,
		MAX_CONVERSATION_TITLE_BYTES, MAX_CONVERSATION_WORKING_DIRECTORY_BYTES,
		MAX_PROVIDER_THREAD_ID_BYTES, ProviderThreadId,
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
		ProgramContinuationDraftDto, ProgramCycleContractError, ProgramCycleDraftDto,
		ProgramCycleDto, ProgramCycleResult, ProgramEdgeDto, ProgramEvidenceDraftDto,
		ProgramListResult, ProgramNodeDto, ProgramNodeFieldDto, ProgramNodeKind,
		ProgramRelationKind, ProgramReviewClassification, ProgramReviewDraftDto, ProgramState,
		ProgramSummaryDto,
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

/// The only protocol generation and revision accepted by this build.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 2, minor: 14 };
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
	pub fn negotiate(self) -> Result<Self, ProtocolVersion> {
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
		assert_eq!(ProtocolVersion { major: 2, minor: 13 }.negotiate(), Err(CURRENT_VERSION));
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
