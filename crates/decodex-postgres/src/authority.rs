//! Steady-state PostgreSQL authority verification for the retained runtime pool.

use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use crate::{StoreError, schema::LATEST_SCHEMA_SQL};
const ALLOWED_EXECUTION_DEPENDENCIES: [&str; 1] =
	["public.digest(pg_catalog.bytea,pg_catalog.text)"];
pub(crate) const SEMANTIC_AUTHORITY_PREDICATE_COUNT: usize = 37;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
enum SemanticAuthorityPredicate {
	ConfiguredRuntimeSession,
	NoForbiddenRoleAttributes,
	NoDatabaseCreate,
	NoSchemaCreate,
	NoEffectiveObjectOwnership,
	NoFunctionGrantOption,
	NoTriggerBypass,
	NoAlterSystemBypass,
	SessionReplicationRoleOrigin,
	NoMembershipAdmin,
	ExactTableAuthority,
	NoUnsafeTableAuthority,
	ExactSequenceContract,
	SequenceUsage,
	NoUnsafeSequenceAuthority,
	ProcessGenerationTypeUsage,
	NoPublicProcessGenerationTypeUsage,
	NoProcessGenerationTypeGrantOption,
	ProviderAttemptTypeUsage,
	NoPublicProviderAttemptTypeUsage,
	NoProviderAttemptTypeGrantOption,
	NoExtensionControl,
	SchemaUsage,
	IdentityCastClosed,
	ExactTriggerInventory,
	NoRelationRules,
	NoRelationPolicies,
	ClosedFunctionDependencies,
	ExactFunctionInventory,
	FunctionMetadata,
	FunctionSemantics,
	FunctionExecuteAuthority,
	RetentionInventory,
	RetentionTriggerBindings,
	RetentionFunctionMetadata,
	RetentionFunctionSemantics,
	NoUnexpectedRuntimeSecurityDefinerAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticAuthorityFailureClass {
	Unsafe,
	Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapAuthorityFailureClass {
	Unsafe,
	Incompatible,
}

impl BootstrapAuthorityFailureClass {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Unsafe => "unsafe",
			Self::Incompatible => "incompatible",
		}
	}
}

