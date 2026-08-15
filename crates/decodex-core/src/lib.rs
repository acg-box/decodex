//! Domain, application, configuration, and owned local-storage foundations for Decodex vNext.

mod account;
mod agent;
mod automation;
mod automation_delivery;
mod blob;
mod cache;
mod config;
mod context_revision;
mod continuation;
mod conversation;
mod execution;
mod experiment;
mod identity;
mod managed_repository;
mod managed_run;
#[cfg(unix)] mod path_unix;
mod paths;
mod policy;
mod process_generation;
mod program;
mod project;
mod provider_attempt;
mod quota;
mod reset_card;
mod routing;
mod storage;
mod wake;
mod work_item;

pub use self::{
	account::{
		AccountError, AccountId, AccountLifecycleReadiness, AccountOperation, AccountOperationId,
		AccountOperationKind, AccountOperationPhase, AccountOperationStatus, AccountProvider,
		AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
		AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
		AccountSelectionRecovery, AccountState, CredentialBinding, CredentialFingerprint,
		CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
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
	cache::{
		BoundedCache, CacheLimits, CacheUsage, MAX_CACHE_BYTES, MAX_CACHE_ENTRIES,
		MAX_CACHE_ENTRY_BYTES,
	},
	config::{
		CacheConfig, ConfigError, DecodexClientConfig, DecodexConfig, LocalProfile,
		LocalTrustPolicy, MAX_CONFIG_BYTES, ProfileName, RemoteProfile, ServerProfile,
	},
	context_revision::{
		ContextRevision, ContextRevisionDecision, ContextRevisionError, ContextRevisionId,
		ContextRevisionItem, ContextRevisionItemId, ContextRevisionItemKind,
		ContextRevisionItemProvenance, ContextRevisionNumber, ContextRevisionOperation,
		ContextRevisionOwner, ContextRevisionReference, ContextRevisionSource,
		MAX_CONTEXT_REVISION_BYTES, MAX_CONTEXT_REVISION_ITEM_BYTES, MAX_CONTEXT_REVISION_ITEMS,
		decide_create_context_revision, decide_pin_context_item, decide_supersede_context_revision,
		decide_unpin_context_item,
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
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_INLINE_HISTORY_BYTES, MIN_CONTEXT_PACK_BYTES,
		NormalizedPayload, PinnedContextSource, PossibleSideEffects, ProfileSnapshot,
		ProposedTransition, ProposedTransitionKind, RuntimeSession, RuntimeSessionId,
		RuntimeSessionState, Turn, TurnId, TurnRole, TurnStatus, compile_context_pack,
		contains_credential_material, is_canonical_media_type, is_credential_metadata_key,
	},
	execution::ExecutionConsumer,
	experiment::{
		CodexExperimentCommandOutcome, CodexExperimentCreationPossible, CodexExperimentIdentity,
		CodexExperimentObservation, CodexExperimentObservationKind, CodexExperimentPrepared,
		CodexExperimentRejection, CodexExperimentRetainedTitleAttestation, CodexExperimentState,
		CodexExperimentThreadBinding, CodexExperimentTitleSetPossible,
	},
	identity::ServerIdentity,
	managed_repository::{
		AdmissionDescriptorDigest, AdmittedRepositoryIdentity, AggregateCheckpoint,
		AllocateRepositoryCommand, AllocateRepositoryDecision, AllocationAvailabilityFacts,
		AssignmentResolution, BeginCommitCommand, BeginCommitDecision, BeginRegistrationCommand,
		BeginRegistrationDecision, BeginWorktreeReadyCommand, BeginWorktreeReadyDecision,
		CanonicalCommitIntent, CanonicalOperationDescriptor, CanonicalOperationPayload,
		CommitEvidence, CommitReadbackRequest, CommitReconciliation, ExactCommitEvidence,
		ExactRegistrationEvidence, ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence,
		ExecutorContractVersion, MAX_MANAGED_REPOSITORY_PATH_BYTES,
		MAX_MANAGED_REPOSITORY_VALUE_BYTES, MAX_REPOSITORY_ADMISSION_OBSERVATIONS,
		MAX_REPOSITORY_COMMIT_MESSAGE_BYTES, MAX_REPOSITORY_OBSERVATION_ROLES,
		MAX_REPOSITORY_REGISTRATION_ID_BYTES, ManagedRepositoryError, ManagedRepositoryFacts,
		ManagedRepositoryId, ManagedRepositoryPhase, ManagedWorktreeId, NoDispatch,
		OperationDescriptorVersion, OperationView, PersistedAbsolutePath,
		PositiveAllocationEvidence, RegistrationEvidence, RegistrationReadbackRequest,
		RegistrationReconciliation, RegistrationTarget, RepositoryAdmissionDescriptor,
		RepositoryAdmissionDescriptorVersion, RepositoryAdmissionFacts,
		RepositoryAdmittedGitLayout, RepositoryAllocationId, RepositoryAmbiguity,
		RepositoryAuthorityTip, RepositoryCommitActor, RepositoryCommitActorEmail,
		RepositoryCommitActorName, RepositoryCommitMessage, RepositoryContentRevision,
		RepositoryEvidenceId, RepositoryGitRegistrationRole, RepositoryObservationPath,
		RepositoryObservedObjectType, RepositoryOperationId, RepositoryOperationKind,
		RepositoryOperationResult, RepositoryOperationState, RepositoryPathObservation,
		RepositoryPathRegistrationRole, RepositoryProjectionUpdate, RepositoryReferenceName,
		RepositoryRegistrationId, WorktreeReadyEvidence, WorktreeReadyPolicy,
		WorktreeReadyReadbackRequest, WorktreeReadyReconciliation, commit_readback_request,
		decide_allocate, decide_begin_commit, decide_begin_registration,
		decide_begin_worktree_ready, decide_commit_readback, decide_registration_readback,
		decide_worktree_ready_readback, registration_readback_request,
		resolve_operation_assignment, worktree_ready_readback_request,
	},
	managed_run::{
		ExecutionAssignment, ExecutionAssignmentRole, ManagedRunError, ManagedRunId,
		ManagedRunIdentity, ManagedRunLifecycle, ManagedRunPhase, ManagedRunState,
		ManagedRunWaitReason,
	},
	paths::{DecodexPaths, DecodexRoot, PathError},
	policy::{
		AcceptedPolicyRevision, MAX_POLICY_PROVENANCE_BYTES, MAX_POLICY_SNAPSHOT_FIELDS,
		MAX_POLICY_SNAPSHOT_KEY_BYTES, MAX_POLICY_SNAPSHOT_VALUE_BYTES, Policy, PolicyError,
		PolicyId, PolicyProvenance, PolicyRepository, PolicyRevision, PolicyRevisionAcceptance,
		PolicyRevisionId, PolicySnapshot, PolicySnapshotValue, PolicyStatus, PolicyTimestamp,
	},
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
		MAX_PROGRAM_TEXT_BYTES, MAX_PROGRAM_TIMESTAMP_MICROSECONDS, MAX_REVIEW_CADENCE_DAYS, Objective,
		ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId, ObjectiveState, Program,
		ProgramClaimId, ProgramContext, ProgramContextDecision, ProgramContextInput,
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

/// Application-facing conversation execution port.
pub trait ConversationRuntime {
	/// Report whether the adapter can currently serve conversation requests.
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

/// Validated status of the two authority-bearing vNext foundations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FoundationStatus {
	product_state: Availability,
	conversation_runtime: Availability,
}
impl FoundationStatus {
	/// Assemble the application status through its owned ports.
	pub fn assemble(
		product_state: &impl ProductState,
		conversation_runtime: &impl ConversationRuntime,
	) -> Self {
		Self {
			product_state: product_state.availability(),
			conversation_runtime: conversation_runtime.availability(),
		}
	}

	/// Report product-state adapter availability.
	pub const fn product_state(self) -> Availability {
		self.product_state
	}

	/// Report conversation runtime availability.
	pub const fn conversation_runtime(self) -> Availability {
		self.conversation_runtime
	}

	/// Return true only when both required authority-bearing adapters are available.
	pub const fn is_operational(self) -> bool {
		matches!(self.product_state, Availability::Available)
			&& matches!(self.conversation_runtime, Availability::Available)
	}
}

#[cfg(test)]
mod tests {
	use crate::{Availability, ConversationRuntime, FoundationStatus, ProductState};

	struct Store;

	impl ProductState for Store {
		fn availability(&self) -> Availability {
			Availability::Unavailable { reason: "not wired" }
		}
	}

	struct Conversation;

	impl ConversationRuntime for Conversation {
		fn availability(&self) -> Availability {
			Availability::Unavailable { reason: "not wired" }
		}
	}

	#[test]
	fn foundation_is_not_operational_until_both_owned_ports_are_available() {
		let status = FoundationStatus::assemble(&Store, &Conversation);

		assert!(!status.is_operational());
	}
}
