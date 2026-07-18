//! Steady-state PostgreSQL authority verification for the retained runtime pool.

use deadpool_postgres::Client;
use sha2::{Digest as _, Sha256};

use crate::StoreError;

const FOUNDATION_MIGRATION: &str = include_str!("../migrations/V1__persistence_foundation.sql");
const CONVERSATION_MIGRATION: &str = include_str!("../migrations/V3__conversation_history.sql");
const PROJECT_AGENT_MIGRATION: &str = include_str!("../migrations/V5__project_agent_authority.sql");
const POLICY_MIGRATION: &str = include_str!("../migrations/V6__project_policy_authority.sql");
const PROGRAM_OBJECTIVE_MIGRATION: &str =
	include_str!("../migrations/V7__program_objective_authority.sql");
const QUOTA_MIGRATION: &str = include_str!("../migrations/V8__quota_exclusions.sql");
const ROLE_PROFILE_MIGRATION: &str = include_str!("../migrations/V9__exact_role_profiles.sql");
const RUNTIME_SESSION_MIGRATION: &str =
	include_str!("../migrations/V10__runtime_session_snapshots.sql");
const WORK_ITEM_MIGRATION: &str = include_str!("../migrations/V11__work_item_authority.sql");
const MANAGED_RUN_MIGRATION: &str = include_str!("../migrations/V12__managed_run_safety.sql");
const MANAGED_REPOSITORY_MIGRATION: &str =
	include_str!("../migrations/V13__managed_repository_authority.sql");
const ALLOWED_EXECUTION_DEPENDENCIES: [&str; 1] =
	["public.digest(pg_catalog.bytea,pg_catalog.text)"];