impl From<SemanticAuthorityFailureClass> for BootstrapAuthorityFailureClass {
	fn from(value: SemanticAuthorityFailureClass) -> Self {
		match value {
			SemanticAuthorityFailureClass::Unsafe => Self::Unsafe,
			SemanticAuthorityFailureClass::Incompatible => Self::Incompatible,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapAuthorityObservation {
	pub(crate) name: &'static str,
	pub(crate) failure_class: BootstrapAuthorityFailureClass,
	pub(crate) passed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapDigestEvidence {
	pub(crate) complete: bool,
	pub(crate) expected_sha256: [u8; 32],
	pub(crate) actual_sha256: Option<[u8; 32]>,
	pub(crate) incomplete_failure_class: BootstrapAuthorityFailureClass,
	pub(crate) mismatch_failure_class: BootstrapAuthorityFailureClass,
}

impl BootstrapDigestEvidence {
	pub(crate) fn passed(&self) -> bool {
		self.complete && self.actual_sha256 == Some(self.expected_sha256)
	}

	pub(crate) fn failure_class(&self) -> BootstrapAuthorityFailureClass {
		if self.complete { self.mismatch_failure_class } else { self.incomplete_failure_class }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapAuthorityEvidence {
	pub(crate) namespace: [BootstrapAuthorityObservation; 2],
	pub(crate) semantic: Vec<BootstrapAuthorityObservation>,
	pub(crate) configured_authority: BootstrapDigestEvidence,
	pub(crate) schema_contract: BootstrapDigestEvidence,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct BootstrapAuthorityProgress {
	pub(crate) namespace: Option<[BootstrapAuthorityObservation; 2]>,
	pub(crate) semantic: Option<Vec<BootstrapAuthorityObservation>>,
	pub(crate) configured_authority: Option<BootstrapDigestEvidence>,
	pub(crate) schema_contract: Option<BootstrapDigestEvidence>,
}

impl BootstrapAuthorityProgress {
	pub(crate) fn completed_components(&self) -> usize {
		let present = [
			self.namespace.is_some(),
			self.semantic.is_some(),
			self.configured_authority.is_some(),
			self.schema_contract.is_some(),
		];
		assert!(present.windows(2).all(|pair| pair[0] || !pair[1]));
		present.iter().filter(|present| **present).count()
	}

	fn into_complete(self) -> BootstrapAuthorityEvidence {
		assert_eq!(self.completed_components(), 4);
		BootstrapAuthorityEvidence {
			namespace: self.namespace.expect("namespace authority evidence is complete"),
			semantic: self.semantic.expect("semantic authority evidence is complete"),
			configured_authority: self
				.configured_authority
				.expect("configured authority evidence is complete"),
			schema_contract: self.schema_contract.expect("schema contract evidence is complete"),
		}
	}
}

impl From<BootstrapAuthorityEvidence> for BootstrapAuthorityProgress {
	fn from(evidence: BootstrapAuthorityEvidence) -> Self {
		Self {
			namespace: Some(evidence.namespace),
			semantic: Some(evidence.semantic),
			configured_authority: Some(evidence.configured_authority),
			schema_contract: Some(evidence.schema_contract),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapAuthorityOperation {
	Namespace,
	Semantic,
	ConfiguredAuthority,
	SchemaContract,
}

impl BootstrapAuthorityOperation {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Namespace => "namespace",
			Self::Semantic => "semantic",
			Self::ConfiguredAuthority => "configured_authority",
			Self::SchemaContract => "schema_contract",
		}
	}

	pub(crate) const fn completed_components_before(self) -> usize {
		match self {
			Self::Namespace => 0,
			Self::Semantic => 1,
			Self::ConfiguredAuthority => 2,
			Self::SchemaContract => 3,
		}
	}
}

#[derive(Debug)]
pub(crate) struct BootstrapAuthorityCollectionFailure {
	pub(crate) progress: BootstrapAuthorityProgress,
	pub(crate) operation: BootstrapAuthorityOperation,
	pub(crate) error: StoreError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticAuthorityFailurePolicy {
	Unsafe,
	Incompatible,
	UnsafeIfExcessOtherwiseIncompatible,
}

impl SemanticAuthorityFailurePolicy {
	const fn permits(self, class: SemanticAuthorityFailureClass) -> bool {
		match self {
			Self::Unsafe => matches!(class, SemanticAuthorityFailureClass::Unsafe),
			Self::Incompatible => matches!(class, SemanticAuthorityFailureClass::Incompatible),
			Self::UnsafeIfExcessOtherwiseIncompatible => true,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticAuthorityDescriptor {
	identity: SemanticAuthorityPredicate,
	name: &'static str,
	failure_policy: SemanticAuthorityFailurePolicy,
}

const fn semantic_authority_descriptor(
	identity: SemanticAuthorityPredicate,
	name: &'static str,
	failure_policy: SemanticAuthorityFailurePolicy,
) -> SemanticAuthorityDescriptor {
	SemanticAuthorityDescriptor { identity, name, failure_policy }
}

const SEMANTIC_AUTHORITY_DEFINITION: [SemanticAuthorityDescriptor;
	SEMANTIC_AUTHORITY_PREDICATE_COUNT] = [
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ConfiguredRuntimeSession,
		"configured_runtime_session",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoForbiddenRoleAttributes,
		"no_forbidden_role_attributes",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoDatabaseCreate,
		"no_database_create",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoSchemaCreate,
		"no_schema_create",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoEffectiveObjectOwnership,
		"no_effective_object_ownership",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoFunctionGrantOption,
		"no_function_grant_option",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoTriggerBypass,
		"no_trigger_bypass",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoAlterSystemBypass,
		"no_alter_system_bypass",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::SessionReplicationRoleOrigin,
		"session_replication_role_origin",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoMembershipAdmin,
		"no_membership_admin",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ExactTableAuthority,
		"exact_table_authority",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoUnsafeTableAuthority,
		"no_unsafe_table_authority",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ExactSequenceContract,
		"exact_sequence_contract",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::SequenceUsage,
		"sequence_usage",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoUnsafeSequenceAuthority,
		"no_unsafe_sequence_authority",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ProcessGenerationTypeUsage,
		"process_generation_type_usage",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoPublicProcessGenerationTypeUsage,
		"no_public_process_generation_type_usage",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoProcessGenerationTypeGrantOption,
		"no_process_generation_type_grant_option",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ProviderAttemptTypeUsage,
		"provider_attempt_type_usage",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoPublicProviderAttemptTypeUsage,
		"no_public_provider_attempt_type_usage",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoProviderAttemptTypeGrantOption,
		"no_provider_attempt_type_grant_option",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoExtensionControl,
		"no_extension_control",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::SchemaUsage,
		"schema_usage",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::IdentityCastClosed,
		"identity_cast_closed",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ExactTriggerInventory,
		"exact_trigger_inventory",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoRelationRules,
		"no_relation_rules",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoRelationPolicies,
		"no_relation_policies",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ClosedFunctionDependencies,
		"closed_function_dependencies",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::ExactFunctionInventory,
		"exact_function_inventory",
		SemanticAuthorityFailurePolicy::UnsafeIfExcessOtherwiseIncompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::FunctionMetadata,
		"function_metadata",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::FunctionSemantics,
		"function_semantics",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::FunctionExecuteAuthority,
		"function_execute_authority",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::RetentionInventory,
		"retention_inventory",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::RetentionTriggerBindings,
		"retention_trigger_bindings",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::RetentionFunctionMetadata,
		"retention_function_metadata",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::RetentionFunctionSemantics,
		"retention_function_semantics",
		SemanticAuthorityFailurePolicy::Incompatible,
	),
	semantic_authority_descriptor(
		SemanticAuthorityPredicate::NoUnexpectedRuntimeSecurityDefinerAuthority,
		"no_unexpected_runtime_security_definer_authority",
		SemanticAuthorityFailurePolicy::Unsafe,
	),
];
static FUNCTION_CONTRACTS: [FunctionContract; 227] = [
	FunctionContract {
		name: "is_canonical_media_type",
		lookup_signature: "decodex.is_canonical_media_type(pg_catalog.text)",
		declaration_signature: "is_canonical_media_type(value text)",
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
		declaration_signature: "is_history_metadata_projection(document jsonb)",
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
		declaration_signature: "normalize_unicode_whitespace(value text)",
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
		declaration_signature: "ascii_lower(value text)",
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
		declaration_signature: "has_credential_material(value text)",
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
		declaration_signature: "has_credential_material(document jsonb)",
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
		declaration_signature: "is_meaningful_evidence(document jsonb)",
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
		declaration_signature: "rfc3339_utc(value timestamp with time zone)",
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
		declaration_signature: "is_valid_operation_duration(value interval)",
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
		declaration_signature: "enforce_lease_operation_time()",
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
		declaration_signature: "enforce_outbox_operation_time()",
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
		declaration_signature: "enforce_quota_observation_monotonicity()",
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
		declaration_signature: "forbid_mutation_of_activity()",
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
		declaration_signature: "enforce_outbox_terminal_retention()",
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
		declaration_signature: "forbid_outbox_truncate()",
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
		declaration_signature: "lease_ttl_milliseconds(value interval)",
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
		declaration_signature: "try_acquire_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_ttl interval\n)",
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
		declaration_signature: "renew_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_lease_token uuid,\n\tp_ttl interval\n)",
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
		declaration_signature: "release_lease(\n\tp_resource_key text,\n\tp_holder_id uuid,\n\tp_lease_token uuid\n)",
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
		declaration_signature: "prune_history_snapshots()",
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
		declaration_signature: "issue_history_cursor(\n\tp_conversation_id uuid,\n\tp_parent_cursor_id uuid,\n\tp_page_size integer\n)",
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
		"enforce_conversation_routing_successor",
		"decodex.enforce_conversation_routing_successor()",
		"enforce_conversation_routing_successor()",
	),
	trigger_contract(
		"enforce_runtime_session_state",
		"decodex.enforce_runtime_session_state()",
		"enforce_runtime_session_state()",
	),
	trigger_contract("enforce_turn_state", "decodex.enforce_turn_state()", "enforce_turn_state()"),
	trigger_contract(
		"enforce_initial_quick_task_admission_complete",
		"decodex.enforce_initial_quick_task_admission_complete()",
		"enforce_initial_quick_task_admission_complete()",
	),
	trigger_contract(
		"enforce_initial_quick_task_admission_owner",
		"decodex.enforce_initial_quick_task_admission_owner()",
		"enforce_initial_quick_task_admission_owner()",
	),
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
		declaration_signature: "is_project_metadata(document jsonb)",
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
		declaration_signature: "bootstrap_advisor(p_agent_id decodex.canonical_uuid_v4_text)",
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
		declaration_signature: "create_project(\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_repository_identity text,\n\tp_repository_root text,\n\tp_default_cwd text,\n\tp_metadata jsonb,\n\tp_lead_id decodex.canonical_uuid_v4_text\n)",
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
		declaration_signature: "transition_project(\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_expected_revision bigint,\n\tp_status decodex.project_status\n)",
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
		declaration_signature: "is_policy_snapshot(document jsonb)",
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
		declaration_signature: "create_policy(\n\tp_policy_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text\n)",
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
		declaration_signature: "accept_policy_revision(\n\tp_policy_id decodex.canonical_uuid_v4_text,\n\tp_project_id decodex.canonical_uuid_v4_text,\n\tp_revision bigint,\n\tp_provenance text,\n\tp_snapshot jsonb,\n\tp_accepted_by decodex.canonical_uuid_v4_text,\n\tp_supersedes_revision bigint\n)",
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
		"enforce_managed_run_event_namespace",
		"decodex.enforce_managed_run_event_namespace()",
		"enforce_managed_run_event_namespace()",
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
	trigger_contract(
		"forbid_routing_history_mutation",
		"decodex.forbid_routing_history_mutation()",
		"forbid_routing_history_mutation()",
	),
	trigger_contract(
		"enforce_routing_completeness",
		"decodex.enforce_routing_completeness()",
		"enforce_routing_completeness()",
	),
	trigger_contract(
		"enforce_routing_command_owner",
		"decodex.enforce_routing_command_owner()",
		"enforce_routing_command_owner()",
	),
	exact_function_contract(
		"complete_exact_routing_rejection",
		"decodex.complete_exact_routing_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_routing_rejection(\n\tp_protocol text, p_idempotency_key text, p_operation text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_operation text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"reserve_exact_routing_command",
		"decodex.reserve_exact_routing_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_routing_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"replace_routing_policy_exact",
		"decodex.replace_routing_policy_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog._uuid,pg_catalog._int8,decodex._routing_member_disposition,decodex._codex_capability)",
		"replace_routing_policy_exact(\n\tp_protocol text, p_idempotency_key text, p_routing_policy_id uuid, p_project_id uuid,\n\tp_expected_revision bigint, p_accepted_policy_id uuid, p_accepted_policy_revision bigint,\n\tp_required_role decodex.role_profile_role, p_required_role_profile_revision bigint,\n\tp_required_build_id text, p_account_ids uuid[], p_account_revisions bigint[],\n\tp_dispositions decodex.routing_member_disposition[],\n\tp_required_capabilities decodex.codex_capability[]\n)",
		"p_protocol text, p_idempotency_key text, p_routing_policy_id uuid, p_project_id uuid, p_expected_revision bigint, p_accepted_policy_id uuid, p_accepted_policy_revision bigint, p_required_role decodex.role_profile_role, p_required_role_profile_revision bigint, p_required_build_id text, p_account_ids uuid[], p_account_revisions bigint[], p_dispositions decodex.routing_member_disposition[], p_required_capabilities decodex.codex_capability[]",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"publish_routing_evidence_exact",
		"decodex.publish_routing_evidence_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex._codex_capability,decodex._capability_evidence_state)",
		"publish_routing_evidence_exact(\n\tp_protocol text, p_idempotency_key text, p_evidence_id uuid, p_account_id uuid,\n\tp_expected_account_revision bigint, p_expected_evidence_revision bigint,\n\tp_role decodex.role_profile_role,\n\tp_role_profile_revision bigint, p_build_id text, p_process_id uuid,\n\tp_process_account_id uuid, p_schema_fingerprint text,\n\tp_capabilities decodex.codex_capability[], p_states decodex.capability_evidence_state[]\n)",
		"p_protocol text, p_idempotency_key text, p_evidence_id uuid, p_account_id uuid, p_expected_account_revision bigint, p_expected_evidence_revision bigint, p_role decodex.role_profile_role, p_role_profile_revision bigint, p_build_id text, p_process_id uuid, p_process_account_id uuid, p_schema_fingerprint text, p_capabilities decodex.codex_capability[], p_states decodex.capability_evidence_state[]",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"resolve_routing_snapshot_exact",
		"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,decodex.routing_authority_shape,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		"resolve_routing_snapshot_exact(\n\tp_protocol text,\n\tp_idempotency_key text,\n\tp_authority_shape decodex.routing_authority_shape,\n\tp_routing_policy_id uuid,\n\tp_expected_routing_policy_revision bigint,\n\tp_expected_account_routing_revision bigint,\n\tp_consumer_kind decodex.provider_attempt_consumer_kind,\n\tp_conversation_id uuid,\n\tp_expected_conversation_revision bigint,\n\tp_source_runtime_session_id uuid,\n\tp_expected_source_runtime_session_revision bigint,\n\tp_turn_id uuid,\n\tp_managed_run_id uuid,\n\tp_expected_managed_run_revision bigint,\n\tp_managed_execution_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_authority_shape decodex.routing_authority_shape, p_routing_policy_id uuid, p_expected_routing_policy_revision bigint, p_expected_account_routing_revision bigint, p_consumer_kind decodex.provider_attempt_consumer_kind, p_conversation_id uuid, p_expected_conversation_revision bigint, p_source_runtime_session_id uuid, p_expected_source_runtime_session_revision bigint, p_turn_id uuid, p_managed_run_id uuid, p_expected_managed_run_revision bigint, p_managed_execution_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	immutable_function_contract(
		"codex_experiment_marker",
		"decodex.codex_experiment_marker(pg_catalog.uuid)",
		"codex_experiment_marker(p_experiment_id uuid)",
		"p_experiment_id uuid",
		"text",
		"sql",
	),
	trigger_contract(
		"forbid_codex_experiment_history_mutation",
		"decodex.forbid_codex_experiment_history_mutation()",
		"forbid_codex_experiment_history_mutation()",
	),
	trigger_contract(
		"enforce_codex_experiment_command_owner",
		"decodex.enforce_codex_experiment_command_owner()",
		"enforce_codex_experiment_command_owner()",
	),
	exact_function_contract(
		"complete_exact_codex_experiment_rejection",
		"decodex.complete_exact_codex_experiment_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_codex_experiment_rejection(\n\tp_protocol text, p_idempotency_key text, p_operation text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_operation text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"reserve_exact_codex_experiment_command",
		"decodex.reserve_exact_codex_experiment_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_codex_experiment_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"prepare_codex_experiment_exact",
		"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"prepare_codex_experiment_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_managed_run_id uuid, p_managed_run_revision bigint, p_routing_snapshot_id uuid,\n\tp_account_id uuid, p_account_revision bigint, p_role_profile_revision bigint,\n\tp_build_id text, p_repository_cwd text, p_thread_title text\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_managed_run_id uuid, p_managed_run_revision bigint, p_routing_snapshot_id uuid, p_account_id uuid, p_account_revision bigint, p_role_profile_revision bigint, p_build_id text, p_repository_cwd text, p_thread_title text",
		"bytea",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "mark_codex_experiment_creation_possible_exact",
		lookup_signature: "decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		declaration_signature: "mark_codex_experiment_creation_possible_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_attempt_id uuid\n)",
		arguments: "p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_attempt_id uuid",
		result: "TABLE(response_bytes bytea, replayed boolean)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	exact_function_contract(
		"bind_codex_experiment_thread_exact",
		"decodex.bind_codex_experiment_thread_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool)",
		"bind_codex_experiment_thread_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_attempt_id uuid, p_thread_id text, p_response_id text,\n\tp_response_title text, p_response_cwd text, p_response_marker text, p_ephemeral boolean\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_attempt_id uuid, p_thread_id text, p_response_id text, p_response_title text, p_response_cwd text, p_response_marker text, p_ephemeral boolean",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"record_codex_experiment_observation_exact",
		"decodex.record_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"record_codex_experiment_observation_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_observation_id uuid,\n\tp_kind decodex.codex_experiment_observation_kind, p_thread_id text,\n\tp_marker text, p_source_id text, p_fact_digest text\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_observation_id uuid, p_kind decodex.codex_experiment_observation_kind, p_thread_id text, p_marker text, p_source_id text, p_fact_digest text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"bind_codex_experiment_start_exact",
		"decodex.bind_codex_experiment_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.text)",
		"bind_codex_experiment_start_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_attempt_id uuid, p_thread_id text,\n\tp_start_request_id bigint, p_start_request_digest text,\n\tp_request_cwd text, p_request_marker text, p_request_ephemeral boolean,\n\tp_start_response_id bigint, p_start_response_digest text,\n\tp_response_cwd text, p_response_marker text, p_response_ephemeral boolean,\n\tp_returned_name text\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_attempt_id uuid, p_thread_id text, p_start_request_id bigint, p_start_request_digest text, p_request_cwd text, p_request_marker text, p_request_ephemeral boolean, p_start_response_id bigint, p_start_response_digest text, p_response_cwd text, p_response_marker text, p_response_ephemeral boolean, p_returned_name text",
		"bytea",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "read_codex_experiment_start_exact",
		lookup_signature: "decodex.read_codex_experiment_start_exact(pg_catalog.uuid,pg_catalog.uuid)",
		declaration_signature: "read_codex_experiment_start_exact(\n\tp_experiment_id uuid, p_attempt_id uuid\n)",
		arguments: "p_experiment_id uuid, p_attempt_id uuid",
		result: "TABLE(experiment_id uuid, attempt_id uuid, experiment_revision bigint, thread_id text, start_request_id bigint, start_request_digest text, request_cwd text, request_marker text, request_ephemeral boolean, start_response_id bigint, start_response_digest text, response_cwd text, response_marker text, response_ephemeral boolean, returned_name text, bound_at_micros bigint)",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	FunctionContract {
		name: "mark_codex_experiment_title_set_possible_exact",
		lookup_signature: "decodex.mark_codex_experiment_title_set_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text)",
		declaration_signature: "mark_codex_experiment_title_set_possible_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_title_attempt_id uuid, p_thread_id text,\n\tp_request_id bigint, p_request_digest text, p_requested_title text\n)",
		arguments: "p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_title_attempt_id uuid, p_thread_id text, p_request_id bigint, p_request_digest text, p_requested_title text",
		result: "TABLE(response_bytes bytea, replayed boolean)",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	exact_function_contract(
		"attest_codex_experiment_retained_title_exact",
		"decodex.attest_codex_experiment_retained_title_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"attest_codex_experiment_retained_title_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_attestation_id uuid, p_title_attempt_id uuid,\n\tp_thread_id text, p_read_request_id bigint, p_read_request_digest text,\n\tp_read_response_id bigint, p_read_response_digest text,\n\tp_returned_title text, p_returned_cwd text, p_returned_marker text\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_attestation_id uuid, p_title_attempt_id uuid, p_thread_id text, p_read_request_id bigint, p_read_request_digest text, p_read_response_id bigint, p_read_response_digest text, p_returned_title text, p_returned_cwd text, p_returned_marker text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"record_attested_codex_experiment_observation_exact",
		"decodex.record_attested_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"record_attested_codex_experiment_observation_exact(\n\tp_protocol text, p_idempotency_key text, p_experiment_id uuid,\n\tp_expected_revision bigint, p_attestation_id uuid, p_observation_id uuid,\n\tp_kind decodex.codex_experiment_observation_kind, p_thread_id text,\n\tp_marker text, p_source_id text, p_fact_digest text\n)",
		"p_protocol text, p_idempotency_key text, p_experiment_id uuid, p_expected_revision bigint, p_attestation_id uuid, p_observation_id uuid, p_kind decodex.codex_experiment_observation_kind, p_thread_id text, p_marker text, p_source_id text, p_fact_digest text",
		"bytea",
		"plpgsql",
		"v",
	),
	trigger_contract(
		"forbid_routing_decision_mutation",
		"decodex.forbid_routing_decision_mutation()",
		"forbid_routing_decision_mutation()",
	),
	trigger_contract(
		"enforce_routing_decision_completeness",
		"decodex.enforce_routing_decision_completeness()",
		"enforce_routing_decision_completeness()",
	),
	exact_function_contract(
		"route_account_exact",
		"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,decodex.routing_authority_shape,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		"route_account_exact(\n\tp_protocol text,\n\tp_idempotency_key text,\n\tp_operation_id uuid,\n\tp_authority_shape decodex.routing_authority_shape,\n\tp_routing_policy_id uuid,\n\tp_expected_routing_policy_revision bigint,\n\tp_expected_account_routing_revision bigint,\n\tp_consumer_kind decodex.provider_attempt_consumer_kind,\n\tp_conversation_id uuid,\n\tp_expected_conversation_revision bigint,\n\tp_source_runtime_session_id uuid,\n\tp_expected_source_runtime_session_revision bigint,\n\tp_turn_id uuid,\n\tp_managed_run_id uuid,\n\tp_expected_managed_run_revision bigint,\n\tp_managed_execution_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_authority_shape decodex.routing_authority_shape, p_routing_policy_id uuid, p_expected_routing_policy_revision bigint, p_expected_account_routing_revision bigint, p_consumer_kind decodex.provider_attempt_consumer_kind, p_conversation_id uuid, p_expected_conversation_revision bigint, p_source_runtime_session_id uuid, p_expected_source_runtime_session_revision bigint, p_turn_id uuid, p_managed_run_id uuid, p_expected_managed_run_revision bigint, p_managed_execution_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"bind_quick_task_continuation_exact",
		"decodex.bind_quick_task_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		"bind_quick_task_continuation_exact(\n\tp_protocol text,p_idempotency_key text,p_operation_id uuid,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_source_runtime_session_id uuid,p_expected_source_runtime_session_revision bigint,\n\tp_turn_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_conversation_id uuid, p_expected_conversation_revision bigint, p_source_runtime_session_id uuid, p_expected_source_runtime_session_revision bigint, p_turn_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	trigger_contract(
		"forbid_continuation_plan_mutation",
		"decodex.forbid_continuation_plan_mutation()",
		"forbid_continuation_plan_mutation()",
	),
	trigger_contract(
		"enforce_continuation_plan_completeness",
		"decodex.enforce_continuation_plan_completeness()",
		"enforce_continuation_plan_completeness()",
	),
	trigger_contract(
		"enforce_continuation_event_namespace",
		"decodex.enforce_continuation_event_namespace()",
		"enforce_continuation_event_namespace()",
	),
	FunctionContract {
		name: "is_canonical_continuation_pack",
		lookup_signature: "decodex.is_canonical_continuation_pack(pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
		declaration_signature: "is_canonical_continuation_pack(\n\tp_conversation_id uuid, p_compiled_bytes bytea, p_compiled_digest text,\n\tp_manifest_digest text, p_max_bytes integer, p_recent_item_limit integer,\n\tp_possible_side_effects text, p_truncated boolean, p_omitted_source_count integer,\n\tp_source_kinds text[], p_source_ids text[], p_source_revisions bigint[],\n\tp_content_digests text[], p_original_lengths bigint[], p_included_lengths bigint[],\n\tp_included_digests text[], p_dispositions text[], p_artifact_ids text[],\n\tp_artifact_revisions bigint[]\n)",
		arguments: "p_conversation_id uuid, p_compiled_bytes bytea, p_compiled_digest text, p_manifest_digest text, p_max_bytes integer, p_recent_item_limit integer, p_possible_side_effects text, p_truncated boolean, p_omitted_source_count integer, p_source_kinds text[], p_source_ids text[], p_source_revisions bigint[], p_content_digests text[], p_original_lengths bigint[], p_included_lengths bigint[], p_included_digests text[], p_dispositions text[], p_artifact_ids text[], p_artifact_revisions bigint[]",
		result: "boolean",
		language: "plpgsql",
		volatility: "i",
		strict: false,
		returns_set: false,
		rows: 0.0,
	},
	exact_function_contract(
		"complete_exact_continuation_rejection",
		"decodex.complete_exact_continuation_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_continuation_rejection(\n\tp_protocol text, p_idempotency_key text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"reserve_exact_continuation_command",
		"decodex.reserve_exact_continuation_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_continuation_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "read_continuation_plan_exact",
		lookup_signature: "decodex.read_continuation_plan_exact(pg_catalog.uuid,pg_catalog.int8)",
		declaration_signature: "read_continuation_plan_exact(\n\tp_plan_id uuid, p_expected_revision bigint\n)",
		arguments: "p_plan_id uuid, p_expected_revision bigint",
		result: "TABLE(response_bytes bytea, effect_envelope jsonb, kind text, codex_thread_id text, fallback_context_pack_id text, fallback_runtime_session_id text, replay_permitted boolean, dispatch_enabled boolean)",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	exact_function_contract(
		"plan_continuation_exact",
		"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
		"plan_continuation_exact(\n\tp_protocol text,p_idempotency_key text,p_operation_id uuid,\n\tp_decision_id uuid,p_expected_consumer_revision bigint,p_plan_id uuid,\n\tp_fallback_session_id uuid,p_account_snapshot_id uuid,p_context_pack_id uuid,\n\tp_compiled_bytes bytea,p_compiled_digest text,p_manifest_digest text,\n\tp_max_bytes integer,p_recent_item_limit integer,p_possible_side_effects text,\n\tp_truncated boolean,p_omitted_source_count integer,\n\tp_source_kinds text[],p_source_ids text[],p_source_revisions bigint[],\n\tp_content_digests text[],p_original_lengths bigint[],p_included_lengths bigint[],\n\tp_included_digests text[],p_dispositions text[],p_artifact_ids text[],\n\tp_artifact_revisions bigint[]\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_decision_id uuid, p_expected_consumer_revision bigint, p_plan_id uuid, p_fallback_session_id uuid, p_account_snapshot_id uuid, p_context_pack_id uuid, p_compiled_bytes bytea, p_compiled_digest text, p_manifest_digest text, p_max_bytes integer, p_recent_item_limit integer, p_possible_side_effects text, p_truncated boolean, p_omitted_source_count integer, p_source_kinds text[], p_source_ids text[], p_source_revisions bigint[], p_content_digests text[], p_original_lengths bigint[], p_included_lengths bigint[], p_included_digests text[], p_dispositions text[], p_artifact_ids text[], p_artifact_revisions bigint[]",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"read_execution_decision_exact",
		"decodex.read_execution_decision_exact(pg_catalog.uuid)",
		"read_execution_decision_exact(p_decision_id uuid)",
		"p_decision_id uuid",
		"jsonb",
		"plpgsql",
		"s",
	),
	exact_function_contract(
		"read_managed_run_execution_exact",
		"decodex.read_managed_run_execution_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
		"read_managed_run_execution_exact(\n\tp_managed_run_id uuid,p_project_id uuid,p_expected_revision bigint\n)",
		"p_managed_run_id uuid, p_project_id uuid, p_expected_revision bigint",
		"jsonb",
		"sql",
		"s",
	),
	trigger_contract(
		"enforce_waiting_usage_wake_command_owner",
		"decodex.enforce_waiting_usage_wake_command_owner()",
		"enforce_waiting_usage_wake_command_owner()",
	),
	trigger_contract(
		"forbid_waiting_usage_wake_transition_mutation",
		"decodex.forbid_waiting_usage_wake_transition_mutation()",
		"forbid_waiting_usage_wake_transition_mutation()",
	),
	trigger_contract(
		"enforce_waiting_usage_wake_transition_complete",
		"decodex.enforce_waiting_usage_wake_transition_complete()",
		"enforce_waiting_usage_wake_transition_complete()",
	),
	trigger_contract(
		"enforce_waiting_usage_wake_head_projection",
		"decodex.enforce_waiting_usage_wake_head_projection()",
		"enforce_waiting_usage_wake_head_projection()",
	),
	trigger_contract(
		"enforce_waiting_usage_wake_event_namespace",
		"decodex.enforce_waiting_usage_wake_event_namespace()",
		"enforce_waiting_usage_wake_event_namespace()",
	),
	exact_function_contract(
		"reserve_exact_waiting_usage_wake_command",
		"decodex.reserve_exact_waiting_usage_wake_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"reserve_exact_waiting_usage_wake_command(\n\tp_protocol text, p_idempotency_key text, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"complete_exact_waiting_usage_wake_rejection",
		"decodex.complete_exact_waiting_usage_wake_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"complete_exact_waiting_usage_wake_rejection(\n\tp_protocol text, p_idempotency_key text, p_operation text, p_code text\n)",
		"p_protocol text, p_idempotency_key text, p_operation text, p_code text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"replay_waiting_usage_wake_operation_exact",
		"decodex.replay_waiting_usage_wake_operation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.jsonb)",
		"replay_waiting_usage_wake_operation_exact(\n\tp_protocol text, p_idempotency_key text, p_operation text,\n\tp_operation_id uuid, p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_operation text, p_operation_id uuid, p_request jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "read_waiting_usage_wake_transition_exact",
		lookup_signature: "decodex.read_waiting_usage_wake_transition_exact(pg_catalog.uuid,pg_catalog.uuid)",
		declaration_signature: "read_waiting_usage_wake_transition_exact(\n\tp_transition_id uuid, p_operation_id uuid\n)",
		arguments: "p_transition_id uuid, p_operation_id uuid",
		result: "TABLE(transition_id text, wake_id text, revision bigint, predecessor_revision bigint, predecessor_transition_id text, operation_id text, transition_kind text, registration_operation_id text, routing_decision_id text, routing_decision_revision bigint, routing_policy_id text, routing_policy_revision bigint, managed_run_id text, managed_run_revision bigint, earliest_ready_at_micros bigint, state text, claim_id text, lease_holder text, lease_fence_id text, lease_acquired_at_micros bigint, lease_expires_at_micros bigint, registered_at_micros bigint, transitioned_at_micros bigint, terminal_reason text, routing_resolution_request_id text, fresh_routing_resolution_only boolean, prior_decision_reusable boolean, production_enabled boolean, effect_envelope jsonb, response_bytes bytea)",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	exact_function_contract(
		"register_waiting_usage_wake_exact",
		"decodex.register_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
		"register_waiting_usage_wake_exact(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid,\n\tp_decision_id uuid, p_expected_managed_run_revision bigint\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_decision_id uuid, p_expected_managed_run_revision bigint",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"claim_due_waiting_usage_wake_exact",
		"decodex.claim_due_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"claim_due_waiting_usage_wake_exact(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid,\n\tp_claim_id uuid, p_holder_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_claim_id uuid, p_holder_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"fire_waiting_usage_wake_exact",
		"decodex.fire_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"fire_waiting_usage_wake_exact(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,\n\tp_expected_revision bigint, p_expected_transition_id uuid,\n\tp_holder_id uuid, p_lease_fence_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid, p_expected_revision bigint, p_expected_transition_id uuid, p_holder_id uuid, p_lease_fence_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"cancel_waiting_usage_wake_exact",
		"decodex.cancel_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		"cancel_waiting_usage_wake_exact(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,\n\tp_expected_revision bigint, p_expected_transition_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid, p_expected_revision bigint, p_expected_transition_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"register_waiting_usage_wake_exact_internal",
		"decodex.register_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.timestamptz)",
		"register_waiting_usage_wake_exact_internal(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid,\n\tp_decision_id uuid, p_expected_managed_run_revision bigint,\n\tp_authority_now timestamp with time zone\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_decision_id uuid, p_expected_managed_run_revision bigint, p_authority_now timestamp with time zone",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"claim_due_waiting_usage_wake_exact_internal",
		"decodex.claim_due_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.timestamptz)",
		"claim_due_waiting_usage_wake_exact_internal(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid,\n\tp_claim_id uuid, p_holder_id uuid,\n\tp_authority_now timestamp with time zone\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_claim_id uuid, p_holder_id uuid, p_authority_now timestamp with time zone",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"fire_waiting_usage_wake_exact_internal",
		"decodex.fire_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.timestamptz)",
		"fire_waiting_usage_wake_exact_internal(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,\n\tp_expected_revision bigint, p_expected_transition_id uuid,\n\tp_holder_id uuid, p_lease_fence_id uuid,\n\tp_authority_now timestamp with time zone\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid, p_expected_revision bigint, p_expected_transition_id uuid, p_holder_id uuid, p_lease_fence_id uuid, p_authority_now timestamp with time zone",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"cancel_waiting_usage_wake_exact_internal",
		"decodex.cancel_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.timestamptz)",
		"cancel_waiting_usage_wake_exact_internal(\n\tp_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,\n\tp_expected_revision bigint, p_expected_transition_id uuid,\n\tp_authority_now timestamp with time zone\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid, p_expected_revision bigint, p_expected_transition_id uuid, p_authority_now timestamp with time zone",
		"bytea",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"read_account_registry_exact",
		"decodex.read_account_registry_exact(pg_catalog.uuid,pg_catalog.int8)",
		"read_account_registry_exact(\n\tp_account_id uuid,\n\tp_limit bigint\n)",
		"p_account_id uuid, p_limit bigint",
		"TABLE(account_id uuid, display_label text, enabled boolean, state decodex.account_state, revision bigint, provider_kind decodex.account_provider_kind, provider_account_id text, credential_store_schema_version integer, credential_version bigint, credential_fingerprint text, credential_writer_operation_id uuid, tombstoned boolean, lifecycle_readiness text, unsettled_operation_id uuid, unsettled_kind decodex.account_operation_kind, unsettled_phase decodex.account_operation_phase, unsettled_recovery_code text, five_hour_disposition text, five_hour_used_percent integer, five_hour_resets_at_micros bigint, five_hour_observed_at_micros bigint, five_hour_error_code decodex.account_quota_observation_error, seven_day_disposition text, seven_day_used_percent integer, seven_day_resets_at_micros bigint, seven_day_observed_at_micros bigint, seven_day_error_code decodex.account_quota_observation_error)",
		"s",
	),
	table_function_contract(
		"read_reset_card_account_admission_exact",
		"decodex.read_reset_card_account_admission_exact(pg_catalog.uuid,pg_catalog.text)",
		"read_reset_card_account_admission_exact(\n\tp_account_id uuid,p_callback_profile_sha256 text\n)",
		"p_account_id uuid, p_callback_profile_sha256 text",
		"TABLE(state decodex.account_state, revision bigint, enabled boolean, tombstoned boolean, credential_store_schema_version integer, credential_version bigint, credential_fingerprint text, credential_writer_operation_id uuid, provider_kind decodex.account_provider_kind, provider_account_id text, credential_store_observation decodex.account_store_observation, operation_unsettled boolean, callback_profile_ready boolean)",
		"v",
	),
	table_function_contract(
		"prepare_account_operation_exact",
		"decodex.prepare_account_operation_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.account_operation_kind,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text)",
		"prepare_account_operation_exact(\n\tp_operation_id uuid,p_account_id uuid,p_kind decodex.account_operation_kind,\n\tp_display_label text,p_enabled boolean,p_expected_account_revision bigint,\n\tp_expected_store_schema_version integer,p_expected_credential_version bigint,\n\tp_expected_credential_fingerprint text,p_expected_credential_writer_operation_id uuid,\n\tp_target_store_schema_version integer,p_target_credential_version bigint,\n\tp_target_credential_fingerprint text,p_target_credential_writer_operation_id uuid,\n\tp_provider_kind decodex.account_provider_kind,p_provider_account_id text\n)",
		"p_operation_id uuid, p_account_id uuid, p_kind decodex.account_operation_kind, p_display_label text, p_enabled boolean, p_expected_account_revision bigint, p_expected_store_schema_version integer, p_expected_credential_version bigint, p_expected_credential_fingerprint text, p_expected_credential_writer_operation_id uuid, p_target_store_schema_version integer, p_target_credential_version bigint, p_target_credential_fingerprint text, p_target_credential_writer_operation_id uuid, p_provider_kind decodex.account_provider_kind, p_provider_account_id text",
		"TABLE(result_code text, account_revision bigint, phase decodex.account_operation_phase)",
		"v",
	),
	table_function_contract(
		"set_account_operation_target_exact",
		"decodex.set_account_operation_target_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
		"set_account_operation_target_exact(\n\tp_operation_id uuid,p_target_store_schema_version integer,\n\tp_target_credential_version bigint,p_target_credential_fingerprint text,\n\tp_target_credential_writer_operation_id uuid\n)",
		"p_operation_id uuid, p_target_store_schema_version integer, p_target_credential_version bigint, p_target_credential_fingerprint text, p_target_credential_writer_operation_id uuid",
		"TABLE(result_code text, account_revision bigint, phase decodex.account_operation_phase)",
		"v",
	),
	table_function_contract(
		"advance_account_operation_exact",
		"decodex.advance_account_operation_exact(pg_catalog.uuid,decodex.account_operation_phase,decodex.account_operation_phase,pg_catalog.text)",
		"advance_account_operation_exact(\n\tp_operation_id uuid,p_expected_phase decodex.account_operation_phase,\n\tp_target_phase decodex.account_operation_phase,p_recovery_code text\n)",
		"p_operation_id uuid, p_expected_phase decodex.account_operation_phase, p_target_phase decodex.account_operation_phase, p_recovery_code text",
		"TABLE(result_code text, account_revision bigint, phase decodex.account_operation_phase)",
		"v",
	),
	table_function_contract(
		"read_unsettled_account_operations_exact",
		"decodex.read_unsettled_account_operations_exact(pg_catalog.int8)",
		"read_unsettled_account_operations_exact(p_limit bigint)",
		"p_limit bigint",
		"SETOF decodex.account_operations",
		"s",
	),
	FunctionContract {
		name: "read_account_operation_exact",
		lookup_signature: "decodex.read_account_operation_exact(pg_catalog.uuid)",
		declaration_signature: "read_account_operation_exact(p_operation_id uuid)",
		arguments: "p_operation_id uuid",
		result: "SETOF decodex.account_operations",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	table_function_contract(
		"set_account_enabled_exact",
		"decodex.set_account_enabled_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.bool)",
		"set_account_enabled_exact(\n\tp_account_id uuid,p_expected_revision bigint,p_enabled boolean\n)",
		"p_account_id uuid, p_expected_revision bigint, p_enabled boolean",
		"TABLE(result_code text, revision bigint)",
		"v",
	),
	table_function_contract(
		"set_fixed_account_selection_exact",
		"decodex.set_fixed_account_selection_exact(pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
		"set_fixed_account_selection_exact(\n\tp_expected_routing_revision bigint,p_account_id uuid,p_expected_account_revision bigint\n)",
		"p_expected_routing_revision bigint, p_account_id uuid, p_expected_account_revision bigint",
		"TABLE(result_code text, routing_revision bigint, account_revision bigint)",
		"v",
	),
	table_function_contract(
		"set_balanced_account_selection_exact",
		"decodex.set_balanced_account_selection_exact(pg_catalog.int8)",
		"set_balanced_account_selection_exact(\n\tp_expected_routing_revision bigint\n)",
		"p_expected_routing_revision bigint",
		"TABLE(result_code text, routing_revision bigint)",
		"v",
	),
	table_function_contract(
		"set_account_order_exact",
		"decodex.set_account_order_exact(pg_catalog.int8,pg_catalog._uuid)",
		"set_account_order_exact(\n\tp_expected_routing_revision bigint,p_order uuid[]\n)",
		"p_expected_routing_revision bigint, p_order uuid[]",
		"TABLE(result_code text, routing_revision bigint)",
		"v",
	),
	exact_function_contract(
		"lock_account_routing_universe_exact",
		"decodex.lock_account_routing_universe_exact()",
		"lock_account_routing_universe_exact()",
		"",
		"boolean",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "read_account_routing_control_exact",
		lookup_signature: "decodex.read_account_routing_control_exact()",
		declaration_signature: "read_account_routing_control_exact()",
		arguments: "",
		result: "TABLE(mode decodex.account_selection_mode, fixed_account_id uuid, revision bigint, account_order uuid[])",
		language: "plpgsql",
		volatility: "v",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	exact_function_contract(
		"observe_account_quota_exact",
		"decodex.observe_account_quota_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int4,pg_catalog.int8,pg_catalog.int8)",
		"observe_account_quota_exact(\n\tp_account_id uuid,p_duration_minutes integer,p_used_percent integer,\n\tp_resets_at_micros bigint,p_observed_at_micros bigint\n)",
		"p_account_id uuid, p_duration_minutes integer, p_used_percent integer, p_resets_at_micros bigint, p_observed_at_micros bigint",
		"text",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"observe_account_quota_error_exact",
		"decodex.observe_account_quota_error_exact(pg_catalog.uuid,pg_catalog.int4,decodex.account_quota_observation_error,pg_catalog.int8)",
		"observe_account_quota_error_exact(\n\tp_account_id uuid,p_duration_minutes integer,\n\tp_error_code decodex.account_quota_observation_error,p_observed_at_micros bigint\n)",
		"p_account_id uuid, p_duration_minutes integer, p_error_code decodex.account_quota_observation_error, p_observed_at_micros bigint",
		"text",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"observe_account_store_exact",
		"decodex.observe_account_store_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,decodex.account_store_observation)",
		"observe_account_store_exact(\n\tp_account_id uuid,p_expected_revision bigint,p_expected_schema integer,\n\tp_expected_version bigint,p_expected_fingerprint text,p_expected_writer_operation_id uuid,\n\tp_expected_provider decodex.account_provider_kind,p_expected_provider_account_id text,\n\tp_observation decodex.account_store_observation\n)",
		"p_account_id uuid, p_expected_revision bigint, p_expected_schema integer, p_expected_version bigint, p_expected_fingerprint text, p_expected_writer_operation_id uuid, p_expected_provider decodex.account_provider_kind, p_expected_provider_account_id text, p_observation decodex.account_store_observation",
		"text",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"attest_codex_account_capability_exact",
		"decodex.attest_codex_account_capability_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.bool)",
		"attest_codex_account_capability_exact(\n\tp_build_identity text,p_executable_sha256 text,p_schema_sha256 text,\n\tp_callback_profile_sha256 text,p_login_chatgpt_auth_tokens boolean,p_refresh_callback boolean\n)",
		"p_build_identity text, p_executable_sha256 text, p_schema_sha256 text, p_callback_profile_sha256 text, p_login_chatgpt_auth_tokens boolean, p_refresh_callback boolean",
		"text",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"plan_initial_thread_continuation_exact",
		"decodex.plan_initial_thread_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
		"plan_initial_thread_continuation_exact(\n\tp_protocol text,p_idempotency_key text,p_operation_id uuid,\n\tp_decision_id uuid,p_expected_conversation_revision bigint,p_plan_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_operation_id uuid, p_decision_id uuid, p_expected_conversation_revision bigint, p_plan_id uuid",
		"bytea",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"begin_quick_task_initial_route_exact",
		"decodex.begin_quick_task_initial_route_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8)",
		"begin_quick_task_initial_route_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint",
		"TABLE(disposition text, response_bytes bytea, snapshot_envelope jsonb)",
		"v",
	),
	exact_function_contract(
		"complete_quick_task_initial_route_exact",
		"decodex.complete_quick_task_initial_route_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.routing_decision_kind,pg_catalog.uuid,pg_catalog.jsonb,pg_catalog.jsonb)",
		"complete_quick_task_initial_route_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_snapshot_id uuid,p_kind decodex.routing_decision_kind,\n\tp_selected_account_id uuid,p_causes jsonb,p_exclusions jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_snapshot_id uuid, p_kind decodex.routing_decision_kind, p_selected_account_id uuid, p_causes jsonb, p_exclusions jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"create_quick_task_routing_successor_exact",
		"decodex.create_quick_task_routing_successor_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8)",
		"create_quick_task_routing_successor_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_source_conversation_id uuid,p_expected_source_revision bigint\n)",
		"p_protocol text, p_idempotency_key text, p_source_conversation_id uuid, p_expected_source_revision bigint",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	exact_function_contract(
		"read_quick_task_initial_route_exact",
		"decodex.read_quick_task_initial_route_exact(pg_catalog.uuid)",
		"read_quick_task_initial_route_exact(p_conversation_id uuid)",
		"p_conversation_id uuid",
		"jsonb",
		"plpgsql",
		"s",
	),
	FunctionContract {
		name: "read_quick_task_request_exact",
		lookup_signature: "decodex.read_quick_task_request_exact(pg_catalog.uuid)",
		declaration_signature: "read_quick_task_request_exact(p_conversation_id uuid)",
		arguments: "p_conversation_id uuid",
		result: "TABLE(message text, working_directory text)",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	table_function_contract(
		"claim_runtime_session_thread_command",
		"decodex.claim_runtime_session_thread_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"claim_runtime_session_thread_command(\n\tp_protocol text,p_idempotency_key text,p_request jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_request jsonb",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	exact_function_contract(
		"complete_runtime_session_thread_command",
		"decodex.complete_runtime_session_thread_command(pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"complete_runtime_session_thread_command(\n\tp_protocol text,p_idempotency_key text,p_effect jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_effect jsonb",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"complete_runtime_session_thread_rejection",
		"decodex.complete_runtime_session_thread_rejection(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,pg_catalog.text)",
		"complete_runtime_session_thread_rejection(\n\tp_protocol text,p_idempotency_key text,p_operation text,\n\tp_request jsonb,p_rejection text\n)",
		"p_protocol text, p_idempotency_key text, p_operation text, p_request jsonb, p_rejection text",
		"bytea",
		"plpgsql",
		"v",
	),
	exact_function_contract(
		"append_runtime_session_thread_effect",
		"decodex.append_runtime_session_thread_effect(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"append_runtime_session_thread_effect(\n\tp_runtime_session_id uuid,p_revision bigint,p_event_kind text,\n\tp_correlation_key text,p_payload jsonb\n)",
		"p_runtime_session_id uuid, p_revision bigint, p_event_kind text, p_correlation_key text, p_payload jsonb",
		"jsonb",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"acknowledge_runtime_session_turn_exact",
		"decodex.acknowledge_runtime_session_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.provider_attempt_terminal_outcome,pg_catalog.uuid,pg_catalog.text)",
		"acknowledge_runtime_session_turn_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_user_turn_id uuid,p_expected_user_turn_revision bigint,\n\tp_assistant_turn_id uuid,p_expected_assistant_turn_revision bigint,\n\tp_provider_attempt_id uuid,p_expected_provider_attempt_revision bigint,\n\tp_provider_evidence_id uuid,\n\tp_provider_outcome decodex.provider_attempt_terminal_outcome,\n\tp_provider_thread_id uuid,p_provider_turn_id text\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_user_turn_id uuid, p_expected_user_turn_revision bigint, p_assistant_turn_id uuid, p_expected_assistant_turn_revision bigint, p_provider_attempt_id uuid, p_expected_provider_attempt_revision bigint, p_provider_evidence_id uuid, p_provider_outcome decodex.provider_attempt_terminal_outcome, p_provider_thread_id uuid, p_provider_turn_id text",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	table_function_contract(
		"admit_initial_quick_task_turn_exact",
		"decodex.admit_initial_quick_task_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
		"admit_initial_quick_task_turn_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_continuation_plan_id uuid,p_turn_id uuid,p_history_item_id uuid,\n\tp_inline_text text,p_blob_hash text,p_media_type text,p_metadata jsonb\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_continuation_plan_id uuid, p_turn_id uuid, p_history_item_id uuid, p_inline_text text, p_blob_hash text, p_media_type text, p_metadata jsonb",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	FunctionContract {
		name: "read_ordinary_runtime_session_for_resume_exact",
		lookup_signature: "decodex.read_ordinary_runtime_session_for_resume_exact(pg_catalog.uuid)",
		declaration_signature: "read_ordinary_runtime_session_for_resume_exact(\n\tp_conversation_id uuid\n)",
		arguments: "p_conversation_id uuid",
		result: "TABLE(conversation_revision bigint, runtime_session_id uuid, runtime_session_revision bigint, codex_thread_id uuid, model text, reasoning_effort text, instructions text, source_account_id uuid, source_account_revision bigint, next_turn_sequence bigint, thread_start_request_id bigint, thread_start_request_sha256 text, thread_start_response_id bigint, thread_start_response_sha256 text, has_acknowledged_turn boolean, has_active_turn boolean, has_unresolved_provider_attempt boolean, conversation_status text, profile_role text)",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
	table_function_contract(
		"read_ordinary_task_conversations_exact",
		"decodex.read_ordinary_task_conversations_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
		"read_ordinary_task_conversations_exact(\n\tp_conversation_id uuid,p_after_updated_at_micros bigint,\n\tp_after_conversation_id uuid,p_limit bigint\n)",
		"p_conversation_id uuid, p_after_updated_at_micros bigint, p_after_conversation_id uuid, p_limit bigint",
		"TABLE(conversation_id uuid, conversation_revision bigint, runtime_session_id uuid, runtime_session_revision bigint, runtime_session_state decodex.runtime_session_state, codex_thread_id uuid, thread_start_request_id bigint, thread_start_request_sha256 text, thread_start_response_id bigint, thread_start_response_sha256 text, has_acknowledged_turn boolean, active_user_turn_id uuid, active_user_turn_count bigint, has_active_provider_attempt boolean, has_unknown_provider_attempt boolean, pre_session_state text, routing_decision_id uuid, updated_at_micros bigint, routing_successor_conversation_id uuid, routing_successor_conversation_revision bigint, has_admitted_user_turn boolean)",
		"s",
	),
	table_function_contract(
		"read_turn_admission_exact",
		"decodex.read_turn_admission_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"read_turn_admission_exact(\n\tp_conversation_id uuid,p_runtime_session_id uuid,p_turn_id uuid\n)",
		"p_conversation_id uuid, p_runtime_session_id uuid, p_turn_id uuid",
		"TABLE(conversation_id uuid, runtime_session_id uuid, turn_id uuid, sequence bigint, role decodex.turn_role, possible_side_effects decodex.side_effect_state, status decodex.turn_status, revision bigint)",
		"s",
	),
	exact_function_contract(
		"prove_initial_quick_task_spawn_not_created_exact",
		"decodex.prove_initial_quick_task_spawn_not_created_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"prove_initial_quick_task_spawn_not_created_exact(\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint,\n\tp_continuation_plan_id uuid,p_routing_decision_id uuid,\n\tp_selected_account_id uuid,p_process_generation_id uuid\n)",
		"p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint, p_continuation_plan_id uuid, p_routing_decision_id uuid, p_selected_account_id uuid, p_process_generation_id uuid",
		"boolean",
		"plpgsql",
		"s",
	),
	table_function_contract(
		"prepare_quick_task_process_generation_exact",
		"decodex.prepare_quick_task_process_generation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"prepare_quick_task_process_generation_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint,\n\tp_continuation_plan_id uuid,p_routing_decision_id uuid,\n\tp_selected_account_id uuid,p_process_generation_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint, p_continuation_plan_id uuid, p_routing_decision_id uuid, p_selected_account_id uuid, p_process_generation_id uuid",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	table_function_contract(
		"fence_runtime_session_thread_start_exact",
		"decodex.fence_runtime_session_thread_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.text)",
		"fence_runtime_session_thread_start_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint,p_continuation_plan_id uuid,\n\tp_process_generation_id uuid,p_process_generation_revision bigint,\n\tp_process_execution_epoch_id uuid,p_thread_start_request_id bigint,\n\tp_thread_start_request_sha256 text\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint, p_continuation_plan_id uuid, p_process_generation_id uuid, p_process_generation_revision bigint, p_process_execution_epoch_id uuid, p_thread_start_request_id bigint, p_thread_start_request_sha256 text",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	table_function_contract(
		"bind_runtime_session_thread_exact",
		"decodex.bind_runtime_session_thread_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
		"bind_runtime_session_thread_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint,p_continuation_plan_id uuid,\n\tp_fence_protocol text,p_fence_idempotency_key text,\n\tp_thread_start_request_id bigint,p_thread_start_request_sha256 text,\n\tp_thread_start_response_id bigint,p_thread_start_response_sha256 text,\n\tp_codex_thread_id uuid\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint, p_continuation_plan_id uuid, p_fence_protocol text, p_fence_idempotency_key text, p_thread_start_request_id bigint, p_thread_start_request_sha256 text, p_thread_start_response_id bigint, p_thread_start_response_sha256 text, p_codex_thread_id uuid",
		"TABLE(response_bytes bytea, replayed boolean)",
		"v",
	),
	exact_function_contract(
		"read_quick_task_thread_establishment_exact",
		"decodex.read_quick_task_thread_establishment_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
		"read_quick_task_thread_establishment_exact(\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint,\n\tp_continuation_plan_id uuid,p_routing_decision_id uuid,\n\tp_selected_account_id uuid,p_process_generation_id uuid\n)",
		"p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint, p_continuation_plan_id uuid, p_routing_decision_id uuid, p_selected_account_id uuid, p_process_generation_id uuid",
		"jsonb",
		"plpgsql",
		"s",
	),
	table_function_contract(
		"terminalize_quick_task_turn_exact",
		"decodex.terminalize_quick_task_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.provider_attempt_terminal_outcome,pg_catalog.uuid,pg_catalog.text)",
		"terminalize_quick_task_turn_exact(\n\tp_protocol text,p_idempotency_key text,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_runtime_session_id uuid,p_expected_runtime_session_revision bigint,\n\tp_user_turn_id uuid,p_expected_user_turn_revision bigint,\n\tp_assistant_turn_id uuid,p_expected_assistant_turn_revision bigint,\n\tp_provider_attempt_id uuid,p_expected_provider_attempt_revision bigint,\n\tp_provider_evidence_id uuid,\n\tp_provider_outcome decodex.provider_attempt_terminal_outcome,\n\tp_provider_thread_id uuid,p_provider_turn_id text\n)",
		"p_protocol text, p_idempotency_key text, p_conversation_id uuid, p_expected_conversation_revision bigint, p_runtime_session_id uuid, p_expected_runtime_session_revision bigint, p_user_turn_id uuid, p_expected_user_turn_revision bigint, p_assistant_turn_id uuid, p_expected_assistant_turn_revision bigint, p_provider_attempt_id uuid, p_expected_provider_attempt_revision bigint, p_provider_evidence_id uuid, p_provider_outcome decodex.provider_attempt_terminal_outcome, p_provider_thread_id uuid, p_provider_turn_id text",
		"TABLE(result_code text, conversation_id uuid, conversation_revision bigint, runtime_session_id uuid, prior_runtime_session_revision bigint, runtime_session_revision bigint, user_turn_id uuid, user_turn_revision bigint, assistant_turn_id uuid, assistant_turn_revision bigint, provider_attempt_id uuid, provider_attempt_revision bigint, provider_evidence_id uuid)",
		"v",
	),
	table_function_contract(
		"reconcile_quick_task_terminalizations_exact",
		"decodex.reconcile_quick_task_terminalizations_exact(pg_catalog.int4)",
		"reconcile_quick_task_terminalizations_exact(p_limit integer)",
		"p_limit integer",
		"TABLE(terminalized_count bigint)",
		"v",
	),
	trigger_contract(
		"enforce_process_generation_transition",
		"decodex.enforce_process_generation_transition()",
		"enforce_process_generation_transition()",
	),
	trigger_contract(
		"record_process_generation_transition",
		"decodex.record_process_generation_transition()",
		"record_process_generation_transition()",
	),
	trigger_contract(
		"forbid_process_generation_history_mutation",
		"decodex.forbid_process_generation_history_mutation()",
		"forbid_process_generation_history_mutation()",
	),
	table_function_contract(
		"prepare_process_generation_exact",
		"decodex.prepare_process_generation_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid)",
		"prepare_process_generation_exact(\n\tp_generation_id uuid,p_account_id uuid,p_execution_epoch_id uuid,\n\tp_authorization_digest text,p_runner_identity text,p_intended_boot_id text,\n\tp_control_kind decodex.process_generation_control_kind,\n\tp_isolation_kind decodex.process_generation_isolation_kind,\n\tp_initial_account_revision bigint,p_credential_store_schema_version integer,\n\tp_credential_version bigint,p_credential_fingerprint text,\n\tp_credential_writer_operation_id uuid,\n\tp_provider_kind decodex.account_provider_kind,p_provider_account_id text,\n\tp_refresh_callback_profile_sha256 text,\n\tp_reset_card_outbox_id bigint,p_reset_card_worker_id uuid,p_reset_card_claim_token uuid,\n\tp_quick_task_protocol text,p_quick_task_idempotency_key text,\n\tp_quick_task_conversation_id uuid,p_quick_task_conversation_revision bigint,\n\tp_quick_task_runtime_session_id uuid,p_quick_task_runtime_session_revision bigint,\n\tp_quick_task_turn_id uuid,p_quick_task_turn_revision bigint,\n\tp_quick_task_continuation_plan_id uuid,p_quick_task_routing_decision_id uuid\n)",
		"p_generation_id uuid, p_account_id uuid, p_execution_epoch_id uuid, p_authorization_digest text, p_runner_identity text, p_intended_boot_id text, p_control_kind decodex.process_generation_control_kind, p_isolation_kind decodex.process_generation_isolation_kind, p_initial_account_revision bigint, p_credential_store_schema_version integer, p_credential_version bigint, p_credential_fingerprint text, p_credential_writer_operation_id uuid, p_provider_kind decodex.account_provider_kind, p_provider_account_id text, p_refresh_callback_profile_sha256 text, p_reset_card_outbox_id bigint, p_reset_card_worker_id uuid, p_reset_card_claim_token uuid, p_quick_task_protocol text, p_quick_task_idempotency_key text, p_quick_task_conversation_id uuid, p_quick_task_conversation_revision bigint, p_quick_task_runtime_session_id uuid, p_quick_task_runtime_session_revision bigint, p_quick_task_turn_id uuid, p_quick_task_turn_revision bigint, p_quick_task_continuation_plan_id uuid, p_quick_task_routing_decision_id uuid",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, created_at_micros bigint, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"bind_process_generation_identity_exact",
		"decodex.bind_process_generation_identity_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
		"bind_process_generation_identity_exact(\n\tp_generation_id uuid,\n\tp_expected_revision bigint,\n\tp_bound_boot_id text,\n\tp_process_id bigint,\n\tp_process_start_id text,\n\tp_process_group_id bigint,\n\tp_session_id bigint\n)",
		"p_generation_id uuid, p_expected_revision bigint, p_bound_boot_id text, p_process_id bigint, p_process_start_id text, p_process_group_id bigint, p_session_id bigint",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"mark_process_generation_ready_exact",
		"decodex.mark_process_generation_ready_exact(pg_catalog.uuid,pg_catalog.int8)",
		"mark_process_generation_ready_exact(\n\tp_generation_id uuid,\n\tp_expected_revision bigint\n)",
		"p_generation_id uuid, p_expected_revision bigint",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"mark_process_generation_stopping_exact",
		"decodex.mark_process_generation_stopping_exact(pg_catalog.uuid,pg_catalog.int8)",
		"mark_process_generation_stopping_exact(\n\tp_generation_id uuid,\n\tp_expected_revision bigint\n)",
		"p_generation_id uuid, p_expected_revision bigint",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"mark_process_generation_death_unknown_exact",
		"decodex.mark_process_generation_death_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.process_generation_loss_reason)",
		"mark_process_generation_death_unknown_exact(\n\tp_generation_id uuid,\n\tp_expected_revision bigint,\n\tp_reason decodex.process_generation_loss_reason\n)",
		"p_generation_id uuid, p_expected_revision bigint, p_reason decodex.process_generation_loss_reason",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"record_process_generation_death_exact",
		"decodex.record_process_generation_death_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.process_generation_death_evidence_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.text)",
		"record_process_generation_death_exact(\n\tp_generation_id uuid,\n\tp_expected_revision bigint,\n\tp_evidence_id uuid,\n\tp_kind decodex.process_generation_death_evidence_kind,\n\tp_observed_boot_id text,\n\tp_process_id bigint,\n\tp_process_start_id text,\n\tp_process_group_id bigint,\n\tp_session_id bigint,\n\tp_witness_digest text\n)",
		"p_generation_id uuid, p_expected_revision bigint, p_evidence_id uuid, p_kind decodex.process_generation_death_evidence_kind, p_observed_boot_id text, p_process_id bigint, p_process_start_id text, p_process_group_id bigint, p_session_id bigint, p_witness_digest text",
		"TABLE(result_code text, revision bigint, state decodex.process_generation_state, observed_at_micros bigint)",
		"v",
	),
	exact_function_contract(
		"project_process_generations_after_supervisor_loss_exact",
		"decodex.project_process_generations_after_supervisor_loss_exact()",
		"project_process_generations_after_supervisor_loss_exact()",
		"",
		"bigint",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"read_process_generations_exact",
		"decodex.read_process_generations_exact(pg_catalog.uuid,pg_catalog.bool,pg_catalog.uuid,pg_catalog.int8)",
		"read_process_generations_exact(\n\tp_account_id uuid,p_include_dead boolean,p_after_generation_id uuid,p_limit bigint\n)",
		"p_account_id uuid, p_include_dead boolean, p_after_generation_id uuid, p_limit bigint",
		"TABLE(generation_id uuid, account_id uuid, execution_epoch_id uuid, runner_identity text, intended_boot_id text, control_kind decodex.process_generation_control_kind, isolation_kind decodex.process_generation_isolation_kind, bound_boot_id text, process_id bigint, process_start_id text, process_group_id bigint, session_id bigint, state decodex.process_generation_state, revision bigint, authority_loss_reason decodex.process_generation_loss_reason, death_evidence_id uuid, created_at_micros bigint, updated_at_micros bigint, initial_account_revision bigint, credential_store_schema_version integer, credential_version bigint, credential_fingerprint text, credential_writer_operation_id uuid, provider_kind decodex.account_provider_kind, provider_account_id text, refresh_callback_profile_sha256 text)",
		"s",
	),
	trigger_contract(
		"enforce_provider_attempt_transition",
		"decodex.enforce_provider_attempt_transition()",
		"enforce_provider_attempt_transition()",
	),
	trigger_contract(
		"enforce_provider_attempt_binding",
		"decodex.enforce_provider_attempt_binding()",
		"enforce_provider_attempt_binding()",
	),
	trigger_contract(
		"record_provider_attempt_transition",
		"decodex.record_provider_attempt_transition()",
		"record_provider_attempt_transition()",
	),
	trigger_contract(
		"enforce_provider_attempt_turn_materialization",
		"decodex.enforce_provider_attempt_turn_materialization()",
		"enforce_provider_attempt_turn_materialization()",
	),
	trigger_contract(
		"forbid_provider_attempt_history_mutation",
		"decodex.forbid_provider_attempt_history_mutation()",
		"forbid_provider_attempt_history_mutation()",
	),
	table_function_contract(
		"prepare_provider_attempt_exact",
		"decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
		"prepare_provider_attempt_exact(\n\tp_attempt_id uuid,p_consumer_kind decodex.provider_attempt_consumer_kind,\n\tp_conversation_id uuid,p_turn_id uuid,p_managed_run_id uuid,\n\tp_managed_run_revision bigint,p_managed_execution_id uuid,\n\tp_continuation_plan_id uuid,p_process_generation_id uuid,\n\tp_process_generation_revision bigint,p_process_execution_epoch_id uuid,\n\tp_request_id uuid,p_request_digest text,\n\tp_provider_idempotency_key text,p_provider_correlation_key text,\n\tp_predecessor_attempt_id uuid,p_duplicate_risk_ack_digest text,\n\tp_runtime_session_binding_protocol text,\n\tp_runtime_session_binding_idempotency_key text,\n\tp_expected_conversation_revision bigint,p_expected_turn_revision bigint\n)",
		"p_attempt_id uuid, p_consumer_kind decodex.provider_attempt_consumer_kind, p_conversation_id uuid, p_turn_id uuid, p_managed_run_id uuid, p_managed_run_revision bigint, p_managed_execution_id uuid, p_continuation_plan_id uuid, p_process_generation_id uuid, p_process_generation_revision bigint, p_process_execution_epoch_id uuid, p_request_id uuid, p_request_digest text, p_provider_idempotency_key text, p_provider_correlation_key text, p_predecessor_attempt_id uuid, p_duplicate_risk_ack_digest text, p_runtime_session_binding_protocol text, p_runtime_session_binding_idempotency_key text, p_expected_conversation_revision bigint, p_expected_turn_revision bigint",
		"TABLE(result_code text, revision bigint, state decodex.provider_attempt_state, created_at_micros bigint, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"authorize_provider_attempt_dispatch_exact",
		"decodex.authorize_provider_attempt_dispatch_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
		"authorize_provider_attempt_dispatch_exact(\n\tp_attempt_id uuid,p_expected_revision bigint,\n\tp_process_generation_id uuid,p_process_generation_revision bigint,\n\tp_conversation_id uuid,p_expected_conversation_revision bigint,\n\tp_turn_id uuid,p_expected_turn_revision bigint\n)",
		"p_attempt_id uuid, p_expected_revision bigint, p_process_generation_id uuid, p_process_generation_revision bigint, p_conversation_id uuid, p_expected_conversation_revision bigint, p_turn_id uuid, p_expected_turn_revision bigint",
		"TABLE(result_code text, revision bigint, state decodex.provider_attempt_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"cancel_provider_attempt_exact",
		"decodex.cancel_provider_attempt_exact(pg_catalog.uuid,pg_catalog.int8)",
		"cancel_provider_attempt_exact(\n\tp_attempt_id uuid,\n\tp_expected_revision bigint\n)",
		"p_attempt_id uuid, p_expected_revision bigint",
		"TABLE(result_code text, revision bigint, state decodex.provider_attempt_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"mark_provider_attempt_unknown_exact",
		"decodex.mark_provider_attempt_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_unknown_reason)",
		"mark_provider_attempt_unknown_exact(\n\tp_attempt_id uuid,\n\tp_expected_revision bigint,\n\tp_reason decodex.provider_attempt_unknown_reason\n)",
		"p_attempt_id uuid, p_expected_revision bigint, p_reason decodex.provider_attempt_unknown_reason",
		"TABLE(result_code text, revision bigint, state decodex.provider_attempt_state, updated_at_micros bigint)",
		"v",
	),
	table_function_contract(
		"record_provider_attempt_positive_evidence_exact",
		"decodex.record_provider_attempt_positive_evidence_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
		"record_provider_attempt_positive_evidence_exact(\n\tp_attempt_id uuid,\n\tp_expected_revision bigint,\n\tp_evidence_id uuid,\n\tp_request_id uuid,\n\tp_source decodex.provider_attempt_evidence_source,\n\tp_outcome decodex.provider_attempt_terminal_outcome,\n\tp_provider_key text,\n\tp_provider_receipt_id text,\n\tp_provider_thread_id text,\n\tp_provider_turn_id text,\n\tp_witness_digest text\n)",
		"p_attempt_id uuid, p_expected_revision bigint, p_evidence_id uuid, p_request_id uuid, p_source decodex.provider_attempt_evidence_source, p_outcome decodex.provider_attempt_terminal_outcome, p_provider_key text, p_provider_receipt_id text, p_provider_thread_id text, p_provider_turn_id text, p_witness_digest text",
		"TABLE(result_code text, revision bigint, state decodex.provider_attempt_state, observed_at_micros bigint)",
		"v",
	),
	exact_function_contract(
		"project_provider_attempts_after_supervisor_loss_exact",
		"decodex.project_provider_attempts_after_supervisor_loss_exact()",
		"project_provider_attempts_after_supervisor_loss_exact()",
		"",
		"bigint",
		"plpgsql",
		"v",
	),
	table_function_contract(
		"read_provider_attempts_exact",
		"decodex.read_provider_attempts_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_state,pg_catalog.uuid,pg_catalog.int8)",
		"read_provider_attempts_exact(\n\tp_attempt_id uuid,\n\tp_account_id uuid,\n\tp_state decodex.provider_attempt_state,\n\tp_after_attempt_id uuid,\n\tp_limit bigint\n)",
		"p_attempt_id uuid, p_account_id uuid, p_state decodex.provider_attempt_state, p_after_attempt_id uuid, p_limit bigint",
		"TABLE(attempt_id uuid, consumer_kind decodex.provider_attempt_consumer_kind, conversation_id uuid, turn_id uuid, managed_run_id uuid, managed_run_revision bigint, managed_execution_id uuid, continuation_plan_id uuid, routing_decision_id uuid, accepted_runtime_session_id uuid, accepted_runtime_session_revision bigint, selected_account_id uuid, process_generation_id uuid, process_generation_revision bigint, process_execution_epoch_id uuid, request_id uuid, request_digest text, provider_idempotency_key text, provider_correlation_key text, predecessor_attempt_id uuid, duplicate_risk_ack_digest text, state decodex.provider_attempt_state, unknown_reason decodex.provider_attempt_unknown_reason, terminal_evidence_id uuid, revision bigint, created_at_micros bigint, updated_at_micros bigint)",
		"s",
	),
	exact_function_contract(
		"observe_account_profile_exact",
		"decodex.observe_account_profile_exact(pg_catalog.uuid,pg_catalog.int8,decodex.account_provider_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.int8,pg_catalog.int4,pg_catalog.int4,pg_catalog._text,pg_catalog._int8)",
		"observe_account_profile_exact(\n\tp_account_id uuid,p_expected_revision bigint,\n\tp_expected_provider decodex.account_provider_kind,p_expected_provider_account_id text,\n\tp_observed_at_micros bigint,p_display_name text,p_username text,\n\tp_lifetime_tokens bigint,p_peak_daily_tokens bigint,p_longest_task_seconds bigint,\n\tp_current_streak_days integer,p_longest_streak_days integer,\n\tp_daily_start_dates text[],p_daily_tokens bigint[]\n)",
		"p_account_id uuid, p_expected_revision bigint, p_expected_provider decodex.account_provider_kind, p_expected_provider_account_id text, p_observed_at_micros bigint, p_display_name text, p_username text, p_lifetime_tokens bigint, p_peak_daily_tokens bigint, p_longest_task_seconds bigint, p_current_streak_days integer, p_longest_streak_days integer, p_daily_start_dates text[], p_daily_tokens bigint[]",
		"text",
		"plpgsql",
		"v",
	),
	FunctionContract {
		name: "read_account_profile_exact",
		lookup_signature: "decodex.read_account_profile_exact(pg_catalog.uuid)",
		declaration_signature: "read_account_profile_exact(\n\tp_account_id uuid\n)",
		arguments: "p_account_id uuid",
		result: "TABLE(account_id uuid, account_revision bigint, provider_kind decodex.account_provider_kind, provider_account_id text, observed_at_micros bigint, display_name text, username text, lifetime_tokens bigint, peak_daily_tokens bigint, longest_task_seconds bigint, current_streak_days integer, longest_streak_days integer, daily_start_dates text[], daily_tokens bigint[])",
		language: "sql",
		volatility: "s",
		strict: false,
		returns_set: true,
		rows: 1_000.0,
	},
];
const RUNTIME_EXECUTE_FUNCTIONS: [&str; 106] = [
	"decodex.is_canonical_media_type(pg_catalog.text)",
	"decodex.is_history_metadata_projection(pg_catalog.jsonb)",
	"decodex.normalize_unicode_whitespace(pg_catalog.text)",
	"decodex.ascii_lower(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.jsonb)",
	"decodex.is_meaningful_evidence(pg_catalog.jsonb)",
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
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,decodex.routing_authority_shape,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_codex_experiment_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.text)",
	"decodex.read_codex_experiment_start_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.mark_codex_experiment_title_set_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text)",
	"decodex.attest_codex_experiment_retained_title_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.record_attested_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,decodex.routing_authority_shape,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_quick_task_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.begin_quick_task_initial_route_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.complete_quick_task_initial_route_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.routing_decision_kind,pg_catalog.uuid,pg_catalog.jsonb,pg_catalog.jsonb)",
	"decodex.create_quick_task_routing_successor_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_quick_task_initial_route_exact(pg_catalog.uuid)",
	"decodex.read_quick_task_request_exact(pg_catalog.uuid)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.plan_initial_thread_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.admit_initial_quick_task_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb)",
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
	"decodex.acknowledge_runtime_session_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.provider_attempt_terminal_outcome,pg_catalog.uuid,pg_catalog.text)",
	"decodex.read_ordinary_runtime_session_for_resume_exact(pg_catalog.uuid)",
	"decodex.read_ordinary_task_conversations_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_turn_admission_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.prove_initial_quick_task_spawn_not_created_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.prepare_quick_task_process_generation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.fence_runtime_session_thread_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.text)",
	"decodex.bind_runtime_session_thread_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
	"decodex.read_quick_task_thread_establishment_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.terminalize_quick_task_turn_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.provider_attempt_terminal_outcome,pg_catalog.uuid,pg_catalog.text)",
	"decodex.reconcile_quick_task_terminalizations_exact(pg_catalog.int4)",
	"decodex.prepare_process_generation_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.bind_process_generation_identity_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
	"decodex.mark_process_generation_ready_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_stopping_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_death_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.process_generation_loss_reason)",
	"decodex.record_process_generation_death_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.process_generation_death_evidence_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.text)",
	"decodex.project_process_generations_after_supervisor_loss_exact()",
	"decodex.read_process_generations_exact(pg_catalog.uuid,pg_catalog.bool,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
	"decodex.authorize_provider_attempt_dispatch_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.cancel_provider_attempt_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_provider_attempt_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_unknown_reason)",
	"decodex.record_provider_attempt_positive_evidence_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.project_provider_attempts_after_supervisor_loss_exact()",
	"decodex.read_provider_attempts_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_state,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.observe_account_profile_exact(pg_catalog.uuid,pg_catalog.int8,decodex.account_provider_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.int8,pg_catalog.int4,pg_catalog.int4,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_account_profile_exact(pg_catalog.uuid)",
];
const SAFETY_FUNCTIONS: [&str; 77] = [
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
	"enforce_conversation_routing_successor",
	"enforce_runtime_session_state",
	"enforce_turn_state",
	"enforce_initial_quick_task_admission_complete",
	"enforce_initial_quick_task_admission_owner",
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
	"enforce_managed_run_event_namespace",
	"forbid_managed_repository_history_mutation",
	"enforce_managed_repository_projection",
	"enforce_repository_operation_scope",
	"enforce_repository_history_completeness",
	"forbid_routing_history_mutation",
	"enforce_routing_completeness",
	"enforce_routing_command_owner",
	"forbid_codex_experiment_history_mutation",
	"enforce_codex_experiment_command_owner",
	"forbid_routing_decision_mutation",
	"enforce_routing_decision_completeness",
	"forbid_continuation_plan_mutation",
	"enforce_continuation_plan_completeness",
	"enforce_continuation_event_namespace",
	"enforce_waiting_usage_wake_command_owner",
	"forbid_waiting_usage_wake_transition_mutation",
	"enforce_waiting_usage_wake_transition_complete",
	"enforce_waiting_usage_wake_head_projection",
	"enforce_waiting_usage_wake_event_namespace",
	"enforce_process_generation_transition",
	"record_process_generation_transition",
	"forbid_process_generation_history_mutation",
	"enforce_provider_attempt_transition",
	"enforce_provider_attempt_binding",
	"record_provider_attempt_transition",
	"enforce_provider_attempt_turn_materialization",
	"forbid_provider_attempt_history_mutation",
];
const SAFETY_TRIGGER_COUNT: usize = 151;
const ROLE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.*
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
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
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
), expected(table_name, can_select, can_insert, can_update, can_delete) AS (VALUES
  ('accounts', true, false, false, false),
  ('quota_windows', true, true, true, false),
  ('quota_exclusions', true, true, false, false),
  ('command_receipts', true, true, true, false),
  ('activity', true, true, false, false),
  ('leases', true, true, true, false),
  ('outbox', true, true, true, true),
  ('conversations', true, true, true, false),
	('conversation_routing_successors', false, false, false, false),
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
	,('repository_admissions', true, true, false, false)
	,('managed_repositories', true, true, true, false)
	,('repository_authority_transitions', true, true, false, false)
	,('repository_operations', true, true, false, false)
	,('repository_operation_events', true, true, false, false)
	,('repository_operation_evidence', true, true, false, false)
	,('repository_operation_results', true, true, false, false)
	,('routing_policy_heads', false, false, false, false)
	,('routing_policy_revisions', false, false, false, false)
	,('routing_policy_members', false, false, false, false)
	,('routing_policy_required_capabilities', false, false, false, false)
	,('routing_compatibility_evidence', false, false, false, false)
	,('routing_capability_evidence', false, false, false, false)
	,('routing_snapshots', false, false, false, false)
	,('routing_snapshot_members', false, false, false, false)
	,('routing_snapshot_quota_facts', false, false, false, false)
	,('routing_snapshot_capability_facts', false, false, false, false)
	,('routing_snapshot_blockers', false, false, false, false)
	,('codex_experiments', false, false, false, false)
	,('codex_experiment_revisions', false, false, false, false)
	,('codex_experiment_creation_attempts', false, false, false, false)
	,('codex_experiment_thread_bindings', false, false, false, false)
	,('codex_experiment_observations', false, false, false, false)
	,('codex_experiment_start_receipts', false, false, false, false)
	,('codex_experiment_title_set_attempts', false, false, false, false)
	,('codex_experiment_retained_title_attestations', false, false, false, false)
	,('codex_experiment_attested_observations', false, false, false, false)
	,('routing_decisions', false, false, false, false)
	,('routing_decision_member_refs', false, false, false, false)
	,('routing_decision_quota_refs', false, false, false, false)
	,('routing_decision_capability_refs', false, false, false, false)
	,('routing_decision_blocker_refs', false, false, false, false)
	,('routing_decision_exclusions', false, false, false, false)
	,('continuation_plans', false, false, false, false)
	,('waiting_usage_wake_transitions', false, false, false, false)
	,('waiting_usage_wake_heads', false, false, false, false)
	,('process_generation_execution_epochs', false, false, false, false)
	,('process_generations', false, false, false, false)
	,('process_generation_death_evidence', false, false, false, false)
	,('process_generation_transitions', false, false, false, false)
	,('provider_attempts', false, false, false, false)
	,('provider_attempt_positive_evidence', false, false, false, false)
	,('provider_attempt_transitions', false, false, false, false)
	,('account_operations', false, false, false, false)
	,('account_routing_control', false, false, false, false)
	,('account_routing_order', false, false, false, false)
	,('account_quota_facts', false, false, false, false)
	,('codex_account_capability', false, false, false, false)
	,('account_profile_snapshots', false, false, false, false)
	,('account_profile_daily_usage', false, false, false, false)
), allowed_relations(
  schema_name, table_name, can_select, can_insert, can_update, can_delete
) AS (
  SELECT 'decodex'::pg_catalog.name, expected.* FROM expected
), tables AS (
  SELECT class.oid, class.relname, expected.*
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN expected ON expected.table_name = class.relname
  WHERE namespace.nspname = 'decodex' AND class.relkind IN ('r', 'p')
), relation_like_entries AS (
  SELECT
    class.oid,
    namespace.nspname,
    class.relname,
    allowed.table_name AS allowed_table_name,
    allowed.can_select,
    allowed.can_insert,
    allowed.can_update,
    allowed.can_delete
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN allowed_relations AS allowed
    ON allowed.schema_name = namespace.nspname
   AND allowed.table_name = class.relname
  WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
    AND namespace.nspname !~ '^pg_(toast_)?temp_[0-9]+$'
    AND class.relkind IN ('r', 'p', 'v', 'm', 'f')
)
SELECT
  NOT EXISTS (
    SELECT expected.table_name FROM expected
    EXCEPT
    SELECT tables.relname FROM tables
  )
    AND NOT EXISTS (
      SELECT tables.relname FROM tables
      EXCEPT
      SELECT expected.table_name FROM expected
    )
    AND COALESCE((
      SELECT pg_catalog.bool_and(
		pg_catalog.has_table_privilege($1::pg_catalog.name, oid, 'SELECT') = can_select
		AND pg_catalog.has_table_privilege($1::pg_catalog.name, oid, 'INSERT') = can_insert
		AND pg_catalog.has_table_privilege($1::pg_catalog.name, oid, 'UPDATE') = can_update
		AND pg_catalog.has_table_privilege($1::pg_catalog.name, oid, 'DELETE') = can_delete
      )
      FROM tables WHERE table_name IS NOT NULL
    ), false),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN relation_like_entries AS entry
    WHERE
      pg_catalog.has_table_privilege(role.oid, entry.oid, 'TRUNCATE')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'TRIGGER')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'REFERENCES')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'MAINTAIN')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'SELECT WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'INSERT WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'UPDATE WITH GRANT OPTION')
      OR pg_catalog.has_table_privilege(role.oid, entry.oid, 'DELETE WITH GRANT OPTION')
      OR pg_catalog.has_any_column_privilege(role.oid, entry.oid, 'REFERENCES')
      OR pg_catalog.has_any_column_privilege(
        role.oid,
        entry.oid,
        'SELECT WITH GRANT OPTION, INSERT WITH GRANT OPTION, UPDATE WITH GRANT OPTION, REFERENCES WITH GRANT OPTION'
      )
      OR (entry.allowed_table_name IS NULL AND (
        pg_catalog.has_table_privilege(role.oid, entry.oid, 'SELECT, INSERT, UPDATE, DELETE')
        OR pg_catalog.has_any_column_privilege(
          role.oid,
          entry.oid,
          'SELECT, INSERT, UPDATE, REFERENCES'
        )
      ))
      OR (NOT entry.can_select AND (
        pg_catalog.has_table_privilege(role.oid, entry.oid, 'SELECT')
        OR pg_catalog.has_any_column_privilege(role.oid, entry.oid, 'SELECT')
      ))
      OR (NOT entry.can_insert AND (
        pg_catalog.has_table_privilege(role.oid, entry.oid, 'INSERT')
        OR pg_catalog.has_any_column_privilege(role.oid, entry.oid, 'INSERT')
      ))
      OR (NOT entry.can_update AND (
        pg_catalog.has_table_privilege(role.oid, entry.oid, 'UPDATE')
        OR pg_catalog.has_any_column_privilege(role.oid, entry.oid, 'UPDATE')
      ))
      OR (
        NOT entry.can_delete
        AND pg_catalog.has_table_privilege(role.oid, entry.oid, 'DELETE')
      )
  )
"#;
const SEQUENCE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
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
	  pg_catalog.has_sequence_privilege($1::pg_catalog.name, oid, 'USAGE') = required_usage
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
const PROCESS_GENERATION_TYPE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
), expected(type_name) AS (VALUES
  ('process_generation_state'),
  ('process_generation_control_kind'),
  ('process_generation_isolation_kind'),
  ('process_generation_loss_reason'),
  ('process_generation_death_evidence_kind')
), actual AS (
  SELECT type.oid, type.typname
  FROM pg_catalog.pg_type AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  JOIN expected ON expected.type_name = type.typname
  WHERE namespace.nspname = 'decodex' AND type.typtype = 'e'
)
SELECT
  (SELECT count(*) FROM actual) = 5
    AND COALESCE((
      SELECT pg_catalog.bool_and(
		pg_catalog.has_type_privilege($1::pg_catalog.name, actual.oid, 'USAGE')
      ) FROM actual
    ), false),
  EXISTS (
    SELECT 1
    FROM actual
    JOIN pg_catalog.pg_type AS type ON type.oid = actual.oid
    CROSS JOIN LATERAL pg_catalog.aclexplode(
      COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
    ) AS privilege
    WHERE privilege.grantee = 0 AND privilege.privilege_type = 'USAGE'
  ),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN actual
    WHERE pg_catalog.has_type_privilege(role.oid, actual.oid, 'USAGE WITH GRANT OPTION')
  )
"#;
const PROVIDER_ATTEMPT_TYPE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
), expected(type_name) AS (VALUES
  ('provider_attempt_state'),
  ('provider_attempt_consumer_kind'),
  ('provider_attempt_unknown_reason'),
  ('provider_attempt_evidence_source'),
  ('provider_attempt_terminal_outcome')
), actual AS (
  SELECT type.oid, type.typname
  FROM pg_catalog.pg_type AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  JOIN expected ON expected.type_name = type.typname
  WHERE namespace.nspname = 'decodex' AND type.typtype = 'e'
)
SELECT
  (SELECT count(*) FROM actual) = 5
    AND COALESCE((
      SELECT pg_catalog.bool_and(
		pg_catalog.has_type_privilege($1::pg_catalog.name, actual.oid, 'USAGE')
      ) FROM actual
    ), false),
  EXISTS (
    SELECT 1
    FROM actual
    JOIN pg_catalog.pg_type AS type ON type.oid = actual.oid
    CROSS JOIN LATERAL pg_catalog.aclexplode(
      COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
    ) AS privilege
    WHERE privilege.grantee = 0 AND privilege.privilege_type = 'USAGE'
  ),
  EXISTS (
    SELECT 1
    FROM set_roles AS role
    CROSS JOIN actual
    WHERE pg_catalog.has_type_privilege(role.oid, actual.oid, 'USAGE WITH GRANT OPTION')
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
  ('conversation_routing_successors', 'conversation_routing_successor_complete', 'enforce_conversation_routing_successor', 5),
  ('conversation_routing_successors', 'conversation_routing_successor_immutable', 'enforce_conversation_routing_successor', 27),
  ('profile_snapshots', 'profile_snapshots_created_at_guard', 'canonicalize_created_at', 7),
  ('account_snapshots', 'account_snapshots_created_at_guard', 'canonicalize_created_at', 7),
  ('runtime_sessions', 'runtime_sessions_state_guard', 'enforce_runtime_session_state', 23),
  ('runtime_sessions', 'runtime_sessions_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('blob_objects', 'blob_objects_state_guard', 'enforce_blob_object_state', 7),
  ('turns', 'turns_state_guard', 'enforce_turn_state', 23),
  ('turns', 'turns_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('turns', 'turns_initial_quick_task_admission_complete', 'enforce_initial_quick_task_admission_complete', 5),
  ('turns', 'turns_initial_quick_task_admission_owner', 'enforce_initial_quick_task_admission_owner', 7),
  ('history_items', 'history_items_state_guard', 'enforce_history_item_state', 23),
  ('history_items', 'history_items_version_capture', 'capture_history_item_version', 21),
  ('history_items', 'history_items_coordinator', 'acquire_hierarchy_coordinator', 30),
  ('history_items', 'history_items_initial_quick_task_admission_owner', 'enforce_initial_quick_task_admission_owner', 7),
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
	,('managed_run_assignments', 'managed_run_assignments_immutable', 'forbid_managed_run_immutable_mutation', 27)
	,('managed_run_assignments', 'managed_run_assignment_scope', 'enforce_managed_run_assignment_scope', 5)
	,('managed_runs', 'managed_runs_inert_state', 'enforce_managed_run_state', 31)
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
	,('routing_policy_revisions', 'routing_policy_revisions_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_policy_members', 'routing_policy_members_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_policy_required_capabilities', 'routing_policy_required_capabilities_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_compatibility_evidence', 'routing_compatibility_evidence_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_capability_evidence', 'routing_capability_evidence_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_snapshots', 'routing_snapshots_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_snapshot_members', 'routing_snapshot_members_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_snapshot_quota_facts', 'routing_snapshot_quota_facts_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_snapshot_capability_facts', 'routing_snapshot_capability_facts_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_snapshot_blockers', 'routing_snapshot_blockers_immutable', 'forbid_routing_history_mutation', 58)
	,('routing_policy_heads', 'routing_policy_heads_command_owner', 'enforce_routing_command_owner', 58)
	,('routing_policy_revisions', 'routing_policy_revision_complete', 'enforce_routing_completeness', 5)
	,('routing_compatibility_evidence', 'routing_evidence_complete', 'enforce_routing_completeness', 5)
	,('routing_snapshots', 'routing_snapshot_complete', 'enforce_routing_completeness', 5)
	,('codex_experiment_revisions', 'codex_experiment_revisions_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_creation_attempts', 'codex_experiment_creation_attempts_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_thread_bindings', 'codex_experiment_thread_bindings_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_observations', 'codex_experiment_observations_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_start_receipts', 'codex_experiment_start_receipts_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_title_set_attempts', 'codex_experiment_title_set_attempts_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_retained_title_attestations', 'codex_experiment_retained_title_attestations_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiment_attested_observations', 'codex_experiment_attested_observations_immutable', 'forbid_codex_experiment_history_mutation', 58)
	,('codex_experiments', 'codex_experiments_command_owner', 'enforce_codex_experiment_command_owner', 62)
	,('routing_decisions', 'routing_decisions_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_member_refs', 'routing_decision_member_refs_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_quota_refs', 'routing_decision_quota_refs_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_capability_refs', 'routing_decision_capability_refs_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_blocker_refs', 'routing_decision_blocker_refs_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_exclusions', 'routing_decision_exclusions_immutable', 'forbid_routing_decision_mutation', 58)
	,('routing_decision_member_refs', 'routing_decision_member_refs_open_insert', 'forbid_routing_decision_mutation', 7)
	,('routing_decision_quota_refs', 'routing_decision_quota_refs_open_insert', 'forbid_routing_decision_mutation', 7)
	,('routing_decision_capability_refs', 'routing_decision_capability_refs_open_insert', 'forbid_routing_decision_mutation', 7)
	,('routing_decision_blocker_refs', 'routing_decision_blocker_refs_open_insert', 'forbid_routing_decision_mutation', 7)
	,('routing_decision_exclusions', 'routing_decision_exclusions_open_insert', 'forbid_routing_decision_mutation', 7)
	,('routing_decisions', 'routing_decision_complete', 'enforce_routing_decision_completeness', 5)
	,('continuation_plans', 'continuation_plans_command_owner', 'forbid_continuation_plan_mutation', 62)
	,('continuation_plans', 'continuation_plan_complete', 'enforce_continuation_plan_completeness', 5)
	,('activity', 'activity_continuation_namespace', 'enforce_continuation_event_namespace', 23)
	,('outbox', 'outbox_continuation_namespace', 'enforce_continuation_event_namespace', 23)
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transitions_command_owner', 'enforce_waiting_usage_wake_command_owner', 62)
	,('waiting_usage_wake_heads', 'waiting_usage_wake_heads_command_owner', 'enforce_waiting_usage_wake_command_owner', 62)
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transitions_immutable', 'forbid_waiting_usage_wake_transition_mutation', 58)
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transition_complete', 'enforce_waiting_usage_wake_transition_complete', 5)
	,('waiting_usage_wake_heads', 'waiting_usage_wake_head_projection', 'enforce_waiting_usage_wake_head_projection', 29)
	,('activity', 'activity_waiting_usage_wake_namespace', 'enforce_waiting_usage_wake_event_namespace', 23)
	,('outbox', 'outbox_waiting_usage_wake_namespace', 'enforce_waiting_usage_wake_event_namespace', 23)
	,('process_generations', 'process_generation_transition_guard', 'enforce_process_generation_transition', 23)
	,('process_generations', 'process_generation_delete_immutable', 'forbid_process_generation_history_mutation', 42)
	,('process_generations', 'process_generation_transition_record', 'record_process_generation_transition', 21)
	,('process_generation_death_evidence', 'process_generation_death_evidence_immutable', 'forbid_process_generation_history_mutation', 58)
	,('process_generation_transitions', 'process_generation_transitions_immutable', 'forbid_process_generation_history_mutation', 58)
	,('provider_attempts', 'provider_attempt_transition_guard', 'enforce_provider_attempt_transition', 23)
	,('provider_attempts', 'provider_attempt_binding_complete', 'enforce_provider_attempt_binding', 5)
	,('provider_attempts', 'provider_attempt_delete_immutable', 'forbid_provider_attempt_history_mutation', 42)
	,('provider_attempts', 'provider_attempt_transition_record', 'record_provider_attempt_transition', 21)
	,('turns', 'turns_provider_attempt_materialization', 'enforce_provider_attempt_turn_materialization', 23)
	,('provider_attempt_positive_evidence', 'provider_attempt_positive_evidence_immutable', 'forbid_provider_attempt_history_mutation', 58)
	,('provider_attempt_transitions', 'provider_attempt_transitions_immutable', 'forbid_provider_attempt_history_mutation', 58)
)
SELECT
  expected.function_name,
  trigger.oid IS NOT NULL
    AND trigger.tgenabled = 'O'
    AND trigger.tgtype = expected.trigger_type
    AND trigger.tgparentid = 0
    AND (trigger.tgconstraint <> 0) = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete', 'routing_policy_revision_complete', 'routing_evidence_complete', 'routing_snapshot_complete', 'routing_decision_complete', 'continuation_plan_complete', 'waiting_usage_wake_transition_complete', 'waiting_usage_wake_head_projection', 'provider_attempt_binding_complete', 'conversation_routing_successor_complete', 'turns_initial_quick_task_admission_complete')
    )
    AND trigger.tgconstrrelid = 0
    AND trigger.tgconstrindid = 0
    AND trigger.tgdeferrable = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete', 'routing_policy_revision_complete', 'routing_evidence_complete', 'routing_snapshot_complete', 'routing_decision_complete', 'continuation_plan_complete', 'waiting_usage_wake_transition_complete', 'waiting_usage_wake_head_projection', 'provider_attempt_binding_complete', 'conversation_routing_successor_complete', 'turns_initial_quick_task_admission_complete')
    )
    AND trigger.tginitdeferred = (
      expected.trigger_name IN ('objectives_completion_coherence', 'objective_evidence_completion_coherence', 'exact_receipts_complete_at_commit', 'role_profiles_exact_global_set', 'work_item_acceptance_coherence', 'managed_run_assignment_scope', 'managed_repositories_projection_complete', 'repository_operations_scope_complete', 'repository_operation_evidence_complete', 'repository_operation_results_complete', 'repository_operation_events_complete', 'repository_authority_transitions_complete', 'routing_policy_revision_complete', 'routing_evidence_complete', 'routing_snapshot_complete', 'routing_decision_complete', 'continuation_plan_complete', 'waiting_usage_wake_transition_complete', 'waiting_usage_wake_head_projection', 'provider_attempt_binding_complete', 'conversation_routing_successor_complete', 'turns_initial_quick_task_admission_complete')
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
    AND proc.prosecdef = (
      expected.function_name IN (
        'capture_history_item_version',
        'enforce_provider_attempt_turn_materialization'
      )
    )
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
	pg_catalog.has_function_privilege($2::pg_catalog.name, proc.oid, 'EXECUTE'),
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
const RUNTIME_ROUTINE_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $2::pg_catalog.name
     OR pg_catalog.pg_has_role($2::pg_catalog.name, role.oid, 'SET')
), expected_runtime_routines(oid) AS (
  SELECT pg_catalog.to_regprocedure(identity)
  FROM pg_catalog.unnest($1::pg_catalog.text[]) AS identity
), required_digest AS (
  SELECT
    proc.*,
    extension.oid AS extension_oid,
    extension.extowner,
    extension.extnamespace,
    extension.extrelocatable,
    extension.extversion,
    extension.extconfig,
    extension.extcondition,
    namespace.nspname,
    language.lanname
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
  JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
  JOIN pg_catalog.pg_extension AS extension ON extension.extname = 'pgcrypto'
  WHERE proc.oid = pg_catalog.to_regprocedure(
    'public.digest(pg_catalog.bytea,pg_catalog.text)'
  )
), digest_contract AS (
  SELECT
    EXISTS (SELECT 1 FROM required_digest) AS exists,
    COALESCE(pg_catalog.bool_and(
      required_digest.extversion = '1.4'
      AND required_digest.extrelocatable
      AND required_digest.extconfig IS NULL
      AND required_digest.extcondition IS NULL
      AND required_digest.nspname = 'public'
      AND required_digest.pronamespace = required_digest.extnamespace
      AND required_digest.extowner = (
        SELECT namespace.nspowner
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname = 'decodex'
      )
      AND required_digest.proowner <> required_digest.extowner
      AND EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles AS owner
        WHERE owner.oid = required_digest.proowner
          AND owner.rolsuper
      )
      AND NOT EXISTS (
        SELECT 1
        FROM set_roles AS role
        WHERE role.oid = required_digest.extowner
           OR pg_catalog.pg_has_role(role.oid, required_digest.extowner, 'USAGE')
           OR role.oid = required_digest.proowner
           OR pg_catalog.pg_has_role(role.oid, required_digest.proowner, 'USAGE')
      )
      AND required_digest.proname = 'digest'
      AND required_digest.pronargs = 2
      AND pg_catalog.array_ndims(
        required_digest.proargtypes::pg_catalog.oid[]
      ) = 1
      AND pg_catalog.array_lower(
        required_digest.proargtypes::pg_catalog.oid[], 1
      ) = 0
      AND pg_catalog.array_upper(
        required_digest.proargtypes::pg_catalog.oid[], 1
      ) = 1
      AND required_digest.proargtypes[0] =
        'pg_catalog.bytea'::pg_catalog.regtype::pg_catalog.oid
      AND required_digest.proargtypes[1] =
        'pg_catalog.text'::pg_catalog.regtype::pg_catalog.oid
      AND required_digest.proallargtypes IS NULL
      AND required_digest.proargmodes IS NULL
      AND required_digest.proargnames IS NULL
      AND required_digest.prorettype = 'pg_catalog.bytea'::pg_catalog.regtype
      AND required_digest.prokind = 'f'
      AND required_digest.lanname = 'c'
      AND required_digest.provolatile = 'i'
      AND required_digest.proparallel = 's'
      AND required_digest.proisstrict
      AND NOT required_digest.proretset
      AND NOT required_digest.prosecdef
      AND NOT required_digest.proleakproof
      AND required_digest.procost = 1
      AND required_digest.prorows = 0
      AND required_digest.proconfig IS NULL
      AND required_digest.probin = '$libdir/pgcrypto'
      AND required_digest.prosrc = 'pg_digest'
      AND required_digest.prosqlbody IS NULL
      AND required_digest.prosupport = 0
      AND required_digest.provariadic = 0
      AND required_digest.protrftypes IS NULL
      AND required_digest.pronargdefaults = 0
      AND required_digest.proargdefaults IS NULL
      AND required_digest.proacl IS NULL
      AND (
        SELECT pg_catalog.count(*) = 1
        FROM pg_catalog.pg_depend AS dependency
        WHERE dependency.classid = 'pg_catalog.pg_proc'::pg_catalog.regclass
          AND dependency.objid = required_digest.oid
          AND dependency.objsubid = 0
          AND dependency.refclassid =
            'pg_catalog.pg_extension'::pg_catalog.regclass
          AND dependency.refobjid = required_digest.extension_oid
          AND dependency.refobjsubid = 0
          AND dependency.deptype = 'e'
      )
      AND (
        SELECT pg_catalog.bool_and(
          pg_catalog.has_function_privilege(role.oid, required_digest.oid, 'EXECUTE')
        )
        FROM set_roles AS role
      )
      AND NOT EXISTS (
        SELECT 1
        FROM set_roles AS role
        WHERE pg_catalog.has_function_privilege(
          role.oid,
          required_digest.oid,
          'EXECUTE WITH GRANT OPTION'
        )
      )
    ), false) AS exact
  FROM required_digest
)
SELECT
  EXISTS (
    SELECT 1
    FROM pg_catalog.pg_proc AS proc
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
      AND namespace.nspname !~ '^pg_(toast_)?temp_[0-9]+$'
      -- CREATE FUNCTION permits WINDOW with SECURITY DEFINER; CREATE AGGREGATE does not.
      AND proc.prokind IN ('f', 'p', 'w')
      AND proc.prosecdef
      AND proc.oid NOT IN (
        SELECT oid FROM expected_runtime_routines WHERE oid IS NOT NULL
      )
      AND EXISTS (
        SELECT 1
        FROM set_roles AS role
        WHERE pg_catalog.has_function_privilege(role.oid, proc.oid, 'EXECUTE')
      )
  ),
  digest_contract.exists,
  digest_contract.exact
FROM digest_contract
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
  ('conversation_routing_successors', 'conversation_routing_successor_complete', 'decodex.enforce_conversation_routing_successor()'),
  ('conversation_routing_successors', 'conversation_routing_successor_immutable', 'decodex.enforce_conversation_routing_successor()'),
  ('profile_snapshots', 'profile_snapshots_created_at_guard', 'decodex.canonicalize_created_at()'),
  ('account_snapshots', 'account_snapshots_created_at_guard', 'decodex.canonicalize_created_at()'),
  ('runtime_sessions', 'runtime_sessions_state_guard', 'decodex.enforce_runtime_session_state()'),
  ('runtime_sessions', 'runtime_sessions_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('blob_objects', 'blob_objects_state_guard', 'decodex.enforce_blob_object_state()'),
  ('turns', 'turns_state_guard', 'decodex.enforce_turn_state()'),
  ('turns', 'turns_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('turns', 'turns_initial_quick_task_admission_complete', 'decodex.enforce_initial_quick_task_admission_complete()'),
  ('turns', 'turns_initial_quick_task_admission_owner', 'decodex.enforce_initial_quick_task_admission_owner()'),
  ('history_items', 'history_items_state_guard', 'decodex.enforce_history_item_state()'),
  ('history_items', 'history_items_coordinator', 'decodex.acquire_hierarchy_coordinator()'),
  ('history_items', 'history_items_version_capture', 'decodex.capture_history_item_version()'),
  ('history_items', 'history_items_initial_quick_task_admission_owner', 'decodex.enforce_initial_quick_task_admission_owner()'),
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
	,('managed_run_assignments', 'managed_run_assignments_immutable', 'decodex.forbid_managed_run_immutable_mutation()')
	,('managed_run_assignments', 'managed_run_assignment_scope', 'decodex.enforce_managed_run_assignment_scope()')
	,('managed_runs', 'managed_runs_inert_state', 'decodex.enforce_managed_run_state()')
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
	,('routing_policy_revisions', 'routing_policy_revisions_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_policy_members', 'routing_policy_members_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_policy_required_capabilities', 'routing_policy_required_capabilities_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_compatibility_evidence', 'routing_compatibility_evidence_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_capability_evidence', 'routing_capability_evidence_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_snapshots', 'routing_snapshots_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_snapshot_members', 'routing_snapshot_members_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_snapshot_quota_facts', 'routing_snapshot_quota_facts_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_snapshot_capability_facts', 'routing_snapshot_capability_facts_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_snapshot_blockers', 'routing_snapshot_blockers_immutable', 'decodex.forbid_routing_history_mutation()')
	,('routing_policy_heads', 'routing_policy_heads_command_owner', 'decodex.enforce_routing_command_owner()')
	,('routing_policy_revisions', 'routing_policy_revision_complete', 'decodex.enforce_routing_completeness()')
	,('routing_compatibility_evidence', 'routing_evidence_complete', 'decodex.enforce_routing_completeness()')
	,('routing_snapshots', 'routing_snapshot_complete', 'decodex.enforce_routing_completeness()')
	,('codex_experiment_revisions', 'codex_experiment_revisions_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_creation_attempts', 'codex_experiment_creation_attempts_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_thread_bindings', 'codex_experiment_thread_bindings_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_observations', 'codex_experiment_observations_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_start_receipts', 'codex_experiment_start_receipts_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_title_set_attempts', 'codex_experiment_title_set_attempts_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_retained_title_attestations', 'codex_experiment_retained_title_attestations_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiment_attested_observations', 'codex_experiment_attested_observations_immutable', 'decodex.forbid_codex_experiment_history_mutation()')
	,('codex_experiments', 'codex_experiments_command_owner', 'decodex.enforce_codex_experiment_command_owner()')
	,('routing_decisions', 'routing_decisions_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_member_refs', 'routing_decision_member_refs_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_quota_refs', 'routing_decision_quota_refs_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_capability_refs', 'routing_decision_capability_refs_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_blocker_refs', 'routing_decision_blocker_refs_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_exclusions', 'routing_decision_exclusions_immutable', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_member_refs', 'routing_decision_member_refs_open_insert', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_quota_refs', 'routing_decision_quota_refs_open_insert', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_capability_refs', 'routing_decision_capability_refs_open_insert', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_blocker_refs', 'routing_decision_blocker_refs_open_insert', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decision_exclusions', 'routing_decision_exclusions_open_insert', 'decodex.forbid_routing_decision_mutation()')
	,('routing_decisions', 'routing_decision_complete', 'decodex.enforce_routing_decision_completeness()')
	,('continuation_plans', 'continuation_plans_command_owner', 'decodex.forbid_continuation_plan_mutation()')
	,('continuation_plans', 'continuation_plan_complete', 'decodex.enforce_continuation_plan_completeness()')
	,('activity', 'activity_continuation_namespace', 'decodex.enforce_continuation_event_namespace()')
	,('outbox', 'outbox_continuation_namespace', 'decodex.enforce_continuation_event_namespace()')
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transitions_command_owner', 'decodex.enforce_waiting_usage_wake_command_owner()')
	,('waiting_usage_wake_heads', 'waiting_usage_wake_heads_command_owner', 'decodex.enforce_waiting_usage_wake_command_owner()')
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transitions_immutable', 'decodex.forbid_waiting_usage_wake_transition_mutation()')
	,('waiting_usage_wake_transitions', 'waiting_usage_wake_transition_complete', 'decodex.enforce_waiting_usage_wake_transition_complete()')
	,('waiting_usage_wake_heads', 'waiting_usage_wake_head_projection', 'decodex.enforce_waiting_usage_wake_head_projection()')
	,('activity', 'activity_waiting_usage_wake_namespace', 'decodex.enforce_waiting_usage_wake_event_namespace()')
	,('outbox', 'outbox_waiting_usage_wake_namespace', 'decodex.enforce_waiting_usage_wake_event_namespace()')
	,('process_generations', 'process_generation_transition_guard', 'decodex.enforce_process_generation_transition()')
	,('process_generations', 'process_generation_delete_immutable', 'decodex.forbid_process_generation_history_mutation()')
	,('process_generations', 'process_generation_transition_record', 'decodex.record_process_generation_transition()')
	,('process_generation_death_evidence', 'process_generation_death_evidence_immutable', 'decodex.forbid_process_generation_history_mutation()')
	,('process_generation_transitions', 'process_generation_transitions_immutable', 'decodex.forbid_process_generation_history_mutation()')
	,('provider_attempts', 'provider_attempt_transition_guard', 'decodex.enforce_provider_attempt_transition()')
	,('provider_attempts', 'provider_attempt_binding_complete', 'decodex.enforce_provider_attempt_binding()')
	,('provider_attempts', 'provider_attempt_delete_immutable', 'decodex.forbid_provider_attempt_history_mutation()')
	,('provider_attempts', 'provider_attempt_transition_record', 'decodex.record_provider_attempt_transition()')
	,('turns', 'turns_provider_attempt_materialization', 'decodex.enforce_provider_attempt_turn_materialization()')
	,('provider_attempt_positive_evidence', 'provider_attempt_positive_evidence_immutable', 'decodex.forbid_provider_attempt_history_mutation()')
	,('provider_attempt_transitions', 'provider_attempt_transitions_immutable', 'decodex.forbid_provider_attempt_history_mutation()')
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
  SELECT class.*, namespace.nspname
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE class.relnamespace IN (SELECT oid FROM decodex_namespace)
    AND class.relkind IN ('r', 'p')
), relation_keys AS MATERIALIZED (
  SELECT
    class.oid,
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname, class.relkind) AS key
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
), type_keys AS MATERIALIZED (
  SELECT
    type.oid,
    pg_catalog.jsonb_build_array(namespace.nspname, type.typname) AS key
  FROM pg_catalog.pg_type AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
), function_keys AS MATERIALIZED (
  SELECT
    proc.oid,
    pg_catalog.jsonb_build_array(
      namespace.nspname,
      proc.proname,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(argument_type.key ORDER BY argument.ordinality)
        FROM pg_catalog.unnest(proc.proargtypes::pg_catalog.oid[])
          WITH ORDINALITY AS argument(type_oid, ordinality)
        LEFT JOIN type_keys AS argument_type ON argument_type.oid = argument.type_oid
      ), '[]'::pg_catalog.jsonb)
    ) AS key,
    COALESCE((
      SELECT pg_catalog.bool_and(argument_type.oid IS NOT NULL)
      FROM pg_catalog.unnest(proc.proargtypes::pg_catalog.oid[]) AS argument(type_oid)
      LEFT JOIN type_keys AS argument_type ON argument_type.oid = argument.type_oid
    ), true) AS resolved
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
), constraint_keys AS MATERIALIZED (
  SELECT
    con.oid,
    CASE
      WHEN con.conrelid <> 0 THEN pg_catalog.jsonb_build_array(
        'relation', relation_namespace.nspname, class.relname, con.conname, con.contype
      )
      WHEN con.contypid <> 0 THEN pg_catalog.jsonb_build_array(
        'domain', domain_namespace.nspname, domain_type.typname, con.conname, con.contype
      )
      ELSE pg_catalog.jsonb_build_array(
        'unresolved_owner', con.conname, con.contype
      )
    END AS key,
    CASE
      WHEN con.conrelid <> 0 THEN class.oid IS NOT NULL AND relation_namespace.oid IS NOT NULL
      WHEN con.contypid <> 0 THEN domain_type.oid IS NOT NULL AND domain_namespace.oid IS NOT NULL
      ELSE false
    END AS resolved
  FROM pg_catalog.pg_constraint AS con
  LEFT JOIN pg_catalog.pg_class AS class ON class.oid = con.conrelid
  LEFT JOIN pg_catalog.pg_namespace AS relation_namespace
    ON relation_namespace.oid = class.relnamespace
  LEFT JOIN pg_catalog.pg_type AS domain_type ON domain_type.oid = con.contypid
  LEFT JOIN pg_catalog.pg_namespace AS domain_namespace
    ON domain_namespace.oid = domain_type.typnamespace
), touching_constraints AS (
  SELECT con.*
  FROM pg_catalog.pg_constraint AS con
  WHERE con.conrelid IN (SELECT oid FROM decodex_relations)
     OR con.confrelid IN (SELECT oid FROM decodex_relations)
), decodex_domain_constraints AS (
  SELECT con.*
  FROM pg_catalog.pg_constraint AS con
  WHERE con.contypid IN (
    SELECT type.oid
    FROM pg_catalog.pg_type AS type
    WHERE type.typnamespace IN (SELECT oid FROM decodex_namespace)
  )
), schema_constraints AS (
  SELECT * FROM touching_constraints
  UNION ALL
  SELECT * FROM decodex_domain_constraints AS domain_constraint
  WHERE domain_constraint.oid NOT IN (SELECT oid FROM touching_constraints)
), relevant_triggers AS (
  SELECT trigger.*
  FROM pg_catalog.pg_trigger AS trigger
  WHERE trigger.tgrelid IN (SELECT oid FROM decodex_relations)
     OR trigger.tgconstraint IN (SELECT oid FROM touching_constraints)
), relevant_internal_triggers AS (
  SELECT trigger.*
  FROM relevant_triggers AS trigger
  WHERE trigger.tgisinternal
), trigger_keys AS MATERIALIZED (
  SELECT
    trigger.oid,
    CASE
      WHEN trigger.tgisinternal THEN pg_catalog.jsonb_build_array(
        relation_namespace.nspname,
        relation.relname,
        constraint_key.key,
        function_key.key
      )
      ELSE pg_catalog.jsonb_build_array(
        'user_trigger', relation_key.key, trigger.tgname
      )
    END AS key,
    CASE
      WHEN trigger.tgisinternal THEN COALESCE(function_key.resolved AND (
        trigger.tgconstraint = 0 OR constraint_key.resolved
      ), false)
      ELSE relation_key.oid IS NOT NULL
    END AS resolved,
    trigger.tgisinternal AS is_internal
  FROM relevant_triggers AS trigger
  JOIN pg_catalog.pg_class AS relation ON relation.oid = trigger.tgrelid
  JOIN pg_catalog.pg_namespace AS relation_namespace
    ON relation_namespace.oid = relation.relnamespace
  JOIN relation_keys AS relation_key ON relation_key.oid = trigger.tgrelid
  LEFT JOIN touching_constraints AS con ON con.oid = trigger.tgconstraint
  LEFT JOIN constraint_keys AS constraint_key ON constraint_key.oid = con.oid
  JOIN function_keys AS function_key ON function_key.oid = trigger.tgfoid
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
  WHERE role.rolname = $1::pg_catalog.name
), authority_dependency_targets(kind, identity, classid, objid, objsubid, resolved) AS (
  SELECT
    'function_dependency',
    function_key.key,
    'pg_catalog.pg_proc'::pg_catalog.regclass,
    proc.oid,
    0,
    function_key.resolved
  FROM decodex_functions AS proc
  JOIN function_keys AS function_key ON function_key.oid = proc.oid
  UNION ALL
  SELECT
    'type_dependency',
    type_key.key,
    'pg_catalog.pg_type'::pg_catalog.regclass,
    type.oid,
    0,
    true
  FROM decodex_types AS type
  JOIN type_keys AS type_key ON type_key.oid = type.oid
), dependency_targets(kind, identity, classid, objid, objsubid, resolved) AS (
  SELECT
    'default',
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname, attribute.attname),
    'pg_catalog.pg_attrdef'::pg_catalog.regclass,
    attrdef.oid,
    0,
    true
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
    constraint_key.key,
    'pg_catalog.pg_constraint'::pg_catalog.regclass,
    con.oid,
    0,
    constraint_key.resolved
  FROM schema_constraints AS con
  JOIN constraint_keys AS constraint_key ON constraint_key.oid = con.oid
  UNION ALL
  SELECT
    'index',
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname),
    'pg_catalog.pg_class'::pg_catalog.regclass,
    class.oid,
    0,
    true
  FROM pg_catalog.pg_index AS index
  JOIN pg_catalog.pg_class AS class ON class.oid = index.indexrelid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  WHERE index.indrelid IN (SELECT oid FROM decodex_relations)
  UNION ALL
  SELECT
    'internal_trigger',
    trigger_key.key,
    'pg_catalog.pg_trigger'::pg_catalog.regclass,
    trigger.oid,
    0,
    trigger_key.resolved
  FROM relevant_internal_triggers AS trigger
  JOIN trigger_keys AS trigger_key
    ON trigger_key.oid = trigger.oid AND trigger_key.is_internal
), all_dependency_targets(
  kind, identity, classid, objid, objsubid, resolved
) AS MATERIALIZED (
  SELECT * FROM authority_dependency_targets
  UNION ALL
  SELECT * FROM dependency_targets
), raw_dependency_edges(
  target_classid, target_objid, target_objsubid,
  dependency_type, refclassid, refobjid, refobjsubid
) AS MATERIALIZED (
  SELECT DISTINCT
    target.classid,
    target.objid,
    target.objsubid,
    dependency.deptype,
    dependency.refclassid,
    dependency.refobjid,
    dependency.refobjsubid
  FROM all_dependency_targets AS target
  JOIN pg_catalog.pg_depend AS dependency
    ON dependency.classid = target.classid
   AND dependency.objid = target.objid
   AND dependency.objsubid = target.objsubid
), raw_dependency_rows(
  kind, identity, target_resolved, target_classid, target_objid, target_objsubid,
  dependency_type, refclassid, refobjid, refobjsubid
) AS (
  SELECT
    target.kind,
    target.identity,
    target.resolved,
    target.classid,
    target.objid,
    target.objsubid,
    dependency.dependency_type,
    dependency.refclassid,
    dependency.refobjid,
    dependency.refobjsubid
  FROM all_dependency_targets AS target
  JOIN raw_dependency_edges AS dependency
    ON dependency.target_classid = target.classid
   AND dependency.target_objid = target.objid
   AND dependency.target_objsubid = target.objsubid
), mapped_dependency_references AS MATERIALIZED (
  SELECT
    dependency.kind,
    dependency.identity,
    dependency.dependency_type,
    CASE
      WHEN dependency.refclassid = 'pg_catalog.pg_proc'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('function')
      WHEN dependency.refclassid = 'pg_catalog.pg_type'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('type')
      WHEN dependency.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('relation_or_column')
      WHEN dependency.refclassid = 'pg_catalog.pg_constraint'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('constraint')
      WHEN dependency.refclassid = 'pg_catalog.pg_trigger'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('trigger')
      WHEN dependency.refclassid = 'pg_catalog.pg_collation'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('collation')
      WHEN dependency.refclassid = 'pg_catalog.pg_operator'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('operator')
      WHEN dependency.refclassid = 'pg_catalog.pg_namespace'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('namespace')
      WHEN dependency.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('extension')
      WHEN dependency.refclassid = 'pg_catalog.pg_language'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('language')
      WHEN dependency.refclassid = 'pg_catalog.pg_opclass'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('operator_class')
      WHEN dependency.refclassid = 'pg_catalog.pg_opfamily'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('operator_family')
      WHEN dependency.refclassid = 'pg_catalog.pg_cast'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('cast')
      WHEN dependency.refclassid = 'pg_catalog.pg_am'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('access_method')
      WHEN dependency.refclassid = 'pg_catalog.pg_attrdef'::pg_catalog.regclass
        THEN pg_catalog.jsonb_build_array('default')
      ELSE pg_catalog.jsonb_build_array(
        'catalog_class', reference_class_namespace.nspname, reference_class.relname
      )
    END AS reference_class,
    CASE
        WHEN dependency.refclassid = 'pg_catalog.pg_proc'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('function', function_key.key)
          FROM function_keys AS function_key
          WHERE function_key.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_type'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('type', type_key.key)
          FROM type_keys AS type_key
          WHERE type_key.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass THEN (
          SELECT CASE
            WHEN dependency.refobjsubid = 0 THEN
              pg_catalog.jsonb_build_array(
                CASE WHEN class.relkind = 'S' THEN 'sequence' ELSE 'relation' END,
                relation_key.key
              )
            ELSE pg_catalog.jsonb_build_array(
              'column', relation_key.key, attribute.attname
            )
          END
          FROM pg_catalog.pg_class AS class
          JOIN relation_keys AS relation_key ON relation_key.oid = class.oid
          LEFT JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = class.oid
           AND attribute.attnum = dependency.refobjsubid
          WHERE class.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_constraint'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('constraint', constraint_key.key)
          FROM constraint_keys AS constraint_key
          WHERE constraint_key.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_trigger'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('trigger', trigger_key.key)
          FROM trigger_keys AS trigger_key
          WHERE trigger_key.oid = dependency.refobjid
            AND dependency.refobjsubid = 0
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_collation'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('collation', namespace.nspname, coll.collname)
          FROM pg_catalog.pg_collation AS coll
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = coll.collnamespace
          WHERE coll.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_operator'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array(
            'operator', namespace.nspname, operator.oprname,
            CASE
              WHEN operator.oprleft = 0 THEN pg_catalog.jsonb_build_array('absent')
              ELSE left_type.key
            END,
            CASE
              WHEN operator.oprright = 0 THEN pg_catalog.jsonb_build_array('absent')
              ELSE right_type.key
            END
          )
          FROM pg_catalog.pg_operator AS operator
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator.oprnamespace
          LEFT JOIN type_keys AS left_type ON left_type.oid = operator.oprleft
          LEFT JOIN type_keys AS right_type ON right_type.oid = operator.oprright
          WHERE operator.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_namespace'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('namespace', namespace.nspname)
          FROM pg_catalog.pg_namespace AS namespace
          WHERE namespace.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('extension', extension.extname)
          FROM pg_catalog.pg_extension AS extension
          WHERE extension.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_language'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('language', language.lanname)
          FROM pg_catalog.pg_language AS language
          WHERE language.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_opclass'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array(
            'operator_class', namespace.nspname, operator_class.opcname,
            access_method.amname, input_type.key
          )
          FROM pg_catalog.pg_opclass AS operator_class
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator_class.opcnamespace
          JOIN pg_catalog.pg_am AS access_method ON access_method.oid = operator_class.opcmethod
          JOIN type_keys AS input_type ON input_type.oid = operator_class.opcintype
          WHERE operator_class.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_opfamily'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array(
            'operator_family', namespace.nspname, operator_family.opfname,
            access_method.amname
          )
          FROM pg_catalog.pg_opfamily AS operator_family
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator_family.opfnamespace
          JOIN pg_catalog.pg_am AS access_method ON access_method.oid = operator_family.opfmethod
          WHERE operator_family.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_cast'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array(
            'cast', source_type.key, target_type.key, conversion.castcontext, conversion.castmethod
          )
          FROM pg_catalog.pg_cast AS conversion
          JOIN type_keys AS source_type ON source_type.oid = conversion.castsource
          JOIN type_keys AS target_type ON target_type.oid = conversion.casttarget
          WHERE conversion.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_am'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array('access_method', access_method.amname)
          FROM pg_catalog.pg_am AS access_method
          WHERE access_method.oid = dependency.refobjid
        )
        WHEN dependency.refclassid = 'pg_catalog.pg_attrdef'::pg_catalog.regclass THEN (
          SELECT pg_catalog.jsonb_build_array(
            'default', namespace.nspname, class.relname, attribute.attname
          )
          FROM pg_catalog.pg_attrdef AS attrdef
          JOIN pg_catalog.pg_class AS class ON class.oid = attrdef.adrelid
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
          JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = attrdef.adrelid
           AND attribute.attnum = attrdef.adnum
           AND NOT attribute.attisdropped
          WHERE attrdef.oid = dependency.refobjid
        )
    END AS reference_key,
    COALESCE(dependency.target_resolved, false) AND CASE
      WHEN dependency.refclassid = 'pg_catalog.pg_proc'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM function_keys AS function_key
          WHERE function_key.oid = dependency.refobjid AND function_key.resolved
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_type'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM type_keys AS type_key WHERE type_key.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass THEN
        EXISTS (
          SELECT 1
          FROM relation_keys AS relation_key
          WHERE relation_key.oid = dependency.refobjid
            AND (
              dependency.refobjsubid = 0
              OR dependency.refobjsubid > 0 AND EXISTS (
                SELECT 1 FROM pg_catalog.pg_attribute AS attribute
                WHERE attribute.attrelid = dependency.refobjid
                  AND attribute.attnum = dependency.refobjsubid
                  AND NOT attribute.attisdropped
              )
            )
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_constraint'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM constraint_keys AS constraint_key
          WHERE constraint_key.oid = dependency.refobjid AND constraint_key.resolved
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_trigger'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM trigger_keys AS trigger_key
          JOIN pg_catalog.pg_trigger AS referenced_trigger
            ON referenced_trigger.oid = trigger_key.oid
          WHERE trigger_key.oid = dependency.refobjid
            AND trigger_key.resolved
            AND (
              dependency.target_classid <> 'pg_catalog.pg_constraint'::pg_catalog.regclass
              OR dependency.target_objsubid = 0
                AND referenced_trigger.tgconstraint = dependency.target_objid
            )
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_collation'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_collation AS coll
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = coll.collnamespace
          WHERE coll.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_operator'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_operator AS operator
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator.oprnamespace
          LEFT JOIN type_keys AS left_type ON left_type.oid = operator.oprleft
          LEFT JOIN type_keys AS right_type ON right_type.oid = operator.oprright
          WHERE operator.oid = dependency.refobjid
            AND (operator.oprleft = 0 OR left_type.oid IS NOT NULL)
            AND (operator.oprright = 0 OR right_type.oid IS NOT NULL)
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_namespace'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_namespace AS namespace
          WHERE namespace.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_extension AS extension
          WHERE extension.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_language'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_language AS language
          WHERE language.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_opclass'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_opclass AS operator_class
          JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = operator_class.opcnamespace
          JOIN pg_catalog.pg_am AS access_method ON access_method.oid = operator_class.opcmethod
          JOIN type_keys AS input_type ON input_type.oid = operator_class.opcintype
          WHERE operator_class.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_opfamily'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_opfamily AS operator_family
          JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = operator_family.opfnamespace
          JOIN pg_catalog.pg_am AS access_method ON access_method.oid = operator_family.opfmethod
          WHERE operator_family.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_cast'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_cast AS conversion
          JOIN type_keys AS source_type ON source_type.oid = conversion.castsource
          JOIN type_keys AS target_type ON target_type.oid = conversion.casttarget
          WHERE conversion.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_am'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_am AS access_method
          WHERE access_method.oid = dependency.refobjid
        )
      WHEN dependency.refclassid = 'pg_catalog.pg_attrdef'::pg_catalog.regclass THEN
        dependency.refobjsubid = 0 AND EXISTS (
          SELECT 1
          FROM pg_catalog.pg_attrdef AS attrdef
          JOIN pg_catalog.pg_class AS class ON class.oid = attrdef.adrelid
          JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
          JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = attrdef.adrelid
           AND attribute.attnum = attrdef.adnum
           AND NOT attribute.attisdropped
          WHERE attrdef.oid = dependency.refobjid
        )
      ELSE false
    END AS resolved
  FROM raw_dependency_rows AS dependency
  LEFT JOIN pg_catalog.pg_class AS reference_class ON reference_class.oid = dependency.refclassid
  LEFT JOIN pg_catalog.pg_namespace AS reference_class_namespace
    ON reference_class_namespace.oid = reference_class.relnamespace
), dependency_rows(kind, identity, dependency_type, reference_class, reference_key, resolved) AS (
  SELECT
    dependency.kind,
    dependency.identity,
    dependency.dependency_type,
    dependency.reference_class,
    dependency.reference_key,
    COALESCE(pg_catalog.bool_and(dependency.resolved), false)
      AND pg_catalog.count(*) = 1
  FROM mapped_dependency_references AS dependency
  GROUP BY
    dependency.kind,
    dependency.identity,
    dependency.dependency_type,
    dependency.reference_class,
    dependency.reference_key
), contract_rows(kind, identity, contract) AS (
  SELECT
    'relation',
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname, class.relkind),
    pg_catalog.jsonb_build_array(
      class.relkind, class.relpersistence, class.relrowsecurity, class.relforcerowsecurity,
      class.relreplident, access_method.amname, class.reloptions
    )::pg_catalog.text
  FROM pg_catalog.pg_class AS class
  JOIN decodex_namespace AS selected ON selected.oid = class.relnamespace
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN pg_catalog.pg_am AS access_method ON access_method.oid = class.relam
  WHERE class.relkind IN ('r', 'p', 'v', 'm', 'f')
  UNION ALL
  SELECT
    'column',
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname, attribute.attname),
    pg_catalog.jsonb_build_array(
      pg_catalog.format_type(attribute.atttypid, attribute.atttypmod),
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
    pg_catalog.jsonb_build_array(namespace.nspname, class.relname, attribute.attname),
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
    constraint_key.key,
    pg_catalog.jsonb_build_array(
      con.contype, pg_catalog.pg_get_constraintdef(con.oid, false),
      con.condeferrable, con.condeferred, con.convalidated,
      con.conenforced, con.confupdtype, con.confdeltype,
      con.confmatchtype, con.conislocal, con.coninhcount,
      con.connoinherit,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(attribute.attname ORDER BY key.ordinality)
        FROM pg_catalog.unnest(con.conkey) WITH ORDINALITY AS key(attnum, ordinality)
        JOIN pg_catalog.pg_attribute AS attribute
          ON attribute.attrelid = con.conrelid
         AND attribute.attnum = key.attnum
      ), '[]'::pg_catalog.jsonb),
      COALESCE((
        SELECT pg_catalog.jsonb_agg(attribute.attname ORDER BY key.ordinality)
        FROM pg_catalog.unnest(con.confkey) WITH ORDINALITY AS key(attnum, ordinality)
        JOIN pg_catalog.pg_attribute AS attribute
          ON attribute.attrelid = con.confrelid
         AND attribute.attnum = key.attnum
      ), '[]'::pg_catalog.jsonb),
      referenced_namespace.nspname, referenced.relname
    )::pg_catalog.text
  FROM touching_constraints AS con
  JOIN constraint_keys AS constraint_key ON constraint_key.oid = con.oid
  JOIN pg_catalog.pg_class AS source ON source.oid = con.conrelid
  JOIN pg_catalog.pg_namespace AS source_namespace ON source_namespace.oid = source.relnamespace
  LEFT JOIN pg_catalog.pg_class AS referenced ON referenced.oid = con.confrelid
  LEFT JOIN pg_catalog.pg_namespace AS referenced_namespace
    ON referenced_namespace.oid = referenced.relnamespace
  UNION ALL
  SELECT
    'index',
    pg_catalog.jsonb_build_array(index_namespace.nspname, index_class.relname),
    pg_catalog.jsonb_build_array(
      table_namespace.nspname, table_class.relname,
      pg_catalog.pg_get_indexdef(index.indexrelid), index.indnatts, index.indnkeyatts,
      index.indisunique, index.indnullsnotdistinct, index.indisprimary,
      index.indisexclusion, index.indimmediate, index.indisclustered,
      index.indisvalid,
      -- The HOT-chain transaction-horizon flag is not index definition.
      index.indisready, index.indislive,
		index.indisreplident,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          CASE WHEN key.attnum = 0 THEN NULL ELSE attribute.attname END
          ORDER BY key.ordinality
        )
        FROM pg_catalog.unnest(index.indkey::pg_catalog.int2[])
          WITH ORDINALITY AS key(attnum, ordinality)
        LEFT JOIN pg_catalog.pg_attribute AS attribute
          ON attribute.attrelid = index.indrelid
         AND attribute.attnum = key.attnum
      ), '[]'::pg_catalog.jsonb),
      index.indoption,
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
    trigger_key.key,
    pg_catalog.jsonb_build_array(
      trigger.tgtype, trigger.tgenabled, trigger.tgparentid = 0,
      trigger.tgconstraint = con.oid, trigger.tgdeferrable,
      trigger.tginitdeferred, trigger.tgnargs, pg_catalog.encode(trigger.tgargs, 'hex'),
      referenced_namespace.nspname, referenced.relname,
      index_namespace.nspname, constraint_index.relname,
      trigger.tgoldtable, trigger.tgnewtable
    )::pg_catalog.text
  FROM relevant_internal_triggers AS trigger
  JOIN trigger_keys AS trigger_key
    ON trigger_key.oid = trigger.oid AND trigger_key.is_internal
  LEFT JOIN touching_constraints AS con ON con.oid = trigger.tgconstraint
  LEFT JOIN pg_catalog.pg_class AS referenced ON referenced.oid = trigger.tgconstrrelid
  LEFT JOIN pg_catalog.pg_namespace AS referenced_namespace
    ON referenced_namespace.oid = referenced.relnamespace
  LEFT JOIN pg_catalog.pg_class AS constraint_index ON constraint_index.oid = trigger.tgconstrindid
  LEFT JOIN pg_catalog.pg_namespace AS index_namespace
    ON index_namespace.oid = constraint_index.relnamespace
  UNION ALL
  SELECT
    'sequence',
    pg_catalog.jsonb_build_array(
      sequence_namespace.nspname,
      sequence_class.relname,
      owner_namespace.nspname,
      owner_class.relname,
      owner_attribute.attname
    ),
    pg_catalog.jsonb_build_array(
      sequence_type.key,
      sequence.seqstart,
      sequence.seqincrement,
      sequence.seqmax,
      sequence.seqmin,
      sequence.seqcache,
      sequence.seqcycle,
      ownership.deptype
    )::pg_catalog.text
  FROM pg_catalog.pg_class AS sequence_class
  JOIN decodex_namespace AS selected ON selected.oid = sequence_class.relnamespace
  JOIN pg_catalog.pg_namespace AS sequence_namespace
    ON sequence_namespace.oid = sequence_class.relnamespace
  JOIN pg_catalog.pg_sequence AS sequence ON sequence.seqrelid = sequence_class.oid
  JOIN type_keys AS sequence_type ON sequence_type.oid = sequence.seqtypid
  LEFT JOIN pg_catalog.pg_depend AS ownership
    ON ownership.classid = 'pg_catalog.pg_class'::pg_catalog.regclass
   AND ownership.objid = sequence_class.oid
   AND ownership.objsubid = 0
   AND ownership.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass
   AND ownership.deptype IN ('a', 'i')
  LEFT JOIN pg_catalog.pg_class AS owner_class ON owner_class.oid = ownership.refobjid
  LEFT JOIN pg_catalog.pg_namespace AS owner_namespace
    ON owner_namespace.oid = owner_class.relnamespace
  LEFT JOIN pg_catalog.pg_attribute AS owner_attribute
    ON owner_attribute.attrelid = owner_class.oid
   AND owner_attribute.attnum = ownership.refobjsubid
  WHERE sequence_class.relkind = 'S'
  UNION ALL
  SELECT
    'type',
    type_key.key,
    pg_catalog.jsonb_build_array(
      type.typtype,
      type.typcategory,
      pg_catalog.format_type(type.typbasetype, type.typtypmod),
      type.typnotnull,
      collation_namespace.nspname,
      coll.collname,
      CASE
        WHEN type.typowner = namespace.nspowner THEN 'schema_owner'
        WHEN type.typowner = (SELECT oid FROM runtime_role) THEN 'runtime'
        ELSE 'role:' || pg_catalog.pg_get_userbyid(type.typowner)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = namespace.nspowner THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY
            pg_catalog.convert_to((CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = namespace.nspowner THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END)::pg_catalog.text, 'UTF8'),
            pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
            privilege.is_grantable
        )
        FROM pg_catalog.aclexplode(
          COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
        ) AS privilege
      ), '[]'::pg_catalog.jsonb)
    )::pg_catalog.text
  FROM decodex_types AS type
  JOIN type_keys AS type_key ON type_key.oid = type.oid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  LEFT JOIN pg_catalog.pg_collation AS coll ON coll.oid = type.typcollation
  LEFT JOIN pg_catalog.pg_namespace AS collation_namespace
    ON collation_namespace.oid = coll.collnamespace
  UNION ALL
  SELECT
    'domain_constraint',
    constraint_key.key,
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_constraintdef(con.oid, false),
      con.convalidated,
      con.conenforced
    )::pg_catalog.text
  FROM pg_catalog.pg_constraint AS con
  JOIN constraint_keys AS constraint_key ON constraint_key.oid = con.oid
  JOIN decodex_types AS type ON type.oid = con.contypid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
  UNION ALL
  SELECT
    'function',
    function_key.key,
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
      CASE
        WHEN proc.proowner = namespace.nspowner THEN 'schema_owner'
        WHEN proc.proowner = (SELECT oid FROM runtime_role) THEN 'runtime'
        ELSE 'role:' || pg_catalog.pg_get_userbyid(proc.proowner)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = namespace.nspowner THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY
            pg_catalog.convert_to((CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = namespace.nspowner THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
              ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END)::pg_catalog.text, 'UTF8'),
            pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
            privilege.is_grantable
        )
        FROM pg_catalog.aclexplode(
          COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))
        ) AS privilege
      ), '[]'::pg_catalog.jsonb)
    )::pg_catalog.text
  FROM decodex_functions AS proc
  JOIN function_keys AS function_key ON function_key.oid = proc.oid
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
  JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
  UNION ALL
  SELECT
    dependency.kind,
    pg_catalog.jsonb_build_array(
      dependency.identity,
      dependency.dependency_type,
      dependency.reference_class,
      dependency.reference_key
    ),
    pg_catalog.jsonb_build_array(dependency.resolved)::pg_catalog.text
  FROM dependency_rows AS dependency
  WHERE dependency.kind IN ('function_dependency', 'type_dependency')
  UNION ALL
  SELECT
    'default_acl',
    pg_catalog.jsonb_build_array(
      CASE
        WHEN default_acl.defaclnamespace = 0 THEN 'global'
        ELSE 'decodex'
      END,
      default_acl.defaclobjtype,
      'schema_owner'
    ),
    COALESCE((
      SELECT pg_catalog.jsonb_agg(
        pg_catalog.jsonb_build_array(
          CASE
            WHEN privilege.grantor = namespace.nspowner THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM runtime_role) THEN 'runtime'
            ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = default_acl.defaclrole THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
            ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY
          pg_catalog.convert_to((CASE
            WHEN privilege.grantor = namespace.nspowner THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM runtime_role) THEN 'runtime'
            ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to((CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = default_acl.defaclrole THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM runtime_role) THEN 'runtime'
            ELSE 'role:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
          privilege.is_grantable
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
    pg_catalog.jsonb_build_array(
      dependency.kind,
      dependency.identity,
      dependency.dependency_type,
      dependency.reference_class,
      dependency.reference_key
    ),
    pg_catalog.jsonb_build_array(dependency.resolved)::pg_catalog.text
  FROM dependency_rows AS dependency
  WHERE dependency.kind NOT IN ('function_dependency', 'type_dependency')
  UNION ALL
  SELECT
    'enum_label',
    pg_catalog.jsonb_build_array(namespace.nspname, type.typname, enum.enumsortorder),
    pg_catalog.jsonb_build_array(enum.enumlabel)::pg_catalog.text
  FROM pg_catalog.pg_enum AS enum
  JOIN pg_catalog.pg_type AS type ON type.oid = enum.enumtypid
  JOIN decodex_namespace AS selected ON selected.oid = type.typnamespace
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
)
SELECT
  (
    SELECT pg_catalog.jsonb_agg(
      pg_catalog.jsonb_build_array(kind, identity, contract)
	  -- Identity text byte order is canonical; accepted catalog changes require a refreeze.
      ORDER BY
        pg_catalog.convert_to(kind, 'UTF8'),
        pg_catalog.convert_to(identity::pg_catalog.text, 'UTF8'),
        pg_catalog.convert_to(contract, 'UTF8')
    )::pg_catalog.text
    FROM contract_rows
  ),
  NOT EXISTS (
    SELECT 1 FROM dependency_rows AS dependency WHERE NOT dependency.resolved
  )
"#;
const SCHEMA_CONTRACT_SHA256: [u8; 32] = [
	0x10, 0x6a, 0xa6, 0x32, 0x7b, 0x87, 0x5a, 0x43, 0xd9, 0x29, 0x87, 0xb3, 0x1b, 0x95, 0x98, 0x08,
	0x56, 0x98, 0x7c, 0x9a, 0xf4, 0xb3, 0x30, 0x3b, 0x58, 0xad, 0xbe, 0xfa, 0x4c, 0xa5, 0x15, 0xfb,
];
// The shipped authority permits no role settings. Record only cardinality so any setting
// fails closed without copying an arbitrary custom-GUC value into the manifest or digest input.
const CONFIGURED_AUTHORITY_SQL: &str = r#"
WITH RECURSIVE configured_principals(label, role_name) AS (
  VALUES ('schema_owner'::pg_catalog.text, $1::pg_catalog.name),
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
), type_keys AS MATERIALIZED (
  SELECT
    type.oid,
    pg_catalog.jsonb_build_array(namespace.nspname, type.typname) AS key
  FROM pg_catalog.pg_type AS type
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type.typnamespace
), function_keys AS MATERIALIZED (
  SELECT
    proc.oid,
    pg_catalog.jsonb_build_array(
      namespace.nspname,
      proc.proname,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(argument_type.key ORDER BY argument.ordinality)
        FROM pg_catalog.unnest(proc.proargtypes::pg_catalog.oid[])
          WITH ORDINALITY AS argument(type_oid, ordinality)
        JOIN type_keys AS argument_type ON argument_type.oid = argument.type_oid
      ), '[]'::pg_catalog.jsonb)
    ) AS key
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
), decodex_classes AS (
  SELECT class.*, namespace.nspname
  FROM pg_catalog.pg_class AS class
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = class.relnamespace AND namespace.nspname = 'decodex'
), authority_classes AS (
	SELECT * FROM decodex_classes
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
    'type', pg_catalog.format('%I.%I', namespace.nspname, type.typname), type.typowner,
    COALESCE(type.typacl, pg_catalog.acldefault('T', type.typowner))
  FROM pg_catalog.pg_type AS type
  JOIN relevant_namespaces AS namespace
    ON namespace.oid = type.typnamespace AND namespace.nspname = 'decodex'
  UNION ALL
  SELECT
    'function',
    function_key.key::pg_catalog.text,
    proc.proowner, COALESCE(proc.proacl, pg_catalog.acldefault('f', proc.proowner))
  FROM pg_catalog.pg_proc AS proc
  JOIN function_keys AS function_key ON function_key.oid = proc.oid
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
        WHEN membership.roleid = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
        WHEN membership.roleid = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(membership.roleid)
      END,
      CASE
        WHEN membership.member = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
        WHEN membership.member = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(membership.member)
      END
    ),
    pg_catalog.jsonb_build_array(
      CASE
        WHEN membership.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
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
        WHEN setting.setrole = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
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
        WHEN object.owner_oid = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
        WHEN object.owner_oid = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(object.owner_oid)
      END,
      COALESCE((
        SELECT pg_catalog.jsonb_agg(
          pg_catalog.jsonb_build_array(
            CASE
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
                THEN 'schema_owner'
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
            END,
            CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
                THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END,
            privilege.privilege_type,
            privilege.is_grantable
          ) ORDER BY
            pg_catalog.convert_to((CASE
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
                THEN 'schema_owner'
              WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
            END)::pg_catalog.text, 'UTF8'),
            pg_catalog.convert_to((CASE
              WHEN privilege.grantee = 0 THEN 'PUBLIC'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
                THEN 'schema_owner'
              WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
                THEN 'runtime'
              ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
            END)::pg_catalog.text, 'UTF8'),
            pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
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
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY
          pg_catalog.convert_to((CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to((CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
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
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(class.relowner)
      END,
      function_key.key,
      CASE
        WHEN proc.proowner = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
        WHEN proc.proowner = (SELECT oid FROM configured_roles WHERE label = 'runtime')
          THEN 'runtime'
        ELSE 'other:' || pg_catalog.pg_get_userbyid(proc.proowner)
      END
    )::pg_catalog.text
  FROM pg_catalog.pg_trigger AS trigger
  JOIN authority_classes AS class ON class.oid = trigger.tgrelid
  JOIN pg_catalog.pg_proc AS proc ON proc.oid = trigger.tgfoid
  JOIN function_keys AS function_key ON function_key.oid = proc.oid
  JOIN pg_catalog.pg_namespace AS function_namespace ON function_namespace.oid = proc.pronamespace
	WHERE NOT trigger.tgisinternal
  UNION ALL
  SELECT
    'rule_definition',
    pg_catalog.format('%I.%I.%I', class.nspname, class.relname, rewrite.rulename),
    pg_catalog.jsonb_build_array(
      pg_catalog.pg_get_ruledef(rewrite.oid, false),
      CASE
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
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
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(policy_role)
          END ORDER BY
          pg_catalog.convert_to((CASE
            WHEN policy_role = 0 THEN 'PUBLIC'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN policy_role = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(policy_role)
          END)::pg_catalog.text, 'UTF8')
        )
        FROM pg_catalog.unnest(policy.polroles) AS policy_role
      ), '[]'::pg_catalog.jsonb),
      pg_catalog.pg_get_expr(policy.polqual, policy.polrelid),
      pg_catalog.pg_get_expr(policy.polwithcheck, policy.polrelid),
      CASE
        WHEN class.relowner = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
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
        WHEN default_acl.defaclrole = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
          THEN 'schema_owner'
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
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END,
          CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END,
          privilege.privilege_type,
          privilege.is_grantable
        ) ORDER BY
          pg_catalog.convert_to((CASE
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantor = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantor)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to((CASE
            WHEN privilege.grantee = 0 THEN 'PUBLIC'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'schema_owner')
              THEN 'schema_owner'
            WHEN privilege.grantee = (SELECT oid FROM configured_roles WHERE label = 'runtime')
              THEN 'runtime'
            ELSE 'other:' || pg_catalog.pg_get_userbyid(privilege.grantee)
          END)::pg_catalog.text, 'UTF8'),
          pg_catalog.convert_to(privilege.privilege_type, 'UTF8'),
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
  ORDER BY
    pg_catalog.convert_to(kind, 'UTF8'),
    pg_catalog.convert_to(identity, 'UTF8'),
    pg_catalog.convert_to(contract, 'UTF8')
)::pg_catalog.text
FROM contract_rows
"#;
const CONFIGURED_AUTHORITY_SHA256: [u8; 32] = [
	0x99, 0x80, 0x39, 0xe6, 0x5b, 0x2b, 0xca, 0x99, 0x4b, 0xc4, 0xaf, 0x1a, 0x02, 0x3e, 0x67, 0x1d,
	0x05, 0xd5, 0xf1, 0x8c, 0x41, 0x92, 0x72, 0xf5, 0x6e, 0x40, 0xae, 0xcb, 0xe3, 0x04, 0x45, 0x1f,
];
const EXTENSION_AUTHORITY_SQL: &str = r#"
WITH set_roles AS (
  SELECT role.oid
  FROM pg_catalog.pg_roles AS role
  WHERE role.rolname = $1::pg_catalog.name
     OR pg_catalog.pg_has_role($1::pg_catalog.name, role.oid, 'SET')
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
	declaration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
	volatility: &'static str,
	strict: bool,
	returns_set: bool,
	rows: f32,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticAuthorityObservation {
	predicate: SemanticAuthorityPredicate,
	passed: bool,
	failure_class: SemanticAuthorityFailureClass,
}

#[derive(Debug)]
struct SemanticAuthorityEvidence {
	observations: Vec<SemanticAuthorityObservation>,
}

#[derive(Debug)]
struct FinalizedSemanticAuthority {
	observations: Vec<SemanticAuthorityObservation>,
}

impl SemanticAuthorityEvidence {
	fn new() -> Self {
		Self { observations: Vec::with_capacity(SEMANTIC_AUTHORITY_PREDICATE_COUNT) }
	}

	fn record_unsafe(&mut self, predicate: SemanticAuthorityPredicate, passed: bool) {
		self.observations.push(SemanticAuthorityObservation {
			predicate,
			passed,
			failure_class: SemanticAuthorityFailureClass::Unsafe,
		});
	}

	fn record_incompatible(&mut self, predicate: SemanticAuthorityPredicate, passed: bool) {
		self.observations.push(SemanticAuthorityObservation {
			predicate,
			passed,
			failure_class: SemanticAuthorityFailureClass::Incompatible,
		});
	}

	fn finalize(self) -> Result<FinalizedSemanticAuthority, StoreError> {
		if self.observations.len() != SEMANTIC_AUTHORITY_PREDICATE_COUNT {
			return Err(StoreError::Incompatible(format!(
				"PostgreSQL semantic authority evidence has {} observations, expected {}",
				self.observations.len(),
				SEMANTIC_AUTHORITY_PREDICATE_COUNT
			)));
		}
		let mut seen = [false; SEMANTIC_AUTHORITY_PREDICATE_COUNT];
		for (position, observation) in self.observations.iter().enumerate() {
			let identity = observation.predicate as usize;
			let Some(seen_identity) = seen.get_mut(identity) else {
				return Err(StoreError::Incompatible(
					"PostgreSQL semantic authority evidence contains an unknown identity".into(),
				));
			};
			if *seen_identity {
				return Err(StoreError::Incompatible(format!(
					"PostgreSQL semantic authority evidence duplicates {}",
					SEMANTIC_AUTHORITY_DEFINITION[identity].name
				)));
			}
			*seen_identity = true;
			let descriptor = &SEMANTIC_AUTHORITY_DEFINITION[position];
			if descriptor.identity != observation.predicate {
				return Err(StoreError::Incompatible(format!(
					"PostgreSQL semantic authority evidence reorders {}",
					descriptor.name
				)));
			}
			if !descriptor.failure_policy.permits(observation.failure_class) {
				return Err(StoreError::Incompatible(format!(
					"PostgreSQL semantic authority evidence misclassifies {}",
					descriptor.name
				)));
			}
		}
		if seen.iter().any(|observed| !observed) {
			return Err(StoreError::Incompatible(
				"PostgreSQL semantic authority evidence omits a required identity".into(),
			));
		}

		Ok(FinalizedSemanticAuthority { observations: self.observations })
	}
}

impl FinalizedSemanticAuthority {
	fn has_unsafe_failure(&self) -> bool {
		self.observations.iter().any(|observation| {
			!observation.passed
				&& observation.failure_class == SemanticAuthorityFailureClass::Unsafe
		})
	}

	fn has_incompatible_failure(&self) -> bool {
		self.observations.iter().any(|observation| {
			!observation.passed
				&& observation.failure_class == SemanticAuthorityFailureClass::Incompatible
		})
	}

	fn bootstrap_observations(&self) -> Vec<BootstrapAuthorityObservation> {
		self.observations
			.iter()
			.zip(SEMANTIC_AUTHORITY_DEFINITION.iter())
			.map(|(observation, descriptor)| BootstrapAuthorityObservation {
				name: descriptor.name,
				failure_class: observation.failure_class.into(),
				passed: observation.passed,
			})
			.collect()
	}
}

pub(crate) async fn verify_runtime<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	verify_namespace_owner_authority(client, schema_owner_role).await?;
	verify_semantic_authority(client, runtime_role, true).await?;
	verify_configured_authority(client, schema_owner_role, runtime_role).await?;
	verify_schema_contract(client, runtime_role).await
}

pub(crate) async fn bootstrap_authority_evidence<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<BootstrapAuthorityEvidence, StoreError>
where
	C: GenericClient + Sync,
{
	collect_bootstrap_authority_evidence(client, schema_owner_role, runtime_role)
		.await
		.map_err(|failure| failure.error)
}

pub(crate) async fn collect_bootstrap_authority_evidence<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<BootstrapAuthorityEvidence, BootstrapAuthorityCollectionFailure>
where
	C: GenericClient + Sync,
{
	let mut progress = BootstrapAuthorityProgress::default();
	let namespace = match namespace_owner_authority_evidence(client, schema_owner_role).await {
		Ok(evidence) => evidence,
		Err(error) =>
			return Err(BootstrapAuthorityCollectionFailure {
				progress,
				operation: BootstrapAuthorityOperation::Namespace,
				error,
			}),
	};
	progress.namespace = Some(namespace);
	let semantic = match semantic_authority_evidence(client, runtime_role, false).await {
		Ok(evidence) => evidence,
		Err(error) =>
			return Err(BootstrapAuthorityCollectionFailure {
				progress,
				operation: BootstrapAuthorityOperation::Semantic,
				error,
			}),
	};
	progress.semantic = Some(semantic.bootstrap_observations());
	let configured_authority =
		match configured_authority_evidence(client, schema_owner_role, runtime_role).await {
			Ok(evidence) => evidence,
			Err(error) =>
				return Err(BootstrapAuthorityCollectionFailure {
					progress,
					operation: BootstrapAuthorityOperation::ConfiguredAuthority,
					error,
				}),
		};
	progress.configured_authority = Some(configured_authority);
	let schema_contract = match schema_contract_evidence(client, runtime_role).await {
		Ok(evidence) => evidence,
		Err(error) =>
			return Err(BootstrapAuthorityCollectionFailure {
				progress,
				operation: BootstrapAuthorityOperation::SchemaContract,
				error,
			}),
	};
	progress.schema_contract = Some(schema_contract);

	Ok(progress.into_complete())
}

pub(crate) fn enforce_bootstrap_authority(
	evidence: &BootstrapAuthorityEvidence,
) -> Result<(), StoreError> {
	if !evidence.namespace[0].passed {
		return Err(StoreError::Incompatible("PostgreSQL Decodex schema is absent".into()));
	}
	if !evidence.namespace[1].passed {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL Decodex schema owner differs from the configured schema-owner role",
		));
	}
	if evidence.semantic.iter().any(|observation| {
		!observation.passed && observation.failure_class == BootstrapAuthorityFailureClass::Unsafe
	}) {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL semantic runtime authority differs from the shipped contract",
		));
	}
	if evidence.semantic.iter().any(|observation| {
		!observation.passed
			&& observation.failure_class == BootstrapAuthorityFailureClass::Incompatible
	}) {
		return Err(StoreError::Incompatible(
			"PostgreSQL semantic runtime contract differs from the shipped contract".into(),
		));
	}
	if !evidence.configured_authority.complete {
		return Err(StoreError::Incompatible(
			"PostgreSQL configured authority inventory is empty".into(),
		));
	}
	if !evidence.configured_authority.passed() {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL configured principal or ACL authority differs from the shipped PG18 inventory",
		));
	}
	if !evidence.schema_contract.complete {
		return Err(StoreError::Incompatible(
			"PostgreSQL Decodex schema dependency inventory is incomplete".into(),
		));
	}
	if !evidence.schema_contract.passed() {
		return Err(StoreError::Incompatible(
			"PostgreSQL Decodex schema contract differs from the shipped PG18 inventory".into(),
		));
	}

