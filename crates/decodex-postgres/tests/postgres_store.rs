//! Real PostgreSQL contract coverage for the latest Decodex schema.

#![cfg(feature = "test-support")]

#[path = "postgres_store/continuation.rs"] mod continuation;
#[cfg(feature = "test-support")]
#[path = "postgres_store/managed_repositories.rs"]
mod managed_repositories;
#[cfg(feature = "test-support")]
#[path = "postgres_store/managed_runs.rs"]
mod managed_runs;
#[path = "postgres_store/quota.rs"] mod quota;
#[path = "postgres_store/reset_cards.rs"] mod reset_cards;
#[cfg(feature = "test-support")]
#[path = "postgres_store/role_profiles.rs"]
mod role_profiles;
#[path = "postgres_store/routing_decision.rs"] mod routing_decision;
#[cfg(feature = "test-support")]
#[path = "postgres_store/runtime_sessions.rs"]
mod runtime_sessions;
#[path = "postgres_store/waiting_wake.rs"] mod waiting_wake;
#[cfg(feature = "test-support")]
#[path = "postgres_store/work_items.rs"]
mod work_items;

use std::{
	collections::{BTreeMap, HashSet},
	env,
	fs::{self, Permissions},
	os::unix::fs::PermissionsExt,
	path::{Path, PathBuf},
	str::{self, FromStr as _},
	time::Duration,
};

use ::time as _;
use deadpool_postgres as _;
use serde_json::Value;
use sha2::{self as _, Digest as _, Sha256};
use tokio::{
	task::{self, JoinSet},
	time,
};
use tokio_postgres::{Client, Config, NoTls, Row, config::Host, types::ToSql};

use decodex_core::{
	self, AcceptedPolicyRevision, ArtifactId, ArtifactStatus, Availability, BlobHash, BlobStore,
	ContextPack, ContextPackInput, ContextPackPolicy, ContextPackSource, ContextSourceKind,
	ConversationId, DecodexConfig, DecodexRoot, HistoryItemId, HistoryItemKind, HistoryMediaType,
	HistoryMetadata, HistoryMetadataValue, ItemStatus, Objective, ObjectiveCompletionEvidence,
	ObjectiveEvidenceId, ObjectiveId, ObjectiveState, PinnedContextSource, PolicyId,
	PolicyProvenance, PolicyRevision, PolicyRevisionAcceptance, PolicyRevisionId, PolicySnapshot,
	PolicySnapshotValue, PossibleSideEffects, PostgresConnectionConfig, PostgresIdentityConfig,
	ProcessExecutionAuthorization, ProcessExecutionEpochId, ProductState as _, Program,
	ProgramContextInput, ProgramCorrelationId, ProgramId, ProgramMetric, ProgramObservationId,
	ProgramObservationProvenance, ProgramProvenance, ProgramSignal, ProgramState, ProgramTimestamp,
	Project, ProjectId, ProjectMetadata, ProjectMetadataValue, ProjectRepositoryBinding,
	ProjectStatus, ProposedTransitionKind, RepositoryIdentity, ReviewCadence, RuntimeSessionId,
	RuntimeSessionState, TurnId, TurnRole, TurnStatus,
};
use decodex_postgres::{
	AccountId, AccountOperationId, AccountProvider, AccountRoutingControl, AccountSelectionMode,
	AccountState, Agent, AgentId, AgentRole, AgentStatus, BootstrapFailure, BootstrapRoleProfiles,
	CLOSED, CommandIdentity, ContextPackRecord, CreateArtifact, CreateConversation, CreateProject,
	CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, CredentialBinding,
	CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, HistoryCursor,
	LocalAccountAuthorityAccount, LocalAccountAuthorityRestore,
	LocalAccountAuthorityRestoreFailure, MAX_OPERATION_DURATION_MILLISECONDS, OutboxClaim,
	OutboxReconciliation, PersistContextPack, PostgresStore, ProposeTransition, ProviderIdentity,
	ReconciliationOutcome, RecordHistoryItem, RoleProfileCommandOutcome, RoleProfileConfiguration,
	RoleProfileRole, RuntimeSessionCommandOutcome, StoreError, UpdateProgramContext,
};

const ACCOUNT_ID: &str = "10000000-0000-0000-0000-000000000001";
const HOLDER_A: &str = "20000000-0000-0000-0000-000000000001";
const HOLDER_B: &str = "20000000-0000-0000-0000-000000000002";
const WORKER_A: &str = "30000000-0000-0000-0000-000000000001";
const WORKER_B: &str = "30000000-0000-0000-0000-000000000002";
const CREDENTIAL_VALUE_VECTORS: &[&str] = &[
	"Bearer abcdefghijklmnop",
	"Bearer\nabcdefghijklmnop",
	"Bearer\u{a0}abcdefghijklmnop",
	"Bearer\u{85}abcdefghijklmnop",
	"Basic\u{202f}dXNlcjpwYXNz",
	"password\u{3000}=\u{3000}forbidden",
	"Basic\tdXNlcjpwYXNz",
	"sk-0123456789abcdef",
	"sk_live_0123456789abcdef",
	"xoxb-1234567890-abcdef",
	"glpat-1234567890abcdef",
	"npm_1234567890abcdef",
	"ghp_01234567890123456789",
	"eyJ0123456789.abcdefghij.klmnopqrst",
	"-----BEGIN RSA PRIVATE\nKEY-----",
	"password\n=\nforbidden",
	"https://user:password@example.invalid/path",
	"AKIA0123456789ABCDEF",
];
const UNICODE_WHITESPACE_VECTORS: &[&str] = &["\u{a0}", "\u{85}", "\u{202f}", "\u{3000}"];

fn exact_profile(marker: &str) -> RoleProfileConfiguration {
	RoleProfileConfiguration {
		model: "test-model".into(),
		reasoning_effort: "medium".into(),
		service_tier: "standard".into(),
		instructions: format!("Exact {marker} fixture instructions."),
		provenance: Some("XY-1337 integration fixture".into()),
	}
}

fn exact_account_snapshot(snapshot_id: String) -> CreateRuntimeSessionAccountSnapshot {
	CreateRuntimeSessionAccountSnapshot {
		account_snapshot_id: snapshot_id,
		source_account_id: AccountId::new(ACCOUNT_ID).expect("fixture account ID is canonical"),
		display_label: "Manual A".into(),
		observed_state: AccountState::Unknown,
		source_revision: 3,
	}
}
const RUNTIME_EXECUTE_SIGNATURES: &[&str] = &[
	"decodex.is_canonical_media_type(pg_catalog.text)",
	"decodex.is_history_metadata_projection(pg_catalog.jsonb)",
	"decodex.normalize_unicode_whitespace(pg_catalog.text)",
	"decodex.ascii_lower(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.jsonb)",
	"decodex.is_meaningful_evidence(pg_catalog.jsonb)",
	"decodex.rfc3339_utc(pg_catalog.timestamptz)",
	"decodex.is_valid_operation_duration(pg_catalog.interval)",
	"decodex.lease_ttl_milliseconds(pg_catalog.interval)",
	"decodex.try_acquire_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.interval)",
	"decodex.renew_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.interval)",
	"decodex.release_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.prune_history_snapshots()",
	"decodex.issue_history_cursor(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int4)",
	"decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text)",
	"decodex.create_project(decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text)",
	"decodex.transition_project(decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.project_status)",
	"decodex.create_policy(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text)",
	"decodex.accept_policy_revision(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.int8)",
	"decodex.create_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.update_program_context(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.transition_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.program_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.create_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog._text,pg_catalog._text,pg_catalog.int8,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.transition_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.objective_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.achieve_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text)",
	"decodex.bootstrap_role_profiles_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.update_role_profile_exact(pg_catalog.text,pg_catalog.text,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.create_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,decodex.role_profile_role,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex.account_state,pg_catalog.int8,pg_catalog.uuid,decodex.runtime_session_state)",
	"decodex.transition_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.runtime_session_state)",
	"decodex.create_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.update_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,decodex.work_item_state,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.assess_work_item_readiness_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.accept_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.guard_work_item_running_resume(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.replace_routing_policy_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog._uuid,pg_catalog._int8,decodex._routing_member_disposition,decodex._codex_capability)",
	"decodex.publish_routing_evidence_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex._codex_capability,decodex._capability_evidence_state)",
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_codex_experiment_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.text)",
	"decodex.read_codex_experiment_start_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.mark_codex_experiment_title_set_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text)",
	"decodex.attest_codex_experiment_retained_title_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.record_attested_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_continuation_plan_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_execution_decision_exact(pg_catalog.uuid)",
	"decodex.read_managed_run_execution_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_waiting_usage_wake_transition_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.register_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.claim_due_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.fire_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.cancel_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.read_account_registry_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_reset_card_account_admission_exact(pg_catalog.uuid,pg_catalog.text)",
	"decodex.prepare_account_operation_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.account_operation_kind,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text)",
	"decodex.set_account_operation_target_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
	"decodex.advance_account_operation_exact(pg_catalog.uuid,decodex.account_operation_phase,decodex.account_operation_phase,pg_catalog.text)",
	"decodex.read_unsettled_account_operations_exact(pg_catalog.int8)",
	"decodex.read_account_operation_exact(pg_catalog.uuid)",
	"decodex.set_account_enabled_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.bool)",
	"decodex.set_fixed_account_selection_exact(pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.set_balanced_account_selection_exact(pg_catalog.int8)",
	"decodex.set_account_order_exact(pg_catalog.int8,pg_catalog._uuid)",
	"decodex.read_account_routing_control_exact()",
	"decodex.observe_account_quota_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int4,pg_catalog.int8,pg_catalog.int8)",
	"decodex.observe_account_quota_error_exact(pg_catalog.uuid,pg_catalog.int4,decodex.account_quota_observation_error,pg_catalog.int8)",
	"decodex.observe_account_store_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,decodex.account_store_observation)",
	"decodex.attest_codex_account_capability_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.bool)",
	"decodex.observe_account_profile_exact(pg_catalog.uuid,pg_catalog.int8,decodex.account_provider_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.int8,pg_catalog.int4,pg_catalog.int4,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_account_profile_exact(pg_catalog.uuid)",
	"decodex.prepare_process_generation_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.bind_process_generation_identity_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
	"decodex.mark_process_generation_ready_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_stopping_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_death_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.process_generation_loss_reason)",
	"decodex.record_process_generation_death_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.process_generation_death_evidence_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.text)",
	"decodex.project_process_generations_after_supervisor_loss_exact()",
	"decodex.read_process_generations_exact(pg_catalog.uuid,pg_catalog.bool,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text)",
	"decodex.authorize_provider_attempt_dispatch_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.cancel_provider_attempt_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_provider_attempt_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_unknown_reason)",
	"decodex.record_provider_attempt_positive_evidence_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.project_provider_attempts_after_supervisor_loss_exact()",
	"decodex.read_provider_attempts_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_state,pg_catalog.uuid,pg_catalog.int8)",
];
const TRIGGER_ONLY_SIGNATURES: &[&str] = &[
	"decodex.enforce_lease_operation_time()",
	"decodex.enforce_outbox_operation_time()",
	"decodex.enforce_quota_observation_monotonicity()",
	"decodex.forbid_mutation_of_activity()",
	"decodex.enforce_outbox_terminal_retention()",
	"decodex.forbid_outbox_truncate()",
	"decodex.enforce_command_receipt_state()",
	"decodex.acquire_hierarchy_coordinator()",
	"decodex.canonicalize_created_at()",
	"decodex.enforce_blob_object_state()",
	"decodex.enforce_conversation_state()",
	"decodex.enforce_runtime_session_state()",
	"decodex.enforce_turn_state()",
	"decodex.enforce_history_item_state()",
	"decodex.capture_history_item_version()",
	"decodex.enforce_artifact_state()",
	"decodex.enforce_artifact_revision_state()",
	"decodex.enforce_context_pack_state()",
	"decodex.enforce_context_pack_source_state()",
	"decodex.enforce_history_cursor_state()",
	"decodex.enforce_policy_identity_state()",
	"decodex.forbid_policy_revision_mutation()",
	"decodex.enforce_program_state()",
	"decodex.enforce_objective_state()",
	"decodex.forbid_objective_evidence_mutation()",
	"decodex.enforce_objective_completion_coherence()",
	"decodex.enforce_exact_receipt_completion()",
	"decodex.forbid_exact_receipt_rewrite()",
	"decodex.forbid_exact_receipt_truncate()",
	"decodex.enforce_complete_role_profile_set()",
	"decodex.forbid_role_profile_identity_rewrite()",
	"decodex.forbid_role_profile_revision_mutation()",
	"decodex.forbid_role_profile_truncate()",
	"decodex.enforce_role_profile_event_namespace()",
	"decodex.enforce_runtime_session_command_owner()",
	"decodex.forbid_runtime_snapshot_mutation()",
	"decodex.enforce_runtime_session_event_namespace()",
	"decodex.enforce_work_item_state()",
	"decodex.enforce_work_item_command_owner()",
	"decodex.forbid_work_item_acceptance_mutation()",
	"decodex.enforce_work_item_acceptance_coherence()",
	"decodex.enforce_work_item_event_namespace()",
	"decodex.enforce_managed_run_command_owner()",
	"decodex.forbid_managed_run_immutable_mutation()",
	"decodex.enforce_managed_run_assignment_scope()",
	"decodex.enforce_managed_run_state()",
	"decodex.enforce_managed_run_event_namespace()",
	"decodex.forbid_managed_repository_history_mutation()",
	"decodex.enforce_managed_repository_projection()",
	"decodex.enforce_repository_operation_scope()",
	"decodex.enforce_repository_history_completeness()",
	"decodex.forbid_routing_history_mutation()",
	"decodex.enforce_routing_completeness()",
	"decodex.enforce_routing_command_owner()",
	"decodex.forbid_codex_experiment_history_mutation()",
	"decodex.enforce_codex_experiment_command_owner()",
	"decodex.forbid_routing_decision_mutation()",
	"decodex.enforce_routing_decision_completeness()",
	"decodex.forbid_continuation_plan_mutation()",
	"decodex.enforce_continuation_plan_completeness()",
	"decodex.enforce_continuation_event_namespace()",
	"decodex.enforce_waiting_usage_wake_command_owner()",
	"decodex.forbid_waiting_usage_wake_transition_mutation()",
	"decodex.enforce_waiting_usage_wake_transition_complete()",
	"decodex.enforce_waiting_usage_wake_head_projection()",
	"decodex.enforce_waiting_usage_wake_event_namespace()",
	"decodex.enforce_process_generation_transition()",
	"decodex.record_process_generation_transition()",
	"decodex.forbid_process_generation_history_mutation()",
	"decodex.enforce_provider_attempt_transition()",
	"decodex.enforce_provider_attempt_binding()",
	"decodex.record_provider_attempt_transition()",
	"decodex.enforce_provider_attempt_turn_materialization()",
	"decodex.forbid_provider_attempt_history_mutation()",
];
const INVALID_PROJECT_AGENT_SQL_CALLS: &[(&str, &str)] = &[
	(
		"SELECT * FROM decodex.bootstrap_advisor('00000000-0000-0000-0000-000000000000')",
		"canonical_uuid_v4_text_exact",
	),
	(
		"SELECT * FROM decodex.bootstrap_advisor('21000000-0000-1000-8000-000000000099')",
		"canonical_uuid_v4_text_exact",
	),
	(
		"SELECT * FROM decodex.create_project('00000000-0000-0000-0000-000000000000',\
		 'hack-ink/invalid-project-id','/srv/repos/invalid-project-id',\
		 '/srv/repos/invalid-project-id','{}','22000000-0000-4000-8000-000000000020')",
		"canonical_uuid_v4_text_exact",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000021',\
		 'hack-ink/invalid-lead-id','/srv/repos/invalid-lead-id',\
		 '/srv/repos/invalid-lead-id','{}','22000000-0000-1000-8000-000000000021')",
		"canonical_uuid_v4_text_exact",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000022',\
		 'hack-ink/backslash-path',E'/srv/repos/bad\\\\path',E'/srv/repos/bad\\\\path',\
		 '{}','22000000-0000-4000-8000-000000000022')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000023',\
		 'hack-ink/dot-dot-path','/srv/repos/canonical','/srv/repos/canonical/../bad',\
		 '{}','22000000-0000-4000-8000-000000000023')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000030',\
		 'hack-ink/dot-path','/srv/./repos/dot','/srv/./repos/dot',\
		 '{}','22000000-0000-4000-8000-000000000030')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000031',\
		 'hack-ink/lf-path',E'/srv/repos/line\\nfeed',E'/srv/repos/line\\nfeed',\
		 '{}','22000000-0000-4000-8000-000000000031')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000032',\
		 'hack-ink/del-path',U&'/srv/repos/del\\007Fcontrol',U&'/srv/repos/del\\007Fcontrol',\
		 '{}','22000000-0000-4000-8000-000000000032')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000033',\
		 'hack-ink/c1-path',U&'/srv/repos/c1\\0085control',U&'/srv/repos/c1\\0085control',\
		 '{}','22000000-0000-4000-8000-000000000033')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000024',\
		 'hack-ink/unicode-path','/' || repeat('é',2048),\
		 '/' || repeat('é',2048),'{}','22000000-0000-4000-8000-000000000024')",
		"projects_paths_bounded_absolute",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000025',\
		 'hack-ink/control-metadata','/srv/repos/control-metadata',\
		 '/srv/repos/control-metadata',jsonb_build_object('note',E'line\\nfeed'),\
		 '22000000-0000-4000-8000-000000000025')",
		"projects_metadata_bounded",
	),
	(
		"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000027',\
		 'hack-ink/unicode-control-metadata','/srv/repos/unicode-control-metadata',\
		 '/srv/repos/unicode-control-metadata',jsonb_build_object('note',U&'before\\0085after'),\
		 '22000000-0000-4000-8000-000000000027')",
		"projects_metadata_bounded",
	),
];

struct ConversationFixture {
	blob_store: BlobStore,
	conversation_id: ConversationId,
	session_a_id: RuntimeSessionId,
	session_b_id: RuntimeSessionId,
}

fn expected_peer_uid() -> u32 {
	// SAFETY: `geteuid` has no arguments, retained pointers, or failure mode.
	unsafe { libc::geteuid() }
}

fn owner_runtime_configs(prefix: &str) -> Result<(Config, Config), Box<dyn std::error::Error>> {
	let schema_owner = Config::from_str(&env::var(format!("{prefix}_SCHEMA_OWNER_DATABASE_URL"))?)?;
	let runtime = Config::from_str(&env::var(format!("{prefix}_RUNTIME_DATABASE_URL"))?)?;

	Ok((schema_owner, runtime))
}

fn latest_schema_connection_config(
	runtime: &Config,
) -> Result<PostgresConnectionConfig, Box<dyn std::error::Error>> {
	let socket_directory = match runtime.get_hosts() {
		[Host::Unix(path)] => path.to_str().ok_or("runtime socket path is not UTF-8")?,
		_ => return Err("runtime fixture requires exactly one Unix socket host".into()),
	};
	let port = match runtime.get_ports() {
		[] => 5_432,
		[port] => *port,
		_ => return Err("runtime fixture requires exactly one PostgreSQL port".into()),
	};
	let database = runtime.get_dbname().ok_or("runtime database is absent")?;
	let runtime_role = runtime.get_user().ok_or("runtime role is absent")?;
	let socket_directory = serde_json::to_string(socket_directory)?;
	let database = serde_json::to_string(database)?;
	let runtime_role = serde_json::to_string(runtime_role)?;
	let source = format!(
		r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}

[postgres]
socket_directory = {socket_directory}
expected_peer_uid = {postgres_uid}
port = {port}
database = {database}

[postgres.runtime]
user = {runtime_role}

[cache]
max_entries = 1
max_bytes = 1
max_entry_bytes = 1
"#,
		service_owner_uid = expected_peer_uid(),
		postgres_uid = expected_peer_uid(),
	);

	Ok(DecodexConfig::parse(source.as_bytes())?.postgres().clone())
}

fn config_password(config: &Config) -> Result<Option<&str>, Box<dyn std::error::Error>> {
	config.get_password().map(str::from_utf8).transpose().map_err(Into::into)
}

#[cfg(feature = "test-support")]
async fn account_routing_projection(
	transaction: &tokio_postgres::Transaction<'_>,
) -> Result<(String, Option<String>, i64, Vec<String>), tokio_postgres::Error> {
	let row = transaction
		.query_one(
			"SELECT mode::text,fixed_account_id::text,revision, \
			 ARRAY(SELECT value::text FROM pg_catalog.unnest(account_order) AS value) \
			 FROM decodex.read_account_routing_control_exact()",
			&[],
		)
		.await?;
	Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
}

#[cfg(feature = "test-support")]
fn assert_account_routing_universe_error(error: &tokio_postgres::Error) {
	assert_eq!(
		error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
		Some("account_routing_universe_complete"),
	);
}

fn isolated_blob_store() -> Result<BlobStore, Box<dyn std::error::Error>> {
	let blob_root = env::var("DECODEX_TEST_BLOB_ROOT")?;

	Ok(BlobStore::open(DecodexRoot::new(blob_root)?.paths())?)
}

fn history_media_type(value: &str) -> HistoryMediaType {
	HistoryMediaType::new(value).expect("fixture media type is canonical")
}

fn history_metadata(value: Value) -> HistoryMetadata {
	serde_json::from_value(value).expect("fixture metadata is canonical")
}

fn blob_shard_path(blob_store: &BlobStore, hash: BlobHash) -> Result<PathBuf, std::io::Error> {
	let candidate_path = blob_store.path_for(hash);

	candidate_path
		.parent()
		.map(Path::to_path_buf)
		.ok_or_else(|| std::io::Error::other("blob path has no shard parent"))
}

fn project_request(
	project_id: &str,
	lead_id: &str,
	repository_identity: &str,
	repository_root: &str,
) -> CreateProject {
	let project_id = ProjectId::new(project_id).expect("Project fixture ID is canonical");
	let project = Project::new(
		project_id.clone(),
		ProjectRepositoryBinding::new(
			RepositoryIdentity::new(repository_identity)
				.expect("repository fixture identity is canonical"),
			PathBuf::from(repository_root),
			PathBuf::from(repository_root),
		)
		.expect("repository fixture paths are canonical"),
		ProjectMetadata::empty(),
	);
	let lead =
		Agent::lead(AgentId::new(lead_id).expect("Lead fixture ID is canonical"), project_id);

	CreateProject { project, lead }
}

fn policy_acceptance(
	project_id: &ProjectId,
	policy_id: &PolicyId,
	accepted_by: &AgentId,
	revision: u64,
	marker: &str,
) -> PolicyRevisionAcceptance {
	let revision_number = PolicyRevision::new(revision).expect("Policy revision is positive");
	let id = PolicyRevisionId::new(project_id.clone(), policy_id.clone(), revision_number);
	let supersedes = revision.checked_sub(1).filter(|value| *value > 0).map(|previous| {
		PolicyRevisionId::new(
			project_id.clone(),
			policy_id.clone(),
			PolicyRevision::new(previous).expect("previous Policy revision is positive"),
		)
	});

	PolicyRevisionAcceptance {
		id,
		provenance: PolicyProvenance::new(format!("accepted fixture {marker}"))
			.expect("Policy provenance is bounded"),
		snapshot: PolicySnapshot::new(BTreeMap::from([
			("marker".into(), PolicySnapshotValue::Text(marker.into())),
			("reviewed".into(), PolicySnapshotValue::Boolean(true)),
		]))
		.expect("Policy snapshot is bounded"),
		accepted_by: accepted_by.clone(),
		supersedes,
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh isolated PostgreSQL 18 latest-schema target"]
async fn postgres_latest_schema_bootstrap_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	if schema_owner.get_hosts() != runtime.get_hosts()
		|| schema_owner.get_ports() != runtime.get_ports()
		|| schema_owner.get_dbname() != runtime.get_dbname()
	{
		return Err("schema-owner and runtime fixtures must select one database endpoint".into());
	}
	let config = latest_schema_connection_config(&runtime)?;
	let schema_owner_identity = PostgresIdentityConfig::new(
		schema_owner.get_user().ok_or("schema-owner role is absent")?.to_owned(),
		None,
	)?;
	let authorization = ProcessExecutionAuthorization::new(
		ProcessExecutionEpochId::new("d1000000-0000-4000-8000-000000001276")?,
		"f".repeat(64),
	)?;

	PostgresStore::bootstrap_latest_schema_explicit(
		&config,
		&schema_owner_identity,
		config_password(&schema_owner)?,
		&authorization,
	)
	.await?;
	let store =
		PostgresStore::connect_runtime_explicit(&config, config_password(&runtime)?).await?;
	assert_eq!(store.availability(), Availability::Available);
	assert!(matches!(
		PostgresStore::bootstrap_latest_schema_explicit(
			&config,
			&schema_owner_identity,
			config_password(&schema_owner)?,
			&authorization,
		)
		.await,
		Err(error) if error.bootstrap_failure() == BootstrapFailure::Incompatible
			&& error.report_json().is_none()
	));
	store.close();

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh isolated PostgreSQL 18 local account restore target"]
async fn postgres_local_account_authority_restore_is_atomic_and_exact()
-> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let config = latest_schema_connection_config(&runtime)?;
	let schema_owner_identity = PostgresIdentityConfig::new(
		schema_owner.get_user().ok_or("schema-owner role is absent")?.to_owned(),
		None,
	)?;
	let authorization = ProcessExecutionAuthorization::new(
		ProcessExecutionEpochId::new("d2000000-0000-4000-8000-000000001276")?,
		"e".repeat(64),
	)?;
	PostgresStore::bootstrap_latest_schema_explicit(
		&config,
		&schema_owner_identity,
		config_password(&schema_owner)?,
		&authorization,
	)
	.await?;

	let account_id = AccountId::new("72000000-0000-4000-8000-000000001276")?;
	let provider =
		ProviderIdentity::new(AccountProvider::Chatgpt, "restore-contract@example.invalid")?;
	let credential = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::V1,
		version: CredentialVersion::new(7)?,
		fingerprint: CredentialFingerprint::new("a".repeat(64))?,
		provider,
		writer_operation_id: AccountOperationId::new("73000000-0000-4000-8000-000000001276")?,
	};
	let restore = LocalAccountAuthorityRestore {
		accounts: vec![LocalAccountAuthorityAccount {
			account_id: account_id.clone(),
			display_label: "Alex".into(),
			enabled: false,
			revision: 11,
			credential: credential.clone(),
		}],
		routing: AccountRoutingControl {
			revision: 9,
			mode: AccountSelectionMode::Fixed(account_id.clone()),
			order: vec![account_id.clone()],
		},
	};

	let mismatch = PostgresStore::restore_local_account_authority_explicit(
		&config,
		&schema_owner_identity,
		config_password(&schema_owner)?,
		&restore,
		|| false,
	)
	.await;
	assert_eq!(mismatch, Err(LocalAccountAuthorityRestoreFailure::PrecommitFence));
	let (rollback_client, rollback_connection) = schema_owner.clone().connect(NoTls).await?;
	let rollback_task = tokio::spawn(rollback_connection);
	let rolled_back: bool = rollback_client
		.query_one(
			"SELECT NOT EXISTS (SELECT 1 FROM decodex.accounts)\
			 AND NOT EXISTS (SELECT 1 FROM decodex.account_routing_order)\
			 AND EXISTS (SELECT 1 FROM decodex.account_routing_control\
			 WHERE singleton AND mode='balanced' AND fixed_account_id IS NULL AND revision=1)",
			&[],
		)
		.await?
		.get(0);
	assert!(rolled_back, "a failed exact host-store fence must roll back every row");
	drop(rollback_client);
	rollback_task.await??;

	PostgresStore::restore_local_account_authority_explicit(
		&config,
		&schema_owner_identity,
		config_password(&schema_owner)?,
		&restore,
		|| true,
	)
	.await?;
	let store =
		PostgresStore::connect_runtime_explicit(&config, config_password(&runtime)?).await?;
	let (accounts, routing) = store.read_account_registry_snapshot(512).await?;
	assert_eq!(accounts.len(), 1);
	assert_eq!(accounts[0].account_id, account_id);
	assert_eq!(accounts[0].label, "Alex");
	assert!(!accounts[0].enabled);
	assert_eq!(accounts[0].revision, 11);
	assert_eq!(accounts[0].credential.as_ref(), Some(&credential));
	assert_eq!(routing, restore.routing);
	store.close();

	let second = PostgresStore::restore_local_account_authority_explicit(
		&config,
		&schema_owner_identity,
		config_password(&schema_owner)?,
		&restore,
		|| true,
	)
	.await;
	assert_eq!(second, Err(LocalAccountAuthorityRestoreFailure::TargetNotFresh));

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 V27 routing harness"]
#[cfg(feature = "test-support")]
#[allow(clippy::too_many_lines)] // One complete routing CAS and invariant contract.
async fn postgres_account_routing_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (mut schema_owner, _) = owner_runtime_configs("DECODEX_TEST")?;
	PostgresStore::apply_trusted_session_invariants_fixture(&mut schema_owner);
	let (mut client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let transaction = client.transaction().await?;
	let first_account = "72000000-0000-4000-8000-000000000001";
	let second_account = "72000000-0000-4000-8000-000000000002";
	let first_writer = "73000000-0000-4000-8000-000000000001";
	let second_writer = "73000000-0000-4000-8000-000000000002";
	let first_fingerprint = "a".repeat(64);
	let second_fingerprint = "b".repeat(64);

	transaction
		.batch_execute(
			"UPDATE decodex.account_routing_control SET mode='balanced',fixed_account_id=NULL,\
			 revision=revision+1,updated_at=pg_catalog.clock_timestamp() WHERE singleton;\
			 DELETE FROM decodex.account_routing_order;\
			 UPDATE decodex.accounts SET enabled=false,credential_store_schema_version=NULL,\
			 credential_version=NULL,credential_fingerprint=NULL,\
			 credential_writer_operation_id=NULL,credential_store_observation='missing',\
			 credential_store_observed_at=pg_catalog.clock_timestamp(),\
			 tombstoned_at=pg_catalog.clock_timestamp(),revision=revision+1,\
			 updated_at=pg_catalog.clock_timestamp()",
		)
		.await?;
	for (account_id, label, provider_id, fingerprint, writer) in [
		(
			first_account,
			"Routing A",
			"routing-contract-a",
			first_fingerprint.as_str(),
			first_writer,
		),
		(
			second_account,
			"Routing B",
			"routing-contract-b",
			second_fingerprint.as_str(),
			second_writer,
		),
	] {
		transaction
			.execute(
				"INSERT INTO decodex.accounts(\
				 account_id,display_label,state,enabled,provider_kind,provider_account_id,\
				 credential_store_schema_version,credential_version,credential_fingerprint,\
				 credential_writer_operation_id,credential_store_observation,\
				 credential_store_observed_at\
				 ) VALUES (\
				 $1::text::uuid,$2,'available',true,'chatgpt',$3,1,1,$4,$5::text::uuid,\
				 'exact',pg_catalog.clock_timestamp())",
				&[&account_id, &label, &provider_id, &fingerprint, &writer],
			)
			.await?;
		transaction
			.execute(
				"INSERT INTO decodex.account_routing_order(account_id,position) \
				 SELECT $1::text::uuid,COALESCE(pg_catalog.max(position)+1,0) \
				 FROM decodex.account_routing_order",
				&[&account_id],
			)
			.await?;
	}
	transaction
		.execute(
			"UPDATE decodex.account_routing_control SET revision=revision+1,\
			 updated_at=pg_catalog.clock_timestamp() WHERE singleton",
			&[],
		)
		.await?;

	let (_, _, seeded_revision, seeded_order) = account_routing_projection(&transaction).await?;
	let fixed = transaction
		.query_one(
			"SELECT result_code,routing_revision,account_revision \
			 FROM decodex.set_fixed_account_selection_exact($1,$2::text::uuid,$3)",
			&[&seeded_revision, &first_account, &1_i64],
		)
		.await?;
	assert_eq!(fixed.get::<_, &str>(0), "updated");
	assert_eq!(fixed.get::<_, i64>(2), 1);
	let (mode, fixed_target, fixed_revision, fixed_order) =
		account_routing_projection(&transaction).await?;
	assert_eq!(fixed_revision, fixed.get::<_, i64>(1));
	assert!(fixed_revision > seeded_revision);
	assert_eq!(mode, "fixed");
	assert_eq!(fixed_target.as_deref(), Some(first_account));
	assert_eq!(fixed_order, seeded_order);
	let fixed_no_change = transaction
		.query_one(
			"SELECT result_code,routing_revision,account_revision \
			 FROM decodex.set_fixed_account_selection_exact($1,$2::text::uuid,$3)",
			&[&fixed_revision, &first_account, &1_i64],
		)
		.await?;
	assert_eq!(fixed_no_change.get::<_, &str>(0), "updated");
	assert_eq!(fixed_no_change.get::<_, i64>(1), fixed_revision);

	let stale_routing = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_balanced_account_selection_exact($1)",
			&[&seeded_revision],
		)
		.await?;
	assert_eq!(stale_routing.get::<_, &str>(0), "stale_routing_control");
	assert_eq!(stale_routing.get::<_, i64>(1), fixed_revision);

	let stale_account = transaction
		.query_one(
			"SELECT result_code,routing_revision,account_revision \
			 FROM decodex.set_fixed_account_selection_exact($1,$2::text::uuid,$3)",
			&[&fixed_revision, &first_account, &2_i64],
		)
		.await?;
	assert_eq!(stale_account.get::<_, &str>(0), "stale_account");
	assert_eq!(stale_account.get::<_, i64>(1), fixed_revision);
	assert_eq!(stale_account.get::<_, i64>(2), 1);

	let reversed_order = seeded_order.iter().rev().cloned().collect::<Vec<_>>();
	let ordered = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_account_order_exact($1,$2::text[]::uuid[])",
			&[&fixed_revision, &reversed_order],
		)
		.await?;
	assert_eq!(ordered.get::<_, &str>(0), "updated");
	let (mode, fixed_target, ordered_revision, current_order) =
		account_routing_projection(&transaction).await?;
	assert_eq!(ordered_revision, ordered.get::<_, i64>(1));
	assert_eq!(mode, "fixed");
	assert_eq!(fixed_target.as_deref(), Some(first_account));
	assert_eq!(current_order, reversed_order);
	let order_no_change = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_account_order_exact($1,$2::text[]::uuid[])",
			&[&ordered_revision, &reversed_order],
		)
		.await?;
	assert_eq!(order_no_change.get::<_, &str>(0), "updated");
	assert_eq!(order_no_change.get::<_, i64>(1), ordered_revision);

	let incomplete_order = reversed_order[..reversed_order.len().saturating_sub(1)].to_vec();
	let incomplete = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_account_order_exact($1,$2::text[]::uuid[])",
			&[&ordered_revision, &incomplete_order],
		)
		.await?;
	assert_eq!(incomplete.get::<_, &str>(0), "invalid_order");
	assert_eq!(incomplete.get::<_, i64>(1), ordered_revision);
	assert_eq!(account_routing_projection(&transaction).await?.3, reversed_order);

	let balanced = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_balanced_account_selection_exact($1)",
			&[&ordered_revision],
		)
		.await?;
	assert_eq!(balanced.get::<_, &str>(0), "updated");
	let (mode, fixed_target, balanced_revision, balanced_order) =
		account_routing_projection(&transaction).await?;
	assert_eq!(balanced_revision, balanced.get::<_, i64>(1));
	assert_eq!(mode, "balanced");
	assert_eq!(fixed_target, None);
	assert_eq!(balanced_order, reversed_order);
	let balanced_no_change = transaction
		.query_one(
			"SELECT result_code,routing_revision \
			 FROM decodex.set_balanced_account_selection_exact($1)",
			&[&balanced_revision],
		)
		.await?;
	assert_eq!(balanced_no_change.get::<_, &str>(0), "updated");
	assert_eq!(balanced_no_change.get::<_, i64>(1), balanced_revision);

	transaction.batch_execute("SAVEPOINT missing_routing_member").await?;
	transaction
		.execute(
			"DELETE FROM decodex.account_routing_order WHERE account_id=$1::text::uuid",
			&[&second_account],
		)
		.await?;
	transaction.batch_execute("SAVEPOINT missing_readback").await?;
	let missing_readback = transaction
		.query_one("SELECT * FROM decodex.read_account_routing_control_exact()", &[])
		.await
		.expect_err("strict routing readback must reject a missing visible member");
	assert_account_routing_universe_error(&missing_readback);
	transaction
		.batch_execute("ROLLBACK TO SAVEPOINT missing_readback; RELEASE SAVEPOINT missing_readback")
		.await?;
	transaction.batch_execute("SAVEPOINT missing_fixed").await?;
	let missing_fixed = transaction
		.query_one(
			"SELECT * FROM decodex.set_fixed_account_selection_exact($1,$2::text::uuid,$3)",
			&[&balanced_revision, &first_account, &1_i64],
		)
		.await
		.expect_err("fixed selection must reject a partial stored routing universe");
	assert_account_routing_universe_error(&missing_fixed);
	transaction
		.batch_execute("ROLLBACK TO SAVEPOINT missing_fixed; RELEASE SAVEPOINT missing_fixed")
		.await?;
	transaction
		.batch_execute(
			"ROLLBACK TO SAVEPOINT missing_routing_member; \
			 RELEASE SAVEPOINT missing_routing_member",
		)
		.await?;

	transaction.batch_execute("SAVEPOINT tombstoned_routing_member").await?;
	transaction
		.execute(
			"INSERT INTO decodex.accounts(\
			 account_id,display_label,state,enabled,tombstoned_at\
			 ) VALUES ($1::text::uuid,'Tombstoned routing member','unknown',false,\
			 pg_catalog.clock_timestamp())",
			&[&"72000000-0000-4000-8000-000000000003"],
		)
		.await?;
	transaction
		.execute(
			"INSERT INTO decodex.account_routing_order(account_id,position) \
			 VALUES ($1::text::uuid,2)",
			&[&"72000000-0000-4000-8000-000000000003"],
		)
		.await?;
	transaction.batch_execute("SAVEPOINT tombstoned_balanced").await?;
	let tombstoned_balanced = transaction
		.query_one(
			"SELECT * FROM decodex.set_balanced_account_selection_exact($1)",
			&[&balanced_revision],
		)
		.await
		.expect_err("balanced selection must reject a tombstoned stored routing member");
	assert_account_routing_universe_error(&tombstoned_balanced);
	transaction
		.batch_execute(
			"ROLLBACK TO SAVEPOINT tombstoned_balanced; \
			 RELEASE SAVEPOINT tombstoned_balanced",
		)
		.await?;
	transaction.batch_execute("SAVEPOINT tombstoned_readback").await?;
	let tombstoned_readback = transaction
		.query_one("SELECT * FROM decodex.read_account_routing_control_exact()", &[])
		.await
		.expect_err("strict routing readback must reject a tombstoned stored member");
	assert_account_routing_universe_error(&tombstoned_readback);
	transaction
		.batch_execute(
			"ROLLBACK TO SAVEPOINT tombstoned_readback; \
			 RELEASE SAVEPOINT tombstoned_readback",
		)
		.await?;
	transaction
		.batch_execute(
			"ROLLBACK TO SAVEPOINT tombstoned_routing_member; \
			 RELEASE SAVEPOINT tombstoned_routing_member",
		)
		.await?;

	transaction.rollback().await?;
	drop(client);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated PostgreSQL 18 V27 routing harness"]