const FUNCTION_CONTRACTS: [FunctionContract; 111] = [
	FunctionContract {
		name: "is_canonical_media_type",
		lookup_signature: "decodex.is_canonical_media_type(pg_catalog.text)",
		migration_signature: "is_canonical_media_type(value text)",
		arguments: "value text",
		result: "boolean",
		language: "sql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "is_history_metadata_projection",
		lookup_signature: "decodex.is_history_metadata_projection(pg_catalog.jsonb)",
		migration_signature: "is_history_metadata_projection(document jsonb)",
		arguments: "document jsonb",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "normalize_unicode_whitespace",
		lookup_signature: "decodex.normalize_unicode_whitespace(pg_catalog.text)",
		migration_signature: "normalize_unicode_whitespace(value text)",
		arguments: "value text",
		result: "text",
		language: "sql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "ascii_lower",
		lookup_signature: "decodex.ascii_lower(pg_catalog.text)",
		migration_signature: "ascii_lower(value text)",
		arguments: "value text",
		result: "text",
		language: "sql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "has_credential_material",
		lookup_signature: "decodex.has_credential_material(pg_catalog.text)",
		migration_signature: "has_credential_material(value text)",
		arguments: "value text",
		result: "boolean",
		language: "sql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "has_credential_material",
		lookup_signature: "decodex.has_credential_material(pg_catalog.jsonb)",
		migration_signature: "has_credential_material(document jsonb)",
		arguments: "document jsonb",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "is_meaningful_evidence",
		lookup_signature: "decodex.is_meaningful_evidence(pg_catalog.jsonb)",
		migration_signature: "is_meaningful_evidence(document jsonb)",
		arguments: "document jsonb",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "rfc3339_utc",
		lookup_signature: "decodex.rfc3339_utc(pg_catalog.timestamptz)",
		migration_signature: "rfc3339_utc(value timestamptz)",
		arguments: "value timestamp with time zone",
		result: "text",
		language: "sql",
		volatility: "s",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "is_valid_operation_duration",
		lookup_signature: "decodex.is_valid_operation_duration(pg_catalog.interval)",
		migration_signature: "is_valid_operation_duration(value interval)",
		arguments: "value interval",
		result: "boolean",
		language: "sql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "enforce_lease_operation_time",
		lookup_signature: "decodex.enforce_lease_operation_time()",
		migration_signature: "enforce_lease_operation_time()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "enforce_outbox_operation_time",
		lookup_signature: "decodex.enforce_outbox_operation_time()",
		migration_signature: "enforce_outbox_operation_time()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "enforce_quota_observation_monotonicity",
		lookup_signature: "decodex.enforce_quota_observation_monotonicity()",
		migration_signature: "enforce_quota_observation_monotonicity()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "forbid_mutation_of_activity",
		lookup_signature: "decodex.forbid_mutation_of_activity()",
		migration_signature: "forbid_mutation_of_activity()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "enforce_outbox_terminal_retention",
		lookup_signature: "decodex.enforce_outbox_terminal_retention()",
		migration_signature: "enforce_outbox_terminal_retention()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "forbid_outbox_truncate",
		lookup_signature: "decodex.forbid_outbox_truncate()",
		migration_signature: "forbid_outbox_truncate()",
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "lease_ttl_milliseconds",
		lookup_signature: "decodex.lease_ttl_milliseconds(pg_catalog.interval)",
		migration_signature: "lease_ttl_milliseconds(value interval)",
		arguments: "value interval",
		result: "bigint",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "try_acquire_lease",
		lookup_signature: "decodex.try_acquire_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.interval)",
		migration_signature: "try_acquire_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_ttl interval\n)",
		arguments: "p_resource_key text, p_holder_id uuid, p_ttl interval",
		result: "TABLE(acquired boolean, lease_token uuid, revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "renew_lease",
		lookup_signature: "decodex.renew_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.interval)",
		migration_signature: "renew_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_lease_token uuid,\n\tp_ttl interval\n)",
		arguments: "p_resource_key text, p_holder_id uuid, p_lease_token uuid, p_ttl interval",
		result: "boolean",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "release_lease",
		lookup_signature: "decodex.release_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid)",
		migration_signature: "release_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_lease_token uuid\n)",
		arguments: "p_resource_key text, p_holder_id uuid, p_lease_token uuid",
		result: "boolean",
		language: "sql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "prune_history_snapshots",
		lookup_signature: "decodex.prune_history_snapshots()",
		migration_signature: "prune_history_snapshots()",
		arguments: "",
		result: "bigint",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "issue_history_cursor",
		lookup_signature: "decodex.issue_history_cursor(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int4)",
		migration_signature: "issue_history_cursor(\n\tp_conversation_id uuid,\n\tp_parent_cursor_id uuid,\n\tp_page_size integer\n)",
		arguments: "p_conversation_id uuid, p_parent_cursor_id uuid, p_page_size integer",
		result: "uuid",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	trigger_contract(
		"enforce_command_receipt_state",
		"decodex.enforce_command_receipt_state()",
		"enforce_command_receipt_state()",
	),
	trigger_contract(
		"acquire_hierarchy_coordinator",
		"decodex.acquire_hierarchy_coordinator()",
		"acquire_hierarchy_coordinator()",
	),
	trigger_contract(
		"canonicalize_created_at",
		"decodex.canonicalize_created_at()",
		"canonicalize_created_at()",
	),
	trigger_contract(
		"enforce_blob_object_state",
		"decodex.enforce_blob_object_state()",
		"enforce_blob_object_state()",
	),
	trigger_contract(
		"enforce_conversation_state",
		"decodex.enforce_conversation_state()",
		"enforce_conversation_state()",
	),
	trigger_contract(
		"enforce_runtime_session_state",
		"decodex.enforce_runtime_session_state()",
		"enforce_runtime_session_state()",
	),
	trigger_contract("enforce_turn_state", "decodex.enforce_turn_state()", "enforce_turn_state()"),
	trigger_contract(
		"enforce_history_item_state",
		"decodex.enforce_history_item_state()",
		"enforce_history_item_state()",
	),
	trigger_contract(
		"capture_history_item_version",
		"decodex.capture_history_item_version()",
		"capture_history_item_version()",
	),
	trigger_contract(
		"enforce_artifact_state",
		"decodex.enforce_artifact_state()",
		"enforce_artifact_state()",
	),
	trigger_contract(
		"enforce_artifact_revision_state",
		"decodex.enforce_artifact_revision_state()",
		"enforce_artifact_revision_state()",
	),
	trigger_contract(
		"enforce_context_pack_state",
		"decodex.enforce_context_pack_state()",
		"enforce_context_pack_state()",
	),
	trigger_contract(
		"enforce_context_pack_source_state",
		"decodex.enforce_context_pack_source_state()",
		"enforce_context_pack_source_state()",
	),
	trigger_contract(
		"enforce_history_cursor_state",
		"decodex.enforce_history_cursor_state()",
		"enforce_history_cursor_state()",
	),
	FunctionContract {
		name: "is_project_metadata",
		lookup_signature: "decodex.is_project_metadata(pg_catalog.jsonb)",
		migration_signature: "is_project_metadata(document jsonb)",
		arguments: "document jsonb",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	FunctionContract {
		name: "bootstrap_advisor",
		lookup_signature: "decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text)",
		migration_signature: "bootstrap_advisor(p_agent_id decodex.canonical_uuid_v4_text)",
		arguments: "p_agent_id decodex.canonical_uuid_v4_text",
		result: "TABLE(agent_id uuid, role decodex.agent_role, project_id uuid, status decodex.agent_status, revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "create_project",
		lookup_signature: "decodex.create_project(decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text)",
		migration_signature: "create_project(\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_repository_identity text,\n\tp_repository_root text,\n\tp_default_cwd text,\n\tp_metadata jsonb,\n\tp_lead_id decodex.canonical_uuid_v4_text\n)",
		arguments: "p_project_id decodex.canonical_uuid_v4_text, p_repository_identity text, p_repository_root text, p_default_cwd text, p_metadata jsonb, p_lead_id decodex.canonical_uuid_v4_text",
		result: "TABLE(project_id uuid, repository_identity text, repository_root text, default_cwd text, project_status decodex.project_status, metadata jsonb, project_revision bigint, agent_id uuid, agent_role decodex.agent_role, agent_status decodex.agent_status, agent_revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "transition_project",
		lookup_signature: "decodex.transition_project(decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.project_status)",
		migration_signature: "transition_project(\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_expected_revision bigint,\n\tp_status decodex.project_status\n)",
		arguments: "p_project_id decodex.canonical_uuid_v4_text, p_expected_revision bigint, p_status decodex.project_status",
		result: "TABLE(project_id uuid, repository_identity text, repository_root text, default_cwd text, project_status decodex.project_status, metadata jsonb, project_revision bigint, agent_id uuid, agent_role decodex.agent_role, agent_status decodex.agent_status, agent_revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "is_policy_snapshot",
		lookup_signature: "decodex.is_policy_snapshot(pg_catalog.jsonb)",
		migration_signature: "is_policy_snapshot(document jsonb)",
		arguments: "document jsonb",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	},
	trigger_contract(
		"enforce_policy_identity_state",
		"decodex.enforce_policy_identity_state()",
		"enforce_policy_identity_state()",
	),
	trigger_contract(
		"forbid_policy_revision_mutation",
		"decodex.forbid_policy_revision_mutation()",
		"forbid_policy_revision_mutation()",
	),
	FunctionContract {
		name: "create_policy",
		lookup_signature: "decodex.create_policy(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text)",
		migration_signature: "create_policy(\n\tp_policy_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text\n)",
		arguments: "p_policy_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text",
		result: "TABLE(policy_id uuid, project_id uuid, created_at timestamp with time zone, current_revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "accept_policy_revision",
		lookup_signature: "decodex.accept_policy_revision(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.int8)",
		migration_signature: "accept_policy_revision(\n\tp_policy_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_revision bigint,\n\tp_provenance text,\n\tp_snapshot jsonb,\n\tp_accepted_by decodex.canonical_uuid_v4_text,\n\tp_supersedes_revision bigint\n)",
		arguments: "p_policy_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_revision bigint, p_provenance text, p_snapshot jsonb, p_accepted_by decodex.canonical_uuid_v4_text, p_supersedes_revision bigint",
		result: "TABLE(policy_id uuid, project_id uuid, revision bigint, provenance text, snapshot jsonb, accepted_by uuid, policy_created_at timestamp with time zone, accepted_at timestamp with time zone, supersedes_revision bigint, revision_accepted boolean, actual_revision bigint)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	immutable_function_contract(
		"program_timestamp",
		"decodex.program_timestamp(pg_catalog.int8)",
		"program_timestamp(value bigint)",
		"value bigint",
		"timestamp with time zone",
		"sql",
	),
	immutable_function_contract(
		"is_program_metrics",
		"decodex.is_program_metrics(pg_catalog.jsonb)",
		"is_program_metrics(document jsonb)",
		"document jsonb",
		"boolean",
		"plpgsql",
	),
	immutable_function_contract(
		"is_program_signals",
		"decodex.is_program_signals(pg_catalog.jsonb)",
		"is_program_signals(document jsonb)",
		"document jsonb",
		"boolean",
		"plpgsql",
	),
	immutable_function_contract(
		"is_objective_criteria",
		"decodex.is_objective_criteria(pg_catalog._text)",
		"is_objective_criteria(document text[])",
		"document text[]",
		"boolean",
		"plpgsql",
	),
	trigger_contract(
		"enforce_program_state",
		"decodex.enforce_program_state()",
		"enforce_program_state()",
	),
	trigger_contract(
		"enforce_objective_state",
		"decodex.enforce_objective_state()",
		"enforce_objective_state()",
	),
	trigger_contract(
		"forbid_objective_evidence_mutation",
		"decodex.forbid_objective_evidence_mutation()",
		"forbid_objective_evidence_mutation()",
	),
	trigger_contract(
		"enforce_objective_completion_coherence",
		"decodex.enforce_objective_completion_coherence()",
		"enforce_objective_completion_coherence()",
	),
	mutator_contract(
		"create_program",
		"decodex.create_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.text)",
		"create_program(\n\tp_program_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_owner_agent_id decodex.canonical_uuid_v4_text,\n\tp_name text,\n\tp_responsibility text,\n\tp_policy_id decodex.canonical_uuid_v4_text,\n\tp_policy_revision bigint,\n\tp_review_interval_days integer,\n\tp_next_review_at bigint,\n\tp_metrics jsonb,\n\tp_signals jsonb,\n\tp_correlation_id decodex.canonical_uuid_v4_text,\n\tp_provenance text\n)",
		"p_program_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_owner_agent_id decodex.canonical_uuid_v4_text, p_name text, p_responsibility text, p_policy_id decodex.canonical_uuid_v4_text, p_policy_revision bigint, p_review_interval_days integer, p_next_review_at bigint, p_metrics jsonb, p_signals jsonb, p_correlation_id decodex.canonical_uuid_v4_text, p_provenance text",
	),
	mutator_contract(
		"update_program_context",
		"decodex.update_program_context(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
		"update_program_context(\n\tp_program_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_expected_revision bigint,\n\tp_review_interval_days integer,\n\tp_next_review_at bigint,\n\tp_metrics jsonb,\n\tp_signals jsonb,\n\tp_actor_id decodex.canonical_uuid_v4_text,\n\tp_correlation_id decodex.canonical_uuid_v4_text,\n\tp_provenance text\n)",
		"p_program_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_expected_revision bigint, p_review_interval_days integer, p_next_review_at bigint, p_metrics jsonb, p_signals jsonb, p_actor_id decodex.canonical_uuid_v4_text, p_correlation_id decodex.canonical_uuid_v4_text, p_provenance text",
	),
	mutator_contract(
		"transition_program",
		"decodex.transition_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.program_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
		"transition_program(\n\tp_program_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_expected_revision bigint,\n\tp_state decodex.program_state,\n\tp_actor_id decodex.canonical_uuid_v4_text,\n\tp_correlation_id decodex.canonical_uuid_v4_text,\n\tp_provenance text\n)",
		"p_program_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_expected_revision bigint, p_state decodex.program_state, p_actor_id decodex.canonical_uuid_v4_text, p_correlation_id decodex.canonical_uuid_v4_text, p_provenance text",
	),
	mutator_contract(
		"create_objective",
		"decodex.create_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog._text,pg_catalog._text,pg_catalog.int8,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
		"create_objective(\n\tp_objective_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_program_id decodex.canonical_uuid_v4_text,\n\tp_outcome text,\n\tp_acceptance_criteria text[],\n\tp_validation_criteria text[],\n\tp_target_at bigint,\n\tp_actor_id decodex.canonical_uuid_v4_text,\n\tp_correlation_id decodex.canonical_uuid_v4_text,\n\tp_provenance text\n)",
		"p_objective_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_program_id decodex.canonical_uuid_v4_text, p_outcome text, p_acceptance_criteria text[], p_validation_criteria text[], p_target_at bigint, p_actor_id decodex.canonical_uuid_v4_text, p_correlation_id decodex.canonical_uuid_v4_text, p_provenance text",
	),
	mutator_contract(
		"transition_objective",
		"decodex.transition_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.objective_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
		"transition_objective(\n\tp_objective_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_expected_revision bigint,\n\tp_state decodex.objective_state,\n\tp_actor_id decodex.canonical_uuid_v4_text,\n\tp_correlation_id decodex.canonical_uuid_v4_text,\n\tp_provenance text\n)",
		"p_objective_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_expected_revision bigint, p_state decodex.objective_state, p_actor_id decodex.canonical_uuid_v4_text, p_correlation_id decodex.canonical_uuid_v4_text, p_provenance text",
	),
	mutator_contract(
		"achieve_objective",
		"decodex.achieve_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text)",
		"achieve_objective(\n\tp_evidence_id decodex.canonical_uuid_v4_text,\n\tp_objective_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_objective_revision bigint,\n\tp_acceptance_result text,\n\tp_accepted_by decodex.canonical_uuid_v4_text,\n\tp_accepted_at bigint,\n\tp_acceptance_provenance text,\n\tp_validation_result text,\n\tp_validated_by decodex.canonical_uuid_v4_text,\n\tp_validated_at bigint,\n\tp_validation_provenance text,\n\tp_correlation_id decodex.canonical_uuid_v4_text\n)",
		"p_evidence_id decodex.canonical_uuid_v4_text, p_objective_id decodex.canonical_uuid_v4_text, p_project_id decodex.canonical_uuid_v4_text, p_objective_revision bigint, p_acceptance_result text, p_accepted_by decodex.canonical_uuid_v4_text, p_accepted_at bigint, p_acceptance_provenance text, p_validation_result text, p_validated_by decodex.canonical_uuid_v4_text, p_validated_at bigint, p_validation_provenance text, p_correlation_id decodex.canonical_uuid_v4_text",
	),
	trigger_contract(
		"enforce_exact_receipt_completion",
		"decodex.enforce_exact_receipt_completion()",
		"enforce_exact_receipt_completion()",
	),
	trigger_contract(
		"forbid_exact_receipt_rewrite",
		"decodex.forbid_exact_receipt_rewrite()",
		"forbid_exact_receipt_rewrite()",
	),
	trigger_contract(
		"forbid_exact_receipt_truncate",
		"decodex.forbid_exact_receipt_truncate()",
		"forbid_exact_receipt_truncate()",
	),
	trigger_contract(
		"enforce_complete_role_profile_set",
		"decodex.enforce_complete_role_profile_set()",
		"enforce_complete_role_profile_set()",
	),
	trigger_contract(
		"forbid_role_profile_identity_rewrite",
		"decodex.forbid_role_profile_identity_rewrite()",
		"forbid_role_profile_identity_rewrite()",
	),
	trigger_contract(
		"forbid_role_profile_revision_mutation",
		"decodex.forbid_role_profile_revision_mutation()",
		"forbid_role_profile_revision_mutation()",
	),
	trigger_contract(
		"forbid_role_profile_truncate",
		"decodex.forbid_role_profile_truncate()",
		"forbid_role_profile_truncate()",
	),
	trigger_contract(
		"enforce_role_profile_event_namespace",
		"decodex.enforce_role_profile_event_namespace()",
		"enforce_role_profile_event_namespace()",
	),
	exact_function_contract(
		"is_role_profile_configuration",
		"decodex.is_role_profile_configuration(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"is_role_profile_configuration(\n\tp_model text, p_reasoning_effort text, p_service_tier text,\n\tp_instructions text, p_provenance text\n)",
		"p_model text, p_reasoning_effort text, p_service_tier text, p_instructions text, p_provenance text",
		"boolean",
		"sql",
		"i",
	),
	exact_function_contract(
		"build_role_profile_bootstrap_request",
		"decodex.build_role_profile_bootstrap_request(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"build_role_profile_bootstrap_request(\n\tp_protocol text,\n\tp_advisor_model text, p_advisor_reasoning_effort text,\n\tp_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text,\n\tp_lead_model text, p_lead_reasoning_effort text,\n\tp_lead_service_tier text, p_lead_instructions text, p_lead_provenance text,\n\tp_task_model text, p_task_reasoning_effort text,\n\tp_task_service_tier text, p_task_instructions text, p_task_provenance text,\n\tp_reviewer_model text, p_reviewer_reasoning_effort text,\n\tp_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text\n)",
		"p_protocol text, p_advisor_model text, p_advisor_reasoning_effort text, p_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text, p_lead_model text, p_lead_reasoning_effort text, p_lead_service_tier text, p_lead_instructions text, p_lead_provenance text, p_task_model text, p_task_reasoning_effort text, p_task_service_tier text, p_task_instructions text, p_task_provenance text, p_reviewer_model text, p_reviewer_reasoning_effort text, p_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text",
		"jsonb",
		"plpgsql",
		"i",
	),
	exact_function_contract(
		"build_role_profile_update_request",
		"decodex.build_role_profile_update_request(pg_catalog.text,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"build_role_profile_update_request(\n\tp_protocol text, p_role decodex.role_profile_role, p_expected_revision bigint,\n\tp_model text, p_reasoning_effort text, p_service_tier text,\n\tp_instructions text, p_provenance text\n)",
		"p_protocol text, p_role decodex.role_profile_role, p_expected_revision bigint, p_model text, p_reasoning_effort text, p_service_tier text, p_instructions text, p_provenance text",
		"jsonb",
		"sql",
		"i",
	),
	exact_function_contract(
		"complete_exact_role_profile_rejection",
		"decodex.complete_exact_role_profile_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_role_profile_rejection(\n\tp_protocol text, p_idempotency_key text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"bootstrap_role_profiles_exact",
		"decodex.bootstrap_role_profiles_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"bootstrap_role_profiles_exact(\n\tp_protocol text, p_idempotency_key text,\n\tp_advisor_model text, p_advisor_reasoning_effort text,\n\tp_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text,\n\tp_lead_model text, p_lead_reasoning_effort text,\n\tp_lead_service_tier text, p_lead_instructions text, p_lead_provenance text,\n\tp_task_model text, p_task_reasoning_effort text,\n\tp_task_service_tier text, p_task_instructions text, p_task_provenance text,\n\tp_reviewer_model text, p_reviewer_reasoning_effort text,\n\tp_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_advisor_model text, p_advisor_reasoning_effort text, p_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text, p_lead_model text, p_lead_reasoning_effort text, p_lead_service_tier text, p_lead_instructions text, p_lead_provenance text, p_task_model text, p_task_reasoning_effort text, p_task_service_tier text, p_task_instructions text, p_task_provenance text, p_reviewer_model text, p_reviewer_reasoning_effort text, p_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"update_role_profile_exact",
		"decodex.update_role_profile_exact(pg_catalog.text,pg_catalog.text,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"update_role_profile_exact(\n\tp_protocol text, p_idempotency_key text,\n\tp_role decodex.role_profile_role, p_expected_revision bigint,\n\tp_model text, p_reasoning_effort text, p_service_tier text,\n\tp_instructions text, p_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_role decodex.role_profile_role, p_expected_revision bigint, p_model text, p_reasoning_effort text, p_service_tier text, p_instructions text, p_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	trigger_contract(
		"enforce_runtime_session_command_owner",
		"decodex.enforce_runtime_session_command_owner()",
		"enforce_runtime_session_command_owner()",
	),
	trigger_contract(
		"forbid_runtime_snapshot_mutation",
		"decodex.forbid_runtime_snapshot_mutation()",
		"forbid_runtime_snapshot_mutation()",
	),
	trigger_contract(
		"enforce_runtime_session_event_namespace",
		"decodex.enforce_runtime_session_event_namespace()",
		"enforce_runtime_session_event_namespace()",
	),
	exact_function_contract(
		"build_runtime_session_create_request",
		"decodex.build_runtime_session_create_request(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,decodex.role_profile_role,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex.account_state,pg_catalog.int8,pg_catalog.uuid,decodex.runtime_session_state)",
		"build_runtime_session_create_request(\n\tp_protocol text, p_session_id uuid, p_conversation_id uuid,\n\tp_role decodex.role_profile_role, p_account_snapshot_id uuid,\n\tp_source_account_id uuid, p_display_label text,\n\tp_observed_state decodex.account_state, p_account_source_revision bigint,\n\tp_codex_thread_id uuid, p_initial_state decodex.runtime_session_state\n)",
		"p_protocol text, p_session_id uuid, p_conversation_id uuid, p_role decodex.role_profile_role, p_account_snapshot_id uuid, p_source_account_id uuid, p_display_label text, p_observed_state decodex.account_state, p_account_source_revision bigint, p_codex_thread_id uuid, p_initial_state decodex.runtime_session_state",
		"jsonb",
		"sql",
		"i",
	),
	exact_function_contract(
		"build_runtime_session_transition_request",
		"decodex.build_runtime_session_transition_request(pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.runtime_session_state)",
		"build_runtime_session_transition_request(\n\tp_protocol text, p_session_id uuid, p_expected_revision bigint,\n\tp_target_state decodex.runtime_session_state\n)",
		"p_protocol text, p_session_id uuid, p_expected_revision bigint, p_target_state decodex.runtime_session_state",
		"jsonb",
		"sql",
		"i",
	),
	exact_function_contract(
		"complete_exact_runtime_session_rejection",
		"decodex.complete_exact_runtime_session_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_runtime_session_rejection(\n\tp_protocol text, p_idempotency_key text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"create_runtime_session_exact",
		"decodex.create_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,decodex.role_profile_role,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex.account_state,pg_catalog.int8,pg_catalog.uuid,decodex.runtime_session_state)",
		"create_runtime_session_exact(\n\tp_protocol text, p_idempotency_key text,\n\tp_session_id uuid, p_conversation_id uuid, p_role decodex.role_profile_role,\n\tp_account_snapshot_id uuid, p_source_account_id uuid, p_display_label text,\n\tp_observed_state decodex.account_state, p_account_source_revision bigint,\n\tp_codex_thread_id uuid, p_initial_state decodex.runtime_session_state\n)",
		"p_protocol text, p_idempotency_key text, p_session_id uuid, p_conversation_id uuid, p_role decodex.role_profile_role, p_account_snapshot_id uuid, p_source_account_id uuid, p_display_label text, p_observed_state decodex.account_state, p_account_source_revision bigint, p_codex_thread_id uuid, p_initial_state decodex.runtime_session_state",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"transition_runtime_session_exact",
		"decodex.transition_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.runtime_session_state)",
		"transition_runtime_session_exact(\n\tp_protocol text, p_idempotency_key text, p_session_id uuid,\n\tp_expected_revision bigint, p_target_state decodex.runtime_session_state\n)",
		"p_protocol text, p_idempotency_key text, p_session_id uuid, p_expected_revision bigint, p_target_state decodex.runtime_session_state",
		"bytea",
		"plpgsql",
		"v",
	),
	immutable_function_contract(
		"is_work_item_text",
		"decodex.is_work_item_text(pg_catalog.text,pg_catalog.int4)",
		"is_work_item_text(value text, maximum_bytes integer)",
		"value text, maximum_bytes integer",
		"boolean",
		"sql",
	),
	immutable_function_contract(
		"is_work_item_criteria",
		"decodex.is_work_item_criteria(pg_catalog._text)",
		"is_work_item_criteria(document text[])",
		"document text[]",
		"boolean",
		"plpgsql",
	),
	trigger_contract(
		"enforce_work_item_state",
		"decodex.enforce_work_item_state()",
		"enforce_work_item_state()",
	),
	trigger_contract(
		"enforce_work_item_command_owner",
		"decodex.enforce_work_item_command_owner()",
		"enforce_work_item_command_owner()",
	),
	trigger_contract(
		"forbid_work_item_acceptance_mutation",
		"decodex.forbid_work_item_acceptance_mutation()",
		"forbid_work_item_acceptance_mutation()",
	),
	trigger_contract(
		"enforce_work_item_acceptance_coherence",
		"decodex.enforce_work_item_acceptance_coherence()",
		"enforce_work_item_acceptance_coherence()",
	),
	trigger_contract(
		"enforce_work_item_event_namespace",
		"decodex.enforce_work_item_event_namespace()",
		"enforce_work_item_event_namespace()",
	),
	exact_function_contract(
		"work_item_document",
		"decodex.work_item_document(pg_catalog.uuid)",
		"work_item_document(p_work_item_id uuid)",
		"p_work_item_id uuid",
		"jsonb",
		"sql",
		"s",
	),
	exact_function_contract(
		"complete_exact_work_item_rejection",
		"decodex.complete_exact_work_item_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_work_item_rejection(\n\tp_protocol text, p_idempotency_key text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"complete_exact_work_item_success",
		"decodex.complete_exact_work_item_success(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.jsonb)",
		"complete_exact_work_item_success(\n\tp_protocol text, p_idempotency_key text, p_event_kind text,\n\tp_work_item_id uuid, p_effect jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_event_kind text, p_work_item_id uuid, p_effect jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"reserve_exact_work_item_command",
		"decodex.reserve_exact_work_item_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_work_item_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"work_item_graph_cycle",
		"decodex.work_item_graph_cycle(pg_catalog.uuid)",
		"work_item_graph_cycle(p_project_id uuid)",
		"p_project_id uuid",
		"boolean",
		"sql",
		"s",
	),
	exact_function_contract(
		"work_item_readiness",
		"decodex.work_item_readiness(pg_catalog.uuid)",
		"work_item_readiness(p_work_item_id uuid)",
		"p_work_item_id uuid",
		"jsonb",
		"plpgsql",
		"s",
	),
	exact_function_contract(
		"create_work_item_exact",
		"decodex.create_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
		"create_work_item_exact(\n\tp_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,\n\tp_lead_agent_id uuid, p_program_id uuid, p_objective_ids uuid[],\n\tp_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text,\n\tp_priority decodex.work_item_priority, p_acceptance_criteria text[],\n\tp_validation_criteria text[], p_actor_id uuid, p_correlation_id uuid, p_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid, p_lead_agent_id uuid, p_program_id uuid, p_objective_ids uuid[], p_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text, p_priority decodex.work_item_priority, p_acceptance_criteria text[], p_validation_criteria text[], p_actor_id uuid, p_correlation_id uuid, p_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"update_work_item_exact",
		"decodex.update_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,decodex.work_item_state,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
		"update_work_item_exact(\n\tp_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,\n\tp_expected_revision bigint, p_program_id uuid, p_objective_ids uuid[],\n\tp_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text,\n\tp_priority decodex.work_item_priority, p_acceptance_criteria text[],\n\tp_validation_criteria text[], p_target_state decodex.work_item_state,\n\tp_actor_id uuid, p_correlation_id uuid, p_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint, p_program_id uuid, p_objective_ids uuid[], p_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text, p_priority decodex.work_item_priority, p_acceptance_criteria text[], p_validation_criteria text[], p_target_state decodex.work_item_state, p_actor_id uuid, p_correlation_id uuid, p_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"assess_work_item_readiness_exact",
		"decodex.assess_work_item_readiness_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
		"assess_work_item_readiness_exact(\n\tp_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,\n\tp_expected_revision bigint, p_actor_id uuid, p_correlation_id uuid, p_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint, p_actor_id uuid, p_correlation_id uuid, p_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"accept_work_item_exact",
		"decodex.accept_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"accept_work_item_exact(\n\tp_protocol text, p_idempotency_key text, p_acceptance_id uuid,\n\tp_work_item_id uuid, p_project_id uuid, p_expected_revision bigint,\n\tp_actor_id uuid, p_correlation_id uuid, p_provenance text, p_criteria_provenance text,\n\tp_evidence_summary text, p_evidence_provenance text\n)",
		"p_protocol text, p_idempotency_key text, p_acceptance_id uuid, p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint, p_actor_id uuid, p_correlation_id uuid, p_provenance text, p_criteria_provenance text, p_evidence_summary text, p_evidence_provenance text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"guard_work_item_running_resume",
		"decodex.guard_work_item_running_resume(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
		"guard_work_item_running_resume(\n\tp_work_item_id uuid, p_project_id uuid, p_expected_revision bigint\n)",
		"p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint",
		"void",
		"plpgsql",
		"v",
	),
	trigger_contract(
		"enforce_managed_run_command_owner",
		"decodex.enforce_managed_run_command_owner()",
		"enforce_managed_run_command_owner()",
	),
	trigger_contract(
		"forbid_managed_run_immutable_mutation",
		"decodex.forbid_managed_run_immutable_mutation()",
		"forbid_managed_run_immutable_mutation()",
	),
	trigger_contract(
		"enforce_managed_run_assignment_scope",
		"decodex.enforce_managed_run_assignment_scope()",
		"enforce_managed_run_assignment_scope()",
	),
	trigger_contract(
		"enforce_managed_run_state",
		"decodex.enforce_managed_run_state()",
		"enforce_managed_run_state()",
	),
	trigger_contract(
		"enforce_effect_barrier_state",
		"decodex.enforce_effect_barrier_state()",
		"enforce_effect_barrier_state()",
	),
	trigger_contract(
		"enforce_managed_run_event_namespace",
		"decodex.enforce_managed_run_event_namespace()",
		"enforce_managed_run_event_namespace()",
	),
	exact_function_contract(
		"reserve_exact_managed_run_safety_command",
		"decodex.reserve_exact_managed_run_safety_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_managed_run_safety_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"complete_exact_managed_run_safety_rejection",
		"decodex.complete_exact_managed_run_safety_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"complete_exact_managed_run_safety_rejection(\n\tp_protocol text, p_idempotency_key text, p_reason text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_reason text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"apply_managed_run_safety_input_exact",
		"decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"apply_managed_run_safety_input_exact(\n\tp_protocol text, p_idempotency_key text, p_managed_run_id uuid, p_project_id uuid,\n\tp_expected_run_revision bigint, p_input_kind decodex.managed_run_safety_input_kind,\n\tp_input_id uuid, p_runtime_session_id uuid, p_turn_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_managed_run_id uuid, p_project_id uuid, p_expected_run_revision bigint, p_input_kind decodex.managed_run_safety_input_kind, p_input_id uuid, p_runtime_session_id uuid, p_turn_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	trigger_contract(
		"forbid_managed_repository_history_mutation",
		"decodex.forbid_managed_repository_history_mutation()",
		"forbid_managed_repository_history_mutation()",
	),
	trigger_contract(
		"enforce_managed_repository_projection",
		"decodex.enforce_managed_repository_projection()",
		"enforce_managed_repository_projection()",
	),
	trigger_contract(
		"enforce_repository_operation_scope",
		"decodex.enforce_repository_operation_scope()",
		"enforce_repository_operation_scope()",
	),
	trigger_contract(
		"enforce_repository_history_completeness",
		"decodex.enforce_repository_history_completeness()",
		"enforce_repository_history_completeness()",
	),
];
const RUNTIME_EXECUTE_FUNCTIONS: [&str; 36] = [
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
	"decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
];
const SAFETY_FUNCTIONS: [&str; 52] = [
	"enforce_lease_operation_time",
	"enforce_outbox_operation_time",
	"enforce_quota_observation_monotonicity",
	"forbid_mutation_of_activity",
	"enforce_outbox_terminal_retention",
	"forbid_outbox_truncate",
	"enforce_command_receipt_state",
	"acquire_hierarchy_coordinator",
	"canonicalize_created_at",
	"enforce_blob_object_state",
	"enforce_conversation_state",
	"enforce_runtime_session_state",
	"enforce_turn_state",
	"enforce_history_item_state",
	"capture_history_item_version",
	"enforce_artifact_state",
	"enforce_artifact_revision_state",
	"enforce_context_pack_state",
	"enforce_context_pack_source_state",
	"enforce_history_cursor_state",
	"enforce_policy_identity_state",
	"forbid_policy_revision_mutation",
	"enforce_program_state",
	"enforce_objective_state",
	"forbid_objective_evidence_mutation",
	"enforce_objective_completion_coherence",
	"enforce_exact_receipt_completion",
	"forbid_exact_receipt_rewrite",
	"forbid_exact_receipt_truncate",
	"enforce_complete_role_profile_set",
	"forbid_role_profile_identity_rewrite",
	"forbid_role_profile_revision_mutation",
	"forbid_role_profile_truncate",
	"enforce_role_profile_event_namespace",
	"enforce_runtime_session_command_owner",
	"forbid_runtime_snapshot_mutation",
	"enforce_runtime_session_event_namespace",
	"enforce_work_item_state",
	"enforce_work_item_command_owner",
	"forbid_work_item_acceptance_mutation",
	"enforce_work_item_acceptance_coherence",
	"enforce_work_item_event_namespace",
	"enforce_managed_run_command_owner",
	"forbid_managed_run_immutable_mutation",
	"enforce_managed_run_assignment_scope",
	"enforce_managed_run_state",
	"enforce_effect_barrier_state",
	"enforce_managed_run_event_namespace",
	"forbid_managed_repository_history_mutation",
	"enforce_managed_repository_projection",
	"enforce_repository_operation_scope",
	"enforce_repository_history_completeness",
];
const SAFETY_TRIGGER_COUNT: usize = 96;
// PostgreSQL 18 catalogs with an owner and a containing namespace, plus the namespace
// itself. Namespace-scoped catalogs without an independent owner (constraints, triggers,
// text-search parsers/templates, and dependent rows) inherit authority from one of these.
#[cfg(test)]
const OWNED_OBJECT_CATALOGS: [(&str, &str); 12] = [
	("pg_namespace", "SELECT 'schema', namespace.nspowner"),
	("pg_class", "FROM pg_catalog.pg_class AS class"),
	("pg_proc", "SELECT 'function', proc.proowner FROM decodex_functions AS proc"),
	("pg_type", "FROM pg_catalog.pg_type AS owned_type"),
	("pg_collation", "FROM pg_catalog.pg_collation AS owned_collation"),
	("pg_conversion", "FROM pg_catalog.pg_conversion AS owned_conversion"),
	("pg_operator", "FROM pg_catalog.pg_operator AS owned_operator"),
	("pg_opclass", "FROM pg_catalog.pg_opclass AS operator_class"),
	("pg_opfamily", "FROM pg_catalog.pg_opfamily AS operator_family"),
	("pg_statistic_ext", "FROM pg_catalog.pg_statistic_ext AS statistics"),
	("pg_ts_config", "FROM pg_catalog.pg_ts_config AS configuration"),
	("pg_ts_dict", "FROM pg_catalog.pg_ts_dict AS dictionary"),
];
const ROLE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.*
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
     OR pg_catalog.pg_has_role(session_user, role.oid, 'SET')
), effective_roles AS (
  SELECT DISTINCT inherited.oid
  FROM set_roles AS active
  JOIN pg_catalog.pg_roles AS inherited
    ON inherited.oid = active.oid
    OR pg_catalog.pg_has_role(active.oid, inherited.oid, 'USAGE')
), decodex_namespace AS (
  SELECT namespace.oid, namespace.nspowner
  FROM pg_catalog.pg_namespace AS namespace
  WHERE namespace.nspname = 'decodex'
), decodex_functions AS (
  SELECT proc.oid, proc.proowner
  FROM pg_catalog.pg_proc AS proc
  JOIN decodex_namespace AS namespace ON namespace.oid = proc.pronamespace
), decodex_owned_objects(object_class, owner_oid) AS (
  SELECT 'schema', namespace.nspowner FROM decodex_namespace AS namespace
  UNION ALL
  SELECT 'relation', class.relowner
  FROM pg_catalog.pg_class AS class
  JOIN decodex_namespace AS namespace ON namespace.oid = class.relnamespace
  UNION ALL
  SELECT 'function', proc.proowner FROM decodex_functions AS proc
  UNION ALL
  SELECT 'type', owned_type.typowner
  FROM pg_catalog.pg_type AS owned_type
  JOIN decodex_namespace AS namespace ON namespace.oid = owned_type.typnamespace
  UNION ALL
  SELECT 'collation', owned_collation.collowner
  FROM pg_catalog.pg_collation AS owned_collation
  JOIN decodex_namespace AS namespace ON namespace.oid = owned_collation.collnamespace
  UNION ALL
  SELECT 'conversion', owned_conversion.conowner
  FROM pg_catalog.pg_conversion AS owned_conversion
  JOIN decodex_namespace AS namespace ON namespace.oid = owned_conversion.connamespace
  UNION ALL
  SELECT 'operator', owned_operator.oprowner
  FROM pg_catalog.pg_operator AS owned_operator
  JOIN decodex_namespace AS namespace ON namespace.oid = owned_operator.oprnamespace
  UNION ALL
  SELECT 'operator class', operator_class.opcowner
  FROM pg_catalog.pg_opclass AS operator_class
  JOIN decodex_namespace AS namespace ON namespace.oid = operator_class.opcnamespace
  UNION ALL
  SELECT 'operator family', operator_family.opfowner
  FROM pg_catalog.pg_opfamily AS operator_family
  JOIN decodex_namespace AS namespace ON namespace.oid = operator_family.opfnamespace
  UNION ALL
  SELECT 'statistics', statistics.stxowner
  FROM pg_catalog.pg_statistic_ext AS statistics
  JOIN decodex_namespace AS namespace ON namespace.oid = statistics.stxnamespace
  UNION ALL
  SELECT 'text search configuration', configuration.cfgowner
  FROM pg_catalog.pg_ts_config AS configuration
  JOIN decodex_namespace AS namespace ON namespace.oid = configuration.cfgnamespace
  UNION ALL
  SELECT 'text search dictionary', dictionary.dictowner
  FROM pg_catalog.pg_ts_dict AS dictionary
  JOIN decodex_namespace AS namespace ON namespace.oid = dictionary.dictnamespace
)
SELECT
  EXISTS (
    SELECT 1 FROM set_roles
    WHERE rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls
  ),
  EXISTS (
    SELECT 1 FROM set_roles
    WHERE pg_catalog.has_database_privilege(oid, pg_catalog.current_database(), 'CREATE')
  ),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    JOIN pg_catalog.pg_namespace AS namespace
      ON namespace.nspname !~ '^pg_'
     AND namespace.nspname <> 'information_schema'
     AND pg_catalog.has_schema_privilege(role.oid, namespace.oid, 'CREATE')
  ),
  EXISTS (
    SELECT 1
    FROM effective_roles AS role
    JOIN decodex_owned_objects AS object ON object.owner_oid = role.oid
  ),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    JOIN decodex_functions AS function
      ON pg_catalog.has_function_privilege(
        role.oid,
        function.oid,
        'EXECUTE WITH GRANT OPTION'
      )
  ),
  EXISTS (
    SELECT 1 FROM set_roles
    WHERE pg_catalog.has_parameter_privilege(
      oid,
      'session_replication_role',
      'SET'
    )
  ),
  EXISTS (
    SELECT 1 FROM set_roles
    WHERE pg_catalog.has_parameter_privilege(
      oid,
      'session_replication_role',
      'ALTER SYSTEM'
    )
  ),
  pg_catalog.current_setting('session_replication_role') <> 'origin',
  EXISTS (
    SELECT 1
    FROM set_roles AS active
    JOIN pg_catalog.pg_roles AS target
      ON target.oid <> active.oid
     AND pg_catalog.pg_has_role(
       active.oid,
       target.oid,
       'MEMBER WITH ADMIN OPTION'
     )
  )
"#;
const TABLE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
     OR pg_catalog.pg_has_role(session_user, role.oid, 'SET')
), expected(table_name, can_select, can_insert, can_update, can_delete) AS (VALUES
  ('accounts', true, true, true, false),
  ('quota_windows', true, true, true, false),
  ('quota_exclusions', true, true, false, false),
  ('command_receipts', true, true, true, false),
  ('activity', true, true, false, false),
  ('leases', true, true, true, false),
  ('outbox', true, true, true, true),
  ('conversations', true, true, true, false),
	  ('profile_snapshots', true, false, false, false),
	  ('account_snapshots', true, false, false, false),
	  ('runtime_sessions', true, false, false, false),
  ('blob_objects', true, true, false, true),
  ('artifacts', true, true, true, false),
  ('artifact_revisions', true, true, false, false),
  ('turns', true, true, true, false),
  ('history_items', true, true, true, false),
  ('history_item_versions', true, false, false, false),
  ('history_cursors', true, false, false, false),
  ('context_packs', true, true, false, false),
  ('context_pack_sources', true, true, false, false),
  ('transition_proposals', true, true, false, false),
  ('projects', true, false, false, false),
  ('agents', true, false, false, false),
  ('policies', true, false, false, false),
  ('policy_revisions', true, false, false, false),
  ('programs', true, false, false, false),
  ('objectives', true, false, false, false),
  ('objective_completion_evidence', true, false, false, false),
  ('exact_command_receipts', false, false, false, false),
  ('role_profiles', false, false, false, false),
  ('role_profile_revisions', false, false, false, false),
  ('work_items', true, false, false, false),
  ('work_item_objectives', true, false, false, false),
  ('work_item_edges', true, false, false, false),
  ('work_item_readiness_blockers', true, false, false, false),
  ('work_item_acceptances', true, false, false, false)
  ,('managed_runs', true, false, false, false)
  ,('managed_run_assignments', true, false, false, false)
  ,('managed_run_effect_barriers', true, false, false, false)
  ,('managed_run_effects', true, false, false, false)
  ,('managed_run_submitted_turn_receipts', true, false, false, false)
  ,('managed_run_safety_inputs', true, false, false, false)
	,('repository_admissions', true, true, false, false)
	,('managed_repositories', true, true, true, false)
	,('repository_authority_transitions', true, true, false, false)
	,('repository_operations', true, true, false, false)
	,('repository_operation_events', true, true, false, false)
	,('repository_operation_evidence', true, true, false, false)
	,('repository_operation_results', true, true, false, false)
), tables AS (
  SELECT class.oid, class.relname, expected.*
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN expected ON expected.table_name = class.relname
  WHERE namespace.nspname = 'decodex' AND class.relkind IN ('r', 'p')
)
SELECT
  (SELECT count(*) FROM tables WHERE table_name IS NOT NULL) = 49
    AND COALESCE((
      SELECT pg_catalog.bool_and(
        pg_catalog.has_table_privilege(session_user, oid, 'SELECT') = can_select
        AND pg_catalog.has_table_privilege(session_user, oid, 'INSERT') = can_insert
        AND pg_catalog.has_table_privilege(session_user, oid, 'UPDATE') = can_update
        AND pg_catalog.has_table_privilege(session_user, oid, 'DELETE') = can_delete
      )
      FROM tables WHERE table_name IS NOT NULL
    ), false),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN tables
    WHERE
      pg_catalog.has_table_privilege(role.oid, tables.oid, 'TRUNCATE')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'TRIGGER')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'REFERENCES')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'MAINTAIN')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'SELECT WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'INSERT WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'UPDATE WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, tables.oid, 'DELETE WITH GRANT OPTION')
      OR pg_catalog.has_any_column_privilege(
        role.oid,
        tables.oid,
        'SELECT WITH GRANT OPTION, INSERT WITH GRANT OPTION, UPDATE WITH GRANT OPTION, REFERENCES WITH GRANT OPTION'
      )
      OR (tables.table_name IS NULL AND (
        pg_catalog.has_table_privilege(role.oid, tables.oid, 'SELECT, INSERT, UPDATE, DELETE')
        OR pg_catalog.has_any_column_privilege(role.oid, tables.oid, 'SELECT, INSERT, UPDATE, REFERENCES')
      ))
      OR (NOT tables.can_select AND (
        pg_catalog.has_table_privilege(role.oid, tables.oid, 'SELECT')
        OR pg_catalog.has_any_column_privilege(role.oid, tables.oid, 'SELECT')
      ))
      OR (NOT tables.can_insert AND (
        pg_catalog.has_table_privilege(role.oid, tables.oid, 'INSERT')
        OR pg_catalog.has_any_column_privilege(role.oid, tables.oid, 'INSERT')
      ))
      OR (NOT tables.can_update AND (
        pg_catalog.has_table_privilege(role.oid, tables.oid, 'UPDATE')
        OR pg_catalog.has_any_column_privilege(role.oid, tables.oid, 'UPDATE')
      ))
      OR (NOT tables.can_delete AND pg_catalog.has_table_privilege(role.oid, tables.oid, 'DELETE'))
  )
