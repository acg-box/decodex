//! Typed vNext wire contracts and same-UID local transport shared by clients and
//! `decodexd`.

mod client;
mod doctor;
mod domain_pack;
mod local_transport;
mod program_cycle;
mod quick_task;
mod retained_session;
mod wire;

pub use self::{
	client::{
		AccountClient, AccountCommandResponse, ClientFailure, ClientProfile, DoctorClient,
		ProfileKind, ResetCardClient, ResetCardConsumeResponse, WorkItemBoardClient,
	},
	doctor::{
		AppServerCapability, DoctorCheck, DoctorComponent, DoctorContractError, DoctorIssue,
		DoctorReport, DoctorStatus, MAX_DOCTOR_CHECKS,
	},
	domain_pack::{
		DomainEntityDto, DomainEntityFieldDto, DomainPackCapabilityDto,
		DomainPackCapabilityStatus, DomainPackContractError, DomainPackDescriptorDto,
		DomainPackProjectionDto, DomainPackViewKind, DomainRelationDto,
		DEVELOPMENT_DOMAIN_PACK_ID, PAPER_INVESTMENT_DOMAIN_PACK_ID,
		MAX_DOMAIN_PACK_CAPABILITIES, MAX_DOMAIN_PACK_ENTITIES, MAX_DOMAIN_PACK_RELATIONS,
	},
	local_transport::{
		LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
		LocalTransportStream,
	},
	program_cycle::{
		MAX_PROGRAM_EDGES, MAX_PROGRAM_LIST_ITEMS, MAX_PROGRAM_LIST_VALUES, MAX_PROGRAM_NODES,
			ProgramContinuationDraftDto, ProgramCycleContractError, ProgramCycleDraftDto,
			ProgramCycleDto, ProgramCycleResult, ProgramEdgeDto, ProgramEvidenceDraftDto,
			ProgramListResult, ProgramNodeDto,
		ProgramNodeFieldDto, ProgramNodeKind, ProgramRelationKind, ProgramReviewClassification,
		ProgramReviewDraftDto, ProgramState, ProgramSummaryDto,
	},
	quick_task::{
		MAX_QUICK_TASK_LIST_SIZE, MAX_QUICK_TASK_MODEL_BYTES,
		MAX_QUICK_TASK_WORKING_DIRECTORY_BYTES, QuickTaskContractError, QuickTaskExecutionSettings,
		QuickTaskListCursor, QuickTaskListPage, QuickTaskListResult, QuickTaskListSize,
		QuickTaskModel, QuickTaskReadError, QuickTaskReasoningEffort, QuickTaskRecoveryAction,
		QuickTaskResult, QuickTaskState, QuickTaskSummary, QuickTaskTurnOutcome,
		QuickTaskUnavailableReason, QuickTaskWorkingDirectory,
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
		CorrelationId, Cursor, EntityId, EntityRevision, EventEnvelope, EventPayload,
		ExecutionConsumerDto, ExecutionDecisionDto, ExecutionDecisionQueryError,
		ExecutionDecisionResult, ExecutionQuotaExclusionDto, ExecutionQuotaWindowDto,
		ExecutionRouteBlockerDto, ExecutionRouteCauseDto, ExecutionRouteDto, HistoryArtifactId,
		HistoryArtifactReference, HistoryArtifactRevision, HistoryBlobLength, HistoryBlobReference,
		HistoryCursorToken, HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto,
		HistoryMediaType, HistoryMetadata, HistoryMetadataValue, HistoryPayloadDto,
		HistoryQueryError, HistorySideEffectState, HistoryText, HistoryTurnRole, IdempotencyKey,
		IdempotencyKeyError, MAX_ACCOUNT_PROFILE_DAILY_USAGE, MAX_HISTORY_INLINE_BYTES,
		MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE, MAX_IDEMPOTENCY_KEY_BYTES,
		MAX_PROJECT_LIST_ITEMS, MAX_RESET_CARD_ITEMS, MAX_WIRE_TEXT_BYTES,
		MAX_WORK_ITEM_BOARD_OBJECTIVES, MAX_WORK_ITEM_BOARD_PAGE_SIZE,
		MAX_WORK_ITEM_BOARD_RELATIONS, MAX_WORK_ITEM_BOARD_TITLE_BYTES, ProjectList,
		ProjectListContractError, ProjectListResult, ProjectSummary, QueryEnvelope, QueryId,
		QueryPayload, QueryResultEnvelope, QueryResultPayload, ReceiptDisposition, ReconnectMode,
		Refusal, RefusalEnvelope, ResetCardDescriptorDto, ResetCardDescriptorError, ResetCardError,
		ResetCardInventoryResult, ResetCardObservationDto, ResetCardOperationResult,
		ResetCardOutcome, ResultPayload, ResumeCursor, ServerId, ServerInstanceId, ServerMessage,
		ServerWelcome, Sha256Digest, SnapshotEnvelope, SnapshotItem, WireScalarTooLong, WireText,
		WorkItemBoardCard, WorkItemBoardContractError, WorkItemBoardLeadId,
		WorkItemBoardObjectiveId, WorkItemBoardPage, WorkItemBoardPageSize, WorkItemBoardProgramId,
		WorkItemBoardProjectId, WorkItemBoardQueryError, WorkItemBoardResult, WorkItemBoardTitle,
		WorkItemBoardWorkItemId, WorkItemPriority, WorkItemState, decode_client_message,
		encode_server_message,
	},
};