#[cfg(feature = "test-support")]
#[allow(clippy::too_many_lines)] // One complete routing/logout concurrency proof.
async fn postgres_account_routing_and_logout_share_one_lock_order()
-> Result<(), Box<dyn std::error::Error>> {
	let (mut schema_owner, _) = owner_runtime_configs("DECODEX_TEST")?;
	PostgresStore::apply_trusted_session_invariants_fixture(&mut schema_owner);
	let (mut owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);
	let setup = owner.transaction().await?;
	let logout_account = "72100000-0000-4000-8000-000000000001";
	let retained_account = "72100000-0000-4000-8000-000000000002";
	let logout_writer = "73100000-0000-4000-8000-000000000001";
	let retained_writer = "73100000-0000-4000-8000-000000000002";
	let logout_operation = "74100000-0000-4000-8000-000000000001";
	let fingerprint = "c".repeat(64);
	setup
		.batch_execute(
			"UPDATE decodex.account_routing_control SET mode='balanced',fixed_account_id=NULL,\
			 revision=revision+1,updated_at=pg_catalog.clock_timestamp() WHERE singleton;\
			 DELETE FROM decodex.account_routing_order;\
			 UPDATE decodex.accounts SET enabled=false,credential_store_schema_version=NULL,\
			 credential_version=NULL,credential_fingerprint=NULL,\
			 credential_writer_operation_id=NULL,credential_store_observation='missing',\
			 credential_store_observed_at=pg_catalog.clock_timestamp(),\
			 tombstoned_at=pg_catalog.clock_timestamp(),revision=revision+1,\
			 updated_at=pg_catalog.clock_timestamp()",
		)
		.await?;
	for (position, account_id, writer, provider_id) in [
		(0_i32, logout_account, logout_writer, "routing-logout-a"),
		(1_i32, retained_account, retained_writer, "routing-logout-b"),
	] {
		setup
			.execute(
				"INSERT INTO decodex.accounts(\
				 account_id,display_label,state,enabled,provider_kind,provider_account_id,\
				 credential_store_schema_version,credential_version,credential_fingerprint,\
				 credential_writer_operation_id,credential_store_observation,\
				 credential_store_observed_at\
				 ) VALUES (\
				 $1::text::uuid,$2,'available',true,'chatgpt',$3,1,1,$4,$5::text::uuid,\
				 'exact',pg_catalog.clock_timestamp())",
				&[
					&account_id,
					&format!("Routing logout {position}"),
					&provider_id,
					&fingerprint,
					&writer,
				],
			)
			.await?;
		setup
			.execute(
				"INSERT INTO decodex.account_routing_order(account_id,position) \
				 VALUES ($1::text::uuid,$2)",
				&[&account_id, &position],
			)
			.await?;
	}
	setup
		.execute(
			"INSERT INTO decodex.account_operations(\
			 operation_id,account_id,kind,phase,expected_account_revision,\
			 expected_store_schema_version,expected_credential_version,\
			 expected_credential_fingerprint,expected_credential_writer_operation_id,\
			 provider_kind,provider_account_id\
			 ) VALUES (\
			 $1::text::uuid,$2::text::uuid,'logout','store_applied',1,1,1,$3,\
			 $4::text::uuid,'chatgpt',$5)",
			&[
				&logout_operation,
				&logout_account,
				&fingerprint,
				&logout_writer,
				&"routing-logout-a",
			],
		)
		.await?;
	let expected_routing_revision: i64 = setup
		.query_one(
			"UPDATE decodex.account_routing_control \
			 SET revision=revision+1,updated_at=pg_catalog.clock_timestamp() \
			 WHERE singleton RETURNING revision",
			&[],
		)
		.await?
		.get(0);
	setup
		.batch_execute(
			"CREATE FUNCTION public.xy1422_pause_logout_routing_lock() \
			 RETURNS trigger LANGUAGE plpgsql AS $$ \
			 BEGIN \
			   IF NEW.account_id='72100000-0000-4000-8000-000000000001'::uuid \
			     AND OLD.tombstoned_at IS NULL AND NEW.tombstoned_at IS NOT NULL \
			   THEN PERFORM pg_catalog.pg_sleep(0.25); END IF; \
			   RETURN NEW; \
			 END \
			 $$;\
			 REVOKE ALL ON FUNCTION public.xy1422_pause_logout_routing_lock() FROM PUBLIC;\
			 CREATE TRIGGER xy1422_pause_logout_routing_lock \
			 AFTER UPDATE ON decodex.accounts FOR EACH ROW \
			 EXECUTE FUNCTION public.xy1422_pause_logout_routing_lock()",
		)
		.await?;
	setup.commit().await?;

	let mut logout_client_config = schema_owner.clone();
	PostgresStore::apply_trusted_session_invariants_fixture(&mut logout_client_config);
	let logout_task = tokio::spawn(async move {
		let (mut client, connection) =
			logout_client_config.connect(NoTls).await.expect("logout connection");
		let connection_task = tokio::spawn(connection);
		let transaction = client.transaction().await.expect("logout transaction");
		let code: String = transaction
			.query_one(
				"SELECT result_code FROM decodex.advance_account_operation_exact(\
				 $1::text::uuid,'store_applied','committed',NULL)",
				&[&logout_operation],
			)
			.await
			.expect("logout result")
			.get(0);
		transaction.commit().await.expect("logout transaction commit");
		drop(client);
		connection_task.await.expect("logout connection task").expect("logout connection");
		code
	});
	time::sleep(Duration::from_millis(50)).await;
	let mut fixed_config = schema_owner.clone();
	PostgresStore::apply_trusted_session_invariants_fixture(&mut fixed_config);
	let fixed_task = tokio::spawn(async move {
		let (mut client, connection) = fixed_config.connect(NoTls).await.expect("fixed connection");
		let connection_task = tokio::spawn(connection);
		let transaction = client.transaction().await.expect("fixed transaction");
		let code: String = transaction
			.query_one(
				"SELECT result_code FROM decodex.set_fixed_account_selection_exact(\
				 $1,$2::text::uuid,$3)",
				&[&expected_routing_revision, &logout_account, &1_i64],
			)
			.await
			.expect("fixed selection result")
			.get(0);
		transaction.commit().await.expect("fixed transaction commit");
		drop(client);
		connection_task.await.expect("fixed connection task").expect("fixed connection");
		code
	});
	time::sleep(Duration::from_millis(20)).await;
	let mut order_config = schema_owner.clone();
	PostgresStore::apply_trusted_session_invariants_fixture(&mut order_config);
	let order_task = tokio::spawn(async move {
		let (mut client, connection) = order_config.connect(NoTls).await.expect("order connection");
		let connection_task = tokio::spawn(connection);
		let transaction = client.transaction().await.expect("order transaction");
		let order = vec![retained_account.to_owned(), logout_account.to_owned()];
		let code: String = transaction
			.query_one(
				"SELECT result_code FROM decodex.set_account_order_exact(\
				 $1,$2::text[]::uuid[])",
				&[&expected_routing_revision, &order],
			)
			.await
			.expect("account order result")
			.get(0);
		transaction.commit().await.expect("order transaction commit");
		drop(client);
		connection_task.await.expect("order connection task").expect("order connection");
		code
	});

	let logout_code = time::timeout(Duration::from_secs(5), logout_task)
		.await
		.expect("logout must not deadlock")?;
	let fixed_code = time::timeout(Duration::from_secs(5), fixed_task)
		.await
		.expect("fixed selection must not deadlock")?;
	let order_code = time::timeout(Duration::from_secs(5), order_task)
		.await
		.expect("account order must not deadlock")?;
	assert_eq!(logout_code, "advanced");
	assert_eq!(fixed_code, "stale_routing_control");
	assert_eq!(order_code, "stale_routing_control");
	let final_routing = owner
		.query_one(
			"SELECT mode::text,fixed_account_id::text, \
			 ARRAY(SELECT value::text FROM pg_catalog.unnest(account_order) AS value) \
			 FROM decodex.read_account_routing_control_exact()",
			&[],
		)
		.await?;
	assert_eq!(final_routing.get::<_, &str>(0), "balanced");
	assert_eq!(final_routing.get::<_, Option<&str>>(1), None);
	assert_eq!(final_routing.get::<_, Vec<String>>(2), vec![retained_account.to_owned()]);

	owner
		.batch_execute(
			"DROP TRIGGER xy1422_pause_logout_routing_lock ON decodex.accounts;\
			 DROP FUNCTION public.xy1422_pause_logout_routing_lock()",
		)
		.await?;
	drop(owner);
	owner_connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 hostile-search-path harness"]
#[cfg(feature = "test-support")]
async fn postgres_trusted_session_invariants_startup_fixture()
-> Result<(), Box<dyn std::error::Error>> {
	let (_, mut runtime) = owner_runtime_configs("DECODEX_TEST")?;
	runtime.options("-csearch_path=public -cTimeZone=UTC");

	PostgresStore::apply_trusted_session_invariants_fixture(&mut runtime);
	assert_eq!(runtime.get_options(), Some("-csearch_path=pg_catalog -cTimeZone=+05:00"));

	let (client, connection) = runtime.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let settings = client
		.query_one(
			"SELECT pg_catalog.current_setting('search_path'), \
			 pg_catalog.current_setting('TimeZone'), \
			 EXTRACT(timezone FROM CURRENT_TIMESTAMP)::pg_catalog.int8",
			&[],
		)
		.await?;
	let search_path: String = settings.get(0);
	let time_zone: String = settings.get(1);
	let time_zone_offset_seconds: i64 = settings.get(2);

	assert_eq!(search_path, "pg_catalog");
	assert_eq!(time_zone, "+05:00");
	assert_eq!(time_zone_offset_seconds, -18_000);

	drop(client);

	connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated PostgreSQL 18 configured-authority database"]
async fn postgres_manifest_readiness_fixture() -> Result<(), Box<dyn std::error::Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	store.close();
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires the isolated PostgreSQL 18 harness"]
async fn postgres_store_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (client, connection) = schema_owner.clone().connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_connection_task = tokio::spawn(runtime_connection);

	assert_eq!(store.availability(), Availability::Available);

	assert_latest_schema_baseline(&client).await?;
	assert_runtime_is_least_privilege(&runtime_client).await?;
	seed_account_read_fixture(&store, &client).await?;
	assert_project_agent_authority(&store, &client, &runtime).await?;
	assert_policy_authority(&store, &client, &runtime).await?;
	assert_program_objective_authority(&store, &client, &schema_owner, &runtime).await?;
	assert_receipt_first_saga(&store, &client).await?;
	assert_concurrent_shard_capacity(&store, &client).await?;

	quota::assert_inert_window_and_credential_boundary(&store, &client).await?;

	assert_conversation_history_context_and_blob_contract(&store, &client, &runtime_client).await?;
	assert_concurrent_hierarchy_serialization(&store, &client, &runtime).await?;
	assert_duration_validation(&store, &client).await?;
	assert_lease_contention_and_reclaim(&store).await?;

	let routing =
		routing_decision::assert_routing_decision_contract(&store, &client, &runtime).await?;
	continuation::assert_continuation_contract(&store, &client, &runtime, &routing).await?;
	waiting_wake::assert_waiting_wake_contract(&store, &client, &schema_owner, &runtime, &routing)
		.await?;
	assert_outbox_concurrency_retry_and_restart(&store, &client, &runtime).await?;

	assert_eq!(store.availability(), Availability::Unavailable { reason: CLOSED });

	assert_closed_pool_behavior(&store).await?;
	assert_primary_indexes_are_plan_eligible(&client).await?;
	drop(client);
	drop(runtime_client);

	connection_task.await??;

	runtime_connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated PostgreSQL 18 missing-extension harness"]
async fn postgres_store_missing_pgcrypto_is_incompatible() -> Result<(), Box<dyn std::error::Error>>
{
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let live = PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);

	client.batch_execute("DROP EXTENSION pgcrypto CASCADE").await?;

	assert!(matches!(live.revalidate().await, Err(StoreError::Incompatible(_))));

	let pgcrypto_absent: bool = client
		.query_one(
			"SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_extension \
			 WHERE extname='pgcrypto')",
			&[],
		)
		.await?
		.get(0);

	assert!(pgcrypto_absent);

	live.close();
	drop(client);
	connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated PostgreSQL 18 restart harness"]
async fn postgres_blob_session_restart_contract() -> Result<(), Box<dyn std::error::Error>> {
	let sync = PathBuf::from(env::var("DECODEX_TEST_BLOB_RESTART_SYNC")?);
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let blob_store = isolated_blob_store()?;
	let conversation_id = ConversationId::new("4b000000-0000-4000-8000-000000000001")?;

	store
		.create_conversation(
			&CommandIdentity::new("restart-conversation", b"restart-conversation-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "Restart fixture".into(),
			},
		)
		.await?;

	let artifact_id = ArtifactId::new("4b000000-0000-4000-8000-000000000002")?;
	let command = CommandIdentity::new("restart-artifact", b"restart-artifact-v1")?;
	let create = CreateArtifact {
		artifact_id: artifact_id.clone(),
		conversation_id,
		bytes: b"restart-fenced-content-addressed-bytes".repeat(1_000),
		media_type: "application/octet-stream".into(),
		display_name: Some("Restart fixture".into()),
	};
	let first_store = store.clone();
	let first_blob_store = blob_store.clone();
	let first_command = command.clone();
	let first_create = create.clone();
	let worker = tokio::spawn(async move {
		first_store.create_artifact(&first_blob_store, &first_command, &first_create).await
	});

	wait_for_path(&sync.join("published")).await?;

	let (observer, observer_connection) = schema_owner.clone().connect(NoTls).await?;
	let observer_task = tokio::spawn(observer_connection);
	let pending = observer
		.query_one(
			"SELECT claim_token::text, receipt_state='pending', \
		 NOT EXISTS (SELECT 1 FROM decodex.blob_objects WHERE blob_hash=$2), \
		 NOT EXISTS (SELECT 1 FROM decodex.artifacts WHERE artifact_id=$3::text::uuid) \
		 FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[
				&"restart-artifact",
				&BlobHash::digest(&create.bytes).to_hex(),
				&artifact_id.as_str(),
			],
		)
		.await?;
	let old_claim: String = pending.get(0);

	assert!(pending.get::<_, bool>(1));
	assert!(pending.get::<_, bool>(2));
	assert!(pending.get::<_, bool>(3));

	fs::write(sync.join("ready"), b"ready")?;

	drop(observer);

	let _ = observer_task.await;

	wait_for_path(&sync.join("restarted")).await?;

	fs::write(sync.join("continue"), b"continue")?;

	assert!(worker.await?.is_err(), "the pre-restart BlobSession cannot complete transaction B");

	let retry_store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let completed = retry_store.create_artifact(&blob_store, &command, &create).await?;

	assert_eq!(completed.artifact_id, artifact_id);

	let (observer, observer_connection) = schema_owner.connect(NoTls).await?;
	let observer_task = tokio::spawn(observer_connection);
	let receipt = observer
		.query_one(
			"SELECT receipt_state='completed', completion_claim_token::text<>$2, \
		 EXISTS (SELECT 1 FROM decodex.blob_objects WHERE blob_hash=$3), \
		 EXISTS (SELECT 1 FROM decodex.artifacts WHERE artifact_id=$4::text::uuid) \
		 FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[
				&"restart-artifact",
				&old_claim,
				&BlobHash::digest(&create.bytes).to_hex(),
				&artifact_id.as_str(),
			],
		)
		.await?;

	assert!(receipt.get::<_, bool>(0));
	assert!(receipt.get::<_, bool>(1));
	assert!(receipt.get::<_, bool>(2));
	assert!(receipt.get::<_, bool>(3));

	drop(observer);

	observer_task.await??;

	Ok(())
}

async fn wait_for_path(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
	for _ in 0..3_000 {
		if path.exists() {
			return Ok(());
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	Err(format!("timed out waiting for {}", path.display()).into())
}

async fn assert_receipt_first_saga(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let conversation_id = ConversationId::new("49600000-0000-4000-8000-000000000010")?;
	let command = CommandIdentity::new("saga-create-conversation", b"saga-create-conversation-v1")?;
	let create = CreateConversation {
		conversation_id: conversation_id.clone(),
		title: "receipt-first saga".into(),
	};
	let original = store.create_conversation(&command, &create).await?;
	let replay = store.create_conversation(&command, &create).await?;

	assert_eq!(replay.conversation_id, original.conversation_id);
	assert_eq!(replay.title, original.title);

	let response_is_exact: bool = client
		.query_one(
			"SELECT convert_from(response_bytes,'UTF8')::jsonb=response \
			 AND receipt_state='completed' AND completion_claim_token IS NOT NULL \
			 FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&"saga-create-conversation"],
		)
		.await?
		.get(0);

	assert!(response_is_exact);

	let conflicting_bytes = b"conflict must precede publication".to_vec();
	let conflicting_hash = BlobHash::digest(&conflicting_bytes);
	let conflict = store
		.create_artifact(
			&blob_store,
			&CommandIdentity::new("saga-create-conversation", b"cross-operation-conflict")?,
			&CreateArtifact {
				artifact_id: ArtifactId::new("49610000-0000-4000-8000-000000000010")?,
				conversation_id: conversation_id.clone(),
				bytes: conflicting_bytes,
				media_type: "application/octet-stream".into(),
				display_name: None,
			},
		)
		.await;

	assert!(matches!(conflict, Err(StoreError::IdempotencyConflict)));
	assert!(!blob_store.path_for(conflicting_hash).exists());

	let conflict_metadata: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.blob_objects WHERE blob_hash=$1",
			&[&conflicting_hash.to_hex()],
		)
		.await?
		.get(0);

	assert_eq!(conflict_metadata, 0);

	let stale_id = "49600000-0000-4000-8000-000000000011";
	let stale_key = "saga-stale-claim";
	let request_hash = Sha256::digest(b"saga-stale-claim-v1")
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect::<String>();

	client
		.execute(
			"INSERT INTO decodex.command_receipts \
			 (idempotency_key,request_hash,operation,project_scope,scope_id,entity_id, \
			 claim_token,claim_expires_at) VALUES \
			 ($1,$2,'create_conversation','global','conversations',$3, \
			 '49620000-0000-4000-8000-000000000011',clock_timestamp()+interval '5 minutes')",
			&[&stale_key, &request_hash, &stale_id],
		)
		.await?;
	client
		.batch_execute(
			"ALTER TABLE decodex.command_receipts DISABLE TRIGGER command_receipts_state_guard; \
			 UPDATE decodex.command_receipts SET created_at=clock_timestamp()-interval '10 minutes', \
			 claim_expires_at=clock_timestamp()-interval '1 second' \
			 WHERE idempotency_key='saga-stale-claim'; \
			 ALTER TABLE decodex.command_receipts ENABLE TRIGGER command_receipts_state_guard",
		)
		.await?;

	let stale_create = CreateConversation {
		conversation_id: ConversationId::new(stale_id)?,
		title: "reclaimed stale saga".into(),
	};

	store
		.create_conversation(
			&CommandIdentity::new(stale_key, b"saga-stale-claim-v1")?,
			&stale_create,
		)
		.await?;

	let fenced: bool = client
		.query_one(
			"SELECT receipt_state='completed' \
			 AND completion_claim_token<>'49620000-0000-4000-8000-000000000011'::uuid \
			 AND claim_token IS NULL FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&stale_key],
		)
		.await?
		.get(0);

	assert!(fenced);
	assert!(
		client
			.execute(
				"UPDATE decodex.command_receipts SET entity_id='forged' WHERE idempotency_key=$1",
				&[&stale_key],
			)
			.await
			.is_err()
	);

	Ok(())
}

async fn assert_concurrent_shard_capacity(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let base = env::var("DECODEX_TEST_BLOB_ROOT")?;
	let capacity_root = DecodexRoot::new(format!("{base}-capacity"))?;
	let blob_store = BlobStore::open(capacity_root.paths())?;
	let conversation_id = ConversationId::new("49600000-0000-4000-8000-000000000020")?;

	store
		.create_conversation(
			&CommandIdentity::new("shard-capacity-conversation", b"shard-capacity-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "shard capacity concurrency".into(),
			},
		)
		.await?;

	let mut payloads = Vec::new();

	for candidate in 0_u64..100_000 {
		let bytes = format!("capacity-candidate-{candidate}").into_bytes();

		if BlobHash::digest(&bytes).to_hex().starts_with("aa") {
			payloads.push(bytes);

			if payloads.len() == 2 {
				break;
			}
		}
	}

	assert_eq!(payloads.len(), 2, "bounded search finds two same-shard payloads");

	let candidate_hashes = payloads.iter().map(|bytes| BlobHash::digest(bytes)).collect::<Vec<_>>();
	let shard = blob_shard_path(&blob_store, candidate_hashes[0])?;

	fs::create_dir_all(&shard)?;
	fs::set_permissions(&shard, Permissions::from_mode(0o700))?;

	let mut inserted = 0_usize;
	let mut suffix = 0_u64;

	while inserted < 4_095 {
		let name = format!("aa{suffix:062x}");

		suffix += 1;

		if candidate_hashes.iter().any(|hash| hash.to_hex() == name) {
			continue;
		}

		fs::write(shard.join(name), [])?;

		inserted += 1;
	}

	let first_store = store.clone();
	let first_blob_store = blob_store.clone();
	let first_conversation = conversation_id.clone();
	let first_bytes = payloads.remove(0);
	let first_artifact_id = ArtifactId::new("49610000-0000-4000-8000-000000000020")?;
	let first = tokio::spawn(async move {
		first_store
			.create_artifact(
				&first_blob_store,
				&CommandIdentity::new("shard-capacity-first", b"shard-capacity-first-v1")?,
				&CreateArtifact {
					artifact_id: first_artifact_id,
					conversation_id: first_conversation,
					bytes: first_bytes,
					media_type: "application/octet-stream".into(),
					display_name: None,
				},
			)
			.await
	});
	let second_store = store.clone();
	let second_blob_store = blob_store.clone();
	let second_bytes = payloads.remove(0);
	let second_artifact_id = ArtifactId::new("49610000-0000-4000-8000-000000000021")?;
	let second = tokio::spawn(async move {
		second_store
			.create_artifact(
				&second_blob_store,
				&CommandIdentity::new("shard-capacity-second", b"shard-capacity-second-v1")?,
				&CreateArtifact {
					artifact_id: second_artifact_id,
					conversation_id,
					bytes: second_bytes,
					media_type: "application/octet-stream".into(),
					display_name: None,
				},
			)
			.await
	});
	let (first, second) =
		time::timeout(Duration::from_secs(10), async { tokio::join!(first, second) }).await?;
	let first = first?;
	let second = second?;

	assert_ne!(
		first.is_ok(),
		second.is_ok(),
		"exactly one capacity contender commits: first={first:?}, second={second:?}"
	);
	assert_eq!(fs::read_dir(&shard)?.count(), 4_096);

	let committed: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.artifacts WHERE artifact_id IN \
			 ('49610000-0000-4000-8000-000000000020','49610000-0000-4000-8000-000000000021')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(committed, 1);

	Ok(())
}

async fn assert_concurrent_hierarchy_serialization(
	store: &PostgresStore,
	setup: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (client_a, connection_a) = runtime.clone().connect(NoTls).await?;
	let (client_b, connection_b) = runtime.clone().connect(NoTls).await?;
	let connection_a = tokio::spawn(connection_a);
	let connection_b = tokio::spawn(connection_b);
	let snapshot_ids = setup
		.query_one(
			"SELECT profile_snapshot_id::text,account_snapshot_id::text \
			 FROM decodex.runtime_sessions \
			 WHERE runtime_session_id='41000000-0000-4000-8000-000000000001'::uuid",
			&[],
		)
		.await?;
	let profile = snapshot_ids.get::<_, String>(0);
	let account = snapshot_ids.get::<_, String>(1);
	let session = |suffix: u8| format!("49100000-0000-4000-8000-{suffix:012x}");
	let turn = |suffix: u8| format!("49200000-0000-4000-8000-{suffix:012x}");
	let artifact = |suffix: u8| format!("49300000-0000-4000-8000-{suffix:012x}");
	let pack = |suffix: u8| format!("49400000-0000-4000-8000-{suffix:012x}");

	for suffix in 1_u8..=9 {
		let suffix_number = i16::from(suffix);

		setup
			.execute(
				"INSERT INTO decodex.conversations (conversation_id,title) \
				 VALUES (format('49000000-0000-4000-8000-%s', lpad($1::smallint::text,12,'0'))::uuid, \
				 'concurrency fixture')",
				&[&suffix_number],
			)
			.await?;
	}

	setup.batch_execute("INSERT INTO decodex.blob_objects (blob_hash,byte_length,verified_at) VALUES (repeat('a',64),1,clock_timestamp() + interval '1 second') ON CONFLICT DO NOTHING").await?;

	setup.execute(&format!("INSERT INTO decodex.runtime_sessions (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) VALUES ('{}','49000000-0000-4000-8000-000000000003','{profile}','{account}','active')", session(3)), &[]).await?;
	setup.execute(&format!("INSERT INTO decodex.turns (turn_id,conversation_id,runtime_session_id,sequence,role) VALUES ('{}','49000000-0000-4000-8000-000000000003','{}',1,'assistant')", turn(3), session(3)), &[]).await?;

	assert_parent_child_race(
		setup, &client_a, &client_b,
		&format!("INSERT INTO decodex.history_items (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) VALUES ('49500000-0000-4000-8000-000000000003','49000000-0000-4000-8000-000000000003',1,'{}',0,'message','completed','race','text/plain')", turn(3)),
		&format!("UPDATE decodex.turns SET status='completed',revision=revision+1 WHERE turn_id='{}'", turn(3)),
		"SELECT status='completed' AND NOT EXISTS (SELECT 1 FROM decodex.history_items WHERE conversation_id='49000000-0000-4000-8000-000000000003') FROM decodex.turns WHERE turn_id='49200000-0000-4000-8000-000000000003'",
	).await?;
	assert_parent_child_race(
		setup, &client_a, &client_b,
		&format!("INSERT INTO decodex.artifacts (artifact_id,conversation_id) VALUES ('{}','49000000-0000-4000-8000-000000000004'); INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) VALUES ('{}','49000000-0000-4000-8000-000000000004',1,repeat('a',64),'application/octet-stream','active')", artifact(4), artifact(4)),
		"UPDATE decodex.conversations SET status='archived',revision=revision+1 WHERE conversation_id='49000000-0000-4000-8000-000000000004'",
		"SELECT status='archived' AND NOT EXISTS (SELECT 1 FROM decodex.artifacts WHERE conversation_id='49000000-0000-4000-8000-000000000004') FROM decodex.conversations WHERE conversation_id='49000000-0000-4000-8000-000000000004'",
	).await?;

	setup.batch_execute(&format!("INSERT INTO decodex.artifacts (artifact_id,conversation_id) VALUES ('{}','49000000-0000-4000-8000-000000000005'); INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) VALUES ('{}','49000000-0000-4000-8000-000000000005',1,repeat('a',64),'application/octet-stream','active')", artifact(5), artifact(5))).await?;

	assert_artifact_revision_coherence_race(setup, &client_a, &client_b, &artifact(5)).await?;
	assert_parent_child_race(
		setup, &client_a, &client_b,
		&format!("INSERT INTO decodex.context_pack_sources (context_pack_id,conversation_id,position,kind,source_id,source_revision,content_digest,original_byte_length,included_byte_length,included_digest,disposition) VALUES ('{}','49000000-0000-4000-8000-000000000006',0,'pinned_revision','pinned',1,repeat('d',64),1,1,repeat('d',64),'complete'); INSERT INTO decodex.context_packs (context_pack_id,conversation_id,pack_revision,compiled_digest,manifest_digest,inline_bytes,byte_length,max_bytes,recent_item_limit,possible_side_effects,truncated,omitted_source_count,source_count) VALUES ('{}','49000000-0000-4000-8000-000000000006',1,repeat('b',64),repeat('c',64),'x',1,1024,1,'none',false,0,1)", pack(6), pack(6)),
		"UPDATE decodex.conversations SET status='archived',revision=revision+1 WHERE conversation_id='49000000-0000-4000-8000-000000000006'",
		"SELECT status='archived' AND NOT EXISTS (SELECT 1 FROM decodex.context_packs WHERE conversation_id='49000000-0000-4000-8000-000000000006') FROM decodex.conversations WHERE conversation_id='49000000-0000-4000-8000-000000000006'",
	).await?;

	setup.execute(&format!("INSERT INTO decodex.runtime_sessions (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) VALUES ('{}','49000000-0000-4000-8000-000000000007','{profile}','{account}','active')", session(7)), &[]).await?;
	setup.execute(&format!("INSERT INTO decodex.turns (turn_id,conversation_id,runtime_session_id,sequence,role) VALUES ('{}','49000000-0000-4000-8000-000000000007','{}',1,'tool')", turn(7), session(7)), &[]).await?;
	setup.batch_execute(&format!("INSERT INTO decodex.artifacts (artifact_id,conversation_id) VALUES ('{}','49000000-0000-4000-8000-000000000007'); INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) VALUES ('{}','49000000-0000-4000-8000-000000000007',1,repeat('a',64),'application/octet-stream','active')", artifact(7), artifact(7))).await?;

	assert_parent_child_race(
		setup, &client_a, &client_b,
		&format!("INSERT INTO decodex.history_items (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type,artifact_id,artifact_revision) VALUES ('49500000-0000-4000-8000-000000000007','49000000-0000-4000-8000-000000000007',1,'{}',0,'artifact','completed','race','text/plain','{}',1)", turn(7), artifact(7)),
		&format!("UPDATE decodex.artifacts SET status='expired',revision=revision+1 WHERE artifact_id='{}'; INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) SELECT artifact_id,conversation_id,2,blob_hash,media_type,'expired' FROM decodex.artifact_revisions WHERE artifact_id='{}' AND revision=1", artifact(7), artifact(7)),
		&format!("SELECT status='expired' AND NOT EXISTS (SELECT 1 FROM decodex.history_items WHERE artifact_id='{}') FROM decodex.artifacts WHERE artifact_id='{}'", artifact(7), artifact(7)),
	).await?;

	setup.batch_execute(&format!("INSERT INTO decodex.artifacts (artifact_id,conversation_id) VALUES ('{}','49000000-0000-4000-8000-000000000008'); INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) VALUES ('{}','49000000-0000-4000-8000-000000000008',1,repeat('a',64),'application/octet-stream','active')", artifact(8), artifact(8))).await?;

	assert_parent_child_race(
		setup, &client_a, &client_b,
		&format!("INSERT INTO decodex.context_pack_sources (context_pack_id,conversation_id,position,kind,source_id,source_revision,content_digest,original_byte_length,included_byte_length,included_digest,disposition) VALUES ('{}','49000000-0000-4000-8000-000000000008',0,'pinned_revision','pinned',1,repeat('d',64),1,1,repeat('d',64),'complete'); INSERT INTO decodex.context_pack_sources (context_pack_id,conversation_id,position,kind,source_id,source_revision,content_digest,original_byte_length,included_byte_length,included_digest,disposition,artifact_id,artifact_revision) VALUES ('{}','49000000-0000-4000-8000-000000000008',1,'artifact','{}',1,repeat('e',64),1,1,repeat('e',64),'complete','{}',1); INSERT INTO decodex.context_packs (context_pack_id,conversation_id,pack_revision,compiled_digest,manifest_digest,inline_bytes,byte_length,max_bytes,recent_item_limit,possible_side_effects,truncated,omitted_source_count,source_count) VALUES ('{}','49000000-0000-4000-8000-000000000008',1,repeat('b',64),repeat('c',64),'x',1,1024,1,'none',false,0,2)", pack(8), pack(8), artifact(8), artifact(8), pack(8)),
		&format!("UPDATE decodex.artifacts SET status='expired',revision=revision+1 WHERE artifact_id='{}'; INSERT INTO decodex.artifact_revisions (artifact_id,conversation_id,revision,blob_hash,media_type,status) SELECT artifact_id,conversation_id,2,blob_hash,media_type,'expired' FROM decodex.artifact_revisions WHERE artifact_id='{}' AND revision=1", artifact(8), artifact(8)),
		&format!("SELECT status='expired' AND NOT EXISTS (SELECT 1 FROM decodex.context_pack_sources WHERE artifact_id='{}') FROM decodex.artifacts WHERE artifact_id='{}'", artifact(8), artifact(8)),
	).await?;

	setup.execute(&format!("INSERT INTO decodex.runtime_sessions (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) VALUES ('{}','49000000-0000-4000-8000-000000000009','{profile}','{account}','active')", session(9)), &[]).await?;
	setup.execute(&format!("INSERT INTO decodex.turns (turn_id,conversation_id,runtime_session_id,sequence,role) VALUES ('{}','49000000-0000-4000-8000-000000000009','{}',1,'assistant')", turn(9), session(9)), &[]).await?;

	assert_concurrent_history_positions(setup, &client_a, &client_b, &turn(9)).await?;
	assert_cursor_capacity_and_retention(setup, &client_a, &client_b, &profile, &account).await?;
	assert_mixed_history_artifact_lock_order(store, setup).await?;
	assert_executor_prelock_order(setup, &client_a, &client_b).await?;
	assert_receipt_precedes_hierarchy(store, setup, &client_a).await?;
	assert_transition_writer_lock_order(store, setup, &client_a).await?;
	assert_typed_mixed_operation_progress(store, setup).await?;
	assert_direct_transition_cursor_lock_order(store, setup, &client_a).await?;
	assert_global_cursor_capacity_and_expiry(store, setup, &client_a, &profile, &account).await?;
	drop(client_a);
	drop(client_b);

	connection_a.await??;

	connection_b.await??;

	Ok(())
}

async fn assert_executor_prelock_order(
	observer: &Client,
	holder: &Client,
	opponent: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let (conversation_id, artifact_id, _session_id, turn_id) =
		seed_lock_order_fixture(observer, &blob_store, 24).await?;
	let history_item_id: String = observer
		.query_one(
			"SELECT history_item_id::text FROM decodex.history_items \
			 WHERE conversation_id=$1::text::uuid ORDER BY history_position LIMIT 1",
			&[&conversation_id.as_str()],
		)
		.await?
		.get(0);
	let statements = [
		format!(
			"UPDATE decodex.conversations SET revision=revision WHERE conversation_id='{conversation_id}'"
		),
		format!("UPDATE decodex.turns SET revision=revision WHERE turn_id='{turn_id}'"),
		format!(
			"UPDATE decodex.history_items SET revision=revision WHERE history_item_id='{history_item_id}'"
		),
		format!("UPDATE decodex.artifacts SET revision=revision WHERE artifact_id='{artifact_id}'"),
	];
	let lock_queries = [
		format!(
			"SELECT 1 FROM decodex.conversations WHERE conversation_id='{conversation_id}' FOR UPDATE"
		),
		format!("SELECT 1 FROM decodex.turns WHERE turn_id='{turn_id}' FOR UPDATE"),
		format!(
			"SELECT 1 FROM decodex.history_items WHERE history_item_id='{history_item_id}' FOR UPDATE"
		),
		format!("SELECT 1 FROM decodex.artifacts WHERE artifact_id='{artifact_id}' FOR UPDATE"),
	];
	let holder_pid: i32 = holder.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let opponent_pid: i32 = opponent.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);

	for (statement, lock_query) in statements.iter().zip(lock_queries.iter()) {
		holder.batch_execute("BEGIN; SELECT pg_advisory_xact_lock(1271)").await?;

		let update = opponent.execute(statement, &[]);

		tokio::pin!(update);

		let observed = wait_for_blocker(observer, holder_pid, opponent_pid);

		tokio::pin!(observed);

		tokio::select! {
			result = &mut update => panic!("hierarchy statement passed its pre-tuple coordinator: {result:?}"),
			result = &mut observed => assert!(result?, "opposing DML never reached the coordinator"),
		}

		holder.query_one(lock_query, &[]).await?;
		holder.batch_execute("COMMIT").await?;

		let error = time::timeout(Duration::from_secs(2), &mut update)
			.await?
			.expect_err("the deliberately illegal post-barrier update is rejected");

		assert_ne!(error.code().map(|code| code.code()), Some("40P01"));
	}

	Ok(())
}