"#;
const MIGRATION_HISTORY_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
     OR pg_catalog.pg_has_role(session_user, role.oid, 'SET')
), effective_roles AS (
  SELECT DISTINCT inherited.oid
  FROM set_roles AS active
  JOIN pg_catalog.pg_roles AS inherited
    ON inherited.oid = active.oid
    OR pg_catalog.pg_has_role(active.oid, inherited.oid, 'USAGE')
), history AS (
  SELECT class.oid, class.relowner
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE namespace.nspname = 'public'
    AND class.relname = 'refinery_schema_history'
    AND class.relkind IN ('r', 'p')
)
SELECT
  (SELECT count(*) FROM history) = 1,
  COALESCE((
    SELECT pg_catalog.has_table_privilege(session_user, oid, 'SELECT') FROM history
  ), false),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN history
    WHERE
      pg_catalog.has_table_privilege(role.oid, history.oid, 'SELECT WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, history.oid, 'INSERT, UPDATE, DELETE')
      OR pg_catalog.has_table_privilege(role.oid, history.oid, 'TRUNCATE')
      OR pg_catalog.has_table_privilege(role.oid, history.oid, 'REFERENCES')
      OR pg_catalog.has_table_privilege(role.oid, history.oid, 'TRIGGER')
      OR pg_catalog.has_table_privilege(role.oid, history.oid, 'MAINTAIN')
      OR pg_catalog.has_any_column_privilege(
        role.oid,
        history.oid,
        'SELECT WITH GRANT OPTION, INSERT, UPDATE, REFERENCES'
      )
  ) OR EXISTS (
    SELECT 1
    FROM effective_roles AS role
    JOIN history ON history.relowner = role.oid
  )
"#;
const SEQUENCE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
     OR pg_catalog.pg_has_role(session_user, role.oid, 'SET')
), expected(table_name, column_name, required_usage) AS (VALUES
  ('activity', 'sequence', true),
  ('outbox', 'id', true),
  ('history_item_versions', 'version_sequence', false)
), expected_sequences AS (
  SELECT
    expected.*,
    pg_catalog.pg_get_serial_sequence(
      pg_catalog.format('decodex.%I', expected.table_name),
      expected.column_name
    )::pg_catalog.regclass::pg_catalog.oid AS oid
  FROM expected
), actual_sequences AS (
  SELECT class.oid
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE namespace.nspname = 'decodex' AND class.relkind = 'S'
)
SELECT
  (SELECT count(*) FROM actual_sequences) = 3
    AND (SELECT count(*) FROM expected_sequences WHERE oid IS NOT NULL) = 3
    AND NOT EXISTS (
      SELECT 1 FROM actual_sequences
      WHERE oid NOT IN (SELECT oid FROM expected_sequences)
    ),
  COALESCE((
    SELECT pg_catalog.bool_and(
      pg_catalog.has_sequence_privilege(session_user, oid, 'USAGE') = required_usage
    ) FROM expected_sequences
  ), false),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN actual_sequences AS sequence
    WHERE
      pg_catalog.has_sequence_privilege(role.oid, sequence.oid, 'SELECT')
      OR pg_catalog.has_sequence_privilege(role.oid, sequence.oid, 'UPDATE')
      OR pg_catalog.has_sequence_privilege(
        role.oid,
        sequence.oid,
        'USAGE WITH GRANT OPTION'
      )
      OR pg_catalog.has_sequence_privilege(
        role.oid,
        sequence.oid,
        'SELECT WITH GRANT OPTION'
      )
      OR pg_catalog.has_sequence_privilege(
        role.oid,
        sequence.oid,
        'UPDATE WITH GRANT OPTION'
      )
  )