	Ok(())
}

const fn trigger_contract(
	name: &'static str,
	lookup_signature: &'static str,
	declaration_signature: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		declaration_signature,
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
	declaration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		declaration_signature,
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
	declaration_signature: &'static str,
	arguments: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		declaration_signature,
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
	declaration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	language: &'static str,
	volatility: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		declaration_signature,
		arguments,
		result,
		language,
		volatility,
		strict: false,
		returns_set: false,
		rows: 0.0,
	}
}

const fn table_function_contract(
	name: &'static str,
	lookup_signature: &'static str,
	declaration_signature: &'static str,
	arguments: &'static str,
	result: &'static str,
	volatility: &'static str,
) -> FunctionContract {
	FunctionContract {
		name,
		lookup_signature,
		declaration_signature,
		arguments,
		result,
		language: "plpgsql",
		volatility,
		strict: false,
		returns_set: true,
		rows: 1_000.0,
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
	canonical_function_source_in_schema(LATEST_SCHEMA_SQL, contract)
}

fn canonical_function_source_in_schema<'a>(
	schema: &'a str,
	contract: &FunctionContract,
) -> Option<&'a str> {
	if !declaration_signature_matches_contract(contract) {
		return None;
	}

	canonical_function_source_with_dump_layout(schema, contract)
}