async fn wait_for_blocker(
	observer: &Client,
	blocker_pid: i32,
	blocked_pid: i32,
) -> Result<bool, tokio_postgres::Error> {
	for _ in 0..10_000 {
		let blocked: bool = observer
			.query_one(
				"SELECT $1 = ANY(pg_catalog.pg_blocking_pids($2))",
				&[&blocker_pid, &blocked_pid],
			)
			.await?
			.get(0);

		if blocked {
			return Ok(true);
		}

		task::yield_now().await;
	}

	Ok(false)
}

async fn wait_for_any_blocked_by(
	observer: &Client,
	blocker_pid: i32,
) -> Result<bool, tokio_postgres::Error> {
	for _ in 0..10_000 {
		let blocked: bool = observer
			.query_one(
				"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity \
				 WHERE $1 = ANY(pg_catalog.pg_blocking_pids(pid)))",
				&[&blocker_pid],
			)
			.await?
			.get(0);

		if blocked {
			return Ok(true);
		}

		task::yield_now().await;
	}

	Ok(false)
}

async fn assert_receipt_precedes_hierarchy(
	store: &PostgresStore,
	observer: &Client,
	holder: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let key = "receipt-before-hierarchy-regression";

	holder.batch_execute("BEGIN").await?;
	holder
		.execute(
			"INSERT INTO decodex.command_receipts \
			 (idempotency_key,request_hash,claim_token,claim_expires_at) \
			 VALUES ($1,repeat('a',64),gen_random_uuid(),clock_timestamp()+interval '5 minutes')",
			&[&key],
		)
		.await?;

	let holder_pid: i32 = holder.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let task_store = store.clone();
	let command = CommandIdentity::new(key, b"different-request")?;
	let create = CreateConversation {
		conversation_id: ConversationId::new("49600000-0000-4000-8000-000000000001")?,
		title: "receipt ordering fixture".into(),
	};
	let task = tokio::spawn(async move { task_store.create_conversation(&command, &create).await });

	assert!(
		wait_for_any_blocked_by(observer, holder_pid).await?,
		"typed mutation reached and waited on its receipt before hierarchy authority"
	);

	holder.query_one("SELECT pg_advisory_xact_lock(1271)", &[]).await?;
	holder.batch_execute("COMMIT").await?;

	let result = time::timeout(Duration::from_secs(2), task).await??;
	let error = result.expect_err("cross-operation receipt reuse is an exact conflict");

	assert!(matches!(error, StoreError::IdempotencyConflict));

	Ok(())
}

async fn assert_cursor_capacity_and_retention(
	setup: &Client,
	first: &Client,
	second: &Client,
	profile: &str,
	account: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let conversation_id = "49000000-0000-4000-8000-000000000010";

	seed_cursor_capacity_fixture(setup, conversation_id, profile, account).await?;

	let parent = issue_cursor_chain(first, conversation_id, 511).await?;

	first.batch_execute("BEGIN").await?;

	let final_cursor: String = first
		.query_one(
			"SELECT decodex.issue_history_cursor( \
			 $1::text::uuid,$2::text::uuid,1)::text",
			&[&conversation_id, &Some(parent.as_str())],
		)
		.await?
		.get(0);

	second.batch_execute("SET lock_timeout='100ms'").await?;

	let blocked = second
		.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)",
			&[&conversation_id],
		)
		.await
		.expect_err("concurrent cursor issuance is globally serialized");

	assert_eq!(
		blocked.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE),
	);

	first.batch_execute("COMMIT").await?;
	second.batch_execute("SET lock_timeout='2s'").await?;

	let exhausted = second
		.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)",
			&[&conversation_id],
		)
		.await
		.expect_err("per-Conversation cursor capacity is enforced");

	assert_eq!(exhausted.as_db_error().map(|error| error.code().code()), Some("54000"));

	let canonical_retry: String = second
		.query_one(
			"SELECT decodex.issue_history_cursor( \
			 $1::text::uuid,$2::text::uuid,1)::text",
			&[&conversation_id, &Some(parent.as_str())],
		)
		.await?
		.get(0);

	assert_eq!(canonical_retry, final_cursor);

	let bounded_inventory: bool = setup
		.query_one(
			"SELECT count(*)=512 \
			 AND pg_catalog.pg_get_functiondef( \
			  'decodex.issue_history_cursor(uuid,uuid,integer)'::pg_catalog.regprocedure \
			 ) LIKE '%global_cursor_count >= 4096%' \
			 FROM decodex.history_cursors WHERE conversation_id=$1::text::uuid",
			&[&conversation_id],
		)
		.await?
		.get(0);

	assert!(bounded_inventory);

	setup
		.execute(
			"UPDATE decodex.history_cursors \
			 SET created_at=clock_timestamp()-interval '2 hours', \
			 expires_at=clock_timestamp()-interval '1 hour' \
			 WHERE conversation_id=$1::text::uuid",
			&[&conversation_id],
		)
		.await?;

	let replacement: String = second
		.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)::text",
			&[&conversation_id],
		)
		.await?
		.get(0);
	let retention_result = setup
		.query_one(
			"SELECT count(*)=1 AND bool_and(expires_at>clock_timestamp()) \
			 AND NOT EXISTS (SELECT 1 FROM decodex.history_cursors WHERE cursor_id=$2::text::uuid) \
			 FROM decodex.history_cursors WHERE conversation_id=$1::text::uuid",
			&[&conversation_id, &final_cursor],
		)
		.await?
		.get::<_, bool>(0);

	assert!(retention_result);
	assert_ne!(replacement, final_cursor);

	Ok(())
}

async fn assert_mixed_history_artifact_lock_order(
	store: &PostgresStore,
	setup: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let (conversation_id, _, _, _) = seed_lock_order_fixture(setup, &blob_store, 20).await?;
	let command = CommandIdentity::new("lock-order-create", b"lock-order-create-v1")?;
	let create = CreateArtifact {
		artifact_id: ArtifactId::new("49300000-0000-4000-8000-000000000120")?,
		conversation_id: conversation_id.clone(),
		bytes: b"concurrent Artifact bytes".to_vec(),
		media_type: "application/octet-stream".into(),
		display_name: None,
	};
	let history = store.conversation_history(&blob_store, &conversation_id, None, 2);
	let create = store.create_artifact(&blob_store, &command, &create);
	let (history, created) =
		time::timeout(Duration::from_secs(3), async { tokio::join!(history, create) }).await?;

	assert!(history?.next_cursor.is_some());
	assert_eq!(created?.revision, 1);

	Ok(())
}

async fn assert_transition_writer_lock_order(
	store: &PostgresStore,
	setup: &Client,
	transition_client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let (conversation_id, artifact_id, session_id, turn_id) =
		seed_lock_order_fixture(setup, &blob_store, 21).await?;

	transition_client.batch_execute("BEGIN").await?;
	transition_client
		.execute(
			"UPDATE decodex.artifacts SET status='expired',revision=2 \
			 WHERE artifact_id=$1::text::uuid",
			&[&artifact_id.as_str()],
		)
		.await?;

	let writer_store = store.clone();
	let writer_blob_store = blob_store.clone();
	let history_item_id = HistoryItemId::new("49500000-0000-4000-8000-000000000121")?;
	let writer_task = tokio::spawn(async move {
		writer_store
			.record_history_item(
				&writer_blob_store,
				&CommandIdentity::new("lock-order-history", b"lock-order-history-v1")?,
				&RecordHistoryItem {
					conversation_id,
					runtime_session_id: session_id,
					turn_id,
					turn_sequence: 1,
					turn_role: TurnRole::Assistant,
					possible_side_effects: PossibleSideEffects::None,
					history_item_id,
					ordinal: 100,
					kind: HistoryItemKind::Message,
					status: ItemStatus::Completed,
					text: "concurrent history writer".into(),
					media_type: history_media_type("text/plain"),
					metadata: HistoryMetadata::empty(),
					expected_revision: None,
					artifact: None,
				},
			)
			.await
	});
	let transition_pid: i32 =
		transition_client.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);

	assert!(
		wait_for_any_blocked_by(setup, transition_pid).await?,
		"history writer reached and waited on the transition coordinator"
	);

	transition_client
		.execute(
			"INSERT INTO decodex.artifact_revisions \
			 (artifact_id,conversation_id,revision,blob_hash,media_type,display_name,status) \
			 SELECT artifact_id,conversation_id,2,blob_hash,media_type,display_name,'expired' \
			 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
			&[&artifact_id.as_str()],
		)
		.await?;
	transition_client.batch_execute("COMMIT").await?;

	time::timeout(Duration::from_secs(3), writer_task).await???;

	let coherent: bool = setup
		.query_one(
			"SELECT a.status='expired' AND a.revision=2 AND ar.status=a.status \
			 AND EXISTS (SELECT 1 FROM decodex.history_items WHERE conversation_id=a.conversation_id \
			 AND ordinal=100) FROM decodex.artifacts a JOIN decodex.artifact_revisions ar \
			 ON (ar.artifact_id,ar.conversation_id,ar.revision)=(a.artifact_id,a.conversation_id,a.revision) \
			 WHERE a.artifact_id=$1::text::uuid",
			&[&artifact_id.as_str()],
		)
		.await?
		.get(0);

	assert!(coherent);

	Ok(())
}

async fn assert_typed_mixed_operation_progress(
	store: &PostgresStore,
	setup: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let (conversation_id, artifact_id, _, _) =
		seed_lock_order_fixture(setup, &blob_store, 22).await?;
	let transition_command =
		CommandIdentity::new("typed-lock-transition", b"typed-lock-transition-v1")?;
	let create_command = CommandIdentity::new("typed-lock-create", b"typed-lock-create-v1")?;
	let create_artifact = CreateArtifact {
		artifact_id: ArtifactId::new("49300000-0000-4000-8000-000000000122")?,
		conversation_id: conversation_id.clone(),
		bytes: b"typed concurrent Artifact".to_vec(),
		media_type: "application/octet-stream".into(),
		display_name: None,
	};
	let history = store.conversation_history(&blob_store, &conversation_id, None, 2);
	let transition = store.transition_artifact(
		&blob_store,
		&transition_command,
		&artifact_id,
		1,
		ArtifactStatus::Expired,
	);
	let create = store.create_artifact(&blob_store, &create_command, &create_artifact);
	let (history, transition, created) =
		time::timeout(Duration::from_secs(5), async { tokio::join!(history, transition, create) })
			.await?;

	assert!(history?.next_cursor.is_some());
	assert_eq!(transition?.status, ArtifactStatus::Expired);
	assert_eq!(created?.revision, 1);

	Ok(())
}

async fn assert_direct_transition_cursor_lock_order(
	store: &PostgresStore,
	setup: &Client,
	transition_client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let (conversation_id, artifact_id, _, _) =
		seed_lock_order_fixture(setup, &blob_store, 23).await?;

	transition_client.batch_execute("BEGIN").await?;
	transition_client
		.execute(
			"UPDATE decodex.artifacts SET status='expired',revision=2 \
			 WHERE artifact_id=$1::text::uuid",
			&[&artifact_id.as_str()],
		)
		.await?;

	let history_store = store.clone();
	let history_blob_store = blob_store.clone();
	let query_conversation = conversation_id.clone();
	let history_task = tokio::spawn(async move {
		history_store.conversation_history(&history_blob_store, &query_conversation, None, 2).await
	});
	let transition_pid: i32 =
		transition_client.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);

	assert!(
		wait_for_any_blocked_by(setup, transition_pid).await?,
		"history query reached and waited on the hierarchy coordinator"
	);

	let issued: String = time::timeout(
		Duration::from_secs(1),
		transition_client.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,1)::text",
			&[&conversation_id.as_str()],
		),
	)
	.await??
	.get(0);

	assert!(!issued.is_empty());

	transition_client
		.execute(
			"INSERT INTO decodex.artifact_revisions \
			 (artifact_id,conversation_id,revision,blob_hash,media_type,display_name,status) \
			 SELECT artifact_id,conversation_id,2,blob_hash,media_type,display_name,'expired' \
			 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
			&[&artifact_id.as_str()],
		)
		.await?;
	transition_client.batch_execute("COMMIT").await?;

	let page = time::timeout(Duration::from_secs(3), history_task).await???;

	assert!(page.next_cursor.is_some());

	Ok(())
}

async fn seed_lock_order_fixture(
	setup: &Client,
	blob_store: &BlobStore,
	suffix: u16,
) -> Result<(ConversationId, ArtifactId, RuntimeSessionId, TurnId), Box<dyn std::error::Error>> {
	let conversation_id = ConversationId::new(format!("49000000-0000-4000-8000-{suffix:012x}"))?;
	let session_id = RuntimeSessionId::new(format!("49100000-0000-4000-8000-{suffix:012x}"))?;
	let turn_id = TurnId::new(format!("49200000-0000-4000-8000-{suffix:012x}"))?;
	let artifact_id = ArtifactId::new(format!("49300000-0000-4000-8000-{suffix:012x}"))?;
	let bytes = format!("lock-order-artifact-{suffix}").into_bytes();
	let hash = blob_store.put(&bytes)?;
	let snapshot_ids = setup
		.query_one(
			"SELECT profile_snapshot_id::text,account_snapshot_id::text \
			 FROM decodex.runtime_sessions \
			 WHERE runtime_session_id='41000000-0000-4000-8000-000000000001'::uuid",
			&[],
		)
		.await?;
	let profile = snapshot_ids.get::<_, String>(0);
	let account = snapshot_ids.get::<_, String>(1);

	setup
		.execute(
			"INSERT INTO decodex.blob_objects (blob_hash,byte_length,verified_at) \
			 VALUES ($1,$2,clock_timestamp()) ON CONFLICT DO NOTHING",
			&[&hash.to_hex(), &i64::try_from(bytes.len())?],
		)
		.await?;
	setup
		.batch_execute(&format!(
			"INSERT INTO decodex.conversations (conversation_id,title) \
			 VALUES ('{conversation_id}','mixed lock fixture'); \
			 INSERT INTO decodex.runtime_sessions \
			 (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) \
			 VALUES ('{session_id}','{conversation_id}','{profile}','{account}','active'); \
			 INSERT INTO decodex.turns \
			 (turn_id,conversation_id,runtime_session_id,sequence,role) \
			 VALUES ('{turn_id}','{conversation_id}','{session_id}',1,'assistant'); \
			 INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) \
			 SELECT md5('{conversation_id}' || position::text)::uuid,'{conversation_id}',999, \
			 '{turn_id}',position,'message','completed',position::text,'text/plain' \
			 FROM generate_series(1,4) AS position; \
			 INSERT INTO decodex.artifacts (artifact_id,conversation_id) \
			 VALUES ('{artifact_id}','{conversation_id}'); \
			 INSERT INTO decodex.artifact_revisions \
			 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
			 VALUES ('{artifact_id}','{conversation_id}',1,'{hash}','application/octet-stream','active')"
		))
		.await?;

	Ok((conversation_id, artifact_id, session_id, turn_id))
}

async fn assert_global_cursor_capacity_and_expiry(
	store: &PostgresStore,
	setup: &Client,
	issuer: &Client,
	profile: &str,
	account: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	setup
		.batch_execute(
			"UPDATE decodex.history_cursors \
			 SET created_at=clock_timestamp()-interval '1 hour',expires_at=clock_timestamp()",
		)
		.await?;

	seed_global_cursor_fixtures(setup, profile, account).await?;

	for suffix in 1_u16..=8 {
		let conversation_id = format!("4a000000-0000-4000-8000-{suffix:012x}");

		issue_cursor_chain_recursive(issuer, &conversation_id, 512).await?;
	}

	let at_capacity: bool = setup
		.query_one(
			"SELECT sum(per_conversation)=4096 AND count(*)=8 \
			 AND min(per_conversation)=512 AND max(per_conversation)=512 FROM ( \
			 SELECT conversation_id,count(*) AS per_conversation \
			 FROM decodex.history_cursors GROUP BY conversation_id) AS counts",
			&[],
		)
		.await?
		.get(0);

	assert!(at_capacity);

	let ninth = "4a000000-0000-4000-8000-000000000009";
	let exhausted = issuer
		.query_one("SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)", &[&ninth])
		.await
		.expect_err("the real multi-Conversation global capacity is enforced");

	assert_eq!(exhausted.as_db_error().map(|error| error.code().code()), Some("54000"));

	setup
		.execute(
			"UPDATE decodex.history_cursors \
			 SET created_at=clock_timestamp()-interval '1 hour',expires_at=clock_timestamp() \
			 WHERE conversation_id='4a000000-0000-4000-8000-000000000001'",
			&[],
		)
		.await?;

	let blob_store = isolated_blob_store()?;
	let create_command = CommandIdentity::new("global-cap-writer", b"global-cap-writer-v1")?;
	let create = CreateArtifact {
		artifact_id: ArtifactId::new("4a300000-0000-4000-8000-000000000010")?,
		conversation_id: ConversationId::new("4a000000-0000-4000-8000-00000000000a")?,
		bytes: b"writer independent from global cursor pruning".to_vec(),
		media_type: "application/octet-stream".into(),
		display_name: None,
	};
	let issue_parameters: [&(dyn ToSql + Sync); 1] = [&ninth];
	let issue = issuer.query_one(
		"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)::text",
		&issue_parameters,
	);
	let write = store.create_artifact(&blob_store, &create_command, &create);
	let (issued, written) =
		time::timeout(Duration::from_secs(5), async { tokio::join!(issue, write) }).await?;

	assert!(!issued?.get::<_, String>(0).is_empty());
	assert_eq!(written?.revision, 1);

	let after_pruning: bool = setup
		.query_one(
			"SELECT count(*)=3585 \
			 AND NOT EXISTS (SELECT 1 FROM decodex.history_cursors \
			 WHERE conversation_id='4a000000-0000-4000-8000-000000000001') \
			 AND EXISTS (SELECT 1 FROM decodex.history_cursors \
			 WHERE conversation_id='4a000000-0000-4000-8000-000000000009') \
			 FROM decodex.history_cursors",
			&[],
		)
		.await?
		.get(0);

	assert!(after_pruning);

	Ok(())
}

async fn seed_global_cursor_fixtures(
	setup: &Client,
	profile: &str,
	account: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	for suffix in 1_u16..=10 {
		let conversation = format!("4a000000-0000-4000-8000-{suffix:012x}");
		let session = format!("4a100000-0000-4000-8000-{suffix:012x}");
		let turn = format!("4a200000-0000-4000-8000-{suffix:012x}");

		setup
			.batch_execute(&format!(
				"INSERT INTO decodex.conversations (conversation_id,title) \
				 VALUES ('{conversation}','global cursor capacity fixture'); \
				 INSERT INTO decodex.runtime_sessions \
				 (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) \
				 VALUES ('{session}','{conversation}','{profile}','{account}','active'); \
				 INSERT INTO decodex.turns \
				 (turn_id,conversation_id,runtime_session_id,sequence,role) \
				 VALUES ('{turn}','{conversation}','{session}',1,'system'); \
				 INSERT INTO decodex.history_items \
				 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status, \
				 inline_text,media_type) \
				 SELECT md5('{conversation}' || position::text)::uuid,'{conversation}',999, \
				 '{turn}',position,'status','completed',position::text,'text/plain' \
				 FROM generate_series(1,513) AS position"
			))
			.await?;
	}

	Ok(())
}

async fn issue_cursor_chain_recursive(
	client: &Client,
	conversation_id: &str,
	length: i32,
) -> Result<String, Box<dyn std::error::Error>> {
	let cursor = client
		.query_one(
			"WITH RECURSIVE chain(depth,cursor_id) AS ( \
			 SELECT 1,decodex.issue_history_cursor($1::text::uuid,NULL,1) \
			 UNION ALL SELECT depth+1,decodex.issue_history_cursor($1::text::uuid,cursor_id,1) \
			 FROM chain WHERE depth<$2) \
			 SELECT cursor_id::text FROM chain WHERE depth=$2",
			&[&conversation_id, &length],
		)
		.await?
		.get(0);

	Ok(cursor)
}

async fn seed_cursor_capacity_fixture(
	setup: &Client,
	conversation_id: &str,
	profile: &str,
	account: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let session_id = "49100000-0000-4000-8000-000000000010";
	let turn_id = "49200000-0000-4000-8000-000000000010";

	setup
		.batch_execute(&format!(
			"INSERT INTO decodex.conversations (conversation_id,title) \
			 VALUES ('{conversation_id}','cursor capacity fixture'); \
			 INSERT INTO decodex.runtime_sessions \
			 (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) \
			 VALUES ('{session_id}','{conversation_id}','{profile}','{account}','active'); \
			 INSERT INTO decodex.turns \
			 (turn_id,conversation_id,runtime_session_id,sequence,role) \
			 VALUES ('{turn_id}','{conversation_id}','{session_id}',1,'system'); \
			 INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status, \
			  inline_text,media_type) \
			 SELECT md5('cursor-capacity-' || position::text)::uuid,'{conversation_id}',999, \
			  '{turn_id}',position,'status','completed',position::text,'text/plain' \
			 FROM generate_series(1,513) AS position"
		))
		.await?;

	Ok(())
}

async fn issue_cursor_chain(
	client: &Client,
	conversation_id: &str,
	length: usize,
) -> Result<String, Box<dyn std::error::Error>> {
	let mut parent: Option<String> = None;

	for _ in 0..length {
		parent = Some(
			client
				.query_one(
					"SELECT decodex.issue_history_cursor( \
					 $1::text::uuid,$2::text::uuid,1)::text",
					&[&conversation_id, &parent.as_deref()],
				)
				.await?
				.get(0),
		);
	}

	Ok(parent.expect("positive cursor chain length issues one parent"))
}

async fn assert_concurrent_history_positions(
	setup: &Client,
	first: &Client,
	second: &Client,
	turn_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let conversation_id = "49000000-0000-4000-8000-000000000009";

	first.batch_execute("BEGIN").await?;
	first
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) \
			 VALUES ('49500000-0000-4000-8000-000000000091',$1::text::uuid,999, \
			 $2::text::uuid,0,'message','completed','first','text/plain')",
			&[&conversation_id, &turn_id],
		)
		.await?;
	second.batch_execute("SET lock_timeout='100ms'").await?;

	let blocked = second
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) \
			 VALUES ('49500000-0000-4000-8000-000000000092',$1::text::uuid,999, \
			 $2::text::uuid,1,'message','completed','second','text/plain')",
			&[&conversation_id, &turn_id],
		)
		.await
		.expect_err("concurrent history append waits for canonical position allocation");

	assert_eq!(
		blocked.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE),
	);

	first.batch_execute("COMMIT").await?;
	second.batch_execute("SET lock_timeout='2s'").await?;
	second
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) \
			 VALUES ('49500000-0000-4000-8000-000000000092',$1::text::uuid,999, \
			 $2::text::uuid,1,'message','completed','second','text/plain')",
			&[&conversation_id, &turn_id],
		)
		.await?;

	let positions: Vec<i64> = setup
		.query_one(
			"SELECT array_agg(history_position ORDER BY history_position) \
			 FROM decodex.history_items WHERE conversation_id=$1::text::uuid",
			&[&conversation_id],
		)
		.await?
		.get(0);

	assert_eq!(positions, vec![1, 2]);

	let cursor_id: String = second
		.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,1)::text",
			&[&conversation_id],
		)
		.await?
		.get(0);
	let canonical_cursor: bool = setup
		.query_one(
			"SELECT snapshot_high_water=2 AND last_position=1 AND page_size=1 \
			 AND expires_at>created_at AND expires_at<=created_at+interval '1 hour' \
			 FROM decodex.history_cursors WHERE cursor_id=$1::text::uuid",
			&[&cursor_id],
		)
		.await?
		.get(0);

	assert!(canonical_cursor);

	Ok(())
}

async fn assert_artifact_revision_coherence_race(
	setup: &Client,
	parent_client: &Client,
	revision_client: &Client,
	artifact_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	parent_client.batch_execute("BEGIN").await?;
	parent_client
		.execute(
			"UPDATE decodex.artifacts SET status='expired',revision=2,updated_at=clock_timestamp() \
			 WHERE artifact_id=$1::text::uuid",
			&[&artifact_id],
		)
		.await?;
	revision_client.batch_execute("SET lock_timeout='100ms'").await?;

	let blocked = revision_client
		.execute(
			"INSERT INTO decodex.artifact_revisions \
			 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
			 SELECT artifact_id,conversation_id,2,blob_hash,media_type,'expired' \
			 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
			&[&artifact_id],
		)
		.await
		.expect_err("revision insertion blocks behind its parent transition");

	assert_eq!(
		blocked.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE),
	);

	parent_client
		.batch_execute("COMMIT")
		.await
		.expect_err("a parent transition without its exact revision cannot commit");
	revision_client.batch_execute("SET lock_timeout='2s'").await?;

	assert!(
		revision_client
			.execute(
				"INSERT INTO decodex.artifact_revisions \
				 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
				 SELECT artifact_id,conversation_id,2,blob_hash,media_type,'expired' \
				 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
				&[&artifact_id],
			)
			.await
			.is_err()
	);

	let coherent: bool = setup
		.query_one(
			"SELECT a.revision=1 AND a.status='active' \
			 AND EXISTS (SELECT 1 FROM decodex.artifact_revisions ar \
			 WHERE ar.artifact_id=a.artifact_id AND ar.conversation_id=a.conversation_id \
			 AND ar.revision=a.revision AND ar.status=a.status) \
			 FROM decodex.artifacts a WHERE artifact_id=$1::text::uuid",
			&[&artifact_id],
		)
		.await?
		.get(0);

	assert!(coherent, "no incoherent parent/revision state committed after the race");

	Ok(())
}

async fn assert_parent_child_race(
	setup: &Client,
	child_client: &Client,
	parent_client: &Client,
	child_insert: &str,
	parent_terminal: &str,
	valid_final_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	child_client.batch_execute("BEGIN").await?;
	child_client.batch_execute(child_insert).await?;
	parent_client.batch_execute("SET lock_timeout='100ms'").await?;

	let blocked = parent_client
		.batch_execute(parent_terminal)
		.await
		.expect_err("parent transition must conflict with an uncommitted child write");

	assert_eq!(
		blocked.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE),
	);

	child_client.batch_execute("ROLLBACK").await?;
	parent_client.batch_execute("SET lock_timeout='2s'").await?;
	parent_client.batch_execute(parent_terminal).await?;
	child_client
		.batch_execute(child_insert)
		.await
		.expect_err("terminal parent must reject a later child write");

	let valid: bool = setup.query_one(valid_final_state, &[]).await?.get(0);

	assert!(valid, "no ineligible parent/child state may commit");

	Ok(())
}

async fn assert_runtime_is_least_privilege(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let attributes: (bool, bool) = client
		.query_one("SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user", &[])
		.await
		.map(|row| (row.get(0), row.get(1)))?;
	let owned: bool = client
		.query_one(
			"SELECT EXISTS (SELECT 1 FROM pg_class AS class \
			 JOIN pg_namespace AS namespace ON namespace.oid = class.relnamespace \
			 WHERE namespace.nspname = 'decodex' AND class.relowner = \
			 (SELECT oid FROM pg_roles WHERE rolname = current_user))",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(attributes, (false, false));
	assert!(!owned);

	for statement in [
		"CREATE TABLE decodex.runtime_must_not_create (id bigint)",
		"ALTER TABLE decodex.outbox ADD COLUMN runtime_must_not_own bigint",
		"TRUNCATE decodex.outbox",
		"SET session_replication_role = replica",
		"SELECT pg_catalog.setval('decodex.activity_sequence_seq', 1, false)",
		"INSERT INTO decodex.history_cursors \
		 (conversation_id,snapshot_high_water,page_size,last_position,history_item_id) VALUES \
		 ('40000000-0000-4000-8000-000000000001',1,1,1, \
		 '44000000-0000-4000-8000-000000000001')",
		"INSERT INTO decodex.projects \
		 (project_id,repository_identity,repository_root,default_cwd) VALUES \
		 ('10000000-0000-4000-8000-000000000099','forbidden/project','/srv/forbidden','/srv/forbidden')",
		"INSERT INTO decodex.agents (agent_id,role) VALUES \
		 ('20000000-0000-4000-8000-000000000099','advisor')",
		"INSERT INTO decodex.policies (policy_id,project_id) VALUES \
		 ('31000000-0000-4000-8000-000000000099',\
		  '11000000-0000-4000-8000-000000000050')",
		"UPDATE decodex.policy_revisions SET provenance='forbidden' WHERE revision=1",
	] {
		let error = client
			.batch_execute(statement)
			.await
			.expect_err("least-privilege runtime operation is rejected");

		assert_eq!(
			error.as_db_error().map(|database| database.code()),
			Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE),
			"statement: {statement}",
		);
	}

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 restore harness"]
async fn postgres_store_restored_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);

	assert_eq!(store.availability(), Availability::Available);

	assert_latest_schema_baseline(&client).await?;

	assert!(store.account(&AccountId::new(ACCOUNT_ID)?).await?.is_some());
	routing_decision::assert_restored_routing_contract(&client).await?;
	continuation::assert_restored_continuation_contract(&client).await?;
	waiting_wake::assert_restored_waiting_wake_contract(&client).await?;

	let advisor = store.advisor().await?.expect("restored global Advisor exists");

	assert!(matches!(
		advisor.id().as_str(),
		"21000000-0000-4000-8000-000000000001" | "21000000-0000-4000-8000-000000000002"
	));
	assert_eq!(advisor.role(), AgentRole::Advisor);
	assert_eq!(advisor.project_id(), None);
	assert_eq!(advisor.status(), AgentStatus::Active);
	assert_eq!(advisor.revision(), 1);

	let restored_project_id: String = client
		.query_one(
			"SELECT project_id::text FROM decodex.projects \
			 WHERE repository_identity='hack-ink/decodex'",
			&[],
		)
		.await?
		.get(0);
	let restored = store
		.project(&ProjectId::new(restored_project_id)?)
		.await?
		.expect("restored Project and canonical Lead exist");

	assert!(matches!(
		restored.project.id().as_str(),
		"11000000-0000-4000-8000-000000000001" | "11000000-0000-4000-8000-000000000002"
	));
	assert_eq!(restored.project.repository().identity().as_str(), "hack-ink/decodex");
	assert_eq!(
		restored.project.repository().root().as_server_path(),
		Path::new("/srv/repos/decodex")
	);
	assert_eq!(
		restored.project.repository().default_cwd().as_server_path(),
		Path::new("/srv/repos/decodex")
	);
	assert_eq!(
		restored.project.metadata().as_map().get("managed"),
		Some(&ProjectMetadataValue::Boolean(true))
	);
	assert_eq!(restored.project.status(), ProjectStatus::Paused);
	assert_eq!(restored.project.revision(), 2);
	assert_eq!(restored.lead.role(), AgentRole::Lead);
	assert_eq!(restored.lead.project_id(), Some(restored.project.id()));
	assert_eq!(restored.lead.status(), AgentStatus::Paused);
	assert_eq!(restored.lead.revision(), 2);

	let restored_fk: bool = client
		.query_one(
			"SELECT lead.project_id = project.project_id \
			 FROM decodex.projects AS project \
			 JOIN decodex.agents AS lead ON lead.project_id=project.project_id AND lead.role='lead' \
			 WHERE project.project_id=$1::text::uuid",
			&[&restored.project.id().as_str()],
		)
		.await?
		.get(0);

	assert!(restored_fk);

	assert_restored_policy_authority(&store).await?;
	assert_restored_program_objective_authority(&store).await?;

	let ordinary_rows: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.accounts) \
			      + (SELECT count(*) FROM decodex.command_receipts) \
			      + (SELECT count(*) FROM decodex.activity) \
			      + (SELECT count(*) FROM decodex.outbox)",
			&[],
		)
		.await?
		.get(0);

	assert!(ordinary_rows > 0);

	store.close();

	assert_eq!(store.availability(), Availability::Unavailable { reason: CLOSED });

	drop(client);

	connection_task.await??;

	Ok(())
}