"#;
const TRIGGER_CONTRACT_SQL: &str = r#"
WITH expected(table_name, trigger_name, function_name, trigger_type) AS (VALUES
  ('leases', 'leases_operation_time', 'enforce_lease_operation_time', 23),
  ('outbox', 'outbox_operation_time', 'enforce_outbox_operation_time', 23),
	('quota_windows', 'quota_windows_observed_at_monotonic', 'enforce_quota_observation_monotonicity', 19),
  ('activity', 'activity_append_only', 'forbid_mutation_of_activity', 27),
  ('outbox', 'outbox_terminal_retention', 'enforce_outbox_terminal_retention', 27),
  ('outbox', 'outbox_truncate_forbidden', 'forbid_outbox_truncate', 34),
  ('command_receipts', 'command_receipts_state_guard', 'enforce_command_receipt_state', 31),
  ('conversations', 'conversations_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('conversations', 'conversations_state_guard', 'enforce_conversation_state', 23),
  ('profile_snapshots', 'profile_snapshots_created_at_guard', 'canonicalize_created_at', 7),
  ('account_snapshots', 'account_snapshots_created_at_guard', 'canonicalize_created_at', 7),
  ('runtime_sessions', 'runtime_sessions_state_guard', 'enforce_runtime_session_state', 23),
  ('runtime_sessions', 'runtime_sessions_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('blob_objects', 'blob_objects_state_guard', 'enforce_blob_object_state', 7),
  ('turns', 'turns_state_guard', 'enforce_turn_state', 23),
  ('turns', 'turns_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('history_items', 'history_items_state_guard', 'enforce_history_item_state', 23),
  ('history_items', 'history_items_version_capture', 'capture_history_item_version', 21),
  ('history_items', 'history_items_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('history_cursors', 'history_cursors_state_guard', 'enforce_history_cursor_state', 7),
  ('artifacts', 'artifacts_state_guard', 'enforce_artifact_state', 23),
  ('artifacts', 'artifacts_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('artifact_revisions', 'artifact_revisions_state_guard', 'enforce_artifact_revision_state', 7),
  ('artifact_revisions', 'artifact_revisions_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('context_packs', 'context_packs_state_guard', 'enforce_context_pack_state', 31),
  ('context_packs', 'context_packs_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('context_pack_sources', 'context_pack_sources_state_guard', 'enforce_context_pack_source_state', 31),
  ('context_pack_sources', 'context_pack_sources_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('transition_proposals', 'transition_proposals_created_at_guard', 'canonicalize_created_at', 7),
  ('transition_proposals', 'transition_proposals_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('policies', 'policies_state_guard', 'enforce_policy_identity_state', 27),
  ('policies', 'policies_truncate_forbidden', 'enforce_policy_identity_state', 34),
  ('policy_revisions', 'policy_revisions_immutable', 'forbid_policy_revision_mutation', 27),
  ('policy_revisions', 'policy_revisions_truncate_forbidden', 'forbid_policy_revision_mutation', 34),
  ('programs', 'programs_state_guard', 'enforce_program_state', 31),
  ('programs', 'programs_truncate_forbidden', 'enforce_program_state', 34),
  ('objectives', 'objectives_state_guard', 'enforce_objective_state', 31),
  ('objectives', 'objectives_truncate_forbidden', 'enforce_objective_state', 34),
  ('objective_completion_evidence', 'objective_evidence_immutable', 'forbid_objective_evidence_mutation', 27),
  ('objective_completion_evidence', 'objective_evidence_truncate_forbidden', 'forbid_objective_evidence_mutation', 34),
  ('objectives', 'objectives_completion_coherence', 'enforce_objective_completion_coherence', 21),
  ('objective_completion_evidence', 'objective_evidence_completion_coherence', 'enforce_objective_completion_coherence', 5),
  ('exact_command_receipts', 'exact_receipts_complete_at_commit', 'enforce_exact_receipt_completion', 21),
  ('exact_command_receipts', 'exact_receipts_immutable', 'forbid_exact_receipt_rewrite', 27),
  ('exact_command_receipts', 'exact_receipts_untruncatable', 'forbid_exact_receipt_truncate', 34),
  ('role_profiles', 'role_profiles_exact_global_set', 'enforce_complete_role_profile_set', 29),
  ('role_profiles', 'role_profiles_identity_immutable', 'forbid_role_profile_identity_rewrite', 27),
  ('role_profile_revisions', 'role_profile_revisions_immutable', 'forbid_role_profile_revision_mutation', 27),
  ('role_profiles', 'role_profiles_untruncatable', 'forbid_role_profile_truncate', 34),
  ('role_profile_revisions', 'role_profile_revisions_untruncatable', 'forbid_role_profile_truncate', 34),
	  ('activity', 'activity_role_profile_namespace', 'enforce_role_profile_event_namespace', 23),
	  ('outbox', 'outbox_role_profile_namespace', 'enforce_role_profile_event_namespace', 23),
	  ('profile_snapshots', 'profile_snapshots_command_owner', 'enforce_runtime_session_command_owner', 62),
	  ('account_snapshots', 'account_snapshots_command_owner', 'enforce_runtime_session_command_owner', 62),
	  ('runtime_sessions', 'runtime_sessions_command_owner', 'enforce_runtime_session_command_owner', 62),
	  ('profile_snapshots', 'profile_snapshots_immutable', 'forbid_runtime_snapshot_mutation', 27),
	  ('account_snapshots', 'account_snapshots_immutable', 'forbid_runtime_snapshot_mutation', 27),
	  ('activity', 'activity_runtime_session_namespace', 'enforce_runtime_session_event_namespace', 23),
	  ('outbox', 'outbox_runtime_session_namespace', 'enforce_runtime_session_event_namespace', 23)
	,('work_items', 'work_items_state_guard', 'enforce_work_item_state', 31)
	,('work_items', 'work_items_command_owner', 'enforce_work_item_command_owner', 62)
	,('work_item_objectives', 'work_item_objectives_command_owner', 'enforce_work_item_command_owner', 62)
	,('work_item_edges', 'work_item_edges_command_owner', 'enforce_work_item_command_owner', 62)
	,('work_item_readiness_blockers', 'work_item_readiness_blockers_command_owner', 'enforce_work_item_command_owner', 62)
	,('work_item_acceptances', 'work_item_acceptances_command_owner', 'enforce_work_item_command_owner', 62)
	,('work_item_acceptances', 'work_item_acceptances_immutable', 'forbid_work_item_acceptance_mutation', 27)
	,('work_item_acceptances', 'work_item_acceptance_coherence', 'enforce_work_item_acceptance_coherence', 5)
	,('activity', 'activity_work_item_namespace', 'enforce_work_item_event_namespace', 23)
	,('outbox', 'outbox_work_item_namespace', 'enforce_work_item_event_namespace', 23)
	,('managed_runs', 'managed_runs_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_assignments', 'managed_run_assignments_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_effect_barriers', 'managed_run_effect_barriers_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_effects', 'managed_run_effects_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_submitted_turn_receipts', 'managed_run_submitted_turn_receipts_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_safety_inputs', 'managed_run_safety_inputs_command_owner', 'enforce_managed_run_command_owner', 62)
	,('managed_run_assignments', 'managed_run_assignments_immutable', 'forbid_managed_run_immutable_mutation', 27)
	,('managed_run_effects', 'managed_run_effects_immutable', 'forbid_managed_run_immutable_mutation', 27)
	,('managed_run_submitted_turn_receipts', 'managed_run_submitted_turn_receipts_immutable', 'forbid_managed_run_immutable_mutation', 27)
	,('managed_run_safety_inputs', 'managed_run_safety_inputs_immutable', 'forbid_managed_run_immutable_mutation', 27)
	,('managed_run_assignments', 'managed_run_assignment_scope', 'enforce_managed_run_assignment_scope', 5)
	,('managed_runs', 'managed_runs_inert_state', 'enforce_managed_run_state', 31)
	,('managed_run_effect_barriers', 'managed_run_effect_barriers_state', 'enforce_effect_barrier_state', 31)
	,('activity', 'activity_managed_run_namespace', 'enforce_managed_run_event_namespace', 23)
	,('outbox', 'outbox_managed_run_namespace', 'enforce_managed_run_event_namespace', 23)
	,('repository_admissions', 'repository_admissions_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('repository_operations', 'repository_operations_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('repository_operation_evidence', 'repository_operation_evidence_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('repository_operation_results', 'repository_operation_results_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('repository_operation_events', 'repository_operation_events_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('repository_authority_transitions', 'repository_authority_transitions_immutable', 'forbid_managed_repository_history_mutation', 58)
	,('managed_repositories', 'managed_repositories_projection_complete', 'enforce_managed_repository_projection', 29)
	,('repository_operations', 'repository_operations_scope_complete', 'enforce_repository_operation_scope', 5)
	,('repository_operation_evidence', 'repository_operation_evidence_complete', 'enforce_repository_history_completeness', 5)
	,('repository_operation_results', 'repository_operation_results_complete', 'enforce_repository_history_completeness', 5)
	,('repository_operation_events', 'repository_operation_events_complete', 'enforce_repository_history_completeness', 5)
	,('repository_authority_transitions', 'repository_authority_transitions_complete', 'enforce_repository_history_completeness', 5)
)
SELECT
  expected.function_name,
  trigger.oid IS NOT NULL
    AND trigger.tgenabled = 'O'
    AND trigger.tgtype = expected.trigger_type
    AND trigger.tgparentid = 0
    AND (trigger.tgconstraint <> 0) = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete')
    )
    AND trigger.tgconstrrelid = 0
    AND trigger.tgconstrindid = 0
    AND trigger.tgdeferrable = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete')
    )
    AND trigger.tginitdeferred = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete')
    )
    AND trigger.tgnargs = 0
    AND trigger.tgattr = ''::pg_catalog.int2vector
    AND trigger.tgqual IS NULL
    AND trigger.tgoldtable IS NULL
    AND trigger.tgnewtable IS NULL
    AND function_namespace.nspname = 'decodex'
    AND proc.proname = expected.function_name,
  COALESCE(proc.oid IS NOT NULL
    AND function_namespace.nspname = 'decodex'
    AND proc.pronargs = 0
    AND proc.prorettype = 'pg_catalog.trigger'::pg_catalog.regtype
    AND proc.prokind = 'f'
    AND language.lanname = 'plpgsql'
    AND proc.provolatile = 'v'
    AND proc.proparallel = 'u'
    AND proc.prosecdef = (expected.function_name = 'capture_history_item_version')
    AND NOT proc.proleakproof
    AND NOT proc.proisstrict
    AND NOT proc.proretset
    AND proc.proconfig = ARRAY['search_path=pg_catalog, decodex']
    AND proc.probin IS NULL
    AND proc.prosqlbody IS NULL, false),
  proc.prosrc
FROM expected
JOIN pg_catalog.pg_namespace AS table_namespace ON table_namespace.nspname = 'decodex'
JOIN pg_catalog.pg_class AS class
  ON class.relnamespace = table_namespace.oid
 AND class.relname = expected.table_name
 AND class.relkind IN ('r', 'p')
LEFT JOIN pg_catalog.pg_trigger AS trigger
  ON trigger.tgrelid = class.oid
 AND trigger.tgname = expected.trigger_name
 AND NOT trigger.tgisinternal
LEFT JOIN pg_catalog.pg_proc AS proc ON proc.oid = trigger.tgfoid
LEFT JOIN pg_catalog.pg_namespace AS function_namespace
  ON function_namespace.oid = proc.pronamespace
LEFT JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
ORDER BY expected.function_name
"#;
const FUNCTION_CONTRACT_SQL: &str = r#"
SELECT
  pg_catalog.pg_get_function_arguments(proc.oid),
  pg_catalog.pg_get_function_result(proc.oid),
  language.lanname,
  proc.provolatile::pg_catalog.text,
  proc.proparallel::pg_catalog.text,
  proc.proisstrict,
  proc.proretset,
  proc.procost,
  proc.prorows,
  proc.prokind <> 'f'
    OR proc.proleakproof
    OR proc.probin IS NOT NULL
    OR proc.prosqlbody IS NOT NULL
    OR proc.prosupport <> 0
    OR proc.provariadic <> 0
    OR proc.protrftypes IS NOT NULL
    OR proc.pronargdefaults <> 0
    OR proc.proargdefaults IS NOT NULL,
  proc.prosecdef,
	proc.proconfig,
	proc.prosrc,
	pg_catalog.has_function_privilege(session_user, proc.oid, 'EXECUTE'),
	EXISTS (
	  SELECT 1
	  FROM pg_catalog.aclexplode(
	    COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))
	  ) AS privilege
	  WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE'
	)
FROM pg_catalog.pg_proc AS proc
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
WHERE namespace.nspname = 'decodex'
  AND proc.oid = pg_catalog.to_regprocedure($1)
"#;
const IDENTITY_CAST_AUTHORITY_SQL: &str = r#"
SELECT NOT EXISTS (
  SELECT 1
  FROM pg_catalog.pg_cast AS conversion
  WHERE conversion.castsource = 'pg_catalog.uuid'::pg_catalog.regtype
    AND conversion.casttarget = 'pg_catalog.text'::pg_catalog.regtype
    AND conversion.castcontext = 'i'
)
"#;
const EXECUTION_PATH_CONTRACT_SQL: &str = r#"
WITH catalog_context AS MATERIALIZED (
  SELECT pg_catalog.set_config('search_path', 'pg_catalog', true)
), decodex_namespace AS (
  SELECT namespace.oid
  FROM pg_catalog.pg_namespace AS namespace
  CROSS JOIN catalog_context
  WHERE namespace.nspname = 'decodex'
), decodex_relations AS (
  SELECT class.oid
  FROM pg_catalog.pg_class AS class
  JOIN decodex_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE class.relkind IN ('r', 'p')
), expected_triggers(table_name, trigger_name, function_signature) AS (VALUES
  ('leases', 'leases_operation_time', 'decodex.enforce_lease_operation_time()'),
  ('outbox', 'outbox_operation_time', 'decodex.enforce_outbox_operation_time()'),
	('quota_windows', 'quota_windows_observed_at_monotonic', 'decodex.enforce_quota_observation_monotonicity()'),
  ('activity', 'activity_append_only', 'decodex.forbid_mutation_of_activity()'),
  ('outbox', 'outbox_terminal_retention', 'decodex.enforce_outbox_terminal_retention()'),
  ('outbox', 'outbox_truncate_forbidden', 'decodex.forbid_outbox_truncate()'),
  ('command_receipts', 'command_receipts_state_guard', 'decodex.enforce_command_receipt_state()'),
  ('conversations', 'conversations_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('conversations', 'conversations_state_guard', 'decodex.enforce_conversation_state()'),
  ('profile_snapshots', 'profile_snapshots_created_at_guard', 'decodex.canonicalize_created_at()'),
  ('account_snapshots', 'account_snapshots_created_at_guard', 'decodex.canonicalize_created_at()'),
  ('runtime_sessions', 'runtime_sessions_state_guard', 'decodex.enforce_runtime_session_state()'),
  ('runtime_sessions', 'runtime_sessions_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('blob_objects', 'blob_objects_state_guard', 'decodex.enforce_blob_object_state()'),
  ('turns', 'turns_state_guard', 'decodex.enforce_turn_state()'),
  ('turns', 'turns_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('history_items', 'history_items_state_guard', 'decodex.enforce_history_item_state()'),
  ('history_items', 'history_items_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('history_items', 'history_items_version_capture', 'decodex.capture_history_item_version()'),
  ('history_cursors', 'history_cursors_state_guard', 'decodex.enforce_history_cursor_state()'),
  ('artifacts', 'artifacts_state_guard', 'decodex.enforce_artifact_state()'),
  ('artifacts', 'artifacts_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('artifact_revisions', 'artifact_revisions_state_guard', 'decodex.enforce_artifact_revision_state()'),
  ('artifact_revisions', 'artifact_revisions_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('context_packs', 'context_packs_state_guard', 'decodex.enforce_context_pack_state()'),
  ('context_packs', 'context_packs_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('context_pack_sources', 'context_pack_sources_state_guard', 'decodex.enforce_context_pack_source_state()'),
  ('context_pack_sources', 'context_pack_sources_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('transition_proposals', 'transition_proposals_created_at_guard', 'decodex.canonicalize_created_at()'),
  ('transition_proposals', 'transition_proposals_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('policies', 'policies_state_guard', 'decodex.enforce_policy_identity_state()'),
  ('policies', 'policies_truncate_forbidden', 'decodex.enforce_policy_identity_state()'),
  ('policy_revisions', 'policy_revisions_immutable', 'decodex.forbid_policy_revision_mutation()'),
  ('policy_revisions', 'policy_revisions_truncate_forbidden', 'decodex.forbid_policy_revision_mutation()'),
  ('programs', 'programs_state_guard', 'decodex.enforce_program_state()'),
  ('programs', 'programs_truncate_forbidden', 'decodex.enforce_program_state()'),
  ('objectives', 'objectives_state_guard', 'decodex.enforce_objective_state()'),
  ('objectives', 'objectives_truncate_forbidden', 'decodex.enforce_objective_state()'),
  ('objective_completion_evidence', 'objective_evidence_immutable', 'decodex.forbid_objective_evidence_mutation()'),
  ('objective_completion_evidence', 'objective_evidence_truncate_forbidden', 'decodex.forbid_objective_evidence_mutation()'),
  ('objectives', 'objectives_completion_coherence', 'decodex.enforce_objective_completion_coherence()'),
  ('objective_completion_evidence', 'objective_evidence_completion_coherence', 'decodex.enforce_objective_completion_coherence()'),
  ('exact_command_receipts', 'exact_receipts_complete_at_commit', 'decodex.enforce_exact_receipt_completion()'),
  ('exact_command_receipts', 'exact_receipts_immutable', 'decodex.forbid_exact_receipt_rewrite()'),
  ('exact_command_receipts', 'exact_receipts_untruncatable', 'decodex.forbid_exact_receipt_truncate()'),
  ('role_profiles', 'role_profiles_exact_global_set', 'decodex.enforce_complete_role_profile_set()'),
  ('role_profiles', 'role_profiles_identity_immutable', 'decodex.forbid_role_profile_identity_rewrite()'),
  ('role_profile_revisions', 'role_profile_revisions_immutable', 'decodex.forbid_role_profile_revision_mutation()'),
  ('role_profiles', 'role_profiles_untruncatable', 'decodex.forbid_role_profile_truncate()'),
  ('role_profile_revisions', 'role_profile_revisions_untruncatable', 'decodex.forbid_role_profile_truncate()'),
	  ('activity', 'activity_role_profile_namespace', 'decodex.enforce_role_profile_event_namespace()'),
	  ('outbox', 'outbox_role_profile_namespace', 'decodex.enforce_role_profile_event_namespace()'),
	  ('profile_snapshots', 'profile_snapshots_command_owner', 'decodex.enforce_runtime_session_command_owner()'),
	  ('account_snapshots', 'account_snapshots_command_owner', 'decodex.enforce_runtime_session_command_owner()'),
	  ('runtime_sessions', 'runtime_sessions_command_owner', 'decodex.enforce_runtime_session_command_owner()'),
	  ('profile_snapshots', 'profile_snapshots_immutable', 'decodex.forbid_runtime_snapshot_mutation()'),
	  ('account_snapshots', 'account_snapshots_immutable', 'decodex.forbid_runtime_snapshot_mutation()'),
	  ('activity', 'activity_runtime_session_namespace', 'decodex.enforce_runtime_session_event_namespace()'),
	  ('outbox', 'outbox_runtime_session_namespace', 'decodex.enforce_runtime_session_event_namespace()')
	,('work_items', 'work_items_state_guard', 'decodex.enforce_work_item_state()')
	,('work_items', 'work_items_command_owner', 'decodex.enforce_work_item_command_owner()')
	,('work_item_objectives', 'work_item_objectives_command_owner', 'decodex.enforce_work_item_command_owner()')
	,('work_item_edges', 'work_item_edges_command_owner', 'decodex.enforce_work_item_command_owner()')
	,('work_item_readiness_blockers', 'work_item_readiness_blockers_command_owner', 'decodex.enforce_work_item_command_owner()')
	,('work_item_acceptances', 'work_item_acceptances_command_owner', 'decodex.enforce_work_item_command_owner()')
	,('work_item_acceptances', 'work_item_acceptances_immutable', 'decodex.forbid_work_item_acceptance_mutation()')
	,('work_item_acceptances', 'work_item_acceptance_coherence', 'decodex.enforce_work_item_acceptance_coherence()')
	,('activity', 'activity_work_item_namespace', 'decodex.enforce_work_item_event_namespace()')
	,('outbox', 'outbox_work_item_namespace', 'decodex.enforce_work_item_event_namespace()')
	,('managed_runs', 'managed_runs_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_assignments', 'managed_run_assignments_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_effect_barriers', 'managed_run_effect_barriers_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_effects', 'managed_run_effects_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_submitted_turn_receipts', 'managed_run_submitted_turn_receipts_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_safety_inputs', 'managed_run_safety_inputs_command_owner', 'decodex.enforce_managed_run_command_owner()')
	,('managed_run_assignments', 'managed_run_assignments_immutable', 'decodex.forbid_managed_run_immutable_mutation()')
	,('managed_run_effects', 'managed_run_effects_immutable', 'decodex.forbid_managed_run_immutable_mutation()')
	,('managed_run_submitted_turn_receipts', 'managed_run_submitted_turn_receipts_immutable', 'decodex.forbid_managed_run_immutable_mutation()')
	,('managed_run_safety_inputs', 'managed_run_safety_inputs_immutable', 'decodex.forbid_managed_run_immutable_mutation()')
	,('managed_run_assignments', 'managed_run_assignment_scope', 'decodex.enforce_managed_run_assignment_scope()')
	,('managed_runs', 'managed_runs_inert_state', 'decodex.enforce_managed_run_state()')
	,('managed_run_effect_barriers', 'managed_run_effect_barriers_state', 'decodex.enforce_effect_barrier_state()')
	,('activity', 'activity_managed_run_namespace', 'decodex.enforce_managed_run_event_namespace()')
	,('outbox', 'outbox_managed_run_namespace', 'decodex.enforce_managed_run_event_namespace()')
	,('repository_admissions', 'repository_admissions_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('repository_operations', 'repository_operations_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('repository_operation_evidence', 'repository_operation_evidence_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('repository_operation_results', 'repository_operation_results_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('repository_operation_events', 'repository_operation_events_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('repository_authority_transitions', 'repository_authority_transitions_immutable', 'decodex.forbid_managed_repository_history_mutation()')
	,('managed_repositories', 'managed_repositories_projection_complete', 'decodex.enforce_managed_repository_projection()')
	,('repository_operations', 'repository_operations_scope_complete', 'decodex.enforce_repository_operation_scope()')
	,('repository_operation_evidence', 'repository_operation_evidence_complete', 'decodex.enforce_repository_history_completeness()')
	,('repository_operation_results', 'repository_operation_results_complete', 'decodex.enforce_repository_history_completeness()')
	,('repository_operation_events', 'repository_operation_events_complete', 'decodex.enforce_repository_history_completeness()')
	,('repository_authority_transitions', 'repository_authority_transitions_complete', 'decodex.enforce_repository_history_completeness()')
), actual_triggers AS (
  SELECT
    class.relname AS table_name,
    trigger.tgname AS trigger_name,
    trigger.tgfoid
  FROM pg_catalog.pg_trigger AS trigger
  JOIN pg_catalog.pg_class AS class ON class.oid = trigger.tgrelid
  WHERE trigger.tgrelid IN (SELECT oid FROM decodex_relations)
    AND NOT trigger.tgisinternal
), execution_objects(classid, objid) AS (
  SELECT 'pg_catalog.pg_attrdef'::pg_catalog.regclass, attrdef.oid
  FROM pg_catalog.pg_attrdef AS attrdef
  WHERE attrdef.adrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT 'pg_catalog.pg_constraint'::pg_catalog.regclass, relation_constraint.oid
  FROM pg_catalog.pg_constraint AS relation_constraint
  WHERE relation_constraint.conrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT 'pg_catalog.pg_class'::pg_catalog.regclass, index.indexrelid
  FROM pg_catalog.pg_index AS index
  WHERE index.indrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT 'pg_catalog.pg_rewrite'::pg_catalog.regclass, rewrite.oid
  FROM pg_catalog.pg_rewrite AS rewrite
  WHERE rewrite.ev_class IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT 'pg_catalog.pg_policy'::pg_catalog.regclass, policy.oid
  FROM pg_catalog.pg_policy AS policy
  WHERE policy.polrelid IN (SELECT oid FROM decodex_relations)
), referenced_functions AS (
  SELECT dependency.refobjid AS oid
  FROM execution_objects AS object
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = object.classid
   AND dependency.objid = object.objid
   AND dependency.refclassid = 'pg_catalog.pg_proc'::pg_catalog.regclass
  UNION
  SELECT referenced_operator.oprcode
  FROM execution_objects AS object
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = object.classid
   AND dependency.objid = object.objid
   AND dependency.refclassid = 'pg_catalog.pg_operator'::pg_catalog.regclass
  JOIN pg_catalog.pg_operator AS referenced_operator
    ON referenced_operator.oid = dependency.refobjid
), allowed_functions AS (
  SELECT pg_catalog.to_regprocedure(signature) AS oid
  FROM pg_catalog.unnest($1::pg_catalog.text[]) AS signature
)
SELECT
  (SELECT count(*) FROM actual_triggers) = (SELECT count(*) FROM expected_triggers)
    AND NOT EXISTS (
      SELECT 1
      FROM expected_triggers AS expected
      LEFT JOIN actual_triggers AS actual
        ON actual.table_name = expected.table_name
       AND actual.trigger_name = expected.trigger_name
       AND actual.tgfoid = pg_catalog.to_regprocedure(expected.function_signature)
      WHERE actual.tgfoid IS NULL
    ),
  NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_rewrite AS rewrite
    WHERE rewrite.ev_class IN (SELECT oid FROM decodex_relations)
  ),
  NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_policy AS policy
    WHERE policy.polrelid IN (SELECT oid FROM decodex_relations)
  ) AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_class AS class
    WHERE class.oid IN (SELECT oid FROM decodex_relations)
      AND (class.relrowsecurity OR class.relforcerowsecurity)
  ),
  NOT EXISTS (
    SELECT 1
    FROM referenced_functions AS referenced
    JOIN pg_catalog.pg_proc AS proc ON proc.oid = referenced.oid
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
    WHERE namespace.nspname <> 'pg_catalog'
      AND referenced.oid NOT IN (SELECT oid FROM allowed_functions WHERE oid IS NOT NULL)
  )
"#;
const SCHEMA_CONTRACT_SQL: &str = r#"
WITH catalog_context AS MATERIALIZED (
  SELECT pg_catalog.set_config('search_path', 'pg_catalog', true)
), decodex_namespace AS (
  SELECT namespace.oid, namespace.nspowner
  FROM pg_catalog.pg_namespace AS namespace
  CROSS JOIN catalog_context
  WHERE namespace.nspname = 'decodex'
), decodex_relations AS (
  SELECT class.oid
  FROM pg_catalog.pg_class AS class
  WHERE class.relnamespace IN (SELECT oid FROM decodex_namespace)
    AND class.relkind IN ('r', 'p')
), touching_constraints AS (
  SELECT con.*
  FROM pg_catalog.pg_constraint AS con
  WHERE con.conrelid IN (SELECT oid FROM decodex_relations)
     OR con.confrelid IN (SELECT oid FROM decodex_relations)
), relevant_internal_triggers AS (
  SELECT trigger.*
  FROM pg_catalog.pg_trigger AS trigger
  WHERE trigger.tgisinternal
    AND (
      trigger.tgrelid IN (SELECT oid FROM decodex_relations)
      OR trigger.tgconstraint IN (SELECT oid FROM touching_constraints)
    )
), decodex_functions AS (
  SELECT proc.*
  FROM pg_catalog.pg_proc AS proc
  WHERE proc.pronamespace IN (SELECT oid FROM decodex_namespace)
), decodex_types AS (
  SELECT type.*
  FROM pg_catalog.pg_type AS type
  WHERE type.typnamespace IN (SELECT oid FROM decodex_namespace)
), runtime_role AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
), authority_dependency_targets(kind, identity, classid, objid, objsubid) AS (
  SELECT
    'function_dependency',
    pg_catalog.format(
      '%I.%I(%s)', namespace.nspname, proc.proname,
      pg_catalog.pg_get_function_identity_arguments(proc.oid)
    ),
    'pg_catalog.pg_proc'::pg_catalog.regclass,
    proc.oid,
    0
  FROM decodex_functions AS proc
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
  UNION ALL
  SELECT
    'type_dependency',
    pg_catalog.format('%I.%I', namespace.nspname, type.typname),
    'pg_catalog.pg_type'::pg_catalog.regclass,
    type.oid,
    0
  FROM decodex_types AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
), dependency_targets(kind, identity, classid, objid, objsubid) AS (
  SELECT
    'default',
    pg_catalog.format('%I.%I.%I', namespace.nspname, class.relname, attribute.attname),
    'pg_catalog.pg_attrdef'::pg_catalog.regclass,
    attrdef.oid,
    0
  FROM pg_catalog.pg_attrdef AS attrdef
  JOIN pg_catalog.pg_class AS class ON class.oid = attrdef.adrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  JOIN pg_catalog.pg_attribute AS attribute
    ON attribute.attrelid = attrdef.adrelid
   AND attribute.attnum = attrdef.adnum
  WHERE attrdef.adrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT
    'constraint',
    pg_catalog.format('%I.%I.%I', namespace.nspname, class.relname, con.conname),
    'pg_catalog.pg_constraint'::pg_catalog.regclass,
    con.oid,
    0
  FROM touching_constraints AS con
  JOIN pg_catalog.pg_class AS class ON class.oid = con.conrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  UNION ALL
  SELECT
    'index',
    pg_catalog.format('%I.%I', namespace.nspname, class.relname),
    'pg_catalog.pg_class'::pg_catalog.regclass,
    class.oid,
    0
  FROM pg_catalog.pg_index AS index
  JOIN pg_catalog.pg_class AS class ON class.oid = index.indexrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE index.indrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT
    'internal_trigger',
    pg_catalog.format(
      '%I.%I:%I.%I:%s',
      relation_namespace.nspname, relation.relname,
      constraint_namespace.nspname, con.conname,
      trigger.tgfoid::pg_catalog.regprocedure
    ),
    'pg_catalog.pg_trigger'::pg_catalog.regclass,
    trigger.oid,
    0
  FROM relevant_internal_triggers AS trigger
  JOIN pg_catalog.pg_class AS relation ON relation.oid = trigger.tgrelid
  JOIN pg_catalog.pg_namespace AS relation_namespace ON relation_namespace.oid = relation.relnamespace
  LEFT JOIN touching_constraints AS con ON con.oid = trigger.tgconstraint
  LEFT JOIN pg_catalog.pg_namespace AS constraint_namespace
    ON constraint_namespace.oid = con.connamespace
), contract_rows(kind, identity, contract) AS (
  SELECT
    'relation',
    pg_catalog.format('%I.%I', namespace.nspname, class.relname),
    pg_catalog.jsonb_build_array(
      class.relkind, class.relpersistence, class.relrowsecurity, class.relforcerowsecurity,
      class.relreplident, access_method.amname, class.reloptions
    )::pg_catalog.text
  FROM pg_catalog.pg_class AS class
  JOIN decodex_namespace AS selected ON selected.oid = class.relnamespace
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN pg_catalog.pg_am AS access_method ON access_method.oid = class.relam
  UNION ALL
  SELECT
    'column',
    pg_catalog.format('%I.%I.%I', namespace.nspname, class.relname, attribute.attname),
    pg_catalog.jsonb_build_array(
      attribute.attnum, pg_catalog.format_type(attribute.atttypid, attribute.atttypmod),
      attribute.attnotnull, attribute.attidentity, attribute.attgenerated,
      attribute.attstorage, attribute.attcompression, attribute.attstattarget,
		collation_namespace.nspname, coll.collname
    )::pg_catalog.text
  FROM pg_catalog.pg_attribute AS attribute
  JOIN pg_catalog.pg_class AS class ON class.oid = attribute.attrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
	LEFT JOIN pg_catalog.pg_collation AS coll ON coll.oid = attribute.attcollation
	LEFT JOIN pg_catalog.pg_namespace AS collation_namespace
		ON collation_namespace.oid = coll.collnamespace
  WHERE attribute.attrelid IN (SELECT oid FROM decodex_relations)
    AND attribute.attnum > 0
    AND NOT attribute.attisdropped
  UNION ALL
  SELECT
    'default',
    pg_catalog.format('%I.%I.%I', namespace.nspname, class.relname, attribute.attname),
    pg_catalog.jsonb_build_array(pg_catalog.pg_get_expr(attrdef.adbin, attrdef.adrelid))::pg_catalog.text
  FROM pg_catalog.pg_attrdef AS attrdef
  JOIN pg_catalog.pg_class AS class ON class.oid = attrdef.adrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  JOIN pg_catalog.pg_attribute AS attribute
    ON attribute.attrelid = attrdef.adrelid
   AND attribute.attnum = attrdef.adnum
  WHERE attrdef.adrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT
    'constraint',
    pg_catalog.format('%I.%I.%I', source_namespace.nspname, source.relname, con.conname),
    pg_catalog.jsonb_build_array(
      con.contype, pg_catalog.pg_get_constraintdef(con.oid, false),
      con.condeferrable, con.condeferred, con.convalidated,
      con.conenforced, con.confupdtype, con.confdeltype,
      con.confmatchtype, con.conislocal, con.coninhcount,
      con.connoinherit, con.conkey, con.confkey,
      referenced_namespace.nspname, referenced.relname
    )::pg_catalog.text
  FROM touching_constraints AS con
  JOIN pg_catalog.pg_class AS source ON source.oid = con.conrelid
  JOIN pg_catalog.pg_namespace AS source_namespace ON source_namespace.oid = source.relnamespace
  LEFT JOIN pg_catalog.pg_class AS referenced ON referenced.oid = con.confrelid
  LEFT JOIN pg_catalog.pg_namespace AS referenced_namespace
    ON referenced_namespace.oid = referenced.relnamespace
  UNION ALL
  SELECT
    'index',
    pg_catalog.format('%I.%I', index_namespace.nspname, index_class.relname),
    pg_catalog.jsonb_build_array(
      table_namespace.nspname, table_class.relname,
      pg_catalog.pg_get_indexdef(index.indexrelid), index.indnatts, index.indnkeyatts,
      index.indisunique, index.indnullsnotdistinct, index.indisprimary,
      index.indisexclusion, index.indimmediate, index.indisclustered,
      index.indisvalid, index.indcheckxmin, index.indisready, index.indislive,
		index.indisreplident, index.indkey, index.indoption,
		pg_catalog.pg_get_expr(index.indexprs, index.indrelid),
      pg_catalog.pg_get_expr(index.indpred, index.indrelid)
    )::pg_catalog.text
  FROM pg_catalog.pg_index AS index
  JOIN pg_catalog.pg_class AS index_class ON index_class.oid = index.indexrelid
  JOIN pg_catalog.pg_namespace AS index_namespace ON index_namespace.oid = index_class.relnamespace
  JOIN pg_catalog.pg_class AS table_class ON table_class.oid = index.indrelid
  JOIN pg_catalog.pg_namespace AS table_namespace ON table_namespace.oid = table_class.relnamespace
  WHERE index.indrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT
    'internal_trigger',
    pg_catalog.format(
      '%I.%I:%I.%I:%s',
      relation_namespace.nspname, relation.relname,
      constraint_namespace.nspname, con.conname,
      trigger.tgfoid::pg_catalog.regprocedure
    ),
    pg_catalog.jsonb_build_array(
      trigger.tgtype, trigger.tgenabled, trigger.tgparentid = 0,
      trigger.tgconstraint = con.oid, trigger.tgdeferrable,
      trigger.tginitdeferred, trigger.tgnargs, pg_catalog.encode(trigger.tgargs, 'hex'),
      referenced_namespace.nspname, referenced.relname,
      index_namespace.nspname, constraint_index.relname,
      trigger.tgoldtable, trigger.tgnewtable
    )::pg_catalog.text
  FROM relevant_internal_triggers AS trigger
  JOIN pg_catalog.pg_class AS relation ON relation.oid = trigger.tgrelid
  JOIN pg_catalog.pg_namespace AS relation_namespace ON relation_namespace.oid = relation.relnamespace
  LEFT JOIN touching_constraints AS con ON con.oid = trigger.tgconstraint
  LEFT JOIN pg_catalog.pg_namespace AS constraint_namespace
    ON constraint_namespace.oid = con.connamespace
  LEFT JOIN pg_catalog.pg_class AS referenced ON referenced.oid = trigger.tgconstrrelid
  LEFT JOIN pg_catalog.pg_namespace AS referenced_namespace
    ON referenced_namespace.oid = referenced.relnamespace
  LEFT JOIN pg_catalog.pg_class AS constraint_index ON constraint_index.oid = trigger.tgconstrindid
  LEFT JOIN pg_catalog.pg_namespace AS index_namespace
    ON index_namespace.oid = constraint_index.relnamespace
  UNION ALL
  SELECT
    'type',
    pg_catalog.format('%I.%I', namespace.nspname, type.typname),
    pg_catalog.jsonb_build_array(
      type.typtype,
      type.typcategory,
      pg_catalog.format_type(type.typbasetype, type.typtypmod),
      type.typnotnull,
      collation_namespace.nspname,
      coll.collname,
      CASE WHEN type.typowner = namespace.nspowner
        THEN 'owner'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(type.typowner)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = type.typowner THEN 'owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY privilege.grantee, privilege.privilege_type
        )
        FROM pg_catalog.aclexplode(
          COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
        ) AS privilege
      ), '[]'::pg_catalog.jsonb)
    )::pg_catalog.text
  FROM decodex_types AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  LEFT JOIN pg_catalog.pg_collation AS coll ON coll.oid = type.typcollation
  LEFT JOIN pg_catalog.pg_namespace AS collation_namespace
    ON collation_namespace.oid = coll.collnamespace
  UNION ALL
  SELECT
    'domain_constraint',
    pg_catalog.format('%I.%I.%I', namespace.nspname, type.typname, con.conname),
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_constraintdef(con.oid, false),
      con.convalidated,
      con.conenforced
    )::pg_catalog.text
  FROM pg_catalog.pg_constraint AS con
  JOIN decodex_types AS type ON type.oid = con.contypid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  UNION ALL
  SELECT
    'function',
    pg_catalog.format(
      '%I.%I(%s)', namespace.nspname, proc.proname,
      pg_catalog.pg_get_function_identity_arguments(proc.oid)
    ),
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_function_arguments(proc.oid),
      pg_catalog.pg_get_function_result(proc.oid),
      language.lanname,
      proc.provolatile,
      proc.proparallel,
      proc.proisstrict,
      proc.prosecdef,
      proc.proleakproof,
      proc.proconfig,
      proc.prosrc,
      CASE WHEN proc.proowner = namespace.nspowner
        THEN 'owner'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(proc.proowner)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = proc.proowner THEN 'owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY privilege.grantee, privilege.privilege_type
        )
        FROM pg_catalog.aclexplode(
          COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))
        ) AS privilege
      ), '[]'::pg_catalog.jsonb)
    )::pg_catalog.text
  FROM decodex_functions AS proc
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
  JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
  UNION ALL
  SELECT
    target.kind,
    target.identity,
    pg_catalog.jsonb_build_array(
      dependency.deptype,
      pg_catalog.pg_describe_object(
        dependency.refclassid,
        dependency.refobjid,
        dependency.refobjsubid
      )
    )::pg_catalog.text
  FROM authority_dependency_targets AS target
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = target.classid
   AND dependency.objid = target.objid
   AND dependency.objsubid = target.objsubid
  UNION ALL
  SELECT
    'default_acl',
    pg_catalog.format('%s:%s', default_acl.defaclnamespace, default_acl.defaclobjtype),
    COALESCE((
      SELECT pg_catalog.jsonb_agg(
        pg_catalog.jsonb_build_array(
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = default_acl.defaclrole THEN 'owner'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY privilege.grantee, privilege.privilege_type
      )
      FROM pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
    ), '[]'::pg_catalog.jsonb)::pg_catalog.text
  FROM pg_catalog.pg_default_acl AS default_acl
  JOIN decodex_namespace AS namespace ON namespace.nspowner = default_acl.defaclrole
  WHERE default_acl.defaclnamespace IN (0, namespace.oid)
    AND default_acl.defaclobjtype IN ('f', 'T')
  UNION ALL
  SELECT
    'dependency',
    target.kind || ':' || target.identity,
    pg_catalog.jsonb_build_array(
      dependency.deptype,
      pg_catalog.pg_describe_object(
        dependency.refclassid,
        dependency.refobjid,
        dependency.refobjsubid
      )
    )::pg_catalog.text
  FROM dependency_targets AS target
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = target.classid
   AND dependency.objid = target.objid
   AND dependency.objsubid = target.objsubid
  UNION ALL
  SELECT
    'enum_label',
    pg_catalog.format('%I.%I.%s', namespace.nspname, type.typname, enum.enumsortorder),
    pg_catalog.jsonb_build_array(enum.enumlabel)::pg_catalog.text
  FROM pg_catalog.pg_enum AS enum
  JOIN pg_catalog.pg_type AS type ON type.oid = enum.enumtypid
  JOIN decodex_namespace AS selected ON selected.oid = type.typnamespace
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
)
SELECT pg_catalog.jsonb_agg(
  pg_catalog.jsonb_build_array(kind, identity, contract)
  ORDER BY kind, identity, contract
)::pg_catalog.text
FROM contract_rows
"#;
const SCHEMA_CONTRACT_SHA256: [u8; 32] = [
	0x99, 0xb6, 0x41, 0xfb, 0xd0, 0xee, 0x07, 0xc1, 0xd8, 0x19, 0x06, 0x0b, 0x89, 0xcd, 0x2d, 0x39,
	0x6a, 0xe5, 0xca, 0x80, 0xee, 0x46, 0x61, 0xab, 0x3c, 0x71, 0xea, 0xd6, 0xaa, 0x34, 0x7b, 0xf0,
];
// The shipped authority permits no role settings. Record only cardinality so any setting
// fails closed without copying an arbitrary custom-GUC value into the manifest or digest input.
const CONFIGURED_AUTHORITY_SQL: &str = r#"
WITH RECURSIVE configured_principals(label, role_name) AS (
  VALUES ('migration'::pg_catalog.text, $1::pg_catalog.name),
         ('runtime'::pg_catalog.text, $2::pg_catalog.name)
), configured_roles AS (
  SELECT
    configured.label, configured.role_name, role.oid, role.rolname,
    role.rolsuper, role.rolinherit, role.rolcreaterole, role.rolcreatedb,
    role.rolcanlogin, role.rolreplication, role.rolconnlimit, role.rolvaliduntil,
    role.rolbypassrls, role.rolconfig
  FROM configured_principals AS configured
  LEFT JOIN pg_catalog.pg_roles AS role ON role.rolname = configured.role_name
), membership_roles(oid) AS (
  SELECT oid FROM configured_roles WHERE oid IS NOT NULL
  UNION
  SELECT endpoint.oid
  FROM membership_roles AS reached
  JOIN pg_catalog.pg_auth_members AS membership
    ON membership.roleid = reached.oid OR membership.member = reached.oid
  CROSS JOIN LATERAL (
    VALUES (membership.roleid), (membership.member)
  ) AS endpoint(oid)
), relevant_roles AS (
  SELECT
    role.oid, role.rolname, role.rolsuper, role.rolinherit, role.rolcreaterole,
    role.rolcreatedb, role.rolcanlogin, role.rolreplication, role.rolconnlimit,
    role.rolvaliduntil, role.rolbypassrls, role.rolconfig
  FROM pg_catalog.pg_roles AS role
  WHERE role.oid IN (SELECT oid FROM membership_roles)
), configured_database AS (
  SELECT database.*
  FROM pg_catalog.pg_database AS database
  WHERE database.datname = pg_catalog.current_database()
), relevant_namespaces AS (
  SELECT namespace.*
  FROM pg_catalog.pg_namespace AS namespace
  WHERE namespace.nspname IN ('decodex', 'public')
), decodex_classes AS (
  SELECT class.*, namespace.nspname
  FROM pg_catalog.pg_class AS class
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = class.relnamespace AND namespace.nspname = 'decodex'
), ledger_class AS (
  SELECT class.*, namespace.nspname
  FROM pg_catalog.pg_class AS class
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = class.relnamespace AND namespace.nspname = 'public'
  WHERE class.relname = 'refinery_schema_history' AND class.relkind IN ('r', 'p')
), authority_classes AS (
  SELECT * FROM decodex_classes
  UNION ALL
  SELECT * FROM ledger_class
), authority_objects(kind, identity, owner_oid, acl) AS (
  SELECT
    'database', 'configured_database', database.datdba,
    COALESCE(database.datacl, pg_catalog.acldefault('d', database.datdba))
  FROM configured_database AS database
  UNION ALL
  SELECT
    'namespace', namespace.nspname, namespace.nspowner,
    COALESCE(namespace.nspacl, pg_catalog.acldefault('n', namespace.nspowner))
  FROM relevant_namespaces AS namespace
  UNION ALL
  SELECT
    CASE WHEN class.relkind = 'S' THEN 'sequence' ELSE 'relation' END,
    pg_catalog.format('%I.%I', class.nspname, class.relname), class.relowner,
    COALESCE(
      class.relacl,
      pg_catalog.acldefault(
        (CASE WHEN class.relkind = 'S' THEN 's' ELSE 'r' END)::pg_catalog."char",
        class.relowner
      )
    )
  FROM decodex_classes AS class
  WHERE class.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
  UNION ALL
  SELECT
    'migration_ledger', pg_catalog.format('%I.%I', class.nspname, class.relname),
    class.relowner, COALESCE(class.relacl, pg_catalog.acldefault('r', class.relowner))
  FROM ledger_class AS class
  UNION ALL
  SELECT
    'type', pg_catalog.format('%I.%I', namespace.nspname, type.typname), type.typowner,
    COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
  FROM pg_catalog.pg_type AS type
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = type.typnamespace AND namespace.nspname = 'decodex'
  UNION ALL
  SELECT
    'function',
    pg_catalog.format(
      '%I.%I(%s)', namespace.nspname, proc.proname,
      pg_catalog.pg_get_function_identity_arguments(proc.oid)
    ),
    proc.proowner, COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))
  FROM pg_catalog.pg_proc AS proc
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = proc.pronamespace AND namespace.nspname = 'decodex'
), contract_rows(kind, identity, contract) AS (
  SELECT
    'principal',
    configured.label,
    pg_catalog.jsonb_build_array(
      configured.oid IS NOT NULL,
      configured.rolsuper,
      configured.rolinherit,
      configured.rolcreaterole,
      configured.rolcreatedb,
      configured.rolcanlogin,
      configured.rolreplication,
      configured.rolconnlimit,
      CASE WHEN configured.rolvaliduntil IS NULL THEN NULL ELSE
        pg_catalog.to_char(
          configured.rolvaliduntil AT TIME ZONE 'UTC',
          'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
        )
      END,
      configured.rolbypassrls,
      COALESCE(pg_catalog.cardinality(configured.rolconfig), 0)
    )::pg_catalog.text
  FROM configured_roles AS configured
  UNION ALL
  SELECT
    'reachable_principal',
    'other:' || role.rolname,
    pg_catalog.jsonb_build_array(
      role.rolsuper, role.rolinherit, role.rolcreaterole, role.rolcreatedb,
      role.rolcanlogin, role.rolreplication, role.rolconnlimit,
      CASE WHEN role.rolvaliduntil IS NULL THEN NULL ELSE
        pg_catalog.to_char(
          role.rolvaliduntil AT TIME ZONE 'UTC',
          'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
        )
      END,
      role.rolbypassrls,
      COALESCE(pg_catalog.cardinality(role.rolconfig), 0)
    )::pg_catalog.text
  FROM relevant_roles AS role
  WHERE role.oid NOT IN (SELECT oid FROM configured_roles WHERE oid IS NOT NULL)
  UNION ALL
  SELECT
    'role_membership',
    pg_catalog.format(
      '%s->%s',
      CASE
        WHEN membership.roleid = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN membership.roleid = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(membership.roleid)
      END,
      CASE
        WHEN membership.member = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN membership.member = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(membership.member)
      END
    ),
    pg_catalog.jsonb_build_array(
      CASE
        WHEN membership.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN membership.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(membership.grantor)
      END,
      membership.admin_option,
      membership.inherit_option,
      membership.set_option
    )::pg_catalog.text
  FROM pg_catalog.pg_auth_members AS membership
  WHERE membership.roleid IN (SELECT oid FROM membership_roles)
    AND membership.member IN (SELECT oid FROM membership_roles)
  UNION ALL
  SELECT
    'role_setting',
    pg_catalog.format(
      '%s:%s',
      CASE
        WHEN setting.setrole = 0 THEN 'ALL'
        WHEN setting.setrole = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN setting.setrole = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(setting.setrole)
      END,
      CASE WHEN setting.setdatabase = 0 THEN 'global' ELSE 'configured_database' END
    ),
    pg_catalog.jsonb_build_array(
      COALESCE(pg_catalog.cardinality(setting.setconfig), 0)
    )::pg_catalog.text
  FROM pg_catalog.pg_db_role_setting AS setting
  WHERE (
      setting.setrole IN (SELECT oid FROM membership_roles)
      AND setting.setdatabase IN (0, (SELECT oid FROM configured_database))
    ) OR (
      setting.setrole = 0 AND setting.setdatabase = (SELECT oid FROM configured_database)
    )
  UNION ALL
  SELECT
    object.kind,
    object.identity,
    pg_catalog.jsonb_build_array(
      CASE
        WHEN object.owner_oid = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN object.owner_oid = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(object.owner_oid)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
                THEN 'migration'
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
            END,
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
                THEN 'migration'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY
            CASE
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
                THEN 'migration'
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
            END,
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
                THEN 'migration'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
        )
        FROM pg_catalog.aclexplode(object.acl) AS privilege
      ), '[]'::pg_catalog.jsonb)
    )::pg_catalog.text
  FROM authority_objects AS object
  UNION ALL
  SELECT
    'relation_mode',
    pg_catalog.format('%I.%I', class.nspname, class.relname),
    pg_catalog.jsonb_build_array(
      class.relkind, class.relpersistence, class.relrowsecurity,
      class.relforcerowsecurity, class.relreplident
    )::pg_catalog.text
  FROM authority_classes AS class
  WHERE class.relkind IN ('r', 'p', 'v', 'm', 'f')
  UNION ALL
  SELECT
    'column_acl',
    pg_catalog.format('%I.%I.%I', class.nspname, class.relname, attribute.attname),
    COALESCE((
      SELECT pg_catalog.jsonb_agg(
        pg_catalog.jsonb_build_array(
          CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY
          CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
      )
      FROM pg_catalog.aclexplode(
        COALESCE(attribute.attacl, pg_catalog.acldefault('c', class.relowner))
      ) AS privilege
    ), '[]'::pg_catalog.jsonb)::pg_catalog.text
  FROM pg_catalog.pg_attribute AS attribute
  JOIN authority_classes AS class ON class.oid = attribute.attrelid
  WHERE attribute.attnum > 0 AND NOT attribute.attisdropped
  UNION ALL
  SELECT
    'trigger_definition',
    pg_catalog.format('%I.%I.%I', class.nspname, class.relname, trigger.tgname),
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_triggerdef(trigger.oid, false),
      trigger.tgenabled,
      CASE
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(class.relowner)
      END,
      pg_catalog.format(
        '%I.%I(%s)', function_namespace.nspname, proc.proname,
        pg_catalog.pg_get_function_identity_arguments(proc.oid)
      ),
      CASE
        WHEN proc.proowner = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN proc.proowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(proc.proowner)
      END
    )::pg_catalog.text
  FROM pg_catalog.pg_trigger AS trigger
  JOIN authority_classes AS class ON class.oid = trigger.tgrelid
  JOIN pg_catalog.pg_proc AS proc ON proc.oid = trigger.tgfoid
  JOIN pg_catalog.pg_namespace AS function_namespace ON function_namespace.oid = proc.pronamespace
  WHERE NOT trigger.tgisinternal
     OR trigger.tgrelid IN (SELECT oid FROM ledger_class)
  UNION ALL
  SELECT
    'rule_definition',
    pg_catalog.format('%I.%I.%I', class.nspname, class.relname, rewrite.rulename),
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_ruledef(rewrite.oid, false),
      CASE
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(class.relowner)
      END
    )::pg_catalog.text
  FROM pg_catalog.pg_rewrite AS rewrite
  JOIN authority_classes AS class ON class.oid = rewrite.ev_class
  UNION ALL
  SELECT
    'policy_definition',
    pg_catalog.format('%I.%I.%I', class.nspname, class.relname, policy.polname),
    pg_catalog.jsonb_build_array(
      policy.polcmd,
      policy.polpermissive,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          CASE
            WHEN policy_role = 0 THEN 'PUBLIC'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(policy_role)
          END ORDER BY
          CASE
            WHEN policy_role = 0 THEN 'PUBLIC'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(policy_role)
          END
        )
        FROM pg_catalog.unnest(policy.polroles) AS policy_role
      ), '[]'::pg_catalog.jsonb),
      pg_catalog.pg_get_expr(policy.polqual, policy.polrelid),
      pg_catalog.pg_get_expr(policy.polwithcheck, policy.polrelid),
      CASE
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(class.relowner)
      END
    )::pg_catalog.text
  FROM pg_catalog.pg_policy AS policy
  JOIN authority_classes AS class ON class.oid = policy.polrelid
  UNION ALL
  SELECT
    'default_acl',
    pg_catalog.format(
      '%s:%s:%s',
      CASE
        WHEN default_acl.defaclrole = (SELECT oid FROM configured_roles WHERE label = 'migration')
          THEN 'migration'
        WHEN default_acl.defaclrole = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(default_acl.defaclrole)
      END,
      CASE
        WHEN default_acl.defaclnamespace = 0 THEN 'global'
        ELSE namespace.nspname
      END,
      default_acl.defaclobjtype
    ),
    COALESCE((
      SELECT pg_catalog.jsonb_agg(
        pg_catalog.jsonb_build_array(
          CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY
          CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'migration')
              THEN 'migration'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
      )
      FROM pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
    ), '[]'::pg_catalog.jsonb)::pg_catalog.text
  FROM pg_catalog.pg_default_acl AS default_acl
  LEFT JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = default_acl.defaclnamespace
  WHERE (
      default_acl.defaclrole IN (SELECT oid FROM configured_roles WHERE oid IS NOT NULL)
      AND (default_acl.defaclnamespace = 0 OR namespace.nspname IN ('decodex', 'public'))
    ) OR namespace.nspname IN ('decodex', 'public')
)
SELECT pg_catalog.jsonb_agg(
  pg_catalog.jsonb_build_array(kind, identity, contract)
  ORDER BY kind, identity, contract
)::pg_catalog.text
FROM contract_rows
"#;
const CONFIGURED_AUTHORITY_SHA256: [u8; 32] = [
	0x83, 0x8a, 0x95, 0x63, 0x93, 0x2c, 0x50, 0xb7, 0xe5, 0xea, 0xa6, 0x0b, 0xa0, 0x32, 0x84, 0x47,
	0xba, 0x24, 0xec, 0x85, 0x4e, 0x0b, 0x5f, 0xd8, 0x7c, 0x09, 0x57, 0x85, 0xfd, 0x8a, 0xe5, 0x1e,
];
const EXTENSION_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = session_user
     OR pg_catalog.pg_has_role(session_user, role.oid, 'SET')
), effective_roles AS (
  SELECT DISTINCT inherited.oid
  FROM set_roles AS active
  JOIN pg_catalog.pg_roles AS inherited
    ON inherited.oid = active.oid
    OR pg_catalog.pg_has_role(active.oid, inherited.oid, 'USAGE')
), decodex_namespace AS (
  SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'decodex'
), decodex_relations AS (
  SELECT class.oid
  FROM pg_catalog.pg_class AS class
  JOIN decodex_namespace AS namespace ON namespace.oid = class.relnamespace
), decodex_objects(classid, objid) AS (
  SELECT 'pg_catalog.pg_namespace'::pg_catalog.regclass, oid FROM decodex_namespace
  UNION
  SELECT 'pg_catalog.pg_class'::pg_catalog.regclass, oid FROM decodex_relations
  UNION
  SELECT 'pg_catalog.pg_proc'::pg_catalog.regclass, proc.oid
  FROM pg_catalog.pg_proc AS proc
  WHERE proc.pronamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_type'::pg_catalog.regclass, owned_type.oid
  FROM pg_catalog.pg_type AS owned_type
  WHERE owned_type.typnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_collation'::pg_catalog.regclass, owned_collation.oid
  FROM pg_catalog.pg_collation AS owned_collation
  WHERE owned_collation.collnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_conversion'::pg_catalog.regclass, owned_conversion.oid
  FROM pg_catalog.pg_conversion AS owned_conversion
  WHERE owned_conversion.connamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_operator'::pg_catalog.regclass, owned_operator.oid
  FROM pg_catalog.pg_operator AS owned_operator
  WHERE owned_operator.oprnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_opclass'::pg_catalog.regclass, operator_class.oid
  FROM pg_catalog.pg_opclass AS operator_class
  WHERE operator_class.opcnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_opfamily'::pg_catalog.regclass, operator_family.oid
  FROM pg_catalog.pg_opfamily AS operator_family
  WHERE operator_family.opfnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_statistic_ext'::pg_catalog.regclass, statistics.oid
  FROM pg_catalog.pg_statistic_ext AS statistics
  WHERE statistics.stxnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_ts_config'::pg_catalog.regclass, configuration.oid
  FROM pg_catalog.pg_ts_config AS configuration
  WHERE configuration.cfgnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_ts_dict'::pg_catalog.regclass, dictionary.oid
  FROM pg_catalog.pg_ts_dict AS dictionary
  WHERE dictionary.dictnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_ts_parser'::pg_catalog.regclass, search_parser.oid
  FROM pg_catalog.pg_ts_parser AS search_parser
  WHERE search_parser.prsnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_ts_template'::pg_catalog.regclass, search_template.oid
  FROM pg_catalog.pg_ts_template AS search_template
  WHERE search_template.tmplnamespace IN (SELECT oid FROM decodex_namespace)
  UNION
  SELECT 'pg_catalog.pg_constraint'::pg_catalog.regclass, owned_constraint.oid
  FROM pg_catalog.pg_constraint AS owned_constraint
  WHERE owned_constraint.connamespace IN (SELECT oid FROM decodex_namespace)
     OR owned_constraint.conrelid IN (SELECT oid FROM decodex_relations)
  UNION
  SELECT 'pg_catalog.pg_attrdef'::pg_catalog.regclass, attrdef.oid
  FROM pg_catalog.pg_attrdef AS attrdef
  WHERE attrdef.adrelid IN (SELECT oid FROM decodex_relations)
  UNION
  SELECT 'pg_catalog.pg_trigger'::pg_catalog.regclass, trigger.oid
  FROM pg_catalog.pg_trigger AS trigger
  WHERE trigger.tgrelid IN (SELECT oid FROM decodex_relations)
  UNION
  SELECT 'pg_catalog.pg_rewrite'::pg_catalog.regclass, rewrite.oid
  FROM pg_catalog.pg_rewrite AS rewrite
  WHERE rewrite.ev_class IN (SELECT oid FROM decodex_relations)
  UNION
  SELECT 'pg_catalog.pg_policy'::pg_catalog.regclass, policy.oid
  FROM pg_catalog.pg_policy AS policy
  WHERE policy.polrelid IN (SELECT oid FROM decodex_relations)
), extension_members(classid, objid, extowner) AS (
  SELECT dependency.classid, dependency.objid, extension.extowner
  FROM pg_catalog.pg_depend AS dependency
  JOIN pg_catalog.pg_extension AS extension
    ON dependency.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass
   AND extension.oid = dependency.refobjid
  WHERE dependency.deptype = 'e'
), controlled_extensions AS (
  SELECT member.extowner
  FROM decodex_objects AS object
  JOIN extension_members AS member
    ON member.classid = object.classid
   AND member.objid = object.objid
  UNION
  SELECT member.extowner
  FROM decodex_objects AS object
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = object.classid
   AND dependency.objid = object.objid
  JOIN extension_members AS member
    ON member.classid = dependency.refclassid
   AND member.objid = dependency.refobjid
)
SELECT EXISTS (
  SELECT 1
  FROM controlled_extensions AS extension
  JOIN effective_roles AS role ON role.oid = extension.extowner
)
"#;