fn declaration_signature_matches_contract(contract: &FunctionContract) -> bool {
	let Some(arguments_start) = contract.declaration_signature.find('(') else {
		return false;
	};
	let Some(arguments_end) =
		matching_parenthesis(contract.declaration_signature, arguments_start + 1)
	else {
		return false;
	};

	contract.declaration_signature[..arguments_start].trim() == contract.name
		&& contract.declaration_signature[arguments_end + 1..].trim().is_empty()
		&& equivalent_sql_spacing(
			&contract.declaration_signature[arguments_start + 1..arguments_end],
			contract.arguments,
		)
}

fn canonical_function_source_with_dump_layout<'a>(
	schema: &'a str,
	contract: &FunctionContract,
) -> Option<&'a str> {
	for prefix in [
		format!("CREATE FUNCTION decodex.{}(", contract.name),
		format!("CREATE OR REPLACE FUNCTION decodex.{}(", contract.name),
	] {
		let mut offset = 0;
		while let Some(relative) = schema[offset..].find(&prefix) {
			let declaration = offset + relative;
			let arguments_start = declaration + prefix.len();
			let arguments_end = matching_parenthesis(schema, arguments_start)?;
			if equivalent_sql_spacing(&schema[arguments_start..arguments_end], contract.arguments) {
				let declaration_tail = &schema[arguments_end + 1..];
				let next_declaration =
					["CREATE FUNCTION decodex.", "CREATE OR REPLACE FUNCTION decodex."]
						.into_iter()
						.filter_map(|marker| declaration_tail.find(marker))
						.min()
						.unwrap_or(declaration_tail.len());
				return dollar_quoted_function_source(&declaration_tail[..next_declaration]);
			}
			offset = arguments_end + 1;
		}
	}

	None
}

