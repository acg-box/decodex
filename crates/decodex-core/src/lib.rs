//! Domain, application, configuration, and owned local-storage foundations for Decodex vNext.

/// Maximum complete native Agent message and persisted approval envelope, in bytes.
pub const MAX_NATIVE_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// Two native messages plus bounded local routing metadata for a file approval.
pub const MAX_APPROVAL_ENVELOPE_BYTES: usize = 2 * MAX_NATIVE_MESSAGE_BYTES + 65536;

mod account;
mod account_alias;
pub use account_alias::account_alias_candidate;
mod agent;
mod automation;
mod automation_delivery;
mod blob;
mod cache;
mod config;
mod continuation;
mod conversation;
mod execution;
mod experiment;
mod identity;
mod managed_run;
#[cfg(unix)] mod path_unix;
mod paths;
mod policy;
mod process_generation;
mod program;
mod project;
mod provider_attempt;
mod quota;
mod repository_revision;
mod reset_card;
mod routing;
mod service_tier;
mod storage;
mod wake;
mod work_item;

pub use service_tier::{InvalidServiceTier, ServiceTier};

pub use self::{
	account::{
		AccountError, AccountId, AccountLifecycleReadiness, AccountOperation, AccountOperationId,
		AccountOperationKind, AccountOperationPhase, AccountOperationStatus, AccountProvider,
		AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
		AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
		AccountSelectionRecovery, AccountState, AccountUsageConditions, AccountUsageObservation,
		CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion,
		ProviderIdentity,
	},
	agent::{
		Agent, AgentError, AgentId, AgentRepository, AgentRole, AgentStatus,
		lead_status_for_project,
	},
	automation::{
		AutomationDedupeKey, AutomationDefinition, AutomationError, AutomationFiring,
		AutomationFiringId, AutomationFiringSource, AutomationId, AutomationOccurrenceId,
		AutomationRepositorySource, AutomationRevision, AutomationSchedule, AutomationState,
		AutomationSymbol, AutomationTarget, AutomationTimestamp, AutomationTrigger,
		MAX_AUTOMATION_RRULE_BYTES, MAX_AUTOMATION_SYMBOL_BYTES,
		MAX_AUTOMATION_TIMESTAMP_MICROSECONDS, MAX_AUTOMATION_TIMEZONE_BYTES,
		propose_automation_firing,
	},
	automation_delivery::{
		AutomationDeliveryError, AutomationDeliveryIntent, AutomationDeliveryIntentId,
		AutomationDeliveryReceipt, AutomationDeliveryReceiptId, AutomationFiringInput,
	},
	blob::{
		BlobHash, BlobInventoryCursor, BlobInventoryEntry, BlobInventoryPage, BlobStore,
		MAX_BLOB_BYTES,
	},
	cache::{CacheLimits, MAX_CACHE_BYTES, MAX_CACHE_ENTRIES, MAX_CACHE_ENTRY_BYTES},
	config::{
		CacheConfig, ConfigError, DecodexClientConfig, DecodexConfig, LocalProfile,
		LocalTrustPolicy, MAX_CONFIG_BYTES, ProfileName, RemoteProfile, ServerProfile,
	},
	continuation::{
		ContinuationCommandOutcome, ContinuationPlan, ContinuationPlanKind, ContinuationRejection,
		SameThreadContinuationEvidence,
	},
	conversation::{
		AccountSnapshot, ArtifactId, ArtifactReference, ArtifactStatus, ContextPack,
		ContextPackInput, ContextPackPolicy, ContextPackSource, ContextSourceDisposition,
		ContextSourceKind, ContextSourceManifest, Conversation, ConversationError, ConversationId,
		ConversationStatus, HistoryItem, HistoryItemId, HistoryItemKind, HistoryMediaType,
		HistoryMetadata, HistoryMetadataValue, ItemStatus, MAX_CONTEXT_PACK_BYTES,
		MAX_CONTEXT_RECENT_ITEMS, MAX_CONTEXT_SOURCE_INPUT_BYTES, MAX_CONTEXT_SOURCES,
		MAX_CONVERSATION_TITLE_BYTES, MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_INLINE_HISTORY_BYTES, MAX_PROVIDER_THREAD_ID_BYTES,
		MIN_CONTEXT_PACK_BYTES, NormalizedPayload, PinnedContextSource, PossibleSideEffects,
		ProfileSnapshot, ProposedTransition, ProposedTransitionKind, RuntimeSession,
		RuntimeSessionId, RuntimeSessionState, Turn, TurnId, TurnRole, TurnStatus,
		compile_context_pack, contains_credential_material, is_canonical_media_type,
		is_credential_metadata_key,
	},
	execution::ExecutionConsumer,
	experiment::{
		CodexExperimentCommandOutcome, CodexExperimentCreationPossible, CodexExperimentIdentity,
		CodexExperimentObservation, CodexExperimentObservationKind, CodexExperimentPrepared,
		CodexExperimentRejection, CodexExperimentRetainedTitleAttestation, CodexExperimentState,
		CodexExperimentThreadBinding, CodexExperimentTitleSetPossible,
	},
	identity::ServerIdentity,
	managed_run::{
		ExecutionAssignment, ExecutionAssignmentRole, ManagedRunError, ManagedRunId,
		ManagedRunIdentity, ManagedRunLifecycle, ManagedRunPhase, ManagedRunState,
		ManagedRunWaitReason,
	},
	paths::{DecodexPaths, DecodexRoot, PathError},
	policy::{PolicyError, PolicyId, PolicyRevision, PolicyRevisionId},
	process_generation::{
		BoundProcessGeneration, MAX_PROCESS_IDENTITY_BYTES, MAX_PROCESS_RUNNER_IDENTITY_BYTES,
		ProcessAccountQuarantine, ProcessAuthorityLossReason, ProcessBootIdentity,
		ProcessControlKind, ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
		ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessGeneration,
		ProcessGenerationAccountBinding, ProcessGenerationError, ProcessGenerationId,
		ProcessGenerationIntent, ProcessGenerationState, ProcessIdentity, ProcessIsolationKind,
		ProcessRunnerIdentity, ProcessStartIdentity,
	},
	program::{
		MAX_OBJECTIVE_CRITERIA, MAX_PROGRAM_CONTEXT_BYTES, MAX_PROGRAM_CONTEXT_DECISIONS,
		MAX_PROGRAM_NAME_BYTES, MAX_PROGRAM_OBSERVATIONS, MAX_PROGRAM_PROJECTION_NODES,
		MAX_PROGRAM_TEXT_BYTES, MAX_PROGRAM_TIMESTAMP_MICROSECONDS, MAX_REVIEW_CADENCE_DAYS,
		Objective, ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId, ObjectiveState,
		Program, ProgramClaimId, ProgramContext, ProgramContextDecision, ProgramContextInput,
		ProgramCorrelationId, ProgramError, ProgramEvidenceId, ProgramEvidenceKind, ProgramId,
		ProgramMetric, ProgramObservationId, ProgramObservationProvenance, ProgramProposalId,
		ProgramProvenance, ProgramQuietPeriod, ProgramReviewClassification, ProgramReviewId,
		ProgramSignal, ProgramState, ProgramTimestamp, ReviewCadence, compile_program_context,
	},
	project::{
		MAX_PROJECT_METADATA_FIELDS, MAX_PROJECT_METADATA_KEY_BYTES,
		MAX_PROJECT_METADATA_VALUE_BYTES, MAX_PROJECT_PATH_BYTES, MAX_REPOSITORY_IDENTITY_BYTES,
		Project, ProjectAuthority, ProjectError, ProjectId, ProjectMetadata, ProjectMetadataValue,
		ProjectRepository, ProjectRepositoryBinding, ProjectStatus, RepositoryIdentity,
		ServerProjectPath,
	},
	provider_attempt::{
		MAX_PROVIDER_EVIDENCE_IDENTITY_BYTES, MAX_PROVIDER_REQUEST_KEY_BYTES, ManagedExecutionId,
		ProviderAttempt, ProviderAttemptConsumer, ProviderAttemptError, ProviderAttemptId,
		ProviderAttemptPreparation, ProviderAttemptState, ProviderAttemptUnknownReason,
		ProviderDuplicateRisk, ProviderEvidenceId, ProviderEvidenceSource,
		ProviderPositiveEvidence, ProviderRequestId, ProviderRequestKey, ProviderRequestKeys,
		ProviderTerminalOutcome,
	},
	quota::{
		AccountQuotaClassification, AccountQuotaFacts, AccountQuotaObservation, AccountReadyAt,
		AllAccountsQuotaFacts, AuthenticationObservation, MalformedObservation,
		ObservationConfidence, ObservationDuration, ObservationInstant, ObservedQuotaWindow,
		ProbeReason, QuotaClassificationPolicy, QuotaWindowClass, QuotaWindowFact,
		QuotaWindowObservation, QuotaWindowState, QuotaWindowValueObservation, RemainingPercent,
		TimeOverflow, UnknownObservation, UnknownWindowDuration, WindowDurationObservation,
		classify_account_quota, classify_all_accounts,
	},
	repository_revision::{RepositoryContentRevision, RepositoryRevisionError},
	reset_card::{
		MAX_RESET_CARD_ITEMS, ManualResetCardAdmissionError,
		RESET_CARD_PROVIDER_BINDING_METADATA_FIELD, ResetCardConsumeOutcome, ResetCardDescriptor,
		ResetCardError, ResetCardTimestamp, admit_manual_reset_card_use,
	},
	routing::{
		AccountRegistryQuotaFact, AccountRegistryQuotaObservation, AccountRegistryRoutingDecision,
		AccountRegistryRoutingDecisionKind, AccountRegistryRoutingExclusion,
		AccountRegistryRoutingKernelError, AccountRegistryRoutingMember,
		AccountRegistryRoutingSnapshot, CodexCapability, RoutingAuthorityShape, RoutingBlocker,
		RoutingCapabilityState, RoutingCommandOutcome, RoutingDecision, RoutingDecisionCandidate,
		RoutingDecisionCause, RoutingDecisionExclusion, RoutingDecisionKind,
		RoutingDecisionQuotaFact, RoutingDecisionSnapshot, RoutingEvidenceEffect,
		RoutingKernelError, RoutingMemberDisposition, RoutingNoRouteReason, RoutingPolicyEffect,
		RoutingPolicyMember, RoutingRejection, RoutingSnapshot, RoutingSnapshotCapabilityFact,
		RoutingSnapshotMember, RoutingSnapshotQuotaFact, RoutingTimestampPrecision,
		RoutingTimestampProvenance, decide_account_registry_routing, decide_routing,
	},
	storage::StorageError,
	wake::{
		WaitingUsageWakeCommandOutcome, WaitingUsageWakeLease, WaitingUsageWakeRejection,
		WaitingUsageWakeState, WaitingUsageWakeTerminalReason, WaitingUsageWakeTransition,
		WaitingUsageWakeTransitionKind,
	},
	work_item::{
		MAX_WORK_ITEM_CRITERIA, MAX_WORK_ITEM_GRAPH_EDGES, MAX_WORK_ITEM_GRAPH_NODES,
		MAX_WORK_ITEM_OBJECTIVES, MAX_WORK_ITEM_READINESS_CONTEXT,
		MAX_WORK_ITEM_READINESS_RELATIONS, MAX_WORK_ITEM_TEXT_BYTES,
		MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS, MAX_WORK_ITEM_TITLE_BYTES, ReadinessAssessment,
		ReadinessObservations, ReadinessReason, RelatedWorkItemObservation, WorkItem,
		WorkItemCorrelationId, WorkItemEdge, WorkItemEdgeKind, WorkItemError, WorkItemId,
		WorkItemNode, WorkItemObjectiveObservation, WorkItemObjectiveRef, WorkItemPriority,
		WorkItemProgramObservation, WorkItemProgramRef, WorkItemProvenance, WorkItemState,
		WorkItemTimestamp, assess_work_item_readiness, validate_work_item_graph,
	},
};

#[cfg(test)] use tempfile as _;

/// Application-facing product-state port.
pub trait ProductState {
	/// Report whether the adapter can currently serve product-state requests.
	fn availability(&self) -> Availability;
}

/// Whether an owned subsystem can currently serve requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
	/// The subsystem can serve its owned application contract.
	Available,
	/// The subsystem is intentionally unable to serve its owned application contract.
	Unavailable {
		/// Stable human-readable explanation of the unavailable boundary.
		reason: &'static str,
	},
}

mod client_drafts;
pub use client_drafts::{
	ClientDraftError, ClientDraftSnapshot, ClientDraftStore, MAX_CLIENT_DRAFT_BYTES,
};

mod fast_mode;
pub use fast_mode::{FastModeFailure, global_fast_mode_enabled, set_global_fast_mode_enabled};