#[derive(Clone, Copy)]
struct FunctionContract {
	name: &'static str,
	lookup_signature: &'static str,
	migration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
	volatility: &'static str,
	strict: bool,
	returns_set: bool,
	rows: f32,
}

#[cfg(feature = "test-support")]
pub(crate) const fn schema_contract_sql_fixture() -> &'static str {
	SCHEMA_CONTRACT_SQL
}

#[cfg(feature = "test-support")]
pub(crate) const fn configured_authority_sql_fixture() -> &'static str {
	CONFIGURED_AUTHORITY_SQL
}

#[cfg(feature = "test-support")]
pub(crate) fn execution_path_contract_fixture() -> (&'static str, Vec<&'static str>) {
	(
		EXECUTION_PATH_CONTRACT_SQL,
		FUNCTION_CONTRACTS
			.iter()
			.map(|contract| contract.lookup_signature)
			.chain(ALLOWED_EXECUTION_DEPENDENCIES)
			.collect(),
	)
}

pub(crate) async fn verify_runtime(
	client: &Client,
	migration_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError> {
	verify_configured_authority(client, migration_role, runtime_role).await?;
	verify_forbidden_authority(client).await?;
	verify_identity_cast_authority(client).await?;
	verify_execution_path_contract(client).await?;
	verify_retention_contract(client).await?;
	verify_function_contract(client).await?;
	verify_schema_contract(client).await?;

	verify_required_authority(client).await
}

const fn trigger_contract(
	name: &'static str,
	lookup_signature: &'static str,
	migration_signature: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		migration_signature,
		arguments: "",
		result: "trigger",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: false,
		rows: 0.0,
	}
}