async fn assert_restored_policy_authority(
	store: &PostgresStore,
) -> Result<(), Box<dyn std::error::Error>> {
	let policy_id = PolicyId::new("31000000-0000-4000-8000-000000000001")?;
	let project_id = ProjectId::new("11000000-0000-4000-8000-000000000050")?;
	let policies = store.policies_for_project(&project_id).await?;
	let first_id =
		PolicyRevisionId::new(project_id.clone(), policy_id.clone(), PolicyRevision::new(1)?);
	let second_id = PolicyRevisionId::new(project_id, policy_id.clone(), PolicyRevision::new(2)?);
	let first = store.policy_revision(&first_id).await?.expect("revision one restored");
	let second = store.policy_revision(&second_id).await?.expect("revision two restored");

	assert_eq!(policies.len(), 1);
	assert_eq!(policies[0].id(), &policy_id);
	assert_eq!(policies[0].current_revision(), Some(PolicyRevision::new(2)?));
	assert_eq!(first.supersedes(), None);
	assert_eq!(second.supersedes(), Some(first.id()));
	assert!(
		[&first, &second]
			.into_iter()
			.all(|revision| revision.accepted_at() >= revision.policy_created_at())
	);
	assert_eq!(store.policy_revision(&second_id).await?, Some(second));

	Ok(())
}

async fn assert_restored_program_objective_authority(
	store: &PostgresStore,
) -> Result<(), Box<dyn std::error::Error>> {
	let program_id = ProgramId::new("41000000-0000-4000-8000-000000000060")?;
	let achieved_id = ObjectiveId::new("71000000-0000-4000-8000-000000000060")?;
	let abandoned_id = ObjectiveId::new("71000000-0000-4000-8000-000000000061")?;
	let program = store.program(&program_id).await?.expect("Program restored");
	let achieved = store.objective(&achieved_id).await?.expect("achieved Objective restored");
	let abandoned = store.objective(&abandoned_id).await?.expect("abandoned Objective restored");

	assert_eq!(program.program.revision(), 3);
	assert!(matches!(program.program.state(), ProgramState::Blocked | ProgramState::Paused));
	assert_eq!(achieved.objective.state(), ObjectiveState::Achieved);
	assert_eq!(achieved.objective.revision(), 3);

	let evidence = achieved.objective.completion().expect("achievement evidence restored");
	let objective_updated_at =
		evidence.objective_updated_at().expect("prior Objective timestamp restored");

	assert!(objective_updated_at <= evidence.accepted_at());
	assert!(evidence.accepted_at() <= evidence.validated_at());
	assert!(evidence.validated_at() <= evidence.recorded_at());
	assert_eq!(abandoned.objective.state(), ObjectiveState::Abandoned);
	assert_eq!(abandoned.objective.revision(), 2);

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 populated restore harness"]
async fn postgres_store_rejects_schema_scoped_default_acl_restore()
-> Result<(), Box<dyn std::error::Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;

	match PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await {
		Err(StoreError::UnsafeAuthority(_)) => Ok(()),
		Err(error) => panic!("unexpected schema-scoped default-ACL restore error: {error:?}"),
		Ok(_) => panic!("schema-scoped default-ACL restore was accepted"),
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 authority harness"]
async fn postgres_store_rejects_implicit_uuid_to_text_cast()
-> Result<(), Box<dyn std::error::Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;

	match PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await {
		Err(StoreError::UnsafeAuthority(_)) => Ok(()),
		Err(error) => panic!("unexpected implicit UUID-to-text cast error: {error:?}"),
		Ok(_) => panic!("implicit UUID-to-text cast authority was accepted"),
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 Turkish ICU collation harness"]
async fn postgres_store_turkish_collation_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST_COLLATION")?;
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let locale = client
		.query_one(
			"SELECT datlocprovider::text, datlocale FROM pg_database \
			 WHERE datname = current_database()",
			&[],
		)
		.await?;
	let provider: String = locale.get(0);
	let locale: Option<String> = locale.get(1);

	assert_eq!(provider, "i");
	assert!(
		locale
			.as_deref()
			.and_then(|value| value.split('-').next())
			.is_some_and(|language| language.eq_ignore_ascii_case("tr"))
	);

	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;

	for (index, response) in [
		serde_json::json!({"AUTHORIZATION": "forbidden"}),
		serde_json::json!({"HEADER": "BEARER ABCDEFGHIJKLMNOP"}),
		serde_json::json!({"PRIVATE_KEY": "forbidden"}),
	]
	.into_iter()
	.enumerate()
	{
		let key = format!("turkish-collation-{index}");

		client
			.execute(
				"INSERT INTO decodex.command_receipts \
				 (idempotency_key,request_hash,claim_token,claim_expires_at) \
				 VALUES ($1,repeat('a',64),gen_random_uuid(),clock_timestamp()+interval '5 minutes')",
				&[&key],
			)
			.await?;

		assert_credential_constraint(
			&client,
			"UPDATE decodex.command_receipts SET response=$2, \
			 response_bytes=convert_to('{}','UTF8'), receipt_state='completed', \
			 completed_at=clock_timestamp(),completion_claim_token=claim_token, \
			 claim_token=NULL,claim_expires_at=NULL WHERE idempotency_key=$1",
			&[&key, &response],
			"command_receipts_no_credentials",
		)
		.await?;
	}

	store.close();

	drop(client);

	connection_task.await??;

	Ok(())
}

async fn assert_latest_schema_baseline(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
	let version: i32 = client
		.query_one("SELECT current_setting('server_version_num')::integer / 10000", &[])
		.await?
		.get(0);
	let checksums: String =
		client.query_one("SELECT current_setting('data_checksums')", &[]).await?.get(0);

	assert_eq!(version, 18);
	assert_eq!(checksums, "on");

	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;

	PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;

	let mut tcp = Config::new();

	tcp.host("127.0.0.1");

	assert!(matches!(
		PostgresStore::connect_runtime_fixture(tcp, expected_peer_uid()).await,
		Err(StoreError::Incompatible(reason)) if reason.contains("Unix socket")
	));

	let mut missing_runtime = runtime;

	missing_runtime.dbname("decodex_xy1267_missing");

	assert!(matches!(
		PostgresStore::connect_runtime_fixture(missing_runtime, expected_peer_uid()).await,
		Err(StoreError::Pool(_))
	));

	Ok(())
}

async fn assert_project_agent_authority(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let advisor_ids =
		["21000000-0000-4000-8000-000000000001", "21000000-0000-4000-8000-000000000002"];
	let mut advisor_tasks = JoinSet::new();

	for id in advisor_ids {
		let store = store.clone();

		advisor_tasks.spawn(async move {
			store
				.bootstrap_advisor(Agent::advisor(
					AgentId::new(id).expect("Advisor fixture ID is canonical"),
				))
				.await
		});
	}

	let mut advisors = Vec::new();

	while let Some(result) = advisor_tasks.join_next().await {
		advisors.push(result??);
	}

	assert_eq!(advisors[0], advisors[1]);
	assert_eq!(store.advisor().await?, Some(advisors[0].clone()));

	let candidates = [
		("11000000-0000-4000-8000-000000000001", "22000000-0000-4000-8000-000000000001"),
		("11000000-0000-4000-8000-000000000002", "22000000-0000-4000-8000-000000000002"),
	];
	let mut project_tasks = JoinSet::new();

	for (project_id, lead_id) in candidates {
		let store = store.clone();

		project_tasks.spawn(async move {
			let project_id = ProjectId::new(project_id).expect("Project fixture ID is canonical");
			let project = Project::new(
				project_id.clone(),
				ProjectRepositoryBinding::new(
					RepositoryIdentity::new("hack-ink/decodex")
						.expect("repository fixture identity is canonical"),
					PathBuf::from("/srv/repos/decodex"),
					PathBuf::from("/srv/repos/decodex"),
				)
				.expect("repository fixture paths are canonical"),
				ProjectMetadata::new(BTreeMap::from([(
					"managed".into(),
					ProjectMetadataValue::Boolean(true),
				)]))
				.expect("Project metadata fixture is bounded"),
			);
			let lead = Agent::lead(
				AgentId::new(lead_id).expect("Lead fixture ID is canonical"),
				project_id,
			);

			store.create_project(CreateProject { project, lead }).await
		});
	}

	let mut authorities = Vec::new();

	while let Some(result) = project_tasks.join_next().await {
		authorities.push(result??);
	}

	assert_eq!(authorities[0], authorities[1]);

	let winner = authorities.pop().expect("concurrent Project creation returned a winner");

	assert_eq!(store.project(winner.project.id()).await?, Some(winner.clone()));

	let paused = store.transition_project(winner.project.id(), 1, ProjectStatus::Paused).await?;

	assert_eq!(paused.project.revision(), 2);
	assert_eq!(paused.lead.revision(), 2);
	assert!(matches!(
		store.transition_project(winner.project.id(), 1, ProjectStatus::Archived).await,
		Err(StoreError::RevisionConflict { actual: Some(2), .. })
	));

	assert_project_agent_canonical_sql_boundary(client, runtime).await?;
	assert_project_identity_pair_conflicts(store, client, runtime).await?;

	client
		.batch_execute(
			"INSERT INTO decodex.agents (agent_id,role,project_id) VALUES \
			 ('22000000-0000-4000-8000-000000000099','lead', \
			 '11000000-0000-4000-8000-000000000099')",
		)
		.await
		.expect_err("Lead foreign key rejects an unknown Project");
	client
		.batch_execute(
			"INSERT INTO decodex.agents (agent_id,role) VALUES \
			 ('21000000-0000-4000-8000-000000000099','advisor')",
		)
		.await
		.expect_err("global Advisor uniqueness is durable");

	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;

	assert_eq!(restarted.project(paused.project.id()).await?, Some(paused));
	assert_eq!(restarted.advisor().await?, store.advisor().await?);

	Ok(())
}

async fn assert_policy_authority(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let primary = project_request(
		"11000000-0000-4000-8000-000000000050",
		"22000000-0000-4000-8000-000000000050",
		"hack-ink/policy-authority-a",
		"/srv/repos/policy-authority-a",
	);
	let secondary = project_request(
		"11000000-0000-4000-8000-000000000051",
		"22000000-0000-4000-8000-000000000051",
		"hack-ink/policy-authority-b",
		"/srv/repos/policy-authority-b",
	);
	let primary_authority = store.create_project(primary).await?;
	let secondary_authority = store.create_project(secondary).await?;
	let project_id = primary_authority.project.id().clone();
	let lead_id = primary_authority.lead.id().clone();
	let policy_id = PolicyId::new("31000000-0000-4000-8000-000000000001")?;
	let policy = store.create_policy(policy_id.clone(), project_id.clone()).await?;

	assert_eq!(policy.id(), &policy_id);
	assert_eq!(policy.project_id(), &project_id);
	assert_eq!(policy.current_revision(), None);
	assert_eq!(policy.status(), decodex_core::PolicyStatus::Unaccepted);
	assert_eq!(store.policies_for_project(&project_id).await?, vec![policy.clone()]);
	assert_eq!(store.create_policy(policy_id.clone(), project_id.clone()).await?, policy,);
	assert!(matches!(
		store.create_policy(policy_id.clone(), secondary_authority.project.id().clone()).await,
		Err(StoreError::InvalidInput("Policy identity is already bound to another Project"))
	));

	let first_request = policy_acceptance(&project_id, &policy_id, &lead_id, 1, "first");
	let first = store.accept_policy_revision(first_request.clone()).await?;

	assert_eq!(first.id(), &first_request.id);
	assert_eq!(first.supersedes(), None);
	assert!(first.accepted_at() >= first.policy_created_at());
	assert_eq!(store.accept_policy_revision(first_request.clone()).await?, first);

	let mut conflicting_first = first_request;

	conflicting_first.provenance = PolicyProvenance::new("accepted fixture conflicting")?;

	assert!(matches!(
		store.accept_policy_revision(conflicting_first).await,
		Err(StoreError::IdempotencyConflict)
	));

	let cross_project = policy_acceptance(
		secondary_authority.project.id(),
		&policy_id,
		secondary_authority.lead.id(),
		2,
		"cross-project",
	);

	assert!(matches!(
		store.accept_policy_revision(cross_project).await,
		Err(StoreError::InvalidInput("Policy revision cannot attach across Projects"))
	));

	let wrong_authority = policy_acceptance(
		&project_id,
		&policy_id,
		secondary_authority.lead.id(),
		2,
		"wrong-authority",
	);

	assert!(matches!(
		store.accept_policy_revision(wrong_authority).await,
		Err(StoreError::InvalidInput("Policy acceptance requires active Project Lead authority"))
	));

	let (winner_request, winner_revision) = accept_concurrent_policy_revision(
		store,
		[
			policy_acceptance(&project_id, &policy_id, &lead_id, 2, "candidate-a"),
			policy_acceptance(&project_id, &policy_id, &lead_id, 2, "candidate-b"),
		],
	)
	.await?;

	assert_eq!(store.accept_policy_revision(winner_request.clone()).await?, winner_revision);
	assert_eq!(store.policy_revision(winner_revision.id()).await?, Some(winner_revision.clone()));

	assert_policy_revision_conflicts(
		store,
		client,
		&project_id,
		&policy_id,
		&lead_id,
		&winner_request,
		&winner_revision,
	)
	.await?;

	let listed = store.policies_for_project(&project_id).await?;

	assert_eq!(listed.len(), 1);
	assert_eq!(listed[0].id(), &policy_id);
	assert_eq!(listed[0].current_revision(), Some(PolicyRevision::new(2)?));
	assert_eq!(listed[0].status(), decodex_core::PolicyStatus::Accepted);
	assert!(store.policies_for_project(secondary_authority.project.id()).await?.is_empty());

	assert_policy_database_guards(store, client, &project_id, &policy_id, &lead_id).await?;

	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;

	assert_eq!(restarted.policy_revision(winner_revision.id()).await?, Some(winner_revision));

	Ok(())
}

async fn assert_program_objective_authority(
	store: &PostgresStore,
	client: &Client,
	schema_owner: &Config,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let authority = store
		.create_project(project_request(
			"11000000-0000-4000-8000-000000000060",
			"22000000-0000-4000-8000-000000000060",
			"hack-ink/program-objective-authority",
			"/srv/repos/program-objective-authority",
		))
		.await?;
	let project_id = authority.project.id().clone();
	let lead_id = authority.lead.id().clone();
	let policy_id = PolicyId::new("31000000-0000-4000-8000-000000000060")?;

	store.create_policy(policy_id.clone(), project_id.clone()).await?;

	let accepted_policy = store
		.accept_policy_revision(policy_acceptance(
			&project_id,
			&policy_id,
			&lead_id,
			1,
			"program-objective",
		))
		.await?;
	let program_id = ProgramId::new("41000000-0000-4000-8000-000000000060")?;
	let program = Program::new(
		program_id.clone(),
		project_id.clone(),
		lead_id.clone(),
		"SEO and GEO operations",
		"Operate search visibility continuously; a conversation mention remains ordinary data",
		accepted_policy.id().clone(),
		ReviewCadence::new(30, ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?)?,
	)?;
	let create_provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000060")?,
		"Lead established the ongoing responsibility",
	)?;
	let create_command = CommandIdentity::new("program-create-60", b"program-create-60")?;
	let created = store.create_program(&create_command, &program, &create_provenance).await?;

	assert_eq!(created.program.state(), ProgramState::Active);
	assert_eq!(
		created.program,
		store.create_program(&create_command, &program, &create_provenance).await?.program
	);

	assert_program_objective_creation_races(
		store,
		client,
		schema_owner,
		&project_id,
		&lead_id,
		accepted_policy.id(),
		&program_id,
	)
	.await?;

	let observed_at = ProgramTimestamp::from_unix_microseconds(1_900_000_000_000_000)?;
	let observation = ProgramObservationProvenance::new(
		"analytics conversation export retained only as inspectable data",
		observed_at,
	)?;
	let metric = ProgramMetric::new("organic_sessions", "1200", "sessions", observation.clone())?;
	let signal = ProgramSignal::new(
		ProgramObservationId::new("61000000-0000-4000-8000-000000000060")?,
		"visibility_change",
		"Search visibility increased",
		observation,
	)?;
	let update = UpdateProgramContext {
		program_id: program_id.clone(),
		project_id: project_id.clone(),
		expected_revision: 1,
		review_cadence: ReviewCadence::new(
			14,
			ProgramTimestamp::from_unix_microseconds(2_000_100_000_000_000)?,
		)?,
		metrics: vec![metric],
		signals: vec![signal],
	};
	let update_provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000061")?,
		"Lead refreshed Program observations",
	)?;
	let update_command = CommandIdentity::new("program-update-60", b"program-update-60")?;
	let updated =
		store.update_program_context(&update_command, &update, &update_provenance).await?;

	assert_eq!(updated.program.revision(), 2);
	assert_eq!(
		updated,
		store.update_program_context(&update_command, &update, &update_provenance).await?,
	);

	assert_program_context_compilation_is_pure(client, &updated.program, &program_id).await?;
	assert_program_transition_replay(store, client, runtime, &project_id, &program_id, &lead_id)
		.await?;
	assert_hostile_program_objective_sql(client, &project_id, &lead_id, &policy_id).await?;
	assert_objective_lifecycle(store, client, &project_id, &lead_id, &program_id).await?;

	Ok(())
}

async fn assert_program_objective_creation_races(
	store: &PostgresStore,
	client: &Client,
	schema_owner: &Config,
	project_id: &ProjectId,
	lead_id: &AgentId,
	policy_revision_id: &PolicyRevisionId,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_program_create_race(store, client, project_id, lead_id, policy_revision_id).await?;
	assert_objective_create_race(store, client, project_id, lead_id, program_id).await?;
	assert_objective_evidence_race(store, client, project_id, lead_id, program_id).await?;
	assert_cross_objective_evidence_identity_race(
		store,
		client,
		schema_owner,
		project_id,
		lead_id,
		program_id,
	)
	.await?;

	let pending: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.command_receipts WHERE receipt_state='pending'\
			 AND operation IN ('create_program','transition_program','create_objective',\
			 'transition_objective','achieve_objective')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(pending, 0);

	Ok(())
}

async fn assert_program_create_race(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	policy_revision_id: &PolicyRevisionId,
) -> Result<(), Box<dyn std::error::Error>> {
	let program_id = ProgramId::new("41000000-0000-4000-8000-000000000080")?;
	let program = Program::new(
		program_id.clone(),
		project_id.clone(),
		lead_id.clone(),
		"Concurrent Program",
		"One canonical responsibility",
		policy_revision_id.clone(),
		ReviewCadence::new(30, ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?)?,
	)?;
	let provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000080")?,
		"Concurrent Program creation",
	)?;
	let commands = [
		CommandIdentity::new("program-create-race-a-80", b"program-create-race-a-80")?,
		CommandIdentity::new("program-create-race-b-80", b"program-create-race-b-80")?,
	];
	let mut tasks = JoinSet::new();

	for command in commands.clone() {
		let store = store.clone();
		let program = program.clone();
		let provenance = provenance.clone();

		tasks.spawn(async move {
			let result = store.create_program(&command, &program, &provenance).await;

			(command, result)
		});
	}

	while let Some(result) = tasks.join_next().await {
		let (command, result) = result?;

		assert_eq!(result?, store.create_program(&command, &program, &provenance).await?);
	}

	assert_single_domain_activity(client, "program", program_id.as_str(), "program_created")
		.await?;
	assert_program_missing_replay(
		store,
		client,
		project_id,
		lead_id,
		policy_revision_id,
		&provenance,
	)
	.await?;

	Ok(())
}

async fn assert_program_missing_replay(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	policy_revision_id: &PolicyRevisionId,
	provenance: &ProgramProvenance,
) -> Result<(), Box<dyn std::error::Error>> {
	let race_id = ProgramId::new("41000000-0000-4000-8000-000000000081")?;
	let race_program = Program::new(
		race_id.clone(),
		project_id.clone(),
		lead_id.clone(),
		"Mutation race Program",
		"Request-scoped mutation authority",
		policy_revision_id.clone(),
		ReviewCadence::new(30, ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?)?,
	)?;
	let transition =
		CommandIdentity::new("program-absent-transition-81", b"program-absent-transition-81")?;

	assert!(store.program(&race_id).await?.is_none());

	let first = store
		.transition_program(
			&transition,
			project_id,
			&race_id,
			1,
			ProgramState::NeedsAttention,
			provenance,
		)
		.await;

	assert!(matches!(first, Err(StoreError::RevisionConflict { actual: None, .. })));

	store
		.create_program(
			&CommandIdentity::new("program-create-after-miss-81", b"program-create-after-miss-81")?,
			&race_program,
			provenance,
		)
		.await?;

	assert!(matches!(
		store
			.transition_program(
				&transition,
				project_id,
				&race_id,
				1,
				ProgramState::NeedsAttention,
				provenance,
			)
			.await,
		Err(StoreError::RevisionConflict { actual: None, .. })
	));

	let receipt_scope: String = client
		.query_one(
			"SELECT scope_id FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&"program-absent-transition-81"],
		)
		.await?
		.get(0);

	assert_eq!(receipt_scope, project_id.as_str());

	let fresh_transition = CommandIdentity::new(
		"program-create-between-read-transition-81",
		b"program-create-between-read-transition-81",
	)?;
	let transitioned = store
		.transition_program(
			&fresh_transition,
			project_id,
			&race_id,
			1,
			ProgramState::NeedsAttention,
			provenance,
		)
		.await?;

	assert_eq!(transitioned.program.state(), ProgramState::NeedsAttention);
	assert_eq!(
		transitioned,
		store
			.transition_program(
				&fresh_transition,
				project_id,
				&race_id,
				1,
				ProgramState::NeedsAttention,
				provenance,
			)
			.await?
	);

	Ok(())
}