fn matching_parenthesis(value: &str, content_start: usize) -> Option<usize> {
	let mut depth = 1_usize;
	let mut quoted = None;
	let mut escaped = false;
	for (relative, character) in value[content_start..].char_indices() {
		if let Some(quote) = quoted {
			if escaped {
				escaped = false;
				continue;
			}
			if character == '\\' && quote == '\'' {
				escaped = true;
			} else if character == quote {
				quoted = None;
			}
			continue;
		}
		match character {
			'\'' | '"' => quoted = Some(character),
			'(' => depth += 1,
			')' => {
				depth -= 1;
				if depth == 0 {
					return Some(content_start + relative);
				}
			},
			_ => {},
		}
	}
	None
}

fn equivalent_sql_spacing(left: &str, right: &str) -> bool {
	left.chars()
		.filter(|character| !character.is_whitespace())
		.eq(right.chars().filter(|character| !character.is_whitespace()))
}

fn dollar_quoted_function_source(declaration: &str) -> Option<&str> {
	let delimiter_start = declaration.find("AS $")? + "AS ".len();
	let delimiter_tail = &declaration[delimiter_start + 1..];
	let tag_end = delimiter_tail.find('$')?;
	let tag = &delimiter_tail[..tag_end];
	if !tag.bytes().all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()) {
		return None;
	}
	let delimiter_end = delimiter_start + tag_end + 2;
	let delimiter = &declaration[delimiter_start..delimiter_end];
	let source_and_tail = &declaration[delimiter_end..];
	let closing = source_and_tail.find(&format!("{delimiter};"))?;
	Some(&source_and_tail[..closing])
}