const fn immutable_function_contract(
	name: &'static str,
	lookup_signature: &'static str,
	migration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		migration_signature,
		arguments,
		result,
		language,
		volatility: "i",
		strict: true,
		returns_set: false,
		rows: 0.0,
	}
}

const fn mutator_contract(
	name: &'static str,
	lookup_signature: &'static str,
	migration_signature: &'static str,
	arguments: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		migration_signature,
		arguments,
		result: "TABLE(result_code text, actual_revision bigint, changed boolean)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	}
}

const fn exact_function_contract(
	name: &'static str,
	lookup_signature: &'static str,
	migration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
	volatility: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		migration_signature,
		arguments,
		result,
		language,
		volatility,
		strict: false,
		returns_set: false,
		rows: 0.0,
	}
}

fn canonical_safety_function_source(function_name: &str) -> Option<&'static str> {
	if !SAFETY_FUNCTIONS.contains(&function_name) {
		return None;
	}

	let contract = FUNCTION_CONTRACTS.iter().find(|contract| contract.name == function_name)?;

	canonical_function_source(contract)
}

fn canonical_function_source(contract: &FunctionContract) -> Option<&'static str> {
	let declarations = [
		format!("CREATE FUNCTION decodex.{}", contract.migration_signature),
		format!("CREATE OR REPLACE FUNCTION decodex.{}", contract.migration_signature),
	];
	[
		FOUNDATION_MIGRATION,
		CONVERSATION_MIGRATION,
		PROJECT_AGENT_MIGRATION,
		POLICY_MIGRATION,
		PROGRAM_OBJECTIVE_MIGRATION,
		QUOTA_MIGRATION,
		ROLE_PROFILE_MIGRATION,
		RUNTIME_SESSION_MIGRATION,
		WORK_ITEM_MIGRATION,
		MANAGED_RUN_MIGRATION,
		MANAGED_REPOSITORY_MIGRATION,
	]
	.into_iter()
	.rev()
	.find_map(|migration| {
		let (declaration_index, declaration_length) = declarations
			.iter()
			.filter_map(|declaration| {
				migration.rfind(declaration.as_str()).map(|index| (index, declaration.len()))
			})
			.max_by_key(|(index, _)| *index)?;
		let declaration_and_tail = &migration[declaration_index + declaration_length..];
		let (_, source_and_tail) = declaration_and_tail.split_once("\nAS $$")?;
		let (source, _) = source_and_tail.split_once("$$;")?;
		Some(source)
	})
}