use serde::{Deserialize, Serialize};

use decodex_core::FoundationStatus;

/// The only protocol generation and revision accepted by this build.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 2, minor: 5 };
/// Build/protocol cohort that must agree across the daemon and every local consumer.
pub const CURRENT_ARTIFACT_COHORT: u32 = 2;
/// The lower bound of the exact-current protocol window.
///
/// This equals [`CURRENT_VERSION`]. The name remains to avoid an unrelated
/// public-symbol rename during the clean break.
pub const PREVIOUS_MINOR_VERSION: ProtocolVersion = CURRENT_VERSION;

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
	pub fn negotiate(self) -> Result<Self, VersionRefusal> {
		if self.major != CURRENT_VERSION.major {
			return Err(VersionRefusal::MajorMismatch {
				requested: self,
				supported: SupportedVersions::current(),
			});
		}
		if self != CURRENT_VERSION {
			return Err(VersionRefusal::UnsupportedMinor {
				requested: self,
				supported: SupportedVersions::current(),
			});
		}

		Ok(self)
	}
}

/// The server's exact-current compatibility window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SupportedVersions {
	/// Required protocol generation.
	pub major: u16,
	/// Oldest accepted minor revision.
	pub minimum_minor: u16,
	/// Newest accepted minor revision.
	pub maximum_minor: u16,
}
impl SupportedVersions {
	/// Return the compatibility window implemented by this build.
	pub const fn current() -> Self {
		Self {
			major: CURRENT_VERSION.major,
			minimum_minor: PREVIOUS_MINOR_VERSION.minor,
			maximum_minor: CURRENT_VERSION.minor,
		}
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

/// A precise version-negotiation refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum VersionRefusal {
	/// The breaking generations differ.
	MajorMismatch {
		/// Version requested by the client.
		requested: ProtocolVersion,
		/// Versions supported by the server.
		supported: SupportedVersions,
	},
	/// The generation matches but the minor is not the exact current revision.
	UnsupportedMinor {
		/// Version requested by the client.
		requested: ProtocolVersion,
		/// Versions supported by the server.
		supported: SupportedVersions,
	},
}

#[cfg(test)]
mod tests {
	use crate::{
		CURRENT_VERSION, PREVIOUS_MINOR_VERSION, ProtocolVersion, SupportedVersions, VersionRefusal,
	};

	#[test]
	fn only_the_exact_current_version_is_accepted() {
		assert_eq!(CURRENT_VERSION.negotiate(), Ok(CURRENT_VERSION));
		assert_eq!(PREVIOUS_MINOR_VERSION.negotiate(), Ok(PREVIOUS_MINOR_VERSION));
		assert!(matches!(
			ProtocolVersion { major: 2, minor: 0 }.negotiate(),
			Err(VersionRefusal::UnsupportedMinor { .. })
		));
	}

	#[test]
	fn a_major_mismatch_is_distinct_from_minor_incompatibility() {
		let requested = ProtocolVersion { major: 1, minor: 5 };

		assert_eq!(
			requested.negotiate(),
			Err(VersionRefusal::MajorMismatch {
				requested,
				supported: SupportedVersions::current(),
			})
		);
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