async fn verify_schema_contract<C>(client: &C, runtime_role: &str) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let evidence = schema_contract_evidence(client, runtime_role).await?;
	if !evidence.complete {
		return Err(StoreError::Incompatible(
			"PostgreSQL Decodex schema dependency inventory is incomplete".into(),
		));
	}
	if !evidence.passed() {
		return Err(StoreError::Incompatible(
			"PostgreSQL Decodex schema contract differs from the shipped PG18 inventory".into(),
		));
	}

	Ok(())
}

async fn schema_contract_evidence<C>(
	client: &C,
	runtime_role: &str,
) -> Result<BootstrapDigestEvidence, StoreError>
where
	C: GenericClient + Sync,
{
	let inventory = client.query_one(SCHEMA_CONTRACT_SQL, &[&runtime_role]).await?;
	let manifest: Option<String> = inventory.get(0);
	let dependencies_complete: bool = inventory.get(1);
	let actual_sha256 =
		manifest.as_ref().map(|manifest| <[u8; 32]>::from(Sha256::digest(manifest.as_bytes())));

	Ok(BootstrapDigestEvidence {
		complete: dependencies_complete && manifest.is_some(),
		expected_sha256: SCHEMA_CONTRACT_SHA256,
		actual_sha256,
		incomplete_failure_class: BootstrapAuthorityFailureClass::Incompatible,
		mismatch_failure_class: BootstrapAuthorityFailureClass::Incompatible,
	})
}