async fn verify_identity_cast_authority(client: &Client) -> Result<(), StoreError> {
	let closed: bool = client.query_one(IDENTITY_CAST_AUTHORITY_SQL, &[]).await?.get(0);

	if !closed {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL permits an implicit UUID-to-text identity conversion",
		));
	}

	Ok(())
}

async fn verify_schema_contract(client: &Client) -> Result<(), StoreError> {
	let manifest: Option<String> = client.query_one(SCHEMA_CONTRACT_SQL, &[]).await?.get(0);
	let manifest = manifest.ok_or_else(|| {
		StoreError::Incompatible("PostgreSQL Decodex schema inventory is empty".into())
	})?;
	let digest = Sha256::digest(manifest.as_bytes());

	if digest.as_slice() != SCHEMA_CONTRACT_SHA256 {
		let actual = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

		return Err(StoreError::Incompatible(format!(
			"PostgreSQL Decodex schema contract differs from the shipped PG18 inventory ({actual})"
		)));
	}

	Ok(())
}

async fn verify_configured_authority(
	client: &Client,
	migration_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError> {
	let session_is_runtime: bool = client
		.query_one(
			"SELECT session_user = $1::pg_catalog.name AND current_user = $1::pg_catalog.name",
			&[&runtime_role],
		)
		.await?
		.get(0);

	if !session_is_runtime {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL session identity differs from the configured runtime principal",
		));
	}

	let manifest: Option<String> =
		client.query_one(CONFIGURED_AUTHORITY_SQL, &[&migration_role, &runtime_role]).await?.get(0);
	let manifest = manifest.ok_or_else(|| {
		StoreError::Incompatible("PostgreSQL configured authority inventory is empty".into())
	})?;
	let digest = Sha256::digest(manifest.as_bytes());

	if digest.as_slice() != CONFIGURED_AUTHORITY_SHA256 {
		#[cfg(feature = "test-support")]
		eprintln!(
			"configured authority actual SHA-256: {}",
			digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
		);
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL configured principal or ACL authority differs from the shipped PG18 inventory",
		));
	}

	Ok(())
}
async fn verify_execution_path_contract(client: &Client) -> Result<(), StoreError> {
	let allowed_functions = FUNCTION_CONTRACTS
		.iter()
		.map(|contract| contract.lookup_signature)
		.chain(ALLOWED_EXECUTION_DEPENDENCIES)
		.collect::<Vec<_>>();
	let row = client.query_one(EXECUTION_PATH_CONTRACT_SQL, &[&allowed_functions]).await?;
	let exact_triggers: bool = row.get(0);
	let no_rules: bool = row.get(1);
	let no_policies: bool = row.get(2);
	let closed_function_dependencies: bool = row.get(3);

	if !exact_triggers || !no_rules || !no_policies || !closed_function_dependencies {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL exposes an unexpected executable path on a Decodex relation",
		));
	}

	Ok(())
}