async fn assert_objective_create_race(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	let objective_id = ObjectiveId::new("71000000-0000-4000-8000-000000000080")?;
	let objective = Objective::new(
		objective_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Resolve one concurrent creation",
		vec!["Accepted once".into()],
		vec!["Validated once".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;
	let provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000082")?,
		"Concurrent Objective creation",
	)?;
	let commands = [
		CommandIdentity::new("objective-create-race-a-80", b"objective-create-race-a-80")?,
		CommandIdentity::new("objective-create-race-b-80", b"objective-create-race-b-80")?,
	];
	let mut tasks = JoinSet::new();

	for command in commands.clone() {
		let store = store.clone();
		let objective = objective.clone();
		let provenance = provenance.clone();

		tasks.spawn(async move {
			let result = store.create_objective(&command, &objective, &provenance).await;

			(command, result)
		});
	}

	while let Some(result) = tasks.join_next().await {
		let (command, result) = result?;

		assert_eq!(result?, store.create_objective(&command, &objective, &provenance).await?);
	}

	assert_single_domain_activity(client, "objective", objective_id.as_str(), "objective_created")
		.await?;
	assert_objective_missing_replay(store, client, project_id, program_id, &provenance).await?;

	Ok(())
}

async fn assert_objective_missing_replay(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	program_id: &ProgramId,
	provenance: &ProgramProvenance,
) -> Result<(), Box<dyn std::error::Error>> {
	let race_id = ObjectiveId::new("71000000-0000-4000-8000-000000000081")?;
	let race_objective = Objective::new(
		race_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Create after a deterministic miss",
		vec!["Accepted".into()],
		vec!["Validated".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;
	let transition =
		CommandIdentity::new("objective-absent-transition-81", b"objective-absent-transition-81")?;

	assert!(store.objective(&race_id).await?.is_none());
	assert!(matches!(
		store
			.transition_objective(
				&transition,
				project_id,
				&race_id,
				1,
				ObjectiveState::Active,
				provenance,
			)
			.await,
		Err(StoreError::RevisionConflict { actual: None, .. })
	));

	store
		.create_objective(
			&CommandIdentity::new(
				"objective-create-after-miss-81",
				b"objective-create-after-miss-81",
			)?,
			&race_objective,
			provenance,
		)
		.await?;

	assert!(matches!(
		store
			.transition_objective(
				&transition,
				project_id,
				&race_id,
				1,
				ObjectiveState::Active,
				provenance,
			)
			.await,
		Err(StoreError::RevisionConflict { actual: None, .. })
	));

	let receipt_scope: String = client
		.query_one(
			"SELECT scope_id FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&"objective-absent-transition-81"],
		)
		.await?
		.get(0);

	assert_eq!(receipt_scope, project_id.as_str());

	let fresh_transition = CommandIdentity::new(
		"objective-create-between-read-transition-81",
		b"objective-create-between-read-transition-81",
	)?;
	let transitioned = store
		.transition_objective(
			&fresh_transition,
			project_id,
			&race_id,
			1,
			ObjectiveState::Active,
			provenance,
		)
		.await?;

	assert_eq!(transitioned.objective.state(), ObjectiveState::Active);
	assert_eq!(
		transitioned,
		store
			.transition_objective(
				&fresh_transition,
				project_id,
				&race_id,
				1,
				ObjectiveState::Active,
				provenance,
			)
			.await?
	);

	Ok(())
}

async fn assert_objective_evidence_race(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	let objective_id = ObjectiveId::new("71000000-0000-4000-8000-000000000082")?;
	let provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000084")?,
		"Concurrent evidence fixture",
	)?;
	let objective = Objective::new(
		objective_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Accept one finite concurrent result",
		vec!["Accepted".into()],
		vec!["Validated".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;

	store
		.create_objective(
			&CommandIdentity::new("objective-create-evidence-82", b"objective-create-evidence-82")?,
			&objective,
			&provenance,
		)
		.await?;
	store
		.transition_objective(
			&CommandIdentity::new(
				"objective-activate-evidence-82",
				b"objective-activate-evidence-82",
			)?,
			project_id,
			&objective_id,
			1,
			ObjectiveState::Active,
			&provenance,
		)
		.await?;

	let prior_updated_at: i64 = client
		.query_one(
			"SELECT (EXTRACT(epoch FROM updated_at)*1000000)::bigint \
			 FROM decodex.objectives WHERE objective_id=$1::text::uuid",
			&[&objective_id.as_str()],
		)
		.await?
		.get(0);
	let timestamp = ProgramTimestamp::from_unix_microseconds(prior_updated_at)?;
	let evidence = ObjectiveCompletionEvidence::proposed(
		ObjectiveEvidenceId::new("81000000-0000-4000-8000-000000000082")?,
		objective_id.clone(),
		project_id.clone(),
		2,
		"Accepted concurrent result",
		lead_id.clone(),
		timestamp,
		"Acceptance retained",
		"Validated concurrent result",
		lead_id.clone(),
		timestamp,
		"Validation retained",
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000085")?,
	)?;
	let commands = [
		CommandIdentity::new("objective-evidence-race-a-82", b"objective-evidence-race-a-82")?,
		CommandIdentity::new("objective-evidence-race-b-82", b"objective-evidence-race-b-82")?,
	];
	let mut tasks = JoinSet::new();

	for command in commands.clone() {
		let store = store.clone();
		let evidence = evidence.clone();

		tasks.spawn(async move {
			let result = store.achieve_objective(&command, &evidence).await;

			(command, result)
		});
	}

	while let Some(result) = tasks.join_next().await {
		let (command, result) = result?;

		assert_eq!(result?, store.achieve_objective(&command, &evidence).await?);
	}

	assert_single_domain_activity(client, "objective", objective_id.as_str(), "objective_achieved")
		.await?;

	Ok(())
}

async fn assert_cross_objective_evidence_identity_race(
	store: &PostgresStore,
	client: &Client,
	schema_owner: &Config,
	project_id: &ProjectId,
	lead_id: &AgentId,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	let (objective_a, timestamp_a) = create_active_evidence_objective(
		store,
		client,
		project_id,
		lead_id,
		program_id,
		ActiveEvidenceObjectiveFixture {
			objective_id: "71000000-0000-4000-8000-000000000083",
			correlation_id: "51000000-0000-4000-8000-000000000086",
			create_key: "objective-create-evidence-83",
			activate_key: "objective-activate-evidence-83",
		},
	)
	.await?;
	let (objective_b, timestamp_b) = create_active_evidence_objective(
		store,
		client,
		project_id,
		lead_id,
		program_id,
		ActiveEvidenceObjectiveFixture {
			objective_id: "71000000-0000-4000-8000-000000000084",
			correlation_id: "51000000-0000-4000-8000-000000000087",
			create_key: "objective-create-evidence-84",
			activate_key: "objective-activate-evidence-84",
		},
	)
	.await?;
	let accepted_at = if timestamp_a >= timestamp_b { timestamp_a } else { timestamp_b };
	let evidence_id = ObjectiveEvidenceId::new("81000000-0000-4000-8000-000000000083")?;
	let cases = [
		(
			CommandIdentity::new("objective-evidence-cross-race-a-83", b"cross-race-a-83")?,
			cross_objective_evidence(
				evidence_id.clone(),
				objective_a,
				project_id,
				lead_id,
				accepted_at,
				"51000000-0000-4000-8000-000000000088",
			)?,
		),
		(
			CommandIdentity::new("objective-evidence-cross-race-b-83", b"cross-race-b-83")?,
			cross_objective_evidence(
				evidence_id,
				objective_b,
				project_id,
				lead_id,
				accepted_at,
				"51000000-0000-4000-8000-000000000089",
			)?,
		),
	];

	let (mut blocker, blocker_connection) = schema_owner.clone().connect(NoTls).await?;
	let blocker_task = task::spawn(blocker_connection);
	let blocker_transaction = blocker.transaction().await?;

	blocker_transaction
		.batch_execute("LOCK TABLE decodex.activity IN ACCESS EXCLUSIVE MODE")
		.await?;

	let mut tasks = JoinSet::new();

	for (command, evidence) in cases {
		let store = store.clone();

		tasks.spawn(async move {
			let result = store.achieve_objective(&command, &evidence).await;

			(command, evidence, result)
		});
	}

	let contention = wait_for_cross_objective_evidence_contention(client).await;

	blocker_transaction.rollback().await?;
	drop(blocker);
	blocker_task.await??;

	if let Err(error) = contention {
		tasks.abort_all();

		while tasks.join_next().await.is_some() {}

		return Err(error);
	}

	let mut results = Vec::new();

	while let Some(joined) = tasks.join_next().await {
		results.push(joined?);
	}

	assert_cross_objective_evidence_results(store, client, results).await
}

struct ActiveEvidenceObjectiveFixture<'a> {
	objective_id: &'a str,
	correlation_id: &'a str,
	create_key: &'a str,
	activate_key: &'a str,
}

async fn create_active_evidence_objective(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	program_id: &ProgramId,
	fixture: ActiveEvidenceObjectiveFixture<'_>,
) -> Result<(ObjectiveId, ProgramTimestamp), Box<dyn std::error::Error>> {
	let objective_id = ObjectiveId::new(fixture.objective_id)?;
	let provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new(fixture.correlation_id)?,
		"Cross-Objective evidence identity race fixture",
	)?;
	let objective = Objective::new(
		objective_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Accept one identity-race outcome",
		vec!["Accepted".into()],
		vec!["Validated".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;

	store
		.create_objective(
			&CommandIdentity::new(fixture.create_key, fixture.create_key.as_bytes())?,
			&objective,
			&provenance,
		)
		.await?;
	store
		.transition_objective(
			&CommandIdentity::new(fixture.activate_key, fixture.activate_key.as_bytes())?,
			project_id,
			&objective_id,
			1,
			ObjectiveState::Active,
			&provenance,
		)
		.await?;

	let updated_at: i64 = client
		.query_one(
			"SELECT (EXTRACT(epoch FROM updated_at)*1000000)::bigint \
			 FROM decodex.objectives WHERE objective_id=$1::text::uuid",
			&[&objective_id.as_str()],
		)
		.await?
		.get(0);

	Ok((objective_id, ProgramTimestamp::from_unix_microseconds(updated_at)?))
}

fn cross_objective_evidence(
	evidence_id: ObjectiveEvidenceId,
	objective_id: ObjectiveId,
	project_id: &ProjectId,
	lead_id: &AgentId,
	timestamp: ProgramTimestamp,
	correlation_id: &str,
) -> Result<ObjectiveCompletionEvidence, Box<dyn std::error::Error>> {
	Ok(ObjectiveCompletionEvidence::proposed(
		evidence_id,
		objective_id,
		project_id.clone(),
		2,
		"Accepted identity-race result",
		lead_id.clone(),
		timestamp,
		"Acceptance identity retained",
		"Validated identity-race result",
		lead_id.clone(),
		timestamp,
		"Validation identity retained",
		ProgramCorrelationId::new(correlation_id)?,
	)?)
}

async fn wait_for_cross_objective_evidence_contention(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	for _ in 0..500 {
		let row = client
			.query_one(
				r#"SELECT
				 count(*) FILTER (WHERE locks.locktype = 'relation'
				 AND locks.relation = 'decodex.activity'::pg_catalog.regclass
				 AND locks.mode = 'RowExclusiveLock' AND locks.granted IS FALSE),
				 count(*) FILTER (WHERE locks.locktype = 'transactionid'
				 AND locks.mode = 'ShareLock' AND locks.granted IS FALSE)
				 FROM pg_catalog.pg_locks AS locks"#,
				&[],
			)
			.await?;
		let waiting_activity: i64 = row.get(0);
		let waiting_evidence: i64 = row.get(1);

		if waiting_activity >= 1 && waiting_evidence >= 1 {
			return Ok(());
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	Err(std::io::Error::other("cross-Objective evidence commands did not reach both lock waits")
		.into())
}

async fn assert_cross_objective_evidence_results(
	store: &PostgresStore,
	client: &Client,
	results: Vec<(
		CommandIdentity,
		ObjectiveCompletionEvidence,
		Result<decodex_postgres::ObjectiveRecord, StoreError>,
	)>,
) -> Result<(), Box<dyn std::error::Error>> {
	let mut winners = 0;
	let mut losers = 0;

	for (command, evidence, result) in results {
		match result {
			Ok(record) => {
				winners += 1;

				assert_eq!(record, store.achieve_objective(&command, &evidence).await?);
			},
			Err(StoreError::IdempotencyConflict) => {
				losers += 1;

				assert!(matches!(
					store.achieve_objective(&command, &evidence).await,
					Err(StoreError::IdempotencyConflict)
				));
			},
			Err(error) => return Err(error.into()),
		}
	}

	assert_eq!((winners, losers), (1, 1));

	let authority = client
		.query_one(
			r#"SELECT
			 (SELECT count(*) FROM decodex.objectives WHERE objective_id IN
			 ('71000000-0000-4000-8000-000000000083','71000000-0000-4000-8000-000000000084')
			 AND state='achieved'),
			 (SELECT count(*) FROM decodex.objective_completion_evidence
			 WHERE evidence_id='81000000-0000-4000-8000-000000000083'),
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='objective'
			 AND aggregate_id IN ('71000000-0000-4000-8000-000000000083',
			 '71000000-0000-4000-8000-000000000084') AND event_kind='objective_achieved'),
			 (SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key IN
			 ('objective-evidence-cross-race-a-83','objective-evidence-cross-race-b-83')
			 AND receipt_state='completed'),
			 (SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key IN
			 ('objective-evidence-cross-race-a-83','objective-evidence-cross-race-b-83')
			 AND receipt_state='pending')"#,
			&[],
		)
		.await?;

	assert_eq!(authority.get::<_, i64>(0), 1);
	assert_eq!(authority.get::<_, i64>(1), 1);
	assert_eq!(authority.get::<_, i64>(2), 1);
	assert_eq!(authority.get::<_, i64>(3), 2);
	assert_eq!(authority.get::<_, i64>(4), 0);

	Ok(())
}

async fn assert_single_domain_activity(
	client: &Client,
	aggregate_kind: &str,
	aggregate_id: &str,
	event_kind: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let count: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.activity WHERE aggregate_kind=$1 \
			 AND aggregate_id=$2 AND event_kind=$3",
			&[&aggregate_kind, &aggregate_id, &event_kind],
		)
		.await?
		.get(0);

	assert_eq!(count, 1);

	Ok(())
}

async fn assert_program_context_compilation_is_pure(
	client: &Client,
	program: &Program,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	let counts = |row: Row| {
		(row.get::<_, i64>(0), row.get::<_, i64>(1), row.get::<_, i64>(2), row.get::<_, i64>(3))
	};
	let query = "SELECT (SELECT count(*) FROM decodex.agents),\
		 (SELECT count(*) FROM decodex.conversations),\
		 (SELECT count(*) FROM decodex.runtime_sessions),\
		 (SELECT count(*) FROM decodex.programs)";
	let before = counts(client.query_one(query, &[]).await?);
	let context = decodex_core::compile_program_context(ProgramContextInput {
		program,
		recent_decisions: Vec::new(),
		quiet_period: None,
	})?;
	let after = counts(client.query_one(query, &[]).await?);

	assert_eq!(before, after);
	assert_eq!(context.program_id(), program_id);
	assert!(!context.bytes().is_empty());

	Ok(())
}

async fn assert_program_transition_replay(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
	project_id: &ProjectId,
	program_id: &ProgramId,
	lead_id: &AgentId,
) -> Result<(), Box<dyn std::error::Error>> {
	let provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000062")?,
		"Lead classified current attention",
	)?;
	let candidates = [
		(CommandIdentity::new("program-race-blocked-60", b"blocked")?, ProgramState::Blocked),
		(CommandIdentity::new("program-race-paused-60", b"paused")?, ProgramState::Paused),
	];
	let mut tasks = JoinSet::new();

	for (command, state) in candidates {
		let store = store.clone();
		let project_id = project_id.clone();
		let program_id = program_id.clone();
		let provenance = provenance.clone();

		tasks.spawn(async move {
			let result = store
				.transition_program(&command, &project_id, &program_id, 2, state, &provenance)
				.await;

			(command, state, result)
		});
	}

	let mut loser = None;
	let mut winner_count = 0;

	while let Some(result) = tasks.join_next().await {
		let (command, state, result) = result?;

		match result {
			Ok(_) => winner_count += 1,
			Err(StoreError::RevisionConflict { actual: Some(3), .. }) =>
				loser = Some((command, state)),
			Err(error) => return Err(error.into()),
		}
	}

	assert_eq!(winner_count, 1);

	let (loser, loser_state) = loser.expect("one optimistic Program transition loses");

	assert!(matches!(
		store.transition_program(&loser, project_id, program_id, 2, loser_state, &provenance).await,
		Err(StoreError::RevisionConflict { actual: Some(3), .. })
	));

	let pending: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.command_receipts WHERE receipt_state='pending'\
			 AND operation IN ('create_program','update_program_context','transition_program',\
			 'create_objective','transition_objective','achieve_objective')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(pending, 0);

	let reopened =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;

	assert_eq!(reopened.program(program_id).await?, store.program(program_id).await?);
	assert!(matches!(
		reopened
			.transition_program(&loser, project_id, program_id, 2, loser_state, &provenance)
			.await,
		Err(StoreError::RevisionConflict { actual: Some(3), .. })
	));

	reopened.close();

	Ok(())
}

async fn assert_hostile_program_objective_sql(
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	policy_id: &PolicyId,
) -> Result<(), Box<dyn std::error::Error>> {
	let boundary_metrics = serde_json::json!([{
		"key":"metric_1","value":"x".repeat(256),"unit":"u".repeat(64),
		"provenance":{"source":"s".repeat(256),"observed_at_microseconds":253_402_300_799_999_999_i64}
	}]);
	let boundary_signals = serde_json::json!([{
		"id":"61000000-0000-4000-8000-000000000098","kind":"k".repeat(64),
		"summary":"x".repeat(4_096),
		"provenance":{"source":"s".repeat(256),"observed_at_microseconds":253_402_300_799_999_999_i64}
	}]);
	let boundary_criteria =
		(0..32).map(|index| format!("{index:02}{}", "x".repeat(4_094))).collect::<Vec<_>>();
	let boundary = client
		.query_one(
			"SELECT decodex.is_program_metrics($1),decodex.is_program_signals($2),\
			 decodex.is_objective_criteria($3)",
			&[&boundary_metrics, &boundary_signals, &boundary_criteria],
		)
		.await?;

	assert!(boundary.get::<_, bool>(0));
	assert!(boundary.get::<_, bool>(1));
	assert!(boundary.get::<_, bool>(2));

	client
		.execute(
			"WITH written AS (SELECT clock_timestamp() AS at)\
			 INSERT INTO decodex.objectives (objective_id,project_id,outcome,\
			 acceptance_criteria,validation_criteria,target_at,last_changed_by,\
			 last_correlation_id,last_provenance,created_at,updated_at)\
			 SELECT '71000000-0000-4000-8000-000000000097',$1::text::uuid,\
			 'valid maximum criteria',$3,$3,to_timestamp(2000000000),$2::text::uuid,\
			 gen_random_uuid(),'direct valid boundary fixture',written.at,written.at FROM written",
			&[&project_id.as_str(), &lead_id.as_str(), &boundary_criteria],
		)
		.await?;

	for document in [
		serde_json::json!([null]),
		serde_json::json!([{}]),
		serde_json::json!([{"key":"metric","value":"","unit":"count","provenance":{"source":"fixture","observed_at_microseconds":1}}]),
		serde_json::json!([{"key":"metric","value":"1","unit":"count","provenance":{"source":"fixture"}}]),
		serde_json::json!([{"key":"metric","value":"1","unit":"count","provenance":{"source":"fixture","observed_at_microseconds":1}}, {"key":"metric","value":"2","unit":"count","provenance":{"source":"fixture","observed_at_microseconds":2}}]),
	] {
		client
			.execute(
				"UPDATE decodex.programs SET metrics=$1,revision=revision+1,\
				 last_correlation_id=gen_random_uuid(),last_provenance='hostile SQL fixture',\
				 updated_at=clock_timestamp()\
				 WHERE program_id='41000000-0000-4000-8000-000000000060'",
				&[&document],
			)
			.await
			.expect_err("PostgreSQL rejects Rust-invalid Program metric JSON");
	}
	for document in [
		serde_json::json!([null]),
		serde_json::json!([{"id":"61000000-0000-4000-8000-000000000099","kind":"bad-kind","summary":"x","provenance":{"source":"fixture","observed_at_microseconds":1}}]),
		serde_json::json!([{"id":"61000000-0000-4000-8000-000000000099","kind":"signal","summary":"","provenance":{"source":"fixture","observed_at_microseconds":1}}]),
	] {
		client
			.execute(
				"UPDATE decodex.programs SET signals=$1,revision=revision+1,\
				 last_correlation_id=gen_random_uuid(),last_provenance='hostile SQL fixture',\
				 updated_at=clock_timestamp()\
				 WHERE program_id='41000000-0000-4000-8000-000000000060'",
				&[&document],
			)
			.await
			.expect_err("PostgreSQL rejects Rust-invalid Program signal JSON");
	}

	let program_timestamp_error = client
		.execute(
			"UPDATE decodex.programs SET next_review_at=TIMESTAMPTZ '10000-01-01 00:00:00+00',\
			 revision=revision+1,last_correlation_id=gen_random_uuid(),\
			 last_provenance='hostile timestamp fixture',updated_at=clock_timestamp()\
			 WHERE program_id='41000000-0000-4000-8000-000000000060'",
			&[],
		)
		.await
		.expect_err("PostgreSQL rejects timestamps outside ProgramTimestamp");

	assert_eq!(
		program_timestamp_error.as_db_error().and_then(|error| error.constraint()),
		Some("programs_finite_timestamps")
	);

	let timestamp_boundaries = client
		.query_one(
			"SELECT decodex.program_timestamp(0) IS NOT NULL,\
			 decodex.program_timestamp(253402300799999999) IS NOT NULL,\
			 decodex.program_timestamp(-1) IS NULL,\
			 decodex.program_timestamp(253402300800000000) IS NULL",
			&[],
		)
		.await?;

	assert!(timestamp_boundaries.get::<_, bool>(0));
	assert!(timestamp_boundaries.get::<_, bool>(1));
	assert!(timestamp_boundaries.get::<_, bool>(2));
	assert!(timestamp_boundaries.get::<_, bool>(3));

	assert_hostile_objective_sql(client, project_id, lead_id).await?;

	let exact_policy_reference: bool = client
		.query_one(
			"SELECT EXISTS (SELECT 1 FROM decodex.programs WHERE project_id=$1::text::uuid \
			 AND policy_id=$2::text::uuid AND policy_revision=1)",
			&[&project_id.as_str(), &policy_id.as_str()],
		)
		.await?
		.get(0);

	assert!(exact_policy_reference);

	Ok(())
}

async fn assert_hostile_objective_sql(
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
) -> Result<(), Box<dyn std::error::Error>> {
	for criteria in [
		Vec::<String>::new(),
		vec![String::new()],
		vec!["duplicate".into(), "duplicate".into()],
		vec!["x".repeat(4_097)],
	] {
		client
			.execute(
				"WITH written AS (SELECT clock_timestamp() AS at)\
				 INSERT INTO decodex.objectives (objective_id,project_id,outcome,\
				 acceptance_criteria,validation_criteria,target_at,last_changed_by,\
				 last_correlation_id,last_provenance,created_at,updated_at)\
				 SELECT gen_random_uuid(),$1::text::uuid,'hostile criteria',$3,$3,\
				 to_timestamp(2000000000),$2::text::uuid,gen_random_uuid(),\
				 'hostile SQL fixture',written.at,written.at FROM written",
				&[&project_id.as_str(), &lead_id.as_str(), &criteria],
			)
			.await
			.expect_err("PostgreSQL rejects Rust-invalid Objective criteria");
	}

	let objective_timestamp_error = client
		.execute(
			"WITH written AS (SELECT clock_timestamp() AS at)\
			 INSERT INTO decodex.objectives (objective_id,project_id,outcome,\
			 acceptance_criteria,validation_criteria,target_at,last_changed_by,\
			 last_correlation_id,last_provenance,created_at,updated_at)\
			 SELECT '71000000-0000-4000-8000-000000000096',$1::text::uuid,\
			 'hostile timestamp',ARRAY['accepted'],ARRAY['validated'],\
			 TIMESTAMPTZ '10000-01-01 00:00:00+00',$2::text::uuid,gen_random_uuid(),\
			 'hostile timestamp fixture',written.at,written.at FROM written",
			&[&project_id.as_str(), &lead_id.as_str()],
		)
		.await
		.expect_err("PostgreSQL rejects Objective timestamps outside ProgramTimestamp");

	assert_eq!(
		objective_timestamp_error.as_db_error().and_then(|error| error.constraint()),
		Some("objectives_finite_timestamps")
	);

	let direct_objective = "71000000-0000-4000-8000-000000000099";
	let row = client
		.query_one(
			"SELECT result_code FROM decodex.create_objective(\
			 $1::text::decodex.canonical_uuid_v4_text,\
			 $2::text::decodex.canonical_uuid_v4_text,NULL,'direct SQL objective',\
			 ARRAY['accepted'],ARRAY['validated'],2000000000000000,\
			 $3::text::decodex.canonical_uuid_v4_text,\
			 '51000000-0000-4000-8000-000000000099'::decodex.canonical_uuid_v4_text,\
			 'direct SQL fixture')",
			&[&direct_objective, &project_id.as_str(), &lead_id.as_str()],
		)
		.await?;

	assert_eq!(row.get::<_, &str>(0), "ok");

	client
		.query_one(
			"SELECT result_code FROM decodex.transition_objective(\
			 $1::text::decodex.canonical_uuid_v4_text,\
			 $2::text::decodex.canonical_uuid_v4_text,1,'active',\
			 $3::text::decodex.canonical_uuid_v4_text,\
			 '51000000-0000-4000-8000-000000000098'::decodex.canonical_uuid_v4_text,\
			 'activate direct SQL fixture')",
			&[&direct_objective, &project_id.as_str(), &lead_id.as_str()],
		)
		.await?;

	assert_hostile_objective_evidence(client).await?;

	let prior_revision_error = client
		.execute(
			"WITH evidence AS (INSERT INTO decodex.objective_completion_evidence\
			 (evidence_id,objective_id,project_id,objective_revision,objective_updated_at,\
			 acceptance_result,accepted_by,accepted_at,acceptance_provenance,validation_result,\
			 validated_by,validated_at,validation_provenance,correlation_id) VALUES\
			 ('81000000-0000-4000-8000-000000000099','71000000-0000-4000-8000-000000000099',\
			 '11000000-0000-4000-8000-000000000060',2,to_timestamp(1700000000),\
			 'accepted','22000000-0000-4000-8000-000000000060',to_timestamp(1700000000),\
			 'accepted','validated','22000000-0000-4000-8000-000000000060',\
			 to_timestamp(1700000001),'validated','51000000-0000-4000-8000-000000000097')\
			 RETURNING evidence_id)\
			 UPDATE decodex.objectives SET state='achieved',revision=revision+1,\
			 completion_evidence_id='81000000-0000-4000-8000-000000000099',\
			 last_changed_by='22000000-0000-4000-8000-000000000060',\
			 last_correlation_id='51000000-0000-4000-8000-000000000097',\
			 last_provenance='hostile stale achievement',updated_at=clock_timestamp()\
			 FROM evidence WHERE objective_id='71000000-0000-4000-8000-000000000099'",
			&[],
		)
		.await
		.expect_err("achievement evidence cannot predate the exact prior Objective revision");

	assert_eq!(
		prior_revision_error.as_db_error().and_then(|error| error.constraint()),
		Some("objective_evidence_prior_revision_time")
	);

	client
		.execute(
			"UPDATE decodex.objectives SET state='achieved',revision=revision+1,\
			 completion_evidence_id=gen_random_uuid(),last_changed_by=$2::text::uuid,\
			 last_correlation_id=gen_random_uuid(),last_provenance='bare achievement',\
			 updated_at=clock_timestamp() WHERE objective_id=$1::text::uuid",
			&[&direct_objective, &lead_id.as_str()],
		)
		.await
		.expect_err("bare Objective achievement without exact evidence is impossible");

	Ok(())
}

async fn assert_hostile_objective_evidence(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	for (statement, expected_constraint) in [
		(
			"INSERT INTO decodex.objective_completion_evidence\
			 (evidence_id,objective_id,project_id,objective_revision,objective_updated_at,acceptance_result,\
			 accepted_by,accepted_at,acceptance_provenance,validation_result,validated_by,\
			 validated_at,validation_provenance,correlation_id) VALUES\
			 (gen_random_uuid(),'71000000-0000-4000-8000-000000000099',\
			 '11000000-0000-4000-8000-000000000060',2,to_timestamp(1700000000),'',\
			 '22000000-0000-4000-8000-000000000060',to_timestamp(1700000000),'accepted',\
			 'validated','22000000-0000-4000-8000-000000000060',\
			 to_timestamp(1700000001),'validated',gen_random_uuid())",
			"objective_evidence_text_bounded",
		),
		(
			"INSERT INTO decodex.objective_completion_evidence\
			 (evidence_id,objective_id,project_id,objective_revision,objective_updated_at,acceptance_result,\
			 accepted_by,accepted_at,acceptance_provenance,validation_result,validated_by,\
			 validated_at,validation_provenance,correlation_id) VALUES\
			 (gen_random_uuid(),'71000000-0000-4000-8000-000000000099',\
			 '11000000-0000-4000-8000-000000000060',2,to_timestamp(1700000000),'accepted',\
			 '22000000-0000-4000-8000-000000000099',to_timestamp(1700000000),'accepted',\
			 'validated','22000000-0000-4000-8000-000000000060',\
			 to_timestamp(1700000001),'validated',gen_random_uuid())",
			"objective_evidence_accepting_agent_project_fk",
		),
		(
			"INSERT INTO decodex.objective_completion_evidence\
			 (evidence_id,objective_id,project_id,objective_revision,objective_updated_at,acceptance_result,\
			 accepted_by,accepted_at,acceptance_provenance,validation_result,validated_by,\
			 validated_at,validation_provenance,correlation_id) VALUES\
			 (gen_random_uuid(),'71000000-0000-4000-8000-000000000099',\
			 '11000000-0000-4000-8000-000000000060',2,to_timestamp(1700000000),'accepted',\
			 '22000000-0000-4000-8000-000000000060',to_timestamp(1700000002),'accepted',\
			 'validated','22000000-0000-4000-8000-000000000060',\
			 to_timestamp(1700000001),'validated',gen_random_uuid())",
			"objective_evidence_chronology",
		),
		(
			"INSERT INTO decodex.objective_completion_evidence\
			 (evidence_id,objective_id,project_id,objective_revision,objective_updated_at,acceptance_result,\
			 accepted_by,accepted_at,acceptance_provenance,validation_result,validated_by,\
			 validated_at,validation_provenance,correlation_id) VALUES\
			 (gen_random_uuid(),'71000000-0000-4000-8000-000000000099',\
			 '11000000-0000-4000-8000-000000000060',2,to_timestamp(-1),'accepted',\
			 '22000000-0000-4000-8000-000000000060',to_timestamp(0),'accepted',\
			 'validated','22000000-0000-4000-8000-000000000060',\
			 to_timestamp(1),'validated',gen_random_uuid())",
			"objective_evidence_chronology",
		),
	] {
		let error = client
			.batch_execute(statement)
			.await
			.expect_err("hostile direct Objective evidence is rejected");

		assert_eq!(
			error.as_db_error().and_then(|error| error.constraint()),
			Some(expected_constraint)
		);
	}

	Ok(())
}

async fn assert_objective_lifecycle(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	lead_id: &AgentId,
	program_id: &ProgramId,
) -> Result<(), Box<dyn std::error::Error>> {
	let achieved_id = ObjectiveId::new("71000000-0000-4000-8000-000000000060")?;
	let achieved = Objective::new(
		achieved_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Publish the finite Q3 search brief",
		vec!["Lead accepts the brief".into()],
		vec!["Independent checks pass".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;
	let create_provenance = ProgramProvenance::new(
		lead_id.clone(),
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000070")?,
		"Lead proposed a finite Objective",
	)?;
	let create_command = CommandIdentity::new("objective-create-60", b"objective-create-60")?;

	assert_eq!(
		store.create_objective(&create_command, &achieved, &create_provenance).await?,
		store.create_objective(&create_command, &achieved, &create_provenance).await?,
	);

	let activate_command = CommandIdentity::new("objective-activate-60", b"objective-activate-60")?;
	let active = store
		.transition_objective(
			&activate_command,
			project_id,
			&achieved_id,
			1,
			ObjectiveState::Active,
			&create_provenance,
		)
		.await?;

	assert_eq!(active.objective.revision(), 2);

	let prior_updated_at: i64 = client
		.query_one(
			"SELECT (EXTRACT(epoch FROM updated_at)*1000000)::bigint \
			 FROM decodex.objectives WHERE objective_id=$1::text::uuid",
			&[&achieved_id.as_str()],
		)
		.await?
		.get(0);
	let accepted_at = ProgramTimestamp::from_unix_microseconds(prior_updated_at)?;

	assert_bare_achievement_replays(store, project_id, &achieved_id, &create_provenance).await?;

	let evidence = ObjectiveCompletionEvidence::proposed(
		ObjectiveEvidenceId::new("81000000-0000-4000-8000-000000000060")?,
		achieved_id.clone(),
		project_id.clone(),
		2,
		"Lead accepted the bounded outcome",
		lead_id.clone(),
		accepted_at,
		"Acceptance checklist retained",
		"Validation criteria passed",
		lead_id.clone(),
		accepted_at,
		"Validation record retained",
		ProgramCorrelationId::new("51000000-0000-4000-8000-000000000071")?,
	)?;
	let achieve_command = CommandIdentity::new("objective-achieve-60", b"objective-achieve-60")?;
	let completed = store.achieve_objective(&achieve_command, &evidence).await?;

	assert_eq!(completed.objective.state(), ObjectiveState::Achieved);
	assert_eq!(completed.objective.revision(), 3);

	let completion = completed.objective.completion().expect("achievement evidence");

	assert_eq!(completion.id(), evidence.id());
	assert_eq!(completion.objective_updated_at(), Some(accepted_at));
	assert_eq!(completed, store.achieve_objective(&achieve_command, &evidence).await?);

	let abandoned_id = ObjectiveId::new("71000000-0000-4000-8000-000000000061")?;
	let abandoned = Objective::new(
		abandoned_id.clone(),
		project_id.clone(),
		Some(program_id.clone()),
		"Evaluate one finite channel experiment",
		vec!["Decision documented".into()],
		vec!["Experiment data reviewed".into()],
		ProgramTimestamp::from_unix_microseconds(2_000_000_000_000_000)?,
	)?;
	let abandoned_create = CommandIdentity::new("objective-create-61", b"objective-create-61")?;

	store.create_objective(&abandoned_create, &abandoned, &create_provenance).await?;

	let abandoned_record = store
		.transition_objective(
			&CommandIdentity::new("objective-abandon-61", b"objective-abandon-61")?,
			project_id,
			&abandoned_id,
			1,
			ObjectiveState::Abandoned,
			&create_provenance,
		)
		.await?;

	assert_eq!(abandoned_record.objective.state(), ObjectiveState::Abandoned);
	assert_eq!(store.objective(&achieved_id).await?, Some(completed));

	Ok(())
}

async fn assert_bare_achievement_replays(
	store: &PostgresStore,
	project_id: &ProjectId,
	objective_id: &ObjectiveId,
	provenance: &ProgramProvenance,
) -> Result<(), Box<dyn std::error::Error>> {
	let command = CommandIdentity::new("objective-bare-achieve-60", b"bare-achieve")?;

	for _ in 0..2 {
		assert!(matches!(
			store
				.transition_objective(
					&command,
					project_id,
					objective_id,
					2,
					ObjectiveState::Achieved,
					provenance,
				)
				.await,
			Err(StoreError::InvalidInput("Program/Objective lifecycle transition is invalid"))
		));
	}

	Ok(())
}

async fn assert_policy_revision_conflicts(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	policy_id: &PolicyId,
	lead_id: &AgentId,
	winner_request: &PolicyRevisionAcceptance,
	winner_revision: &AcceptedPolicyRevision,
) -> Result<(), Box<dyn std::error::Error>> {
	let stale = client
		.query_one(
			"SELECT revision_accepted,actual_revision \
			 FROM decodex.accept_policy_revision(\
			 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
			 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,3,'stale','{}'::jsonb,\
			 $3::pg_catalog.text::decodex.canonical_uuid_v4_text,1)",
			&[&policy_id.as_str(), &project_id.as_str(), &lead_id.as_str()],
		)
		.await?;

	assert!(!stale.get::<_, bool>(0));
	assert_eq!(stale.get::<_, Option<i64>>(1), Some(2));

	let gapped_request = policy_acceptance(project_id, policy_id, lead_id, 4, "gapped");

	assert!(matches!(
		store.accept_policy_revision(gapped_request).await,
		Err(StoreError::RevisionConflict {
			ref entity,
			expected: Some(4),
			actual: Some(2),
		}) if entity == policy_id.as_str()
	));
	assert_eq!(store.policy_revision(winner_revision.id()).await?, Some(winner_revision.clone()));
	assert_eq!(
		store.accept_policy_revision(winner_request.clone()).await?,
		winner_revision.clone()
	);

	Ok(())
}

async fn accept_concurrent_policy_revision(
	store: &PostgresStore,
	candidates: [PolicyRevisionAcceptance; 2],
) -> Result<(PolicyRevisionAcceptance, AcceptedPolicyRevision), Box<dyn std::error::Error>> {
	let mut tasks = JoinSet::new();

	for candidate in candidates {
		let store = store.clone();

		tasks.spawn(async move {
			let result = store.accept_policy_revision(candidate.clone()).await;

			(candidate, result)
		});
	}

	let mut winner = None;
	let mut conflicts = 0;

	while let Some(result) = tasks.join_next().await {
		let (candidate, result) = result?;

		match result {
			Ok(accepted) => winner = Some((candidate, accepted)),
			Err(StoreError::IdempotencyConflict) => conflicts += 1,
			Err(error) => return Err(error.into()),
		}
	}

	let (winner_request, winner_revision) = winner.expect("one concurrent acceptance wins");

	assert_eq!(conflicts, 1);

	Ok((winner_request, winner_revision))
}

async fn assert_policy_database_guards(
	store: &PostgresStore,
	client: &Client,
	project_id: &ProjectId,
	policy_id: &PolicyId,
	lead_id: &AgentId,
) -> Result<(), Box<dyn std::error::Error>> {
	for statement in [
		"UPDATE decodex.policy_revisions SET provenance='changed' \
		 WHERE policy_id='31000000-0000-4000-8000-000000000001' AND revision=1",
		"DELETE FROM decodex.policy_revisions \
		 WHERE policy_id='31000000-0000-4000-8000-000000000001' AND revision=1",
		"UPDATE decodex.policies SET created_at=created_at+interval '1 second' \
		 WHERE policy_id='31000000-0000-4000-8000-000000000001'",
	] {
		client
			.batch_execute(statement)
			.await
			.expect_err("accepted Policy authority rejects retroactive mutation");
	}

	client
		.batch_execute(
			"INSERT INTO decodex.policy_revisions \
			 (policy_id,project_id,revision,provenance,snapshot,accepted_by,supersedes_revision) VALUES \
			 ('31000000-0000-4000-8000-000000000001',\
			  '11000000-0000-4000-8000-000000000050',3,'cross-agent','{}',\
			  '22000000-0000-4000-8000-000000000051',2)",
		)
		.await
		.expect_err("cross-Project accepting Agent foreign key rejects attachment");
	store.transition_project(project_id, 1, ProjectStatus::Paused).await?;

	let paused_acceptance = policy_acceptance(project_id, policy_id, lead_id, 3, "paused-project");

	assert!(matches!(
		store.accept_policy_revision(paused_acceptance).await,
		Err(StoreError::InvalidInput("Policy acceptance requires active Project Lead authority"))
	));

	Ok(())
}

async fn assert_project_identity_pair_conflicts(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let first = project_request(
		"11000000-0000-4000-8000-000000000010",
		"22000000-0000-4000-8000-000000000010",
		"hack-ink/identity-a",
		"/srv/repos/identity-a",
	);
	let second = project_request(
		"11000000-0000-4000-8000-000000000011",
		"22000000-0000-4000-8000-000000000011",
		"hack-ink/identity-b",
		"/srv/repos/identity-b",
	);

	store.create_project(first.clone()).await?;
	store.create_project(second.clone()).await?;

	let rows_before: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.projects) + (SELECT count(*) FROM decodex.agents)",
			&[],
		)
		.await?
		.get(0);
	let split_pair = project_request(
		first.project.id().as_str(),
		"22000000-0000-4000-8000-000000000012",
		second.project.repository().identity().as_str(),
		"/srv/repos/identity-b",
	);
	let rebound_id = project_request(
		first.project.id().as_str(),
		"22000000-0000-4000-8000-000000000013",
		"hack-ink/identity-c",
		"/srv/repos/identity-c",
	);

	for request in [split_pair, rebound_id] {
		assert!(matches!(
			store.create_project(request).await,
			Err(StoreError::InvalidInput(
				"Project and repository identities are already bound differently"
			))
		));
	}

	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	let error = runtime_client
		.query(
			"SELECT * FROM decodex.create_project(\
			 '11000000-0000-4000-8000-000000000010','hack-ink/identity-b',\
			 '/srv/repos/identity-b','/srv/repos/identity-b','{}',\
			 '22000000-0000-4000-8000-000000000014')",
			&[],
		)
		.await
		.expect_err("direct SQL rejects a split Project/repository identity pair");

	assert_eq!(
		error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
		Some("projects_identity_pair")
	);

	drop(runtime_client);

	runtime_task.await??;

	let rows_after: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.projects) + (SELECT count(*) FROM decodex.agents)",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(rows_after, rows_before, "identity conflicts commit no partial rows");

	Ok(())
}

async fn assert_project_metadata_credential_sql_boundary(
	runtime_client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	for (statement, exposed_value) in [
		(
			"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000026',\
			 'hack-ink/credential-key','/srv/repos/credential-key','/srv/repos/credential-key',\
			 '{\"refresh_token\":\"sk-proj-0123456789abcdef\"}',\
			 '22000000-0000-4000-8000-000000000026')",
			"sk-proj",
		),
		(
			"SELECT * FROM decodex.create_project('11000000-0000-4000-8000-000000000028',\
			 'hack-ink/credential-value','/srv/repos/credential-value',\
			 '/srv/repos/credential-value','{\"note\":\"Bearer abcdefghijklmnop\"}',\
			 '22000000-0000-4000-8000-000000000028')",
			"Bearer",
		),
	] {
		let error = runtime_client
			.query(statement, &[])
			.await
			.expect_err("credential-shaped Project metadata is rejected");

		assert_eq!(
			error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
			Some("projects_metadata_no_credentials"),
		);

		let closed_error = StoreError::from(error);

		assert!(matches!(closed_error, StoreError::CredentialRejected));
		assert!(!closed_error.to_string().contains(exposed_value));
	}

	Ok(())
}

async fn assert_project_path_sql_acceptance(
	runtime_client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let row = runtime_client
		.query_one(
			"SELECT repository_root,default_cwd FROM decodex.create_project(\
			 '11000000-0000-4000-8000-000000000029','hack-ink/international-path',\
			 '/srv/répos/décodex','/srv/répos/décodex/crates','{}',\
			 '22000000-0000-4000-8000-000000000029')",
			&[],
		)
		.await?;

	assert_eq!(row.get::<_, &str>(0), "/srv/répos/décodex");
	assert_eq!(row.get::<_, &str>(1), "/srv/répos/décodex/crates");

	Ok(())
}

async fn assert_project_agent_canonical_sql_boundary(
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);

	assert_identity_ingress_authority(&runtime_client).await?;
	assert_project_path_sql_acceptance(&runtime_client).await?;

	let rows_before: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.projects) + (SELECT count(*) FROM decodex.agents)",
			&[],
		)
		.await?
		.get(0);

	for &(statement, constraint) in INVALID_PROJECT_AGENT_SQL_CALLS {
		let error = runtime_client
			.query(statement, &[])
			.await
			.expect_err("canonical SQL authority rejects invalid Project/Agent input");

		assert_eq!(
			error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
			Some(constraint),
			"statement: {statement}",
		);
	}

	assert_project_metadata_credential_sql_boundary(&runtime_client).await?;
	drop(runtime_client);

	runtime_task.await??;

	let rows_after: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.projects) + (SELECT count(*) FROM decodex.agents)",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(rows_after, rows_before, "invalid SQL calls commit no partial rows");

	Ok(())
}

async fn assert_identity_ingress_authority(
	runtime: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_identity_ingress_catalog(runtime).await?;
	assert_identity_ingress_exact_domain(runtime).await;
	assert_identity_ingress_unknown_literal(runtime).await;
	assert_identity_ingress_explicit_text(runtime).await;
	assert_identity_ingress_prepared_bound_text(runtime).await;
	assert_identity_ingress_invalid_text(runtime).await;
	assert_identity_ingress_null(runtime).await;
	assert_identity_ingress_bare_uuid(runtime).await;
	assert_identity_ingress_explicit_normalization(runtime).await;

	let exact_execute_count: i64 = runtime
		.query_one(
			"SELECT count(*) FROM pg_catalog.pg_proc AS proc
			 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=proc.pronamespace
			 WHERE namespace.nspname='decodex'
			 AND pg_catalog.has_function_privilege(session_user,proc.oid,'EXECUTE')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(exact_execute_count, RUNTIME_EXECUTE_SIGNATURES.len() as i64);

	for signature in RUNTIME_EXECUTE_SIGNATURES {
		let executable: bool = runtime
			.query_one(
				"SELECT pg_catalog.has_function_privilege(
				 session_user,pg_catalog.to_regprocedure($1),'EXECUTE')",
				&[signature],
			)
			.await?
			.get(0);

		assert!(executable, "runtime lacks {signature}");
	}
	for signature in TRIGGER_ONLY_SIGNATURES {
		let executable: bool = runtime
			.query_one(
				"SELECT pg_catalog.has_function_privilege(
				 session_user,pg_catalog.to_regprocedure($1),'EXECUTE')",
				&[signature],
			)
			.await?
			.get(0);

		assert!(!executable, "runtime executes trigger-only {signature}");
	}

	let public_and_defaults_closed: bool = runtime
		.query_one(
			"SELECT
			 NOT EXISTS (
			  SELECT 1 FROM pg_catalog.pg_proc AS proc
			  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=proc.pronamespace
			  CROSS JOIN LATERAL pg_catalog.aclexplode(
			   COALESCE(proc.proacl,pg_catalog.acldefault('f',proc.proowner))) AS privilege
			  WHERE namespace.nspname='decodex' AND privilege.grantee=0
			   AND privilege.privilege_type='EXECUTE')
			 AND NOT EXISTS (
			  SELECT 1 FROM pg_catalog.pg_type AS type
			  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace
			  CROSS JOIN LATERAL pg_catalog.aclexplode(
			   COALESCE(type.typacl,pg_catalog.acldefault('T',type.typowner))) AS privilege
			  WHERE namespace.nspname='decodex' AND type.typtype IN ('e','d')
			   AND privilege.grantee=0
			   AND privilege.privilege_type='USAGE')
			 AND (
			  SELECT count(*)=2
			   AND bool_and(default_acl.defaclnamespace=0)
			   AND count(*) FILTER (WHERE default_acl.defaclobjtype='f')=1
			   AND count(*) FILTER (WHERE default_acl.defaclobjtype='T')=1
			   AND bool_and(NOT EXISTS (
			    SELECT 1 FROM pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
			    WHERE privilege.grantee=0))
			  FROM pg_catalog.pg_default_acl AS default_acl
			  JOIN pg_catalog.pg_namespace AS namespace ON namespace.nspowner=default_acl.defaclrole
			  WHERE default_acl.defaclnamespace IN (0,namespace.oid)
			   AND default_acl.defaclobjtype IN ('f','T'))",
			&[],
		)
		.await?
		.get(0);

	assert!(public_and_defaults_closed);

	Ok(())
}