async fn verify_configured_authority<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let evidence = configured_authority_evidence(client, schema_owner_role, runtime_role).await?;
	if !evidence.complete {
		return Err(StoreError::Incompatible(
			"PostgreSQL configured authority inventory is empty".into(),
		));
	}
	if !evidence.passed() {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL configured principal or ACL authority differs from the shipped PG18 inventory",
		));
	}

	Ok(())
}

async fn configured_authority_evidence<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<BootstrapDigestEvidence, StoreError>
where
	C: GenericClient + Sync,
{
	let manifest: Option<String> = client
		.query_one(CONFIGURED_AUTHORITY_SQL, &[&schema_owner_role, &runtime_role])
		.await?
		.get(0);
	let actual_sha256 =
		manifest.as_ref().map(|manifest| <[u8; 32]>::from(Sha256::digest(manifest.as_bytes())));

	Ok(BootstrapDigestEvidence {
		complete: manifest.is_some(),
		expected_sha256: CONFIGURED_AUTHORITY_SHA256,
		actual_sha256,
		incomplete_failure_class: BootstrapAuthorityFailureClass::Incompatible,
		mismatch_failure_class: BootstrapAuthorityFailureClass::Unsafe,
	})
}

async fn verify_namespace_owner_authority<C>(
	client: &C,
	schema_owner_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let evidence = namespace_owner_authority_evidence(client, schema_owner_role).await?;
	if !evidence[0].passed {
		return Err(StoreError::Incompatible("PostgreSQL Decodex schema is absent".into()));
	}
	if !evidence[1].passed {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL Decodex schema owner differs from the configured schema-owner role",
		));
	}
	Ok(())
}