async fn verify_function_contract(client: &Client) -> Result<(), StoreError> {
	let actual_count: i64 = client
		.query_one(
			r#"SELECT count(*)
			FROM pg_catalog.pg_proc AS proc
			JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
			WHERE namespace.nspname = 'decodex'"#,
			&[],
		)
		.await?
		.get(0);
	let expected_count = i64::try_from(FUNCTION_CONTRACTS.len()).map_err(|_| {
		StoreError::Incompatible("PostgreSQL function inventory is too large".into())
	})?;

	if actual_count > expected_count {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL exposes an unexpected runtime-callable Decodex function",
		));
	}
	if actual_count < expected_count {
		return Err(StoreError::Incompatible(
			"PostgreSQL runtime function inventory is incomplete".into(),
		));
	}

	for contract in FUNCTION_CONTRACTS {
		let Some(row) =
			client.query_opt(FUNCTION_CONTRACT_SQL, &[&contract.lookup_signature]).await?
		else {
			return Err(StoreError::UnsafeAuthority(
				"PostgreSQL substitutes an unexpected Decodex function or overload",
			));
		};
		let arguments: String = row.get(0);
		let result: String = row.get(1);
		let language: String = row.get(2);
		let volatility: String = row.get(3);
		let parallel: String = row.get(4);
		let strict: bool = row.get(5);
		let returns_set: bool = row.get(6);
		let cost: f32 = row.get(7);
		let rows: f32 = row.get(8);
		let unsafe_metadata: bool = row.get(9);
		let security_definer: bool = row.get(10);
		let settings: Option<Vec<String>> = row.get(11);
		let installed_source: String = row.get(12);
		let executable: bool = row.get(13);
		let public_executable: bool = row.get(14);
		let expected_security_definer = matches!(
			contract.name,
			"issue_history_cursor"
				| "prune_history_snapshots"
				| "capture_history_item_version"
				| "bootstrap_advisor"
				| "create_project"
				| "transition_project"
				| "create_policy"
				| "accept_policy_revision"
				| "create_program"
				| "update_program_context"
				| "transition_program"
				| "create_objective"
				| "transition_objective"
				| "achieve_objective"
				| "bootstrap_role_profiles_exact"
				| "update_role_profile_exact"
				| "create_runtime_session_exact"
				| "transition_runtime_session_exact"
				| "create_work_item_exact"
				| "update_work_item_exact"
				| "assess_work_item_readiness_exact"
				| "accept_work_item_exact"
				| "guard_work_item_running_resume"
				| "apply_managed_run_safety_input_exact"
		);
		let expected_executable = RUNTIME_EXECUTE_FUNCTIONS.contains(&contract.lookup_signature);
		let expected_settings = vec!["search_path=pg_catalog, decodex".to_owned()];

		if unsafe_metadata
			|| security_definer != expected_security_definer
			|| settings.as_ref() != Some(&expected_settings)
			|| arguments != contract.arguments
			|| result != contract.result
			|| language != contract.language
			|| volatility != contract.volatility
			|| parallel != "u"
			|| strict != contract.strict
			|| returns_set != contract.returns_set
			|| cost != 100.0
			|| rows != contract.rows
		{
			return Err(StoreError::UnsafeAuthority(
				"PostgreSQL runtime-callable Decodex function metadata is unsafe",
			));
		}

		let expected_source = canonical_function_source(&contract).ok_or_else(|| {
			StoreError::Incompatible("unknown canonical PostgreSQL function contract".into())
		})?;

		if installed_source != expected_source {
			return Err(StoreError::Incompatible(
				"PostgreSQL function semantics differ from the shipped migration".into(),
			));
		}
		if executable != expected_executable || public_executable {
			return Err(StoreError::Incompatible(
				"runtime identity has an incorrect PostgreSQL function privilege".into(),
			));
		}
	}

	Ok(())
}

async fn verify_forbidden_authority(client: &Client) -> Result<(), StoreError> {
	let role = client.query_one(ROLE_AUTHORITY_SQL, &[]).await?;
	let forbidden_role_attributes: bool = role.get(0);
	let database_create: bool = role.get(1);
	let schema_create: bool = role.get(2);
	let effective_object_ownership: bool = role.get(3);
	let function_grant_option: bool = role.get(4);
	let trigger_bypass: bool = role.get(5);
	let alter_system_bypass: bool = role.get(6);
	let unsafe_replication_role: bool = role.get(7);
	let membership_admin: bool = role.get(8);
	let table = client.query_one(TABLE_AUTHORITY_SQL, &[]).await?;
	let unsafe_table_authority: bool = table.get(1);
	let history = client.query_one(MIGRATION_HISTORY_AUTHORITY_SQL, &[]).await?;
	let unsafe_history_authority: bool = history.get(2);
	let sequence = client.query_one(SEQUENCE_AUTHORITY_SQL, &[]).await?;
	let unsafe_sequence_authority: bool = sequence.get(2);
	let extension_control: bool = client.query_one(EXTENSION_AUTHORITY_SQL, &[]).await?.get(0);

	if forbidden_role_attributes
		|| database_create
		|| schema_create
		|| effective_object_ownership
		|| function_grant_option
		|| trigger_bypass
		|| alter_system_bypass
		|| unsafe_replication_role
		|| membership_admin
		|| unsafe_table_authority
		|| unsafe_history_authority
		|| unsafe_sequence_authority
		|| extension_control
	{
		return Err(StoreError::UnsafeAuthority(
			"runtime identity or a SET-reachable role retains forbidden PostgreSQL authority",
		));
	}

	Ok(())
}

async fn verify_required_authority(client: &Client) -> Result<(), StoreError> {
	let schema_usage: bool = client
		.query_one("SELECT pg_catalog.has_schema_privilege(session_user, 'decodex', 'USAGE')", &[])
		.await?
		.get(0);
	let table = client.query_one(TABLE_AUTHORITY_SQL, &[]).await?;
	let exact_table_authority: bool = table.get(0);
	let history = client.query_one(MIGRATION_HISTORY_AUTHORITY_SQL, &[]).await?;
	let migration_history_exists: bool = history.get(0);
	let migration_history_select: bool = history.get(1);
	let sequence = client.query_one(SEQUENCE_AUTHORITY_SQL, &[]).await?;
	let exact_sequence_contract: bool = sequence.get(0);
	let sequence_usage: bool = sequence.get(1);

	if !schema_usage
		|| !exact_table_authority
		|| !migration_history_exists
		|| !migration_history_select
		|| !exact_sequence_contract
		|| !sequence_usage
	{
		return Err(StoreError::Incompatible(
			"runtime identity lacks the exact required PostgreSQL privileges".into(),
		));
	}

	Ok(())
}

async fn verify_retention_contract(client: &Client) -> Result<(), StoreError> {
	let rows = client.query(TRIGGER_CONTRACT_SQL, &[]).await?;

	if rows.len() != SAFETY_TRIGGER_COUNT {
		return Err(StoreError::Incompatible(
			"PostgreSQL retention function contract is incomplete".into(),
		));
	}

	for row in rows {
		let function_name: String = row.get(0);
		let trigger_matches: bool = row.get(1);
		let function_metadata_matches: bool = row.get(2);
		let installed_source: Option<String> = row.get(3);

		if !trigger_matches {
			return Err(StoreError::UnsafeAuthority(
				"PostgreSQL retention trigger contract is disabled or misbound",
			));
		}

		let expected_source =
			canonical_safety_function_source(&function_name).ok_or_else(|| {
				StoreError::Incompatible("unknown PostgreSQL retention function contract".into())
			})?;

		if !function_metadata_matches || installed_source.as_deref() != Some(expected_source) {
			return Err(StoreError::Incompatible(
				"PostgreSQL retention function semantics differ from the shipped migration".into(),
			));
		}
	}

	Ok(())
}
#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use crate::authority::{
		CONFIGURED_AUTHORITY_SHA256, CONFIGURED_AUTHORITY_SQL, CONVERSATION_MIGRATION,
		FOUNDATION_MIGRATION, FUNCTION_CONTRACTS, IDENTITY_CAST_AUTHORITY_SQL,
		MANAGED_REPOSITORY_MIGRATION, MANAGED_RUN_MIGRATION, OWNED_OBJECT_CATALOGS,
		POLICY_MIGRATION, PROGRAM_OBJECTIVE_MIGRATION, PROJECT_AGENT_MIGRATION, QUOTA_MIGRATION,
		ROLE_AUTHORITY_SQL, ROLE_PROFILE_MIGRATION, RUNTIME_SESSION_MIGRATION, SAFETY_FUNCTIONS,
		SCHEMA_CONTRACT_SHA256, SCHEMA_CONTRACT_SQL, WORK_ITEM_MIGRATION,
	};

	#[test]
	fn configured_authority_manifest_closes_postgres_18_principals_and_memberships() {
		for required in [
			"$1::pg_catalog.name",
			"$2::pg_catalog.name",
			"configured.rolsuper",
			"configured.rolinherit",
			"configured.rolcreaterole",
			"configured.rolcreatedb",
			"configured.rolcanlogin",
			"configured.rolreplication",
			"configured.rolconnlimit",
			"configured.rolvaliduntil",
			"configured.rolbypassrls",
			"configured.rolconfig",
			"membership.grantor",
			"membership.admin_option",
			"membership.inherit_option",
			"membership.set_option",
			"pg_catalog.pg_db_role_setting",
		] {
			assert!(CONFIGURED_AUTHORITY_SQL.contains(required), "{required}");
		}

		assert!(!CONFIGURED_AUTHORITY_SQL.contains("rolpassword"));
		assert!(!CONFIGURED_AUTHORITY_SQL.contains("pg_authid"));
		assert_ne!(CONFIGURED_AUTHORITY_SHA256, [0; 32]);
	}

	#[test]
	fn configured_authority_never_serializes_role_setting_values() {
		assert!(!CONFIGURED_AUTHORITY_SQL.contains("unnest(configured.rolconfig)"));
		assert!(!CONFIGURED_AUTHORITY_SQL.contains("unnest(role.rolconfig)"));
		assert!(!CONFIGURED_AUTHORITY_SQL.contains("unnest(setting.setconfig)"));
		assert!(CONFIGURED_AUTHORITY_SQL.contains("cardinality(configured.rolconfig)"));
		assert!(CONFIGURED_AUTHORITY_SQL.contains("cardinality(role.rolconfig)"));
		assert!(CONFIGURED_AUTHORITY_SQL.contains("cardinality(setting.setconfig)"));
	}

	#[test]
	fn configured_authority_manifest_closes_owned_objects_and_acl_grantors() {
		for required in [
			"'database', 'configured_database'",
			"'namespace', namespace.nspname",
			"'migration_ledger'",
			"authority_classes AS (",
			"JOIN authority_classes AS class ON class.oid = attribute.attrelid",
			"JOIN authority_classes AS class ON class.oid = trigger.tgrelid",
			"trigger.tgrelid IN (SELECT oid FROM ledger_class)",
			"JOIN authority_classes AS class ON class.oid = rewrite.ev_class",
			"JOIN authority_classes AS class ON class.oid = policy.polrelid",
			"'relation_mode'",
			"'column_acl'",
			"'trigger_definition'",
			"'rule_definition'",
			"'policy_definition'",
			"'default_acl'",
			"class.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')",
			"COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))",
			"COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))",
			"privilege.grantor",
			"privilege.grantee",
			"privilege.is_grantable",
			"NOT trigger.tgisinternal",
			"default_acl.defaclobjtype",
		] {
			assert!(CONFIGURED_AUTHORITY_SQL.contains(required), "{required}");
		}
	}

	#[test]
	fn configured_authority_semantic_labels_are_closed_to_configured_roles_and_public() {
		assert_eq!(CONFIGURED_AUTHORITY_SQL.matches("VALUES ('migration'").count(), 1);
		assert_eq!(CONFIGURED_AUTHORITY_SQL.matches("('runtime'").count(), 1);
		assert!(CONFIGURED_AUTHORITY_SQL.contains("THEN 'PUBLIC'"));
		assert!(CONFIGURED_AUTHORITY_SQL.contains("ELSE 'other:' ||"));
		assert!(!CONFIGURED_AUTHORITY_SQL.contains("password"));
	}

	#[test]
	fn postgres_18_owned_object_inventory_is_closed_in_one_authority_query() {
		let inventory = ROLE_AUTHORITY_SQL
			.split_once("), decodex_owned_objects(object_class, owner_oid) AS (")
			.expect("owned-object inventory starts")
			.1
			.split_once("\n)\nSELECT\n")
			.expect("owned-object inventory ends")
			.0;

		for (catalog, ownership_branch) in OWNED_OBJECT_CATALOGS {
			assert_eq!(inventory.matches(ownership_branch).count(), 1, "{catalog}");
		}
	}

	#[test]
	fn postgres_18_schema_manifest_closes_both_foreign_key_sides_and_internal_triggers() {
		for required in [
			"con.conrelid IN (SELECT oid FROM decodex_relations)",
			"con.confrelid IN (SELECT oid FROM decodex_relations)",
			"trigger.tgisinternal",
			"trigger.tgconstraint IN (SELECT oid FROM touching_constraints)",
			"pg_catalog.pg_get_constraintdef",
			"pg_catalog.pg_get_indexdef",
			"pg_catalog.pg_get_expr(attrdef.adbin",
			"pg_catalog.pg_describe_object",
		] {
			assert!(SCHEMA_CONTRACT_SQL.contains(required), "{required}");
		}

		assert_ne!(SCHEMA_CONTRACT_SHA256, [0; 32]);
	}

	#[test]
	fn schema_manifest_attests_global_and_decodex_scoped_owner_default_acls() {
		assert!(SCHEMA_CONTRACT_SQL.contains(
			"default_acl.defaclnamespace IN (0, namespace.oid)\n    AND default_acl.defaclobjtype IN ('f', 'T')"
		));
		assert!(!SCHEMA_CONTRACT_SQL.contains("default_acl.defaclnamespace = 0"));
	}

	#[test]
	fn canonical_inventory_covers_every_shipped_decodex_function_once() {
		assert_eq!(
			[
				FOUNDATION_MIGRATION,
				CONVERSATION_MIGRATION,
				PROJECT_AGENT_MIGRATION,
				POLICY_MIGRATION,
				PROGRAM_OBJECTIVE_MIGRATION,
				QUOTA_MIGRATION,
				ROLE_PROFILE_MIGRATION,
				RUNTIME_SESSION_MIGRATION,
				WORK_ITEM_MIGRATION,
				MANAGED_RUN_MIGRATION,
				MANAGED_REPOSITORY_MIGRATION,
			]
			.into_iter()
			.map(|migration| migration.matches("CREATE FUNCTION decodex.").count())
			.sum::<usize>(),
			FUNCTION_CONTRACTS.len()
		);

		let mut lookup_signatures = HashSet::new();

		for contract in FUNCTION_CONTRACTS {
			assert!(lookup_signatures.insert(contract.lookup_signature));
			assert_eq!(
				[
					FOUNDATION_MIGRATION,
					CONVERSATION_MIGRATION,
					PROJECT_AGENT_MIGRATION,
					POLICY_MIGRATION,
					PROGRAM_OBJECTIVE_MIGRATION,
					QUOTA_MIGRATION,
					ROLE_PROFILE_MIGRATION,
					RUNTIME_SESSION_MIGRATION,
					WORK_ITEM_MIGRATION,
					MANAGED_RUN_MIGRATION,
					MANAGED_REPOSITORY_MIGRATION,
				]
				.into_iter()
				.map(|migration| migration
					.matches(&format!("CREATE FUNCTION decodex.{}", contract.migration_signature))
					.count())
				.sum::<usize>(),
				1
			);

			let source = super::canonical_function_source(&contract)
				.expect("shipped function has a canonical migration body");

			assert!(source.starts_with('\n'));
			assert!(source.ends_with('\n'));
			assert!(!source.trim().is_empty());
		}
	}

	#[test]
	fn identity_mutators_share_one_first_statement_null_guard_and_cast_audit() {
		for name in [
			"bootstrap_advisor",
			"create_project",
			"transition_project",
			"create_policy",
			"accept_policy_revision",
			"create_program",
			"update_program_context",
			"transition_program",
			"create_objective",
			"transition_objective",
			"achieve_objective",
		] {
			let contract = FUNCTION_CONTRACTS
				.iter()
				.find(|contract| contract.name == name)
				.expect("identity mutator is in the closed function inventory");
			let source = super::canonical_function_source(contract)
				.expect("identity mutator has canonical migration source");
			let first_statement =
				source.split_once("BEGIN\n").expect("PL/pgSQL body begins").1.trim_start();

			assert!(first_statement.starts_with("IF p_"), "{name}");
			assert!(first_statement.contains("identity ingress requires canonical UUID-v4 text"));
			assert!(first_statement.contains("CONSTRAINT = 'canonical_uuid_v4_text_ingress'"));
		}

		assert!(IDENTITY_CAST_AUTHORITY_SQL.contains("pg_catalog.pg_cast"));
		assert!(IDENTITY_CAST_AUTHORITY_SQL.contains("conversion.castcontext = 'i'"));
	}

	#[test]
	fn every_safety_function_has_one_nonempty_canonical_migration_body() {
		for function_name in SAFETY_FUNCTIONS {
			let source = super::canonical_safety_function_source(function_name)
				.expect("shipped safety function has a canonical migration body");

			assert!(source.starts_with('\n'));
			assert!(source.ends_with("END\n") || source.ends_with("END;\n"));
			assert!(!source.trim().is_empty());
		}
	}
}