async fn assert_identity_ingress_catalog(
	runtime: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	const SQL: &str = "SELECT
	 pg_catalog.to_regprocedure('decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text)') IS NOT NULL
	 AND pg_catalog.to_regprocedure('decodex.bootstrap_advisor(pg_catalog.uuid)') IS NULL
	 AND pg_catalog.to_regprocedure('decodex.bootstrap_advisor(pg_catalog.text)') IS NULL
	 AND pg_catalog.to_regprocedure('decodex.create_project(decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text)') IS NOT NULL
	 AND pg_catalog.to_regprocedure('decodex.create_project(pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,pg_catalog.uuid)') IS NULL
	 AND pg_catalog.to_regprocedure('decodex.create_project(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,pg_catalog.text)') IS NULL
	 AND pg_catalog.to_regprocedure('decodex.transition_project(decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.project_status)') IS NOT NULL
	 AND pg_catalog.to_regprocedure('decodex.transition_project(pg_catalog.uuid,pg_catalog.int8,decodex.project_status)') IS NULL
	 AND pg_catalog.to_regprocedure('decodex.transition_project(pg_catalog.text,pg_catalog.int8,decodex.project_status)') IS NULL
	 AND NOT EXISTS (
	  SELECT 1 FROM pg_catalog.pg_cast AS conversion
	  WHERE conversion.castsource='pg_catalog.uuid'::pg_catalog.regtype
	   AND conversion.casttarget='pg_catalog.text'::pg_catalog.regtype
	   AND conversion.castcontext='i')";

	let signatures_are_domain_only: bool = runtime.query_one(SQL, &[]).await?.get(0);

	assert!(signatures_are_domain_only, "catalog identity contract; SQL: {SQL}");

	Ok(())
}

async fn assert_identity_ingress_exact_domain(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor(
	 '21000000-0000-4000-8000-000000000040'::decodex.canonical_uuid_v4_text)";

	let result = runtime.query(SQL, &[]).await;

	assert!(result.is_ok(), "exact-domain expression; SQL: {SQL}; result: {result:?}");
}

async fn assert_identity_ingress_unknown_literal(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor(
	 '21000000-0000-4000-8000-000000000040')";

	let result = runtime.query(SQL, &[]).await;

	assert!(result.is_ok(), "unknown-literal expression; SQL: {SQL}; result: {result:?}");
}

async fn assert_identity_ingress_explicit_text(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor(
	 '21000000-0000-4000-8000-000000000040'::pg_catalog.text)";

	let result = runtime.query(SQL, &[]).await;

	assert!(result.is_ok(), "explicit-text expression; SQL: {SQL}; result: {result:?}");
}

async fn assert_identity_ingress_prepared_bound_text(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor($1::pg_catalog.text)";

	let id = "21000000-0000-4000-8000-000000000040";
	let result = runtime.query(SQL, &[&id]).await;

	assert!(
		result.is_ok(),
		"prepared/bound-text expression; SQL: {SQL}; bound value: canonical UUID-v4; result: {result:?}"
	);
}

async fn assert_identity_ingress_invalid_text(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor($1::pg_catalog.text)";

	for invalid in [
		"21000000-0000-4000-8000-0000000000AA",
		"00000000-0000-0000-0000-000000000000",
		"21000000-0000-1000-8000-000000000099",
		"21000000000040008000000000000099",
	] {
		let error = runtime
			.query(SQL, &[&invalid])
			.await
			.expect_err(&format!("invalid-text expression; SQL: {SQL}; input: {invalid}"));
		let database = error.as_db_error().expect("invalid-text expression has PostgreSQL detail");

		assert_eq!(
			database.code(),
			&tokio_postgres::error::SqlState::CHECK_VIOLATION,
			"invalid-text expression; SQL: {SQL}; input: {invalid}"
		);
		assert_eq!(
			database.constraint(),
			Some("canonical_uuid_v4_text_exact"),
			"invalid-text expression; SQL: {SQL}; input: {invalid}"
		);
	}
}

async fn assert_identity_ingress_null(runtime: &Client) {
	for (case, statement) in [
		(
			"bootstrap Advisor empty scalar subquery",
			"SELECT * FROM decodex.bootstrap_advisor(
			 (SELECT value FROM (VALUES(NULL::decodex.canonical_uuid_v4_text)) AS empty(value)
			  WHERE false))",
		),
		(
			"create Project empty project-ID scalar subquery",
			"SELECT * FROM decodex.create_project(
			 (SELECT value FROM (VALUES(NULL::decodex.canonical_uuid_v4_text)) AS empty(value)
			  WHERE false),
			 'hack-ink/null-project','/srv/repos/null-project','/srv/repos/null-project','{}',
			 '22000000-0000-4000-8000-000000000040'::pg_catalog.text::decodex.canonical_uuid_v4_text)",
		),
		(
			"create Project empty Lead-ID scalar subquery",
			"SELECT * FROM decodex.create_project(
			 '11000000-0000-4000-8000-000000000040'::pg_catalog.text::decodex.canonical_uuid_v4_text,
			 'hack-ink/null-lead','/srv/repos/null-lead','/srv/repos/null-lead','{}',
			 (SELECT value FROM (VALUES(NULL::decodex.canonical_uuid_v4_text)) AS empty(value)
			  WHERE false))",
		),
		(
			"transition Project empty project-ID scalar subquery",
			"SELECT * FROM decodex.transition_project(
			 (SELECT value FROM (VALUES(NULL::decodex.canonical_uuid_v4_text)) AS empty(value)
			  WHERE false),1,'paused')",
		),
	] {
		let error = runtime
			.query(statement, &[])
			.await
			.expect_err(&format!("null expression case: {case}; SQL: {statement}"));
		let database = error.as_db_error().expect("null expression has PostgreSQL detail");

		assert_eq!(
			database.code(),
			&tokio_postgres::error::SqlState::CHECK_VIOLATION,
			"null expression case: {case}; SQL: {statement}"
		);
		assert_eq!(
			database.constraint(),
			Some("canonical_uuid_v4_text_ingress"),
			"null expression case: {case}; SQL: {statement}"
		);
		assert_eq!(
			database.message(),
			"identity ingress requires canonical UUID-v4 text",
			"null expression case: {case}; SQL: {statement}"
		);
	}
}

async fn assert_identity_ingress_bare_uuid(runtime: &Client) {
	const SQL: &str = "SELECT * FROM decodex.bootstrap_advisor(
	 '21000000-0000-4000-8000-000000000040'::pg_catalog.uuid)";

	let error = runtime
		.query(SQL, &[])
		.await
		.expect_err(&format!("bare-UUID expression must not resolve; SQL: {SQL}"));

	assert_eq!(
		error.as_db_error().map(tokio_postgres::error::DbError::code),
		Some(&tokio_postgres::error::SqlState::UNDEFINED_FUNCTION),
		"bare-UUID expression; SQL: {SQL}"
	);
}

async fn assert_identity_ingress_explicit_normalization(runtime: &Client) {
	const SQL: &str = "SET search_path=pg_temp,public;
	 SELECT * FROM decodex.bootstrap_advisor(
	 '21000000-0000-4000-8000-000000000040'::pg_catalog.uuid::pg_catalog.text::decodex.canonical_uuid_v4_text)";

	let result = runtime.batch_execute(SQL).await;

	assert!(
		result.is_ok(),
		"explicit uuid::text::domain normalization; SQL: {SQL}; result: {result:?}"
	);
}

async fn seed_account_read_fixture(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let account_id = AccountId::new(ACCOUNT_ID)?;
	client
		.batch_execute(
			"BEGIN; \
			 SELECT decodex.lock_account_routing_universe_exact(); \
			 INSERT INTO decodex.accounts(\
			 account_id,display_label,state,metadata,revision,enabled\
			 ) VALUES(\
			 '10000000-0000-0000-0000-000000000001',\
			 'Primary metadata','unavailable',\
			 '{\"observation\":\"manual_fixture_not_ready\"}'::jsonb,9,false); \
			 INSERT INTO decodex.account_routing_order(account_id,position) \
			 SELECT '10000000-0000-0000-0000-000000000001',\
			 pg_catalog.count(*)::integer FROM decodex.account_routing_order; \
			 UPDATE decodex.account_routing_control SET revision=revision+1,\
			 updated_at=pg_catalog.clock_timestamp() WHERE singleton; \
			 COMMIT",
		)
		.await?;
	let account = store.account(&account_id).await?.expect("fixture account exists");

	assert_eq!(account.account_id, account_id);
	assert_eq!(account.display_label, "Primary metadata");
	assert_eq!(account.state, AccountState::Unavailable);
	assert_eq!(account.metadata, serde_json::json!({"observation": "manual_fixture_not_ready"}));
	assert_eq!(account.revision, 9);
	assert!(!store.account_is_ready_at_revision(&account.account_id, account.revision).await?);

	Ok(())
}

async fn assert_direct_credential_and_scope_boundary(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let rejected_receipts: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.command_receipts \
			 WHERE idempotency_key LIKE '%credential%'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(rejected_receipts, 0);

	assert_credential_constraint(
		client,
		"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('forbidden', 'account', $1, 1, '{\"access_token\": \"forbidden\"}')",
		&[&ACCOUNT_ID],
		"outbox_no_credentials",
	)
	.await?;

	for (index, candidate) in CREDENTIAL_VALUE_VECTORS.iter().enumerate() {
		let aggregate_id = format!("credential-vector-{index}");
		let error = client
			.execute(
				"INSERT INTO decodex.activity \
				 (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) \
				 VALUES ('account', $1, 1, 'credential_vector_tested', $2, '{}')",
				&[&aggregate_id, candidate],
			)
			.await
			.expect_err("SQL credential boundary rejected the shared Rust vector");

		assert_eq!(
			error.as_db_error().and_then(|error| error.constraint()),
			Some("activity_no_credentials"),
			"candidate {candidate:?}",
		);
	}

	assert_direct_delivered_invariant(store, client).await?;
	assert_direct_credential_rows(client).await?;

	assert_no_credential_columns_or_routing(client).await
}

async fn assert_direct_credential_rows(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
	client
		.batch_execute(
			"INSERT INTO decodex.command_receipts \
			 (idempotency_key,request_hash,claim_token,claim_expires_at) VALUES \
			 ('direct-credential',repeat('a',64),gen_random_uuid(),clock_timestamp()+interval '5 minutes'), \
			 ('direct-value-credential',repeat('b',64),gen_random_uuid(),clock_timestamp()+interval '5 minutes')",
		)
		.await?;

	for (statement, constraint) in [
		(
			"INSERT INTO decodex.accounts (account_id, display_label) VALUES ('10000000-0000-0000-0000-000000000099', 'Bearer abcdefghijklmnop')",
			"accounts_no_credentials",
		),
		(
			"INSERT INTO decodex.quota_windows (account_id,window_class,duration_minutes,observed_at,confidence,metadata) VALUES ('10000000-0000-0000-0000-000000000001','seven_day',10080,TIMESTAMPTZ '1970-01-01 00:00:00+00','unknown','{\"api_key\":\"forbidden\"}')",
			"quota_windows_no_credentials",
		),
		(
			"INSERT INTO decodex.command_receipts (idempotency_key, request_hash, claim_token, claim_expires_at) VALUES ('Basic dXNlcjpwYXNz', repeat('c', 64), gen_random_uuid(), clock_timestamp()+interval '5 minutes')",
			"command_receipts_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'credential-test', 1, 'tested', 'credential-test', '{\"nested\":[{\"sessionToken\":\"forbidden\"}]}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'credential-value-test', 1, 'tested', 'credential-value-test', '{\"header\":\"Basic dXNlcjpwYXNz\"}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'Bearer abcdefghijklmnop', 1, 'tested', 'ordinary', '{}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'ordinary', 1, 'tested', 'sk-proj-0123456789', '{}')",
			"activity_no_credentials",
		),
		(
			"UPDATE decodex.command_receipts SET response='{\"Authorization\":\"forbidden\"}',response_bytes=convert_to('{}','UTF8'),receipt_state='completed',completed_at=clock_timestamp(),completion_claim_token=claim_token,claim_token=NULL,claim_expires_at=NULL WHERE idempotency_key='direct-credential'",
			"command_receipts_no_credentials",
		),
		(
			"UPDATE decodex.command_receipts SET response='{\"value\":\"xoxb-1234567890-abcdef\"}',response_bytes=convert_to('{}','UTF8'),receipt_state='completed',completed_at=clock_timestamp(),completion_claim_token=claim_token,claim_token=NULL,claim_expires_at=NULL WHERE idempotency_key='direct-value-credential'",
			"command_receipts_no_credentials",
		),
		(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) VALUES ('Basic dXNlcjpwYXNz', '20000000-0000-0000-0000-000000000099', statement_timestamp() + interval '1 minute', statement_timestamp())",
			"leases_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, effect_state, receipt, reconciliation) VALUES ('forbidden-evidence', 'account', 'credential-test', 1, '{}', 'receipt_recorded', '{\"bearer\":\"forbidden\"}', '{\"api-key\":\"forbidden\"}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, effect_state, receipt, reconciliation) VALUES ('forbidden-value-evidence', 'account', 'credential-test', 1, '{}', 'receipt_recorded', '{\"value\":\"glpat-1234567890abcdef\"}', '{\"value\":\"npm_1234567890abcdef\"}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) VALUES ('Bearer abcdefghijklmnop', 'account', 'ordinary', 1, '{}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, last_failure_code) VALUES ('ordinary-effect', 'account', 'ordinary', 1, '{}', 'sk_proj_0123456789')",
			"outbox_no_credentials",
		),
	] {
		assert_credential_constraint(client, statement, &[], constraint).await?;
	}

	client
		.batch_execute(
			"INSERT INTO decodex.activity \
			 (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) \
			 VALUES ('token_budget', 'session_id', 1, 'session_id', 'token_budget', '{}')",
		)
		.await?;

	Ok(())
}

async fn assert_no_credential_columns_or_routing(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let forbidden_columns: i64 = client
		.query_one(
			"SELECT count(*) FROM information_schema.columns \
			 WHERE table_schema = 'decodex' AND table_name IN ('accounts', 'quota_windows') \
			 AND lower(column_name) IN \
			 ('credential', 'credentials', 'password', 'private_key', 'secret', \
			  'access_token', 'refresh_token', 'api_key')",
			&[],
		)
		.await?
		.get(0);
	let expected_inert_routing_functions: i64 = client
		.query_one(
			"SELECT count(*) FROM pg_proc JOIN pg_namespace ON pg_namespace.oid = pronamespace \
			 WHERE nspname = 'decodex' AND proname = 'route_account_exact'",
			&[],
		)
		.await?
		.get(0);
	let unexpected_routing_functions: i64 = client
		.query_one(
			"SELECT count(*) FROM pg_proc JOIN pg_namespace ON pg_namespace.oid = pronamespace \
			 WHERE nspname = 'decodex' AND (proname LIKE '%eligible%' \
			 OR (proname LIKE '%route%' AND proname <> 'route_account_exact') \
			 OR proname LIKE '%select_account%')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(forbidden_columns, 0);
	assert_eq!(expected_inert_routing_functions, 1);
	assert_eq!(unexpected_routing_functions, 0);

	Ok(())
}

async fn assert_direct_delivered_invariant(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_invalid_delivered_evidence(client).await?;
	assert_unicode_whitespace_evidence(client).await?;
	assert_delivered_retention(client).await?;
	assert_delivered_is_terminal(store, client).await?;

	let delivered_rows: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.outbox WHERE effect_key LIKE 'direct-delivered-%'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(delivered_rows, 0);

	Ok(())
}

async fn assert_invalid_delivered_evidence(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let invalid_evidence = [
		("'receipt_recorded'", "NULL", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'null'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'\"   \"'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{}'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'[]'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{\"nested\":{}}'", "'{\"observed\":true}'"),
		("'ambiguous'", "NULL", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "NULL"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'null'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'{}'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'[]'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'{\"nested\":[]}'"),
		(
			"'receipt_recorded'",
			"'{\"provider_receipt\":\"receipt\"}'",
			"'{\"Authorization\":\"forbidden\"}'",
		),
		(
			"'receipt_recorded'",
			"'{\"value\":\"Bearer abcdefghijklmnop\"}'",
			"'{\"observed\":true}'",
		),
	];

	for (index, (effect_state, receipt, reconciliation)) in invalid_evidence.into_iter().enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-delivered-{index}', 'account', 'direct-delivered', 1, '{{}}', \
			  'delivered', {effect_state}, {receipt}, {reconciliation}, \
			  statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + interval '1 day')"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("delivered outbox evidence invariant rejected direct SQL bypass");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION),
		);
	}

	Ok(())
}

async fn assert_unicode_whitespace_evidence(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	for (index, whitespace) in UNICODE_WHITESPACE_VECTORS.iter().enumerate() {
		let whitespace_evidence = Value::String((*whitespace).into());
		let meaningful_receipt = serde_json::json!({"provider_receipt": "receipt"});
		let meaningful_reconciliation = serde_json::json!({"observed": true});

		for (suffix, receipt, reconciliation) in [
			("receipt", &whitespace_evidence, &meaningful_reconciliation),
			("reconciliation", &meaningful_receipt, &whitespace_evidence),
		] {
			let effect_key = format!("direct-unicode-{index}-{suffix}");
			let error = client
				.execute(
					"INSERT INTO decodex.outbox \
					 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
					  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
					 VALUES ($1, 'account', 'unicode-whitespace', 1, '{}', 'delivered', \
					  'receipt_recorded', $2, $3, statement_timestamp(), statement_timestamp(), \
					  statement_timestamp() + interval '1 day')",
					&[&effect_key, receipt, reconciliation],
				)
				.await
				.expect_err("Unicode-whitespace-only evidence rejected");

			assert_eq!(
				error.as_db_error().map(|error| error.code()),
				Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
			);
		}
	}

	Ok(())
}

async fn assert_delivered_retention(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
	for (index, retain_until) in [
		"statement_timestamp()",
		"statement_timestamp() + interval '0.0005 seconds'",
		"statement_timestamp() + 31622400000 * interval '1 millisecond'",
		"'infinity'::timestamptz",
	]
	.into_iter()
	.enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-{index}', 'account', 'retention', 1, '{{}}', 'delivered', \
			  'receipt_recorded', '{{\"provider_receipt\":\"receipt\"}}', '{{\"observed\":true}}', \
			  statement_timestamp(), statement_timestamp(), {retain_until})"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("invalid delivered retention rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let chronology_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-chronology', 'account', 'retention', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp(), statement_timestamp() - interval '1 millisecond', \
			  statement_timestamp() + interval '1 day')",
			&[],
		)
		.await
		.expect_err("delivered timestamp cannot predate row creation");

	assert_eq!(
		chronology_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_terminal_chronology")
	);

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-shifted-anchor', 'account', 'retention', 1, '{}', \
			  'delivered', 'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', \
			  '{\"observed\":true}', statement_timestamp(), \
			  statement_timestamp() + interval '1000 days', \
			  statement_timestamp() + interval '1000 days 1 millisecond')",
			&[],
		)
		.await
		.expect_err("future-shifted retention anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	let shifted_retry_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, available_at) \
			 VALUES ('direct-outbox-retry-shifted', 'account', 'retry', 1, '{}', \
			  statement_timestamp() + interval '1000 days')",
			&[],
		)
		.await
		.expect_err("future-shifted direct retry schedule rejected");

	assert_eq!(
		shifted_retry_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	client
		.batch_execute(
			"BEGIN; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-valid', 'account', 'retention', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + 31536000000 * interval '1 millisecond'); \
			 ROLLBACK",
		)
		.await?;

	Ok(())
}

async fn assert_delivered_is_terminal(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	client
		.batch_execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, available_at, created_at, delivered_at, \
			  retain_until) \
			 VALUES ('terminal-retention-guard', 'account', 'terminal', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  '1970-01-01T00:00:00Z', statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + interval '1 day')",
		)
		.await?;

	let error = client
		.execute(
			"UPDATE decodex.outbox SET state = 'pending', effect_state = 'not_started', \
			 receipt = NULL, reconciliation = NULL, delivered_at = NULL, retain_until = NULL \
			 WHERE effect_key = 'terminal-retention-guard'",
			&[],
		)
		.await
		.expect_err("delivered outbox row cannot regress to replayable state");

	assert_eq!(
		error.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
	);

	let early_delete_error = client
		.execute("DELETE FROM decodex.outbox WHERE effect_key = 'terminal-retention-guard'", &[])
		.await
		.expect_err("delivered outbox row cannot be deleted before retention is due");

	assert_eq!(
		early_delete_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_retention_pruning_only")
	);

	let truncate_error = client
		.batch_execute("TRUNCATE decodex.outbox")
		.await
		.expect_err("outbox truncate cannot bypass retained delivery evidence");

	assert_eq!(
		truncate_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_truncate_forbidden")
	);

	let reinsert_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('terminal-retention-guard', 'account', 'terminal', 2, '{}')",
			&[],
		)
		.await
		.expect_err("early delete cannot release the effect key for replay");

	assert_eq!(
		reinsert_error.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
	);

	let claims = store.claim_outbox(WORKER_B, 1_000, Duration::from_millis(1)).await?;

	assert!(claims.iter().all(|claim| claim.effect_key != "terminal-retention-guard"));

	let state: String = client
		.query_one(
			"SELECT state::text FROM decodex.outbox \
			 WHERE effect_key = 'terminal-retention-guard'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(state, "delivered");

	client
		.batch_execute(
			"UPDATE decodex.outbox SET state = 'pending', lease_holder = NULL, claim_token = NULL, \
			 lease_acquired_at = NULL, lease_expires_at = NULL \
			 WHERE state = 'in_flight' AND lease_holder = \
			 '30000000-0000-0000-0000-000000000002'; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-delivered-prunable', 'account', 'terminal', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp() - interval '2 days', \
			  statement_timestamp() - interval '2 days', \
			  statement_timestamp() - interval '1 day'); \
			 DELETE FROM decodex.outbox WHERE effect_key = 'direct-delivered-prunable'",
		)
		.await?;

	Ok(())
}

async fn assert_credential_constraint(
	client: &Client,
	statement: &str,
	parameters: &[&(dyn ToSql + Sync)],
	constraint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let error = client.execute(statement, parameters).await.expect_err("credential row rejected");

	assert_eq!(
		error.as_db_error().and_then(|error| error.constraint()),
		Some(constraint),
		"unexpected PostgreSQL rejection: {error:?}"
	);

	Ok(())
}

async fn assert_lease_contention_and_reclaim(
	store: &PostgresStore,
) -> Result<(), Box<dyn std::error::Error>> {
	let mut tasks = JoinSet::new();

	for contender in 0..32 {
		let store = store.clone();
		let holder = format!("40000000-0000-0000-0000-{contender:012}");

		tasks.spawn(async move {
			store.try_acquire_lease("managed-run/one", &holder, Duration::from_secs(1)).await
		});
	}

	let mut winners = 0;

	while let Some(result) = tasks.join_next().await {
		if result??.acquired {
			winners += 1;
		}
	}

	assert_eq!(winners, 1);

	time::sleep(Duration::from_millis(1_100)).await;

	let reclaimed =
		store.try_acquire_lease("managed-run/one", HOLDER_A, Duration::from_secs(1)).await?;

	assert!(reclaimed.acquired);
	assert!(reclaimed.revision.is_some_and(|revision| revision >= 2));
	assert!(matches!(
		store
			.renew_lease(
				"managed-run/one",
				HOLDER_B,
				reclaimed.token.as_deref().expect("lease token"),
				Duration::from_secs(1),
			)
			.await,
		Err(StoreError::OwnershipLost("lease"))
	));

	store
		.release_lease(
			"managed-run/one",
			HOLDER_A,
			reclaimed.token.as_deref().expect("lease token"),
		)
		.await?;

	let reacquired =
		store.try_acquire_lease("managed-run/one", HOLDER_B, Duration::from_secs(1)).await?;

	assert!(reacquired.acquired);
	assert!(reacquired.revision > reclaimed.revision);

	Ok(())
}

async fn assert_conversation_history_context_and_blob_contract(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_root = env::var("DECODEX_TEST_BLOB_ROOT")?;
	let blob_store = BlobStore::open(DecodexRoot::new(blob_root)?.paths())?;
	let conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000001")?;
	let create = CreateConversation {
		conversation_id: conversation_id.clone(),
		title: "Synthetic conversation fixture".into(),
	};
	let create_command = CommandIdentity::new("conversation-create", b"conversation-create-v1")?;
	let first = store.create_conversation(&create_command, &create).await?;
	let duplicate = store.create_conversation(&create_command, &create).await?;

	assert_eq!(first, duplicate);

	let profiles = BootstrapRoleProfiles {
		advisor: exact_profile("advisor"),
		lead: exact_profile("lead"),
		task: exact_profile("task"),
		reviewer: exact_profile("reviewer"),
	};
	assert!(matches!(
		store.bootstrap_role_profiles("history-role-bootstrap", &profiles).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let session_a_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000001")?;
	let session_b_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000002")?;

	for (index, session_id) in [session_a_id.clone(), session_b_id.clone()].into_iter().enumerate()
	{
		let create_session = CreateRuntimeSession {
			runtime_session_id: session_id,
			conversation_id: conversation_id.clone(),
			role: RoleProfileRole::Task,
			account_snapshot: exact_account_snapshot(format!(
				"43000000-0000-4000-8000-{:012x}",
				index + 1
			)),
			codex_thread_id: None,
			initial_state: RuntimeSessionState::Active,
		};

		assert!(matches!(
			store
				.create_runtime_session(&format!("manual-session-{index}"), &create_session)
				.await?,
			RuntimeSessionCommandOutcome::Success(_)
		));
	}

	let null_thread_sessions: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.runtime_sessions \
			 WHERE conversation_id = $1::text::uuid AND codex_thread_id IS NULL",
			&[&conversation_id.as_str()],
		)
		.await?
		.get(0);

	assert_eq!(null_thread_sessions, 2, "manual fixtures may span multiple RuntimeSessions");

	assert_initial_lifecycle_timestamps_are_canonical(store, runtime_client, &conversation_id)
		.await?;
	assert_runtime_session_correlation_is_immutable(store, runtime_client, &conversation_id)
		.await?;

	let fixture = ConversationFixture { blob_store, conversation_id, session_a_id, session_b_id };

	assert_history_blob_crash_contract(store, client, &fixture).await?;
	assert_stream_revision_and_pagination(store, client, runtime_client, &fixture).await?;
	assert_context_pack_and_transition(store, client, runtime_client, &fixture).await?;
	assert_blob_writer_reclaimer_race(store, client, runtime_client, &fixture).await?;

	let root = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 4)
		.await?
		.next_cursor
		.expect("four-item page has an Artifact continuation");
	let artifact_page = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, Some(&root), 4)
		.await?;

	assert_eq!(artifact_page.entries.len(), 2);
	assert!(artifact_page.entries[0].artifact.is_some());

	assert_exact_receipt_responses_survive_later_mutation(store, runtime_client, &fixture).await?;

	Ok(())
}

async fn assert_exact_receipt_responses_survive_later_mutation(
	store: &PostgresStore,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let sync = PathBuf::from(env::var("DECODEX_TEST_BLOB_ROOT")?).join("post-commit-sync");

	fs::create_dir(&sync)?;

	// SAFETY: the isolated contract is one test process and removes this test-only variable below.
	unsafe { env::set_var("DECODEX_TEST_POST_COMMIT_SYNC", &sync) };

	let session_id = assert_session_receipt_response(store, runtime_client, fixture).await?;

	assert_history_receipt_response(store, runtime_client, fixture, &session_id, &sync).await?;

	// SAFETY: paired with the isolated test-only set above after all barrier users finish.
	unsafe { env::remove_var("DECODEX_TEST_POST_COMMIT_SYNC") };

	fs::remove_dir_all(sync)?;

	Ok(())
}

async fn assert_session_receipt_response(
	store: &PostgresStore,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<RuntimeSessionId, Box<dyn std::error::Error>> {
	let session_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000088")?;
	let create_session = CreateRuntimeSession {
		runtime_session_id: session_id.clone(),
		conversation_id: fixture.conversation_id.clone(),
		role: RoleProfileRole::Task,
		account_snapshot: exact_account_snapshot("43000000-0000-4000-8000-000000000088".into()),
		codex_thread_id: Some("44000000-0000-4000-8000-000000000088".into()),
		initial_state: RuntimeSessionState::Starting,
	};
	let RuntimeSessionCommandOutcome::Success(first_session) =
		store.create_runtime_session("receipt-response-session", &create_session).await?
	else {
		panic!("exact session creation must succeed");
	};
	assert!(matches!(
		store
			.transition_runtime_session(
				"receipt-response-session-active",
				&session_id,
				1,
				RuntimeSessionState::Active,
			)
			.await?,
		RuntimeSessionCommandOutcome::Success(_)
	));
	let RuntimeSessionCommandOutcome::Success(replay_session) =
		store.create_runtime_session("receipt-response-session", &create_session).await?
	else {
		panic!("exact session replay must succeed");
	};

	assert_eq!(first_session, replay_session);
	assert_eq!(first_session.runtime_session.state, RuntimeSessionState::Starting);

	let current_session: String = runtime_client
		.query_one(
			"SELECT state::text FROM decodex.runtime_sessions WHERE runtime_session_id=$1::text::uuid",
			&[&session_id.as_str()],
		)
		.await?
		.get(0);

	assert_eq!(current_session, "active");

	Ok(session_id)
}

async fn assert_history_receipt_response(
	store: &PostgresStore,
	runtime_client: &Client,
	fixture: &ConversationFixture,
	session_id: &RuntimeSessionId,
	sync: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
	let item_id = HistoryItemId::new("44000000-0000-4000-8000-000000000088")?;
	let mutation = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: session_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000088")?,
		turn_sequence: 88,
		turn_role: TurnRole::Assistant,
		possible_side_effects: PossibleSideEffects::None,
		history_item_id: item_id.clone(),
		ordinal: 0,
		kind: HistoryItemKind::Message,
		status: ItemStatus::Streaming,
		text: "{\"phase\":\"first\"}".into(),
		media_type: history_media_type("application/json"),
		metadata: history_metadata(serde_json::json!({
			"source":"normalized",
			"visible":true,
			"note":"secret sauce",
			"summary":"token budget",
			"context":"session summary"
		})),
		expected_revision: None,
		artifact: None,
	};
	let first_store = store.clone();
	let first_blob_store = fixture.blob_store.clone();
	let first_mutation = mutation.clone();
	let history_committed = sync.join("history_item.committed");
	let mut history_worker = tokio::spawn(async move {
		first_store
			.record_history_item(
				&first_blob_store,
				&CommandIdentity::new("receipt-response-history", b"receipt-response-history-v1")?,
				&first_mutation,
			)
			.await
	});

	tokio::select! {
		result = &mut history_worker => return Err(format!("history response worker exited before barrier: {result:?}").into()),
		result = wait_for_path(&history_committed) => result?,
	}

	runtime_client
		.execute(
			"UPDATE decodex.history_items SET status='completed',inline_text='{\"phase\":\"later\"}', \
		 metadata='{\"source\":\"later\",\"visible\":true}'::jsonb,revision=revision+1, \
		 updated_at=clock_timestamp() WHERE history_item_id=$1::text::uuid",
			&[&item_id.as_str()],
		)
		.await?;

	fs::write(sync.join("history_item.continue"), b"continue")?;

	let first_history = history_worker.await??;
	let replay_history = store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("receipt-response-history", b"receipt-response-history-v1")?,
			&mutation,
		)
		.await?;

	assert_eq!(first_history, replay_history);
	assert_eq!(first_history.status, ItemStatus::Streaming);
	assert_eq!(first_history.inline_text.as_deref(), Some("{\"phase\":\"first\"}"));
	assert_eq!(first_history.media_type.as_str(), "application/json");
	assert_eq!(
		first_history.metadata.as_map().get("source"),
		Some(&HistoryMetadataValue::Text("normalized".into())),
	);
	assert_eq!(
		first_history.metadata.as_map().get("visible"),
		Some(&HistoryMetadataValue::Boolean(true)),
	);
	assert_eq!(
		first_history.metadata.as_map().get("note"),
		Some(&HistoryMetadataValue::Text("secret sauce".into())),
	);

	let current_history: String = runtime_client
		.query_one(
			"SELECT status::text FROM decodex.history_items WHERE history_item_id=$1::text::uuid",
			&[&item_id.as_str()],
		)
		.await?
		.get(0);

	assert_eq!(current_history, "completed");

	Ok(())
}