async fn namespace_owner_authority_evidence<C>(
	client: &C,
	schema_owner_role: &str,
) -> Result<[BootstrapAuthorityObservation; 2], StoreError>
where
	C: GenericClient + Sync,
{
	let owner_matches = client
		.query_opt(
			"SELECT owner.rolname=$1 \
			 FROM pg_catalog.pg_namespace AS namespace \
			 JOIN pg_catalog.pg_roles AS owner ON owner.oid=namespace.nspowner \
			 WHERE namespace.nspname='decodex'",
			&[&schema_owner_role],
		)
		.await?
		.map(|row| row.get::<_, bool>(0));

	Ok([
		BootstrapAuthorityObservation {
			name: "namespace_present",
			failure_class: BootstrapAuthorityFailureClass::Incompatible,
			passed: owner_matches.is_some(),
		},
		BootstrapAuthorityObservation {
			name: "namespace_owner",
			failure_class: BootstrapAuthorityFailureClass::Unsafe,
			passed: owner_matches == Some(true),
		},
	])
}

fn function_is_security_definer(function_name: &str) -> bool {
	matches!(
		function_name,
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
			| "replace_routing_policy_exact"
			| "publish_routing_evidence_exact"
			| "resolve_routing_snapshot_exact"
			| "prepare_codex_experiment_exact"
			| "mark_codex_experiment_creation_possible_exact"
			| "bind_codex_experiment_thread_exact"
			| "record_codex_experiment_observation_exact"
			| "bind_codex_experiment_start_exact"
			| "read_codex_experiment_start_exact"
			| "mark_codex_experiment_title_set_possible_exact"
			| "attest_codex_experiment_retained_title_exact"
			| "record_attested_codex_experiment_observation_exact"
			| "route_account_exact"
			| "bind_quick_task_continuation_exact"
			| "begin_quick_task_initial_route_exact"
			| "complete_quick_task_initial_route_exact"
			| "create_quick_task_routing_successor_exact"
			| "read_quick_task_initial_route_exact"
			| "read_quick_task_request_exact"
			| "plan_continuation_exact"
			| "plan_initial_thread_continuation_exact"
			| "admit_initial_quick_task_turn_exact"
			| "read_continuation_plan_exact"
			| "read_execution_decision_exact"
			| "read_managed_run_execution_exact"
			| "register_waiting_usage_wake_exact"
			| "claim_due_waiting_usage_wake_exact"
			| "fire_waiting_usage_wake_exact"
			| "cancel_waiting_usage_wake_exact"
			| "read_waiting_usage_wake_transition_exact"
			| "read_account_registry_exact"
			| "read_reset_card_account_admission_exact"
			| "prepare_account_operation_exact"
			| "set_account_operation_target_exact"
			| "advance_account_operation_exact"
			| "read_unsettled_account_operations_exact"
			| "read_account_operation_exact"
			| "set_account_enabled_exact"
			| "set_fixed_account_selection_exact"
			| "set_balanced_account_selection_exact"
			| "set_account_order_exact"
			| "read_account_routing_control_exact"
			| "observe_account_quota_exact"
			| "observe_account_quota_error_exact"
			| "observe_account_store_exact"
			| "attest_codex_account_capability_exact"
			| "observe_account_profile_exact"
			| "read_account_profile_exact"
			| "acknowledge_runtime_session_turn_exact"
			| "read_ordinary_runtime_session_for_resume_exact"
			| "read_ordinary_task_conversations_exact"
			| "read_turn_admission_exact"
			| "prove_initial_quick_task_spawn_not_created_exact"
			| "prepare_quick_task_process_generation_exact"
			| "fence_runtime_session_thread_start_exact"
			| "bind_runtime_session_thread_exact"
			| "read_quick_task_thread_establishment_exact"
			| "terminalize_quick_task_turn_exact"
			| "reconcile_quick_task_terminalizations_exact"
			| "prepare_process_generation_exact"
			| "bind_process_generation_identity_exact"
			| "mark_process_generation_ready_exact"
			| "mark_process_generation_stopping_exact"
			| "mark_process_generation_death_unknown_exact"
			| "record_process_generation_death_exact"
			| "project_process_generations_after_supervisor_loss_exact"
			| "read_process_generations_exact"
			| "enforce_provider_attempt_turn_materialization"
			| "prepare_provider_attempt_exact"
			| "authorize_provider_attempt_dispatch_exact"
			| "cancel_provider_attempt_exact"
			| "mark_provider_attempt_unknown_exact"
			| "record_provider_attempt_positive_evidence_exact"
			| "project_provider_attempts_after_supervisor_loss_exact"
			| "read_provider_attempts_exact"
	)
}

async fn inspect_function_contract<C>(
	client: &C,
	runtime_role: &str,
	evidence: &mut SemanticAuthorityEvidence,
) -> Result<bool, StoreError>
where
	C: GenericClient + Sync,
{
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
	let mut exact_inventory = actual_count == expected_count;
	let mut metadata_matches = true;
	let mut semantics_match = true;
	let mut execute_authority_matches = true;

	for contract in &FUNCTION_CONTRACTS {
		let expected_source = canonical_function_source(contract);
		if expected_source.is_none() {
			semantics_match = false;
		}
		let Some(row) = client
			.query_opt(FUNCTION_CONTRACT_SQL, &[&contract.lookup_signature, &runtime_role])
			.await?
		else {
			exact_inventory = false;
			continue;
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
		let expected_security_definer = function_is_security_definer(contract.name);
		let expected_executable = RUNTIME_EXECUTE_FUNCTIONS.contains(&contract.lookup_signature);
		let expected_settings = vec!["search_path=pg_catalog, decodex".to_owned()];

		metadata_matches &= !(unsafe_metadata
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
			|| rows != contract.rows);

		if let Some(expected_source) = expected_source {
			semantics_match &= installed_source == expected_source;
		}
		execute_authority_matches &= executable == expected_executable && !public_executable;
	}

	if !exact_inventory && actual_count >= expected_count {
		evidence.record_unsafe(SemanticAuthorityPredicate::ExactFunctionInventory, exact_inventory);
	} else {
		evidence.record_incompatible(
			SemanticAuthorityPredicate::ExactFunctionInventory,
			exact_inventory,
		);
	}
	evidence.record_unsafe(SemanticAuthorityPredicate::FunctionMetadata, metadata_matches);
	evidence.record_incompatible(SemanticAuthorityPredicate::FunctionSemantics, semantics_match);

	let runtime_routines = RUNTIME_EXECUTE_FUNCTIONS.to_vec();
	let runtime_entry = client
		.query_one(RUNTIME_ROUTINE_AUTHORITY_SQL, &[&runtime_routines, &runtime_role])
		.await?;
	let unexpected_runtime_security_definer: bool = runtime_entry.get(0);
	let required_digest_exists: bool = runtime_entry.get(1);
	let required_digest_exact: bool = runtime_entry.get(2);
	let execute_contract_matches =
		execute_authority_matches && required_digest_exists && required_digest_exact;
	evidence.record_incompatible(
		SemanticAuthorityPredicate::FunctionExecuteAuthority,
		execute_contract_matches,
	);

	Ok(!unexpected_runtime_security_definer)
}

async fn inspect_retention_contract<C>(
	client: &C,
	evidence: &mut SemanticAuthorityEvidence,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let rows = client.query(TRIGGER_CONTRACT_SQL, &[]).await?;
	let inventory_matches = rows.len() == SAFETY_TRIGGER_COUNT;
	let mut trigger_bindings_match = true;
	let mut function_metadata_matches = true;
	let mut function_semantics_match = true;

	for row in rows {
		let function_name: String = row.get(0);
		trigger_bindings_match &= row.get::<_, bool>(1);
		function_metadata_matches &= row.get::<_, bool>(2);
		let installed_source: Option<String> = row.get(3);
		function_semantics_match &= canonical_safety_function_source(&function_name)
			.is_some_and(|expected| installed_source.as_deref() == Some(expected));
	}

	evidence.record_incompatible(SemanticAuthorityPredicate::RetentionInventory, inventory_matches);
	evidence.record_unsafe(
		SemanticAuthorityPredicate::RetentionTriggerBindings,
		trigger_bindings_match,
	);
	evidence.record_incompatible(
		SemanticAuthorityPredicate::RetentionFunctionMetadata,
		function_metadata_matches,
	);
	evidence.record_incompatible(
		SemanticAuthorityPredicate::RetentionFunctionSemantics,
		function_semantics_match,
	);

	Ok(())
}

async fn semantic_authority_evidence<C>(
	client: &C,
	runtime_role: &str,
	require_runtime_session: bool,
) -> Result<FinalizedSemanticAuthority, StoreError>
where
	C: GenericClient + Sync,
{
	let mut evidence = SemanticAuthorityEvidence::new();
	let session_is_runtime: bool = if require_runtime_session {
		client
			.query_one(
				"SELECT session_user = $1::pg_catalog.name \
				 AND current_user = $1::pg_catalog.name",
				&[&runtime_role],
			)
			.await?
			.get(0)
	} else {
		true
	};
	evidence
		.record_unsafe(SemanticAuthorityPredicate::ConfiguredRuntimeSession, session_is_runtime);

	let role = client.query_one(ROLE_AUTHORITY_SQL, &[&runtime_role]).await?;
	for (predicate, index) in [
		(SemanticAuthorityPredicate::NoForbiddenRoleAttributes, 0),
		(SemanticAuthorityPredicate::NoDatabaseCreate, 1),
		(SemanticAuthorityPredicate::NoSchemaCreate, 2),
		(SemanticAuthorityPredicate::NoEffectiveObjectOwnership, 3),
		(SemanticAuthorityPredicate::NoFunctionGrantOption, 4),
		(SemanticAuthorityPredicate::NoTriggerBypass, 5),
		(SemanticAuthorityPredicate::NoAlterSystemBypass, 6),
		(SemanticAuthorityPredicate::SessionReplicationRoleOrigin, 7),
		(SemanticAuthorityPredicate::NoMembershipAdmin, 8),
	] {
		evidence.record_unsafe(predicate, !role.get::<_, bool>(index));
	}

	let table = client.query_one(TABLE_AUTHORITY_SQL, &[&runtime_role]).await?;
	evidence.record_incompatible(SemanticAuthorityPredicate::ExactTableAuthority, table.get(0));
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoUnsafeTableAuthority,
		!table.get::<_, bool>(1),
	);

	let sequence = client.query_one(SEQUENCE_AUTHORITY_SQL, &[&runtime_role]).await?;
	evidence
		.record_incompatible(SemanticAuthorityPredicate::ExactSequenceContract, sequence.get(0));
	evidence.record_incompatible(SemanticAuthorityPredicate::SequenceUsage, sequence.get(1));
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoUnsafeSequenceAuthority,
		!sequence.get::<_, bool>(2),
	);

	let generation_types =
		client.query_one(PROCESS_GENERATION_TYPE_AUTHORITY_SQL, &[&runtime_role]).await?;
	evidence.record_incompatible(
		SemanticAuthorityPredicate::ProcessGenerationTypeUsage,
		generation_types.get(0),
	);
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoPublicProcessGenerationTypeUsage,
		!generation_types.get::<_, bool>(1),
	);
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoProcessGenerationTypeGrantOption,
		!generation_types.get::<_, bool>(2),
	);

	let attempt_types =
		client.query_one(PROVIDER_ATTEMPT_TYPE_AUTHORITY_SQL, &[&runtime_role]).await?;
	evidence.record_incompatible(
		SemanticAuthorityPredicate::ProviderAttemptTypeUsage,
		attempt_types.get(0),
	);
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoPublicProviderAttemptTypeUsage,
		!attempt_types.get::<_, bool>(1),
	);
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoProviderAttemptTypeGrantOption,
		!attempt_types.get::<_, bool>(2),
	);

	let extension_control: bool =
		client.query_one(EXTENSION_AUTHORITY_SQL, &[&runtime_role]).await?.get(0);
	evidence.record_unsafe(SemanticAuthorityPredicate::NoExtensionControl, !extension_control);

	let schema_usage: bool = client
		.query_one(
			"SELECT COALESCE((SELECT pg_catalog.has_schema_privilege(\
			 $1::pg_catalog.name, namespace.oid, 'USAGE') \
			 FROM pg_catalog.pg_namespace AS namespace \
			 WHERE namespace.nspname = 'decodex'), false)",
			&[&runtime_role],
		)
		.await?
		.get(0);
	evidence.record_incompatible(SemanticAuthorityPredicate::SchemaUsage, schema_usage);

	let identity_cast_closed: bool =
		client.query_one(IDENTITY_CAST_AUTHORITY_SQL, &[]).await?.get(0);
	evidence.record_unsafe(SemanticAuthorityPredicate::IdentityCastClosed, identity_cast_closed);

	let allowed_functions = FUNCTION_CONTRACTS
		.iter()
		.map(|contract| contract.lookup_signature)
		.chain(ALLOWED_EXECUTION_DEPENDENCIES)
		.collect::<Vec<_>>();
	let execution = client.query_one(EXECUTION_PATH_CONTRACT_SQL, &[&allowed_functions]).await?;
	for (predicate, index) in [
		(SemanticAuthorityPredicate::ExactTriggerInventory, 0),
		(SemanticAuthorityPredicate::NoRelationRules, 1),
		(SemanticAuthorityPredicate::NoRelationPolicies, 2),
		(SemanticAuthorityPredicate::ClosedFunctionDependencies, 3),
	] {
		evidence.record_unsafe(predicate, execution.get(index));
	}

	let no_unexpected_runtime_security_definer =
		inspect_function_contract(client, runtime_role, &mut evidence).await?;
	inspect_retention_contract(client, &mut evidence).await?;
	evidence.record_unsafe(
		SemanticAuthorityPredicate::NoUnexpectedRuntimeSecurityDefinerAuthority,
		no_unexpected_runtime_security_definer,
	);

	evidence.finalize()
}

async fn verify_semantic_authority<C>(
	client: &C,
	runtime_role: &str,
	require_runtime_session: bool,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let evidence =
		semantic_authority_evidence(client, runtime_role, require_runtime_session).await?;
	if evidence.has_unsafe_failure() {
		return Err(StoreError::UnsafeAuthority(
			"PostgreSQL semantic runtime authority differs from the shipped contract",
		));
	}
	if evidence.has_incompatible_failure() {
		return Err(StoreError::Incompatible(
			"PostgreSQL semantic runtime contract differs from the shipped contract".into(),
		));
	}

	Ok(())
}

#[cfg(test)]
pub(crate) fn passing_bootstrap_authority_evidence_fixture() -> BootstrapAuthorityEvidence {
	let semantic = SEMANTIC_AUTHORITY_DEFINITION
		.iter()
		.map(|descriptor| BootstrapAuthorityObservation {
			name: descriptor.name,
			failure_class: match descriptor.failure_policy {
				SemanticAuthorityFailurePolicy::Unsafe => BootstrapAuthorityFailureClass::Unsafe,
				SemanticAuthorityFailurePolicy::Incompatible
				| SemanticAuthorityFailurePolicy::UnsafeIfExcessOtherwiseIncompatible =>
					BootstrapAuthorityFailureClass::Incompatible,
			},
			passed: true,
		})
		.collect();
	BootstrapAuthorityEvidence {
		namespace: [
			BootstrapAuthorityObservation {
				name: "namespace_present",
				failure_class: BootstrapAuthorityFailureClass::Incompatible,
				passed: true,
			},
			BootstrapAuthorityObservation {
				name: "namespace_owner",
				failure_class: BootstrapAuthorityFailureClass::Unsafe,
				passed: true,
			},
		],
		semantic,
		configured_authority: BootstrapDigestEvidence {
			complete: true,
			expected_sha256: CONFIGURED_AUTHORITY_SHA256,
			actual_sha256: Some(CONFIGURED_AUTHORITY_SHA256),
			incomplete_failure_class: BootstrapAuthorityFailureClass::Incompatible,
			mismatch_failure_class: BootstrapAuthorityFailureClass::Unsafe,
		},
		schema_contract: BootstrapDigestEvidence {
			complete: true,
			expected_sha256: SCHEMA_CONTRACT_SHA256,
			actual_sha256: Some(SCHEMA_CONTRACT_SHA256),
			incomplete_failure_class: BootstrapAuthorityFailureClass::Incompatible,
			mismatch_failure_class: BootstrapAuthorityFailureClass::Incompatible,
		},
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use super::{
		FUNCTION_CONTRACTS, FunctionContract, LATEST_SCHEMA_SQL, SEMANTIC_AUTHORITY_DEFINITION,
		SEMANTIC_AUTHORITY_PREDICATE_COUNT, canonical_function_source,
		declaration_signature_matches_contract, equivalent_sql_spacing, matching_parenthesis,
	};

	fn matching_schema_declaration_count(contract: &FunctionContract) -> usize {
		let mut matches = 0;
		for prefix in [
			format!("CREATE FUNCTION decodex.{}(", contract.name),
			format!("CREATE OR REPLACE FUNCTION decodex.{}(", contract.name),
		] {
			let mut offset = 0;
			while let Some(relative) = LATEST_SCHEMA_SQL[offset..].find(&prefix) {
				let declaration = offset + relative;
				let arguments_start = declaration + prefix.len();
				let arguments_end = matching_parenthesis(LATEST_SCHEMA_SQL, arguments_start)
					.expect("function declaration has balanced arguments");
				if equivalent_sql_spacing(
					&LATEST_SCHEMA_SQL[arguments_start..arguments_end],
					contract.arguments,
				) {
					matches += 1;
				}
				offset = arguments_end + 1;
			}
		}
		matches
	}

	#[test]
	fn bootstrap_semantic_report_contract_is_closed_ordered_unique_and_bounded() {
		assert_eq!(SEMANTIC_AUTHORITY_DEFINITION.len(), SEMANTIC_AUTHORITY_PREDICATE_COUNT);
		let mut names = HashSet::new();
		for (position, descriptor) in SEMANTIC_AUTHORITY_DEFINITION.iter().enumerate() {
			assert_eq!(descriptor.identity as usize, position);
			assert!(names.insert(descriptor.name));
			assert!(!descriptor.name.is_empty());
			assert!(descriptor.name.len() <= 64);
			assert!(descriptor.name.bytes().all(|byte| byte == b'_' || byte.is_ascii_lowercase()));
		}
	}

	#[test]
	fn every_function_contract_has_one_canonical_schema_declaration_and_body() {
		let mut lookup_signatures = HashSet::new();
		for contract in &FUNCTION_CONTRACTS {
			assert!(
				lookup_signatures.insert(contract.lookup_signature),
				"duplicate function lookup identity: {}",
				contract.lookup_signature
			);
			assert!(
				declaration_signature_matches_contract(contract),
				"function declaration and arguments differ: {}",
				contract.lookup_signature
			);
			assert_eq!(
				matching_schema_declaration_count(contract),
				1,
				"function does not have one matching schema declaration: {}",
				contract.lookup_signature
			);
			let source = canonical_function_source(contract).unwrap_or_else(|| {
				panic!("function has no canonical source: {}", contract.lookup_signature)
			});
			assert!(
				!source.trim().is_empty(),
				"function has an empty canonical body: {}",
				contract.lookup_signature
			);
		}
	}
}