async fn assert_initial_lifecycle_timestamps_are_canonical(
	store: &PostgresStore,
	runtime_client: &Client,
	conversation_id: &ConversationId,
) -> Result<(), Box<dyn std::error::Error>> {
	for (index, (session_id, state)) in [
		("41000000-0000-4000-8000-000000000004", RuntimeSessionState::Active),
		("41000000-0000-4000-8000-000000000005", RuntimeSessionState::Starting),
	]
	.into_iter()
	.enumerate()
	{
		let create = CreateRuntimeSession {
			runtime_session_id: RuntimeSessionId::new(session_id)?,
			conversation_id: conversation_id.clone(),
			role: RoleProfileRole::Task,
			account_snapshot: exact_account_snapshot(format!(
				"43000000-0000-4000-8000-00000000009{}",
				index + 1
			)),
			codex_thread_id: None,
			initial_state: state,
		};
		assert!(matches!(
			store.create_runtime_session(&format!("timestamp-session-{index}"), &create).await?,
			RuntimeSessionCommandOutcome::Success(_)
		));

		let canonical: bool = runtime_client
			.query_one(
				"SELECT session.created_at=session.updated_at AND session.ended_at IS NULL \
				 AND session.created_at>'2020-01-01Z' AND profile.created_at>'2020-01-01Z' \
				 AND account.created_at>'2020-01-01Z' FROM decodex.runtime_sessions AS session \
				 JOIN decodex.profile_snapshots AS profile USING (profile_snapshot_id) \
				 JOIN decodex.account_snapshots AS account USING (account_snapshot_id) \
				 WHERE runtime_session_id=$1::text::uuid",
				&[&session_id],
			)
			.await?
			.get(0);

		assert!(canonical, "RuntimeSession timestamps are canonical");
	}
	for (index, (session_id, state)) in [
		("41000000-0000-4000-8000-000000000006", RuntimeSessionState::Ended),
		("41000000-0000-4000-8000-000000000007", RuntimeSessionState::Diverged),
	]
	.into_iter()
	.enumerate()
	{
		let create = CreateRuntimeSession {
			runtime_session_id: RuntimeSessionId::new(session_id)?,
			conversation_id: conversation_id.clone(),
			role: RoleProfileRole::Task,
			account_snapshot: exact_account_snapshot(format!(
				"43000000-0000-4000-8000-00000000019{}",
				index + 1
			)),
			codex_thread_id: None,
			initial_state: state,
		};
		assert!(matches!(
			store.create_runtime_session(&format!("terminal-session-{index}"), &create).await?,
			RuntimeSessionCommandOutcome::Rejected(_)
		));
	}

	runtime_client
		.execute(
			"INSERT INTO decodex.turns \
			 (turn_id,conversation_id,runtime_session_id,sequence,role,status, \
			  created_at,updated_at,completed_at) \
			 VALUES ('45000000-0000-4000-8000-000000000099',$1::text::uuid, \
			 '41000000-0000-4000-8000-000000000004',99,'system','active', \
			 '2000-01-01Z','2099-01-01Z','2001-01-01Z')",
			&[&conversation_id.as_str()],
		)
		.await?;

	let active_turn_canonical: bool = runtime_client
		.query_one(
			"SELECT created_at=updated_at AND completed_at IS NULL \
			 AND created_at>'2020-01-01Z' FROM decodex.turns \
			 WHERE turn_id='45000000-0000-4000-8000-000000000099'",
			&[],
		)
		.await?
		.get(0);

	assert!(active_turn_canonical);

	for status in ["completed", "failed"] {
		let statement = format!(
			"INSERT INTO decodex.turns \
			 (turn_id,conversation_id,runtime_session_id,sequence,role,status,completed_at) \
			 VALUES (gen_random_uuid(),$1::text::uuid, \
			 '41000000-0000-4000-8000-000000000004',100,'system','{status}',clock_timestamp())"
		);

		assert!(runtime_client.execute(&statement, &[&conversation_id.as_str()]).await.is_err());
	}

	Ok(())
}

async fn assert_runtime_session_correlation_is_immutable(
	store: &PostgresStore,
	runtime_client: &Client,
	conversation_id: &ConversationId,
) -> Result<(), Box<dyn std::error::Error>> {
	let session_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000003")?;
	let original_thread = "44000000-0000-4000-8000-000000000003";
	let create = CreateRuntimeSession {
		runtime_session_id: session_id.clone(),
		conversation_id: conversation_id.clone(),
		role: RoleProfileRole::Task,
		account_snapshot: exact_account_snapshot("43000000-0000-4000-8000-000000000003".into()),
		codex_thread_id: Some(original_thread.into()),
		initial_state: RuntimeSessionState::Starting,
	};

	assert!(matches!(
		store.create_runtime_session("manual-session-c", &create).await?,
		RuntimeSessionCommandOutcome::Success(_)
	));

	for forged_assignment in [
		"codex_thread_id='44000000-0000-4000-8000-000000000013'",
		"last_known_turn_id='forged-turn'",
		"created_at=created_at - interval '1 second'",
	] {
		let statement = format!(
			"UPDATE decodex.runtime_sessions SET state='active', revision=revision+1, \
			 {forged_assignment} WHERE runtime_session_id=$1::text::uuid"
		);

		assert!(runtime_client.execute(&statement, &[&session_id.as_str()]).await.is_err());
	}

	let unchanged_starting: bool = runtime_client
		.query_one(
			"SELECT state='starting' AND revision=1 AND codex_thread_id=$2::text::uuid \
			 AND last_known_turn_id IS NULL FROM decodex.runtime_sessions \
			 WHERE runtime_session_id=$1::text::uuid",
			&[&session_id.as_str(), &original_thread],
		)
		.await?
		.get(0);

	assert!(unchanged_starting);

	let RuntimeSessionCommandOutcome::Success(active) = store
		.transition_runtime_session(
			"manual-session-c-active",
			&session_id,
			1,
			RuntimeSessionState::Active,
		)
		.await?
	else {
		panic!("exact active transition must succeed");
	};

	assert_eq!(active.runtime_session.codex_thread_id.as_deref(), Some(original_thread));
	assert_eq!(active.runtime_session.revision, 2);

	for forged_assignment in [
		"codex_thread_id='44000000-0000-4000-8000-000000000023'",
		"last_known_turn_id='forged-terminal-turn'",
		"created_at=created_at - interval '1 second'",
	] {
		let statement = format!(
			"UPDATE decodex.runtime_sessions SET state='ended', revision=revision+1, \
			 {forged_assignment} WHERE runtime_session_id=$1::text::uuid"
		);

		assert!(runtime_client.execute(&statement, &[&session_id.as_str()]).await.is_err());
	}

	let RuntimeSessionCommandOutcome::Success(ended) = store
		.transition_runtime_session(
			"manual-session-c-ended",
			&session_id,
			2,
			RuntimeSessionState::Ended,
		)
		.await?
	else {
		panic!("exact terminal transition must succeed");
	};

	assert_eq!(ended.runtime_session.codex_thread_id.as_deref(), Some(original_thread));
	assert_eq!(ended.runtime_session.revision, 3);

	let canonical_terminal: bool = runtime_client
		.query_one(
			"SELECT state='ended' AND codex_thread_id=$2::text::uuid AND last_known_turn_id IS NULL \
			 AND ended_at=updated_at AND updated_at>=created_at \
			 FROM decodex.runtime_sessions WHERE runtime_session_id=$1::text::uuid",
			&[&session_id.as_str(), &original_thread],
		)
		.await?
		.get(0);

	assert!(canonical_terminal);

	Ok(())
}

async fn assert_blob_writer_reclaimer_race(
	store: &PostgresStore,
	client: &Client,
	observer: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let bytes = b"writer-reclaimer-race".repeat(1_000);
	let hash = fixture.blob_store.put(&bytes)?;

	client
		.execute(
			"INSERT INTO decodex.blob_objects (blob_hash,byte_length,verified_at) \
			 VALUES ($1,$2,clock_timestamp() + interval '1 second')",
			&[&hash.to_hex(), &i64::try_from(bytes.len())?],
		)
		.await?;
	client
		.execute(
			"INSERT INTO decodex.turns \
			 (turn_id,conversation_id,runtime_session_id,sequence,role) \
			 VALUES ('45000000-0000-4000-8000-000000000006',$1::text::uuid,$2::text::uuid,6,'assistant')",
			&[&fixture.conversation_id.as_str(), &fixture.session_b_id.as_str()],
		)
		.await?;

	time::sleep(Duration::from_millis(2)).await;

	let mut inventoried = false;
	let mut cursor = None;
	let mut reclaim_after = None;

	loop {
		let page_after = cursor;
		let page = fixture.blob_store.old_inventory(Duration::from_millis(1), 256, cursor)?;

		if page.entries.iter().any(|entry| entry.hash == hash) {
			inventoried = true;
			reclaim_after = page_after;
		}

		cursor = page.next_cursor;

		if inventoried || cursor.is_none() {
			break;
		}
	}

	assert!(inventoried, "racing blob is present in the bounded old inventory");

	client.batch_execute("BEGIN").await?;
	client
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,blob_hash,media_type) \
			 VALUES ('44000000-0000-4000-8000-000000000006',$1::text::uuid,6, \
			 '45000000-0000-4000-8000-000000000006',0,'message','completed',$2,'text/plain')",
			&[&fixture.conversation_id.as_str(), &hash.to_hex()],
		)
		.await?;

	let reclaim = store.reclaim_orphan_blobs(
		&fixture.blob_store,
		Duration::from_millis(1),
		256,
		reclaim_after,
	);

	tokio::pin!(reclaim);

	let holder_pid: i32 = client.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let blocked = wait_for_any_blocked_by(observer, holder_pid);

	tokio::pin!(blocked);

	tokio::select! {
		result = &mut reclaim => panic!("reclaimer bypassed the racing writer lock: {result:?}"),
		result = &mut blocked => assert!(result?, "reclaimer never reached the writer-held resource"),
	}

	client.batch_execute("COMMIT").await?;

	time::timeout(Duration::from_secs(2), &mut reclaim).await??;

	assert!(fixture.blob_store.path_for(hash).exists());

	let referenced: bool = client
		.query_one(
			"SELECT EXISTS (SELECT 1 FROM decodex.history_items WHERE blob_hash=$1) \
			 AND EXISTS (SELECT 1 FROM decodex.blob_objects WHERE blob_hash=$1)",
			&[&hash.to_hex()],
		)
		.await?
		.get(0);

	assert!(referenced, "a committed racing reference retains verified bytes and metadata");

	Ok(())
}

async fn assert_history_blob_crash_contract(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
) -> Result<BlobHash, Box<dyn std::error::Error>> {
	let large_text = "large-history-payload-".repeat(900);
	let large_item_id = HistoryItemId::new("44000000-0000-4000-8000-000000000001")?;
	let large_mutation = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_a_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000001")?,
		turn_sequence: 1,
		turn_role: TurnRole::User,
		possible_side_effects: PossibleSideEffects::None,
		history_item_id: large_item_id.clone(),
		ordinal: 0,
		kind: HistoryItemKind::Message,
		status: ItemStatus::Completed,
		text: large_text.clone(),
		media_type: history_media_type("text/plain"),
		metadata: history_metadata(serde_json::json!({"source": "synthetic"})),
		expected_revision: None,
		artifact: None,
	};
	let large_command = CommandIdentity::new("large-history-item", b"large-history-item-v1")?;
	let large_entry =
		store.record_history_item(&fixture.blob_store, &large_command, &large_mutation).await?;
	let large_hash = large_entry.blob_hash.expect("large history is offloaded");

	assert!(large_entry.inline_text.is_none());
	assert_eq!(large_entry.media_type.as_str(), "text/plain");
	assert_eq!(
		large_entry.metadata.as_map().get("source"),
		Some(&HistoryMetadataValue::Text("synthetic".into())),
	);
	assert!(client
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type) \
			 VALUES ('44000000-0000-4000-8000-000000000099',$1::text::uuid,99, \
			 '45000000-0000-4000-8000-000000000001',1,'message','completed','invalid','not a media type')",
			&[&fixture.conversation_id.as_str()],
		)
		.await
		.is_err());

	assert_direct_history_metadata_rejected(client, fixture).await?;

	let orphan_hash = fixture.blob_store.put(b"crash-before-database-transaction")?;
	let orphan_rows: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.blob_objects WHERE blob_hash = $1",
			&[&orphan_hash.to_hex()],
		)
		.await?
		.get(0);

	assert_eq!(orphan_rows, 0, "pre-transaction crash leaves a harmless orphan");

	time::sleep(Duration::from_millis(5)).await;

	let mut cursor = None;
	let mut removed = 0_u16;

	loop {
		let page = store
			.reclaim_orphan_blobs(&fixture.blob_store, Duration::from_millis(1), 16, cursor)
			.await?;

		removed += page.removed;
		cursor = page.next_cursor;

		if cursor.is_none() {
			break;
		}
	}

	assert_eq!(removed, 1);
	assert!(!fixture.blob_store.path_for(orphan_hash).exists());
	assert!(
		fixture.blob_store.path_for(large_hash).exists(),
		"referenced bytes survive collection"
	);

	assert_post_metadata_commit_crash_is_reclaimable(store, client, fixture).await?;

	let blob_path = fixture.blob_store.path_for(large_hash);

	fs::write(&blob_path, b"tampered")?;

	let tampered_history =
		store.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 1).await;

	assert!(
		matches!(&tampered_history, Err(StoreError::Blob(_))),
		"unexpected tampered-history result: {tampered_history:?}"
	);

	fs::remove_file(&blob_path)?;

	assert!(matches!(
		store.record_history_item(&fixture.blob_store, &large_command, &large_mutation).await,
		Err(StoreError::Blob(_))
	));

	fixture.blob_store.put(large_text.as_bytes())?;

	fs::remove_file(&blob_path)?;

	assert!(matches!(
		store.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 1).await,
		Err(StoreError::Blob(_))
	));

	fixture.blob_store.put(large_text.as_bytes())?;

	Ok(large_hash)
}

async fn assert_direct_history_metadata_rejected(
	client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let benign = serde_json::json!({
		"note": "secret sauce",
		"summary": "token budget",
		"context": "session summary",
		"visible": true,
	});
	let maximum = Value::Object(
		(0..32)
			.map(|index| {
				let key = if index == 0 { "é".repeat(32) } else { format!("field-{index}") };
				let value =
					if index == 0 { Value::String("é".repeat(128)) } else { Value::Bool(true) };

				(key, value)
			})
			.collect(),
	);

	for accepted in [&benign, &maximum] {
		let accepted: bool = client
			.query_one("SELECT decodex.is_history_metadata_projection($1::jsonb)", &[accepted])
			.await?
			.get(0);

		assert!(accepted);
	}
	for rejected in [
		serde_json::json!({"token": "ordinary"}),
		serde_json::json!({"auth_session": "ordinary"}),
		serde_json::json!({"note": "Bearer abcdefgh"}),
		serde_json::json!({"note": "secret=abcd"}),
		serde_json::json!({"nested": {"unsafe": true}}),
		serde_json::json!({"number": 1}),
		serde_json::json!({"é".repeat(33): true}),
		serde_json::json!({"note": "é".repeat(129)}),
	] {
		let rejected: bool = client
			.query_one("SELECT decodex.is_history_metadata_projection($1::jsonb)", &[&rejected])
			.await?
			.get(0);

		assert!(!rejected);
	}

	client.batch_execute("BEGIN").await?;
	client
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type,metadata) \
			 VALUES ('44000000-0000-4000-8000-000000000097',$1::text::uuid,97, \
			 '45000000-0000-4000-8000-000000000001',97,'message','completed','benign', \
			 'application/json',$2::jsonb)",
			&[&fixture.conversation_id.as_str(), &benign],
		)
		.await?;
	client.batch_execute("ROLLBACK").await?;

	let result = client.execute(
		"INSERT INTO decodex.history_items \
		 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type,metadata) \
		 VALUES ('44000000-0000-4000-8000-000000000098',$1::text::uuid,98, \
		 '45000000-0000-4000-8000-000000000001',2,'message','completed','invalid', \
		 'application/json','{\"nested\":{\"unsafe\":true}}'::jsonb)",
		&[&fixture.conversation_id.as_str()],
	).await;

	assert!(result.is_err());

	let result = client
		.execute(
			"INSERT INTO decodex.history_items \
			 (history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,inline_text,media_type,metadata) \
			 VALUES ('44000000-0000-4000-8000-000000000096',$1::text::uuid,96, \
			 '45000000-0000-4000-8000-000000000001',96,'message','completed','invalid', \
			 'application/json','{\"token\":\"ordinary\"}'::jsonb)",
			&[&fixture.conversation_id.as_str()],
		)
		.await;

	assert!(result.is_err());

	Ok(())
}

async fn assert_post_metadata_commit_crash_is_reclaimable(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let hash = fixture.blob_store.put(b"crash-after-metadata-commit")?;

	client
		.execute(
			"INSERT INTO decodex.blob_objects (blob_hash,byte_length,verified_at) \
			 VALUES ($1,27,clock_timestamp())",
			&[&hash.to_hex()],
		)
		.await?;
	client
		.execute("DELETE FROM decodex.blob_objects WHERE blob_hash=$1", &[&hash.to_hex()])
		.await?;

	assert!(fixture.blob_store.path_for(hash).exists());

	time::sleep(Duration::from_millis(5)).await;

	let mut cursor = None;

	loop {
		let page = store
			.reclaim_orphan_blobs(&fixture.blob_store, Duration::from_millis(1), 16, cursor)
			.await?;

		cursor = page.next_cursor;

		if cursor.is_none() {
			break;
		}
	}

	assert!(
		!fixture.blob_store.path_for(hash).exists(),
		"a crash after metadata commit leaves reclaimable orphan bytes"
	);

	Ok(())
}

async fn assert_stream_revision_and_pagination(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let streaming_id = HistoryItemId::new("44000000-0000-4000-8000-000000000002")?;
	let mut streaming = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_b_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000002")?,
		turn_sequence: 2,
		turn_role: TurnRole::Assistant,
		possible_side_effects: PossibleSideEffects::Unknown,
		history_item_id: streaming_id,
		ordinal: 0,
		kind: HistoryItemKind::Message,
		status: ItemStatus::Streaming,
		text: "streaming-one-".repeat(1_500),
		media_type: history_media_type("text/plain"),
		metadata: history_metadata(serde_json::json!({"correlation": "synthetic-stream"})),
		expected_revision: None,
		artifact: None,
	};
	let streamed = store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("stream-item", b"stream-item-v1")?,
			&streaming,
		)
		.await?;

	assert_eq!(streamed.revision, 1);
	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.history_items SET revision=revision+1, \
				 created_at=created_at - interval '1 second' \
				 WHERE history_item_id=$1::text::uuid",
				&[&streaming.history_item_id.as_str()],
			)
			.await
			.is_err()
	);

	let mut superseded = vec![streamed.blob_hash.expect("streaming payload is offloaded")];

	for revision in 1_i64..=3 {
		streaming.text = format!("streaming-{}-", revision + 1).repeat(1_500);
		streaming.expected_revision = Some(revision);

		let updated = store
			.record_history_item(
				&fixture.blob_store,
				&CommandIdentity::new(
					format!("stream-replace-{revision}"),
					format!("stream-replace-{revision}-v1").as_bytes(),
				)?,
				&streaming,
			)
			.await?;

		assert_eq!(updated.revision, revision + 1);

		superseded.push(updated.blob_hash.expect("streaming payload is offloaded"));
	}

	streaming.status = ItemStatus::Completed;
	streaming.text = "{\"complete\":true}".into();
	streaming.media_type = history_media_type("application/json");
	streaming.metadata = history_metadata(serde_json::json!({
		"correlation":"synthetic-stream",
		"visible":true,
		"note":"secret sauce",
		"summary":"token budget",
		"context":"session summary"
	}));
	streaming.expected_revision = Some(4);

	let completed = store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("complete-item", b"complete-item-v1")?,
			&streaming,
		)
		.await?;

	assert_eq!(completed.revision, 5);

	assert_superseded_stream_blobs_reclaimed(store, client, fixture, superseded).await?;

	let third = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_b_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000003")?,
		turn_sequence: 3,
		turn_role: TurnRole::System,
		possible_side_effects: PossibleSideEffects::Possible,
		history_item_id: HistoryItemId::new("44000000-0000-4000-8000-000000000003")?,
		ordinal: 0,
		kind: HistoryItemKind::Status,
		status: ItemStatus::Streaming,
		text: "manual boundary".into(),
		media_type: history_media_type("text/plain"),
		metadata: HistoryMetadata::empty(),
		expected_revision: None,
		artifact: None,
	};

	store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("third-item", b"third-item-v1")?,
			&third,
		)
		.await?;

	assert_snapshot_pagination(store, client, runtime_client, fixture).await
}

async fn assert_superseded_stream_blobs_reclaimed(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
	superseded: Vec<BlobHash>,
) -> Result<(), Box<dyn std::error::Error>> {
	time::sleep(Duration::from_millis(5)).await;

	let mut cursor = None;

	loop {
		let page = store
			.reclaim_orphan_blobs(&fixture.blob_store, Duration::from_millis(1), 2, cursor)
			.await?;

		cursor = page.next_cursor;

		if cursor.is_none() {
			break;
		}
	}

	for hash in superseded {
		assert!(fixture.blob_store.path_for(hash).exists());

		let metadata_exists: bool = client
			.query_one(
				"SELECT EXISTS (SELECT 1 FROM decodex.blob_objects WHERE blob_hash=$1)",
				&[&hash.to_hex()],
			)
			.await?
			.get(0);

		assert!(metadata_exists, "durable exact-replay receipts retain referenced bytes");
	}

	Ok(())
}

async fn assert_snapshot_pagination(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let first_page =
		store.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 2).await?;

	assert_eq!(first_page.entries.len(), 2);

	let issued = first_page.next_cursor.as_ref().expect("first page continuation");
	let issued_token = issued.encode();
	let issued_binding = client
		.query_one(
			"SELECT conversation_id::text,snapshot_high_water,last_position,history_item_id::text \
			 FROM decodex.history_cursors WHERE cursor_id=$1::text::uuid",
			&[&issued_token.trim_start_matches("v1:")],
		)
		.await?;

	assert_eq!(issued_binding.get::<_, String>(0), fixture.conversation_id.as_str());
	assert_eq!(issued_binding.get::<_, i64>(1), 3);
	assert_eq!(issued_binding.get::<_, i64>(2), 2);
	assert_eq!(issued_binding.get::<_, String>(3), first_page.entries[1].history_item_id);

	let completed_after_snapshot = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_b_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000003")?,
		turn_sequence: 3,
		turn_role: TurnRole::System,
		possible_side_effects: PossibleSideEffects::Possible,
		history_item_id: HistoryItemId::new("44000000-0000-4000-8000-000000000003")?,
		ordinal: 0,
		kind: HistoryItemKind::Status,
		status: ItemStatus::Completed,
		text: "manual boundary completed".into(),
		media_type: history_media_type("text/plain"),
		metadata: HistoryMetadata::empty(),
		expected_revision: Some(1),
		artifact: None,
	};

	store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("third-item-complete", b"third-item-complete-v1")?,
			&completed_after_snapshot,
		)
		.await?;

	let late = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_b_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000004")?,
		turn_sequence: 4,
		turn_role: TurnRole::System,
		possible_side_effects: PossibleSideEffects::None,
		history_item_id: HistoryItemId::new("44000000-0000-4000-8000-000000000004")?,
		ordinal: 0,
		kind: HistoryItemKind::Status,
		status: ItemStatus::Completed,
		text: "late append".into(),
		media_type: history_media_type("text/plain"),
		metadata: HistoryMetadata::empty(),
		expected_revision: None,
		artifact: None,
	};

	store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("late-item", b"late-item-v1")?,
			&late,
		)
		.await?;

	assert_cursor_attack_rejection(store, runtime_client, fixture, issued, &issued_token).await?;

	let second_page = store
		.conversation_history(
			&fixture.blob_store,
			&fixture.conversation_id,
			first_page.next_cursor.as_ref(),
			2,
		)
		.await?;

	assert_eq!(second_page.entries.len(), 1);
	assert!(second_page.next_cursor.is_none());
	assert_eq!(second_page.entries[0].revision, 1);
	assert_eq!(second_page.entries[0].status, ItemStatus::Streaming);
	assert_eq!(second_page.entries[0].inline_text.as_deref(), Some("manual boundary"));

	let replayed_second_page = store
		.conversation_history(
			&fixture.blob_store,
			&fixture.conversation_id,
			first_page.next_cursor.as_ref(),
			2,
		)
		.await?;

	assert_eq!(replayed_second_page.entries, second_page.entries);
	assert_eq!(
		store
			.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 10)
			.await?
			.entries
			.len(),
		4
	);

	assert_cursor_parent_provenance(store, client, fixture).await?;

	Ok(())
}

async fn assert_cursor_attack_rejection(
	store: &PostgresStore,
	runtime_client: &Client,
	fixture: &ConversationFixture,
	issued: &HistoryCursor,
	issued_token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_cursor_counter_forgery_rejected(store, runtime_client, fixture).await?;

	assert!(
		runtime_client
			.execute(
				"INSERT INTO decodex.history_cursors \
				 (cursor_id,conversation_id,snapshot_high_water,page_size,last_position,history_item_id) \
				 VALUES ('44000000-0000-4000-8000-000000000099',$1::text::uuid,4,3,3, \
				 '44000000-0000-4000-8000-000000000003')",
				&[&fixture.conversation_id.as_str()],
			)
			.await
			.is_err()
	);
	assert!(
		runtime_client
			.execute(
				"INSERT INTO decodex.history_cursors \
				 (cursor_id,conversation_id,snapshot_high_water,page_size,last_position,history_item_id,parent_cursor_id) \
				 VALUES ('44000000-0000-4000-8000-000000000097',$1::text::uuid,4,1,3, \
				 '44000000-0000-4000-8000-000000000003',$2::text::uuid)",
				&[&fixture.conversation_id.as_str(), &issued_token.trim_start_matches("v1:")],
			)
			.await
			.is_err()
	);

	let other = ConversationId::new("40000000-0000-4000-8000-000000000099")?;

	assert!(
		store.conversation_history(&fixture.blob_store, &other, Some(issued), 2).await.is_err()
	);
	assert!(
		store
			.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 0)
			.await
			.is_err()
	);
	assert!(
		store
			.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 101)
			.await
			.is_err()
	);

	for forged in
		["v1:44000000-0000-4000-8000-000000000098", "v1:44000000-0000-4000-8000-000000000099"]
	{
		let forged = HistoryCursor::parse(forged)?;

		assert!(
			store
				.conversation_history(
					&fixture.blob_store,
					&fixture.conversation_id,
					Some(&forged),
					2
				)
				.await
				.is_err()
		);
	}

	assert!(HistoryCursor::parse(&format!("{}:edited-boundary", issued.encode())).is_err());

	Ok(())
}

async fn assert_cursor_counter_forgery_rejected(
	store: &PostgresStore,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let stored_counter_absent: bool = runtime_client
		.query_one(
			"SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute \
			 WHERE attrelid='decodex.conversations'::pg_catalog.regclass \
			 AND attname='next_history_position' AND NOT attisdropped)",
			&[],
		)
		.await?
		.get(0);

	assert!(stored_counter_absent);

	let derived_fixture = "40000000-0000-4000-8000-000000000098";

	runtime_client
		.execute(
			"INSERT INTO decodex.conversations (conversation_id,title) \
			 VALUES ($1::text::uuid,'derived field fixture')",
			&[&derived_fixture],
		)
		.await?;

	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.conversations SET status='archived',revision=2, \
				 created_at=created_at - interval '1 second' WHERE conversation_id=$1::text::uuid",
				&[&derived_fixture],
			)
			.await
			.is_err()
	);

	runtime_client
		.execute(
			"UPDATE decodex.conversations SET status='archived',revision=2,updated_at='infinity' \
			 WHERE conversation_id=$1::text::uuid",
			&[&derived_fixture],
		)
		.await?;

	let canonical_archive: bool = runtime_client
		.query_one(
			"SELECT status='archived' AND revision=2 AND isfinite(updated_at) \
			 AND updated_at>=created_at FROM decodex.conversations WHERE conversation_id=$1::text::uuid",
			&[&derived_fixture],
		)
		.await?
		.get(0);

	assert!(canonical_archive);

	let cursor_count_before: i64 = runtime_client
		.query_one(
			"SELECT count(*) FROM decodex.history_cursors WHERE conversation_id=$1::text::uuid",
			&[&fixture.conversation_id.as_str()],
		)
		.await?
		.get(0);

	runtime_client.batch_execute("BEGIN").await?;

	let lowered = runtime_client
		.execute(
			"UPDATE decodex.conversations SET next_history_position=3, revision=revision+1 \
			 WHERE conversation_id=$1::text::uuid",
			&[&fixture.conversation_id.as_str()],
		)
		.await;
	let forged_issuance = runtime_client
		.query_one(
			"SELECT decodex.issue_history_cursor($1::text::uuid,NULL,2)",
			&[&fixture.conversation_id.as_str()],
		)
		.await;
	let restored = runtime_client
		.execute(
			"UPDATE decodex.conversations SET next_history_position=5, revision=revision+1 \
			 WHERE conversation_id=$1::text::uuid",
			&[&fixture.conversation_id.as_str()],
		)
		.await;

	runtime_client.batch_execute("ROLLBACK").await?;

	assert!(lowered.is_err());
	assert!(forged_issuance.is_err());
	assert!(restored.is_err());
	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.conversations SET revision=revision+1 \
				 WHERE conversation_id=$1::text::uuid",
				&[&fixture.conversation_id.as_str()],
			)
			.await
			.is_err()
	);

	let cursor_count_after: i64 = runtime_client
		.query_one(
			"SELECT count(*) FROM decodex.history_cursors WHERE conversation_id=$1::text::uuid",
			&[&fixture.conversation_id.as_str()],
		)
		.await?
		.get(0);

	assert_eq!(cursor_count_after, cursor_count_before);
	assert_eq!(
		store
			.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 8)
			.await?
			.entries
			.len(),
		4
	);

	Ok(())
}

async fn assert_cursor_parent_provenance(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let root_one = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 1)
		.await?
		.next_cursor
		.expect("one-item first page has a continuation");
	let from_one = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, Some(&root_one), 1)
		.await?
		.next_cursor
		.expect("same-size continuation has another page");
	let repeated = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, Some(&root_one), 1)
		.await?
		.next_cursor
		.expect("same continuation retry has another page");
	let root_two = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 2)
		.await?
		.next_cursor
		.expect("two-item first page has a continuation");

	assert_eq!(from_one, repeated);
	assert_ne!(from_one, root_two);
	assert!(
		store
			.conversation_history(&fixture.blob_store, &fixture.conversation_id, Some(&root_one), 2)
			.await
			.is_err()
	);

	let bindings = client
		.query(
			"SELECT cursor_id::text,parent_cursor_id::text,page_size,last_position \
			 FROM decodex.history_cursors WHERE cursor_id = ANY($1::text[]::uuid[]) \
			 ORDER BY last_position,cursor_id",
			&[&vec![
				root_one.encode().trim_start_matches("v1:").to_owned(),
				from_one.encode().trim_start_matches("v1:").to_owned(),
				root_two.encode().trim_start_matches("v1:").to_owned(),
			]],
		)
		.await?;

	assert_eq!(bindings.len(), 3);
	assert!(bindings.iter().any(|row| {
		row.get::<_, Option<String>>(1).as_deref()
			== Some(root_one.encode().trim_start_matches("v1:"))
			&& row.get::<_, i32>(2) == 1
			&& row.get::<_, i64>(3) == 2
	}));
	assert!(bindings.iter().any(|row| {
		row.get::<_, Option<String>>(1).is_none()
			&& row.get::<_, i32>(2) == 2
			&& row.get::<_, i64>(3) == 2
	}));

	Ok(())
}

async fn assert_context_pack_and_transition(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let artifact_id = ArtifactId::new("48000000-0000-4000-8000-000000000001")?;
	let artifact = store
		.create_artifact(
			&fixture.blob_store,
			&CommandIdentity::new("artifact-create", b"artifact-create-v1")?,
			&CreateArtifact {
				artifact_id: artifact_id.clone(),
				conversation_id: fixture.conversation_id.clone(),
				bytes: b"artifact provenance bytes".to_vec(),
				media_type: "application/octet-stream".into(),
				display_name: Some("evidence.bin".into()),
			},
		)
		.await?;

	assert_eq!(artifact.bytes, b"artifact provenance bytes");

	let policy = ContextPackPolicy::new(70_000, 8)?;
	let input = ContextPackInput {
		conversation_id: fixture.conversation_id.clone(),
		possible_side_effects: PossibleSideEffects::Unknown,
		policy,
		pinned: PinnedContextSource::new(
			"project-revision",
			9,
			"pinned-context-line\n".repeat(2_000),
		)?,
		optional_sources: vec![
			ContextPackSource::new(
				ContextSourceKind::Decision,
				"decision-1",
				2,
				"keep routing disabled\n".repeat(3_000),
			)?,
			ContextPackSource::artifact(
				artifact_id.clone(),
				1,
				b"artifact provenance bytes".to_vec(),
			)?,
			ContextPackSource::new(ContextSourceKind::RecentRaw, "turn-2", 2, "complete")?,
		],
	};
	let pack = decodex_core::compile_context_pack(input.clone())?;

	assert_eq!(pack, decodex_core::compile_context_pack(input)?);
	assert!(pack.truncated());
	assert!(pack.bytes().len() <= policy.max_bytes());

	let context_pack_id = "46000000-0000-4000-8000-000000000001".to_owned();
	let context_request =
		PersistContextPack { context_pack_id: context_pack_id.clone(), pack_revision: 1 };
	let context_command = CommandIdentity::new("context-pack-1", b"context-pack-1-v1")?;
	let stored_pack = store
		.persist_context_pack(&fixture.blob_store, &context_command, &context_request, &pack)
		.await?;

	assert_eq!(stored_pack.compiled_digest, pack.digest());

	let source_rows: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.context_pack_sources WHERE context_pack_id = $1::text::uuid",
			&[&context_pack_id],
		)
		.await?
		.get(0);

	assert_eq!(usize::try_from(source_rows)?, pack.source_manifest().len());

	assert_context_pack_integrity_and_sealing(store, client, fixture, &pack, &stored_pack).await?;

	assert_artifact_and_transition_matrix(
		store,
		client,
		runtime_client,
		fixture,
		artifact_id,
		context_pack_id,
	)
	.await
}

async fn assert_context_pack_integrity_and_sealing(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
	pack: &ContextPack,
	stored_pack: &ContextPackRecord,
) -> Result<(), Box<dyn std::error::Error>> {
	let context_pack_id = &stored_pack.context_pack_id;
	let pack_blob: String = client
		.query_one(
			"SELECT blob_hash FROM decodex.context_packs WHERE context_pack_id = $1::text::uuid",
			&[&context_pack_id],
		)
		.await?
		.get(0);
	let pack_blob_hash = BlobHash::parse(&pack_blob)?;
	let pack_path = fixture.blob_store.path_for(pack_blob_hash);

	fs::write(&pack_path, b"tampered-context-pack")?;

	assert!(store.context_pack(&fixture.blob_store, context_pack_id).await.is_err());

	fs::remove_file(&pack_path)?;

	fixture.blob_store.put(pack.bytes())?;

	assert_eq!(&store.context_pack(&fixture.blob_store, context_pack_id).await?, stored_pack);
	assert!(client.execute("UPDATE decodex.context_packs SET possible_side_effects='none' WHERE context_pack_id=$1::text::uuid", &[&context_pack_id]).await.is_err());
	assert!(client.execute("UPDATE decodex.context_pack_sources SET content_digest=repeat('0',64), included_digest=repeat('0',64) WHERE context_pack_id=$1::text::uuid AND position=0", &[&context_pack_id]).await.is_err());
	assert!(
		client
			.execute(
				"DELETE FROM decodex.context_pack_sources WHERE context_pack_id=$1::text::uuid AND position=0",
				&[&context_pack_id]
			)
			.await
			.is_err()
	);
	assert!(client.execute(
		"INSERT INTO decodex.context_pack_sources \
		 (context_pack_id,conversation_id,position,kind,source_id,source_revision,content_digest, \
		 original_byte_length,included_byte_length,included_digest,disposition) \
		 VALUES ($1::text::uuid,$2::text::uuid,511,'fact','poison',1,repeat('f',64),1,1,repeat('f',64),'complete')",
		&[&context_pack_id, &fixture.conversation_id.as_str()],
	).await.is_err());
	assert_eq!(&store.context_pack(&fixture.blob_store, context_pack_id).await?, stored_pack);

	let source_artifact_path =
		fixture.blob_store.path_for(BlobHash::digest(b"artifact provenance bytes"));

	fs::write(&source_artifact_path, b"tampered Context Pack source")?;

	assert!(matches!(
		store.context_pack(&fixture.blob_store, context_pack_id).await,
		Err(StoreError::Blob(_)) | Err(StoreError::Incompatible(_))
	));

	fs::remove_file(&source_artifact_path)?;

	assert!(matches!(
		store.context_pack(&fixture.blob_store, context_pack_id).await,
		Err(StoreError::Blob(_))
	));

	fixture.blob_store.put(b"artifact provenance bytes")?;

	let incomplete_pack_id = "46000000-0000-4000-8000-000000000009";

	client.batch_execute("BEGIN").await?;

	let incomplete = client
		.batch_execute(&format!(
			"INSERT INTO decodex.context_pack_sources \
			 (context_pack_id,conversation_id,position,kind,source_id,source_revision,content_digest, \
			 original_byte_length,included_byte_length,included_digest,disposition) VALUES \
			 ('{incomplete_pack_id}','{}',0,'pinned_revision','pinned',1,repeat('d',64),1,1,repeat('d',64),'complete'), \
			 ('{incomplete_pack_id}','{}',2,'fact','gapped',1,repeat('e',64),1,1,repeat('e',64),'complete'); \
			 INSERT INTO decodex.context_packs \
			 (context_pack_id,conversation_id,pack_revision,compiled_digest,manifest_digest,inline_bytes, \
			 byte_length,max_bytes,recent_item_limit,possible_side_effects,truncated,omitted_source_count,source_count) \
			 VALUES ('{incomplete_pack_id}','{}',1,repeat('b',64),repeat('c',64),'x',1,1024,1,'none',false,0,2)",
			fixture.conversation_id.as_str(),
			fixture.conversation_id.as_str(),
			fixture.conversation_id.as_str(),
		))
		.await;

	assert!(incomplete.is_err());

	client.batch_execute("ROLLBACK").await?;

	let incomplete_rows: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.context_packs WHERE context_pack_id=$1::text::uuid) \
			 + (SELECT count(*) FROM decodex.context_pack_sources WHERE context_pack_id=$1::text::uuid)",
			&[&incomplete_pack_id],
		)
		.await?
		.get(0);

	assert_eq!(incomplete_rows, 0);

	Ok(())
}

async fn assert_artifact_and_transition_matrix(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
	fixture: &ConversationFixture,
	artifact_id: ArtifactId,
	context_pack_id: String,
) -> Result<(), Box<dyn std::error::Error>> {
	let second_artifact_id =
		assert_second_artifact_media_authority(store, client, runtime_client, fixture).await?;
	let artifact_history = assert_artifact_history_correlation(
		store,
		client,
		fixture,
		&artifact_id,
		&second_artifact_id,
	)
	.await?;

	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.artifacts SET status='expired',revision=2, \
				 created_at=created_at - interval '1 second' \
				 WHERE artifact_id=$1::text::uuid",
				&[&artifact_id.as_str()],
			)
			.await
			.is_err()
	);

	let expired = store
		.transition_artifact(
			&fixture.blob_store,
			&CommandIdentity::new("artifact-expire", b"artifact-expire-v1")?,
			&artifact_id,
			1,
			ArtifactStatus::Expired,
		)
		.await?;

	assert_eq!(expired.revision, 2);
	assert_eq!(
		store.artifact(&fixture.blob_store, &artifact_id, Some(1)).await?.status,
		ArtifactStatus::Active
	);
	assert!(client.batch_execute("UPDATE decodex.artifacts SET conversation_id='40000000-0000-4000-8000-000000000099', revision=revision+1 WHERE artifact_id='48000000-0000-4000-8000-000000000001'").await.is_err());
	assert!(client.batch_execute("UPDATE decodex.history_items SET status='streaming', revision=revision+1 WHERE history_item_id='44000000-0000-4000-8000-000000000005'").await.is_err());
	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.turns SET status='completed',revision=2, \
				 created_at=created_at - interval '1 second' \
				 WHERE turn_id=$1::text::uuid",
				&[&artifact_history.turn_id.as_str()],
			)
			.await
			.is_err()
	);
	assert_eq!(
		store
			.transition_turn(
				&CommandIdentity::new("artifact-turn-complete", b"artifact-turn-complete-v1")?,
				&artifact_history.turn_id,
				1,
				decodex_core::TurnStatus::Completed
			)
			.await?,
		2
	);
	assert!(client.batch_execute("UPDATE decodex.turns SET status='active', revision=revision+1 WHERE turn_id='45000000-0000-4000-8000-000000000005'").await.is_err());

	assert_transition_proposal_and_session_terminalization(store, client, fixture, context_pack_id)
		.await
}

async fn assert_second_artifact_media_authority(
	store: &PostgresStore,
	client: &Client,
	runtime_client: &Client,
	fixture: &ConversationFixture,
) -> Result<ArtifactId, Box<dyn std::error::Error>> {
	let artifact_id = ArtifactId::new("48000000-0000-4000-8000-000000000002")?;

	store
		.create_artifact(
			&fixture.blob_store,
			&CommandIdentity::new("artifact-create-second", b"artifact-create-second-v1")?,
			&CreateArtifact {
				artifact_id: artifact_id.clone(),
				conversation_id: fixture.conversation_id.clone(),
				bytes: b"second artifact provenance".to_vec(),
				media_type: "application/octet-stream".into(),
				display_name: None,
			},
		)
		.await?;

	assert!(
		client
			.execute(
				"INSERT INTO decodex.artifact_revisions \
				 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
				 SELECT artifact_id,conversation_id,2,blob_hash,'not a media type',status \
				 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
				&[&artifact_id.as_str()],
			)
			.await
			.is_err()
	);
	assert!(
		client
			.execute(
				"INSERT INTO decodex.artifact_revisions \
				 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
				 SELECT artifact_id,conversation_id,2,blob_hash,'application/octet-stream',status \
				 FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=1",
				&[&artifact_id.as_str()],
			)
			.await
			.is_err()
	);
	assert!(
		client
			.execute(
				"UPDATE decodex.artifacts SET status='expired',revision=2,updated_at=clock_timestamp() \
				 WHERE artifact_id=$1::text::uuid",
				&[&artifact_id.as_str()],
			)
			.await
			.is_err()
	);

	let coherent_parent = client
		.query_one(
			"SELECT status::text,revision,EXISTS (SELECT 1 FROM decodex.artifact_revisions ar \
			 WHERE ar.artifact_id=a.artifact_id AND ar.conversation_id=a.conversation_id \
			 AND ar.revision=a.revision AND ar.status=a.status) \
			 FROM decodex.artifacts a WHERE artifact_id=$1::text::uuid",
			&[&artifact_id.as_str()],
		)
		.await?;

	assert_eq!(coherent_parent.get::<_, &str>(0), "active");
	assert_eq!(coherent_parent.get::<_, i64>(1), 1);
	assert!(coherent_parent.get::<_, bool>(2));

	assert_noncontiguous_artifact_attacks(runtime_client, fixture, &artifact_id).await?;

	Ok(artifact_id)
}

async fn assert_noncontiguous_artifact_attacks(
	runtime_client: &Client,
	fixture: &ConversationFixture,
	source_artifact_id: &ArtifactId,
) -> Result<(), Box<dyn std::error::Error>> {
	let missing_first = "48000000-0000-4000-8000-000000000003";

	runtime_client.batch_execute("BEGIN").await?;
	runtime_client
		.execute(
			"INSERT INTO decodex.artifacts (artifact_id,conversation_id) \
			 VALUES ($1::text::uuid,$2::text::uuid)",
			&[&missing_first, &fixture.conversation_id.as_str()],
		)
		.await?;

	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.artifacts SET status='expired',revision=2 \
			 WHERE artifact_id=$1::text::uuid",
				&[&missing_first],
			)
			.await
			.is_err()
	);

	runtime_client.batch_execute("ROLLBACK").await?;

	let missing_middle = "48000000-0000-4000-8000-000000000004";

	runtime_client.batch_execute("BEGIN").await?;
	runtime_client
		.execute(
			"INSERT INTO decodex.artifacts (artifact_id,conversation_id) \
			 VALUES ($1::text::uuid,$2::text::uuid)",
			&[&missing_middle, &fixture.conversation_id.as_str()],
		)
		.await?;
	runtime_client
		.execute(
			"INSERT INTO decodex.artifact_revisions \
			 (artifact_id,conversation_id,revision,blob_hash,media_type,status) \
			 SELECT $1::text::uuid,$2::text::uuid,1,blob_hash,media_type,'active' \
			 FROM decodex.artifact_revisions \
			 WHERE artifact_id=$3::text::uuid AND revision=1",
			&[&missing_middle, &fixture.conversation_id.as_str(), &source_artifact_id.as_str()],
		)
		.await?;
	runtime_client
		.execute(
			"UPDATE decodex.artifacts SET status='expired',revision=2 \
			 WHERE artifact_id=$1::text::uuid",
			&[&missing_middle],
		)
		.await?;

	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.artifacts SET status='deleted',revision=3 \
			 WHERE artifact_id=$1::text::uuid",
				&[&missing_middle],
			)
			.await
			.is_err()
	);

	runtime_client.batch_execute("ROLLBACK").await?;

	let attack_rows: i64 = runtime_client
		.query_one(
			"SELECT count(*) FROM decodex.artifacts \
			 WHERE artifact_id IN ($1::text::uuid,$2::text::uuid)",
			&[&missing_first, &missing_middle],
		)
		.await?
		.get(0);

	assert_eq!(attack_rows, 0);

	Ok(())
}

async fn assert_artifact_history_correlation(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
	artifact_id: &ArtifactId,
	second_artifact_id: &ArtifactId,
) -> Result<RecordHistoryItem, Box<dyn std::error::Error>> {
	let mut mutation = RecordHistoryItem {
		conversation_id: fixture.conversation_id.clone(),
		runtime_session_id: fixture.session_a_id.clone(),
		turn_id: TurnId::new("45000000-0000-4000-8000-000000000005")?,
		turn_sequence: 5,
		turn_role: TurnRole::Tool,
		possible_side_effects: PossibleSideEffects::None,
		history_item_id: HistoryItemId::new("44000000-0000-4000-8000-000000000005")?,
		ordinal: 0,
		kind: HistoryItemKind::Artifact,
		status: ItemStatus::Streaming,
		text: "artifact reference".into(),
		media_type: history_media_type("text/plain"),
		metadata: HistoryMetadata::empty(),
		expected_revision: None,
		artifact: Some((artifact_id.clone(), 1)),
	};
	let stored = store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("artifact-history", b"artifact-history-v1")?,
			&mutation,
		)
		.await?;

	assert_eq!(stored.artifact, Some((artifact_id.clone(), 1)));

	let mut mismatch = mutation.clone();

	mismatch.status = ItemStatus::Completed;
	mismatch.expected_revision = Some(1);
	mismatch.artifact = Some((second_artifact_id.clone(), 1));

	assert!(matches!(
		store
			.record_history_item(
				&fixture.blob_store,
				&CommandIdentity::new(
					"artifact-history-mismatch",
					b"artifact-history-mismatch-v1"
				)?,
				&mismatch,
			)
			.await,
		Err(StoreError::RevisionConflict { actual: Some(1), .. })
	));
	assert!(client.execute("UPDATE decodex.history_items SET artifact_id=$2::text::uuid, artifact_revision=1,revision=revision+1 WHERE history_item_id=$1::text::uuid", &[&mutation.history_item_id.as_str(), &second_artifact_id.as_str()]).await.is_err());

	let retained = client
		.query_one(
			"SELECT artifact_id::text,artifact_revision,revision, EXISTS (SELECT 1 FROM decodex.command_receipts WHERE idempotency_key='artifact-history-mismatch' AND receipt_state='pending') FROM decodex.history_items WHERE history_item_id=$1::text::uuid",
			&[&mutation.history_item_id.as_str()],
		)
		.await?;

	assert_eq!(retained.get::<_, String>(0), artifact_id.as_str());
	assert_eq!(retained.get::<_, i64>(1), 1);
	assert_eq!(retained.get::<_, i64>(2), 1);
	assert!(retained.get::<_, bool>(3));

	mutation.status = ItemStatus::Completed;
	mutation.expected_revision = Some(1);

	let stored = store
		.record_history_item(
			&fixture.blob_store,
			&CommandIdentity::new("artifact-history-complete", b"artifact-history-complete-v1")?,
			&mutation,
		)
		.await?;

	assert_eq!(stored.revision, 2);
	assert_eq!(stored.artifact, Some((artifact_id.clone(), 1)));

	let history = store
		.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 100)
		.await?;
	let readback = history
		.entries
		.iter()
		.find(|entry| entry.history_item_id == mutation.history_item_id.as_str())
		.expect("typed Artifact history readback");

	assert_eq!(readback.artifact, Some((artifact_id.clone(), 1)));

	let artifact_path = fixture.blob_store.path_for(BlobHash::digest(b"artifact provenance bytes"));

	fs::write(&artifact_path, b"tampered transitive Artifact bytes")?;

	assert!(matches!(
		store.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 100).await,
		Err(StoreError::Blob(_))
	));

	fs::remove_file(&artifact_path)?;

	assert!(matches!(
		store.conversation_history(&fixture.blob_store, &fixture.conversation_id, None, 100).await,
		Err(StoreError::Blob(_))
	));

	fixture.blob_store.put(b"artifact provenance bytes")?;

	Ok(mutation)
}

async fn assert_transition_proposal_and_session_terminalization(
	store: &PostgresStore,
	client: &Client,
	fixture: &ConversationFixture,
	context_pack_id: String,
) -> Result<(), Box<dyn std::error::Error>> {
	let proposal = ProposeTransition {
		transition_id: "47000000-0000-4000-8000-000000000001".into(),
		conversation_id: fixture.conversation_id.clone(),
		from_runtime_session_id: fixture.session_a_id.clone(),
		context_pack_id,
		kind: ProposedTransitionKind::Fallback,
		reason: "manual proposal fixture".into(),
	};

	store
		.propose_transition(
			&CommandIdentity::new("transition-proposal", b"transition-proposal-v1")?,
			&proposal,
		)
		.await?;

	let dispatch_enabled: bool = client
		.query_one(
			"SELECT dispatch_enabled FROM decodex.transition_proposals \
			 WHERE transition_id = $1::text::uuid",
			&[&proposal.transition_id],
		)
		.await?
		.get(0);

	assert!(!dispatch_enabled);

	client.execute("INSERT INTO decodex.conversations (conversation_id,title) VALUES ('40000000-0000-4000-8000-000000000099','Other conversation')", &[]).await?;

	assert!(client.batch_execute("INSERT INTO decodex.transition_proposals (transition_id,conversation_id,from_runtime_session_id,context_pack_id,kind,reason) VALUES ('47000000-0000-4000-8000-000000000099','40000000-0000-4000-8000-000000000099','41000000-0000-4000-8000-000000000001','46000000-0000-4000-8000-000000000001','fallback','cross conversation')").await.is_err());
	let session_snapshots = client
		.query_one(
			"SELECT profile_snapshot_id::text,account_snapshot_id::text \
			 FROM decodex.runtime_sessions WHERE runtime_session_id=$1::text::uuid",
			&[&fixture.session_a_id.as_str()],
		)
		.await?;
	let profile_snapshot_id = session_snapshots.get::<_, String>(0);
	let account_snapshot_id = session_snapshots.get::<_, String>(1);
	let invalid_initial_state = client
		.execute(
			"INSERT INTO decodex.runtime_sessions \
			 (runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id,state) \
			 VALUES ('41000000-0000-4000-8000-000000000099', \
			 '40000000-0000-4000-8000-000000000099',$1::text::uuid,$2::text::uuid,'ended')",
			&[&profile_snapshot_id, &account_snapshot_id],
		)
		.await
		.expect_err("a RuntimeSession cannot be inserted initially terminal");
	assert_eq!(
		invalid_initial_state.as_db_error().map(tokio_postgres::error::DbError::message),
		Some("illegal initial runtime session state"),
		"terminal-state rejection must come from the RuntimeSession invariant",
	);
	assert!(matches!(
		store
			.transition_runtime_session(
				"session-a-end-too-early",
				&fixture.session_a_id,
				1,
				RuntimeSessionState::Ended
			)
			.await?,
		RuntimeSessionCommandOutcome::Rejected(_)
	));

	store
		.transition_turn(
			&CommandIdentity::new("large-turn-complete", b"large-turn-complete-v1")?,
			&TurnId::new("45000000-0000-4000-8000-000000000001")?,
			1,
			TurnStatus::Completed,
		)
		.await?;

	let RuntimeSessionCommandOutcome::Success(ended) = store
		.transition_runtime_session(
			"session-a-end",
			&fixture.session_a_id,
			1,
			RuntimeSessionState::Ended,
		)
		.await?
	else {
		panic!("exact session end must succeed");
	};
	assert_eq!(ended.runtime_session.state, RuntimeSessionState::Ended);
	assert!(client.batch_execute("UPDATE decodex.runtime_sessions SET state='active', revision=revision+1 WHERE runtime_session_id='41000000-0000-4000-8000-000000000001'").await.is_err());
	assert!(
		client
			.batch_execute(
				"UPDATE decodex.transition_proposals SET dispatch_enabled = true \
			 WHERE transition_id = '47000000-0000-4000-8000-000000000001'",
			)
			.await
			.is_err()
	);

	Ok(())
}

async fn assert_duration_validation(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	const INVALID_DURATION: &str =
		"duration must be a positive whole number of milliseconds no greater than 365 days";

	let overflow = Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS + 1);
	let huge = Duration::MAX;
	let boundary = i64::try_from(MAX_OPERATION_DURATION_MILLISECONDS)?;
	let interval_is_finite: bool = client
		.query_one("SELECT isfinite($1::bigint * interval '1 millisecond')", &[&boundary])
		.await?
		.get(0);

	assert!(interval_is_finite);

	let boundary_claim = store
		.try_acquire_lease(
			"duration/boundary",
			HOLDER_A,
			Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
		)
		.await?;

	assert!(boundary_claim.acquired);

	let boundary_token =
		boundary_claim.token.as_deref().expect("acquired boundary lease has token");

	store
		.renew_lease(
			"duration/boundary",
			HOLDER_A,
			boundary_token,
			Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
		)
		.await?;
	store.release_lease("duration/boundary", HOLDER_A, boundary_token).await?;

	assert_direct_lease_duration_boundary(client).await?;
	assert_direct_outbox_lease_duration_boundary(client).await?;

	for duration in
		[Duration::ZERO, Duration::from_nanos(1), Duration::from_micros(1_500), overflow, huge]
	{
		assert!(matches!(
			store.try_acquire_lease("duration/lease", HOLDER_A, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store.claim_outbox(WORKER_A, 1, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store.renew_outbox_claim(0, WORKER_A, WORKER_A, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.retry_outbox_before_effect(0, WORKER_A, WORKER_A, "temporary_failure", duration)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.reconcile_outbox(
					0,
					WORKER_A,
					WORKER_A,
					&OutboxReconciliation {
						readback: serde_json::json!({"observed": false}),
						outcome: ReconciliationOutcome::EffectAbsent,
					},
					duration,
					Duration::from_millis(1),
				)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.reconcile_outbox(
					0,
					WORKER_A,
					WORKER_A,
					&OutboxReconciliation {
						readback: serde_json::json!({"observed": false}),
						outcome: ReconciliationOutcome::EffectAbsent,
					},
					Duration::from_millis(1),
					duration,
				)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
	}

	let valid = store
		.try_acquire_lease("session_id/token_budget", HOLDER_A, Duration::from_millis(1))
		.await?;

	assert!(valid.acquired);

	Ok(())
}

async fn assert_direct_lease_duration_boundary(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let direct_token: String = client
		.query_one(
			"SELECT lease_token::text FROM decodex.try_acquire_lease( \
			 'duration/direct', $1::text::uuid, interval '365 days') WHERE acquired",
			&[&HOLDER_A],
		)
		.await?
		.get(0);

	for statement in [
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-overflow', '20000000-0000-0000-0000-000000000001', interval '366 days')",
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-fractional', '20000000-0000-0000-0000-000000000001', interval '0.0005 seconds')",
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-month', '20000000-0000-0000-0000-000000000001', interval '1 month')",
	] {
		let error =
			client.execute(statement, &[]).await.expect_err("invalid direct lease TTL rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
		);
	}
	for ttl in ["interval '366 days'", "interval '0.0005 seconds'"] {
		let statement = format!(
			"SELECT decodex.renew_lease( \
			 'duration/direct', '20000000-0000-0000-0000-000000000001', \
			 '{direct_token}', {ttl})"
		);
		let error =
			client.execute(&statement, &[]).await.expect_err("invalid direct renewal rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
		);
	}
	for statement in [
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-overflow', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + 31622400000 * interval '1 millisecond', \
			 statement_timestamp())",
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-fractional', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + interval '0.0005 seconds', statement_timestamp())",
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-infinity', '20000000-0000-0000-0000-000000000001', \
			 'infinity', statement_timestamp())",
	] {
		let error =
			client.execute(statement, &[]).await.expect_err("invalid direct lease row rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-shifted-anchor', \
			 '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + interval '1000 days 1 millisecond', \
			 statement_timestamp() + interval '1000 days')",
			&[],
		)
		.await
		.expect_err("future-shifted direct lease anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("leases_operation_time")
	);

	client
		.batch_execute(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/expired-row', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() - interval '1 day', statement_timestamp()); \
			 DELETE FROM decodex.leases WHERE resource_key IN ('duration/direct', 'duration/expired-row')",
		)
		.await?;

	Ok(())
}

async fn assert_direct_outbox_lease_duration_boundary(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	for (index, lease_expires_at) in [
		"statement_timestamp() + interval '0.0005 seconds'",
		"statement_timestamp() + interval '366 days'",
		"'infinity'::timestamptz",
	]
	.into_iter()
	.enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-{index}', 'account', 'lease', 1, '{{}}', 'in_flight', \
			  '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', statement_timestamp(), \
			  {lease_expires_at}, statement_timestamp())"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("invalid direct outbox lease rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-shifted-anchor', 'account', 'lease', 1, '{}', \
			  'in_flight', '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', \
			  statement_timestamp() + interval '1000 days', \
			  statement_timestamp() + interval '1000 days 1 millisecond', \
			  statement_timestamp())",
			&[],
		)
		.await
		.expect_err("future-shifted direct outbox lease anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	client
		.batch_execute(
			"BEGIN; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-valid', 'account', 'lease', 1, '{}', 'in_flight', \
			  '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', statement_timestamp(), \
			  statement_timestamp() + 31536000000 * interval '1 millisecond', \
			  statement_timestamp()); \
			 ROLLBACK",
		)
		.await?;

	Ok(())
}

async fn assert_closed_pool_behavior(
	store: &PostgresStore,
) -> Result<(), Box<dyn std::error::Error>> {
	assert!(matches!(store.account(&AccountId::new(ACCOUNT_ID)?).await, Err(StoreError::Pool(_))));
	assert!(matches!(store.activity_after(0, 1).await, Err(StoreError::Pool(_))));
	assert!(matches!(
		store
			.try_acquire_lease(
				"closed/boundary-duration",
				HOLDER_A,
				Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
			)
			.await,
		Err(StoreError::Pool(_))
	));
	assert!(matches!(
		store
			.try_acquire_lease(
				"closed/overflow-duration",
				HOLDER_A,
				Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS + 1),
			)
			.await,
		Err(StoreError::InvalidInput(
			"duration must be a positive whole number of milliseconds no greater than 365 days"
		))
	));
	assert!(matches!(
		store.claim_outbox(WORKER_A, 1, Duration::from_millis(1)).await,
		Err(StoreError::Pool(_))
	));
	assert!(matches!(store.prune_delivered_outbox(1).await, Err(StoreError::Pool(_))));
	assert!(decodex_postgres::parse_quota_timestamp_rfc3339("infinity").is_err());

	Ok(())
}

async fn assert_outbox_concurrency_retry_and_restart(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	seed_outbox_fixtures(client).await?;

	let available: i64 = client
		.query_one("SELECT count(*) FROM decodex.outbox WHERE state = 'pending'", &[])
		.await?
		.get(0);
	let mut tasks = JoinSet::new();

	for worker in 0..8 {
		let store = store.clone();
		let worker_id = format!("60000000-0000-0000-0000-{worker:012}");

		tasks.spawn(
			async move { store.claim_outbox(&worker_id, 200, Duration::from_secs(2)).await },
		);
	}

	let mut claims = Vec::new();

	while let Some(result) = tasks.join_next().await {
		claims.extend(result??);
	}

	let unique: HashSet<_> = claims.iter().map(|claim| claim.id).collect();

	assert_eq!(claims.len(), usize::try_from(available)?);
	assert_eq!(unique.len(), claims.len());

	client
		.execute(
			"UPDATE decodex.outbox SET state = 'pending', lease_holder = NULL, claim_token = NULL, \
			 lease_acquired_at = NULL, lease_expires_at = NULL WHERE state = 'in_flight'",
			&[],
		)
		.await?;

	assert_outbox_retry_and_restart(store, client, runtime).await
}

async fn seed_outbox_fixtures(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
	let inserted = client
		.execute(
			"INSERT INTO decodex.outbox(\
			 effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload\
			 ) SELECT 'outbox-fixture-'||fixture::text,'fixture',\
			 'outbox-fixture-'||fixture::text,1,\
			 pg_catalog.jsonb_build_object('fixture',fixture) \
			 FROM pg_catalog.generate_series(0,95) AS fixture",
			&[],
		)
		.await?;

	assert_eq!(inserted, 96);
	Ok(())
}

async fn assert_outbox_retry_and_restart(
	store: &PostgresStore,
	client: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let retry = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	client
		.execute(
			"UPDATE decodex.outbox SET available_at = clock_timestamp() + interval '1 hour' \
			 WHERE state = 'pending' AND id <> $1",
			&[&retry.id],
		)
		.await?;
	store
		.retry_outbox_before_effect(
			retry.id,
			WORKER_A,
			&retry.claim_token,
			"temporary_failure",
			Duration::from_millis(60),
		)
		.await?;

	assert!(store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.is_empty());

	time::sleep(Duration::from_millis(80)).await;

	let retry_claim = store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(retry_claim.id, retry.id);
	assert_eq!(retry_claim.attempt_count, retry.attempt_count + 1);

	store
		.retry_outbox_before_effect(
			retry_claim.id,
			WORKER_B,
			&retry_claim.claim_token,
			"fixture_release",
			Duration::from_secs(30),
		)
		.await?;
	client
		.execute(
			"UPDATE decodex.outbox SET available_at = clock_timestamp() \
			 WHERE id = (SELECT min(id) FROM decodex.outbox WHERE state = 'pending' AND id <> $1)",
			&[&retry_claim.id],
		)
		.await?;

	let ambiguous = store.claim_outbox(WORKER_A, 1, Duration::from_millis(40)).await?.remove(0);

	store.begin_outbox_effect(ambiguous.id, WORKER_A, &ambiguous.claim_token).await?;
	store.close();

	assert_restart_reconciliation(client, runtime, &ambiguous).await
}

async fn assert_restart_reconciliation(
	client: &Client,
	runtime: &Config,
	ambiguous: &OutboxClaim,
) -> Result<(), Box<dyn std::error::Error>> {
	time::sleep(Duration::from_millis(60)).await;

	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let recovered = restarted.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(recovered.id, ambiguous.id);
	assert!(recovered.requires_reconciliation);
	assert_ne!(recovered.claim_token, ambiguous.claim_token);

	assert_stale_outbox_claim_rejected(&restarted, ambiguous, &recovered).await?;

	let receiptless_state: (String, String, Option<Value>) = {
		let row = client
			.query_one(
				"SELECT state::text, effect_state::text, receipt FROM decodex.outbox WHERE id = $1",
				&[&recovered.id],
			)
			.await?;

		(row.get(0), row.get(1), row.get(2))
	};

	assert_eq!(receiptless_state, ("in_flight".into(), "ambiguous".into(), None));

	restarted
		.record_outbox_receipt(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&serde_json::json!({"provider_receipt": "receipt-1"}),
		)
		.await?;

	assert_invalid_reconciliation_evidence(&restarted, &recovered).await?;

	restarted
		.reconcile_outbox(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"effect_key": recovered.effect_key, "observed": true}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_millis(1),
		)
		.await?;

	let delivered: (String, String, Value, Value) = {
		let row = client
			.query_one(
				"SELECT state::text, effect_state::text, receipt, reconciliation \
				 FROM decodex.outbox WHERE id = $1",
				&[&recovered.id],
			)
			.await?;

		(row.get(0), row.get(1), row.get(2), row.get(3))
	};

	assert_eq!(delivered.0, "delivered");
	assert_eq!(delivered.1, "receipt_recorded");
	assert_eq!(delivered.2, serde_json::json!({"provider_receipt": "receipt-1"}));
	assert_eq!(
		delivered.3,
		serde_json::json!({"effect_key": recovered.effect_key, "observed": true})
	);

	time::sleep(Duration::from_millis(10)).await;

	assert_eq!(restarted.prune_delivered_outbox(10).await?, 1);

	assert_effect_absent_reconciliation(&restarted, client).await?;

	Ok(())
}

async fn assert_invalid_reconciliation_evidence(
	store: &PostgresStore,
	claim: &OutboxClaim,
) -> Result<(), Box<dyn std::error::Error>> {
	for readback in [
		Value::Null,
		serde_json::json!(" \n "),
		serde_json::json!("\u{a0}"),
		serde_json::json!("\u{85}"),
		serde_json::json!("\u{202f}"),
		serde_json::json!("\u{3000}"),
		serde_json::json!({}),
		serde_json::json!([]),
		serde_json::json!({"nested": []}),
	] {
		assert!(matches!(
			store
				.reconcile_outbox(
					claim.id,
					WORKER_A,
					&claim.claim_token,
					&OutboxReconciliation {
						readback,
						outcome: ReconciliationOutcome::EffectPresent,
					},
					Duration::from_millis(1),
					Duration::from_millis(1),
				)
				.await,
			Err(StoreError::InvalidInput("outbox evidence must contain a non-empty JSON value"))
		));
	}

	assert!(matches!(
		store
			.reconcile_outbox(
				claim.id,
				WORKER_A,
				&claim.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"Authorization": "forbidden"}),
					outcome: ReconciliationOutcome::EffectPresent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	Ok(())
}

async fn assert_stale_outbox_claim_rejected(
	store: &PostgresStore,
	ambiguous: &OutboxClaim,
	recovered: &OutboxClaim,
) -> Result<(), Box<dyn std::error::Error>> {
	assert!(matches!(
		store.begin_outbox_effect(recovered.id, WORKER_A, &ambiguous.claim_token).await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.renew_outbox_claim(
				recovered.id,
				WORKER_A,
				&ambiguous.claim_token,
				Duration::from_secs(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.record_outbox_receipt(
				recovered.id,
				WORKER_A,
				&ambiguous.claim_token,
				&serde_json::json!({"provider_receipt": "stale"}),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.record_outbox_receipt(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				&serde_json::json!({"accessToken": "forbidden"}),
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	for evidence in [
		Value::Null,
		serde_json::json!(" \t "),
		serde_json::json!("\u{a0}"),
		serde_json::json!("\u{85}"),
		serde_json::json!("\u{202f}"),
		serde_json::json!("\u{3000}"),
		serde_json::json!({}),
		serde_json::json!([]),
		serde_json::json!({"nested": {}}),
	] {
		assert!(matches!(
			store
				.record_outbox_receipt(recovered.id, WORKER_A, &recovered.claim_token, &evidence,)
				.await,
			Err(StoreError::InvalidInput("outbox evidence must contain a non-empty JSON value"))
		));
	}

	assert!(matches!(
		store
			.reconcile_outbox(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"observed": true}),
					outcome: ReconciliationOutcome::EffectPresent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));

	Ok(())
}

async fn assert_effect_absent_reconciliation(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('effect-absent-retry', 'account', $1, 1, '{\"fixture\":\"absent\"}')",
			&[&ACCOUNT_ID],
		)
		.await?;

	let first = store.claim_outbox(WORKER_A, 1, Duration::from_millis(40)).await?.remove(0);

	store.begin_outbox_effect(first.id, WORKER_A, &first.claim_token).await?;

	time::sleep(Duration::from_millis(60)).await;

	let recovered = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(recovered.id, first.id);
	assert!(recovered.requires_reconciliation);
	assert_ne!(recovered.claim_token, first.claim_token);
	assert!(matches!(
		store
			.reconcile_outbox(
				recovered.id,
				WORKER_A,
				&first.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"observed": false}),
					outcome: ReconciliationOutcome::EffectAbsent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.retry_outbox_before_effect(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				"blind_replay_forbidden",
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));

	store
		.reconcile_outbox(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"observed": false}),
				outcome: ReconciliationOutcome::EffectAbsent,
			},
			Duration::from_millis(60),
			Duration::from_millis(1),
		)
		.await?;

	assert!(store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.is_empty());

	time::sleep(Duration::from_millis(80)).await;

	let retry = store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(retry.id, recovered.id);
	assert_eq!(retry.attempt_count, recovered.attempt_count + 1);
	assert!(!retry.requires_reconciliation);

	store
		.retry_outbox_before_effect(
			retry.id,
			WORKER_B,
			&retry.claim_token,
			"fixture_release",
			Duration::from_secs(30),
		)
		.await?;

	assert_effect_absent_dead_letter(store, client).await?;

	Ok(())
}

async fn assert_effect_absent_dead_letter(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, max_attempts) \
			 VALUES ('effect-absent-dead-letter', 'account', $1, 1, \
			         '{\"fixture\":\"dead-letter\"}', 1)",
			&[&ACCOUNT_ID],
		)
		.await?;

	let final_attempt = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	store.begin_outbox_effect(final_attempt.id, WORKER_A, &final_attempt.claim_token).await?;
	store
		.reconcile_outbox(
			final_attempt.id,
			WORKER_A,
			&final_attempt.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"observed": false}),
				outcome: ReconciliationOutcome::EffectAbsent,
			},
			Duration::from_millis(1),
			Duration::from_millis(1),
		)
		.await?;

	let state: String = client
		.query_one("SELECT state::text FROM decodex.outbox WHERE id = $1", &[&final_attempt.id])
		.await?
		.get(0);

	assert_eq!(state, "dead_letter");

	Ok(())
}

async fn assert_primary_indexes_are_plan_eligible(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	client
		.batch_execute("ANALYZE decodex.activity; ANALYZE decodex.outbox; SET enable_seqscan = off")
		.await?;

	let activity_plan = client
		.query(
			"EXPLAIN (COSTS OFF) SELECT sequence FROM decodex.activity \
			 WHERE aggregate_kind = 'account' AND aggregate_id = $1 \
			 ORDER BY sequence DESC LIMIT 50",
			&[&ACCOUNT_ID],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>()
		.join("\n");
	let outbox_plan = client
		.query(
			"EXPLAIN (COSTS OFF) SELECT id FROM decodex.outbox \
			 WHERE state IN ('pending', 'in_flight') AND available_at <= clock_timestamp() \
			 ORDER BY available_at, id LIMIT 100",
			&[],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>()
		.join("\n");

	client.batch_execute("RESET enable_seqscan").await?;

	assert!(activity_plan.contains("activity_timeline_idx"), "activity plan: {activity_plan}");
	assert!(outbox_plan.contains("outbox_claim_idx"), "outbox plan: {outbox_plan}");

	Ok(())
}
