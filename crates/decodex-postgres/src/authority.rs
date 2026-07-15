//! Steady-state PostgreSQL authority verification for the retained runtime pool.

use deadpool_postgres::Client;
use sha2::{Digest as _, Sha256};

use crate::StoreError;

const FOUNDATION_MIGRATION: &str = include_str!("../migrations/V1__persistence_foundation.sql");
const CONVERSATION_MIGRATION: &str = include_str!("../migrations/V3__conversation_history.sql");
const FUNCTION_CONTRACTS: [FunctionContract; 34] = [
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
];
const SAFETY_FUNCTIONS: [&str; 19] = [
	"enforce_lease_operation_time",
	"enforce_outbox_operation_time",
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
];
const SAFETY_TRIGGER_COUNT: usize = 29;
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
  ('command_receipts', true, true, true, false),
  ('activity', true, true, false, false),
  ('leases', true, true, true, false),
  ('outbox', true, true, true, true),
  ('conversations', true, true, true, false),
  ('profile_snapshots', true, true, false, false),
  ('account_snapshots', true, true, false, false),
  ('runtime_sessions', true, true, true, false),
  ('blob_objects', true, true, false, true),
  ('artifacts', true, true, true, false),
  ('artifact_revisions', true, true, false, false),
  ('turns', true, true, true, false),
  ('history_items', true, true, true, false),
  ('history_item_versions', true, false, false, false),
  ('history_cursors', true, false, false, false),
  ('context_packs', true, true, false, false),
  ('context_pack_sources', true, true, false, false),
  ('transition_proposals', true, true, false, false)
), tables AS (
  SELECT class.oid, class.relname, expected.*
  FROM pg_catalog.pg_class AS class
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
  LEFT JOIN expected ON expected.table_name = class.relname
  WHERE namespace.nspname = 'decodex' AND class.relkind IN ('r', 'p')
)
SELECT
  (SELECT count(*) FROM tables WHERE table_name IS NOT NULL) = 20
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
  ('transition_proposals', 'transition_proposals_coordinator', 'acquire_hierarchy_coordinator', 30)
)
SELECT
  expected.function_name,
  trigger.oid IS NOT NULL
    AND trigger.tgenabled = 'O'
    AND trigger.tgtype = expected.trigger_type
    AND trigger.tgparentid = 0
    AND trigger.tgconstraint = 0
    AND trigger.tgconstrrelid = 0
    AND trigger.tgconstrindid = 0
    AND NOT trigger.tgdeferrable
    AND NOT trigger.tginitdeferred
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
  pg_catalog.has_function_privilege(session_user, proc.oid, 'EXECUTE')
FROM pg_catalog.pg_proc AS proc
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace
JOIN pg_catalog.pg_language AS language ON language.oid = proc.prolang
WHERE namespace.nspname = 'decodex'
  AND proc.oid = pg_catalog.to_regprocedure($1)
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
  ('transition_proposals', 'transition_proposals_coordinator', 'decodex.acquire_hierarchy_coordinator()')
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
  SELECT namespace.oid
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
	0x83, 0xed, 0x77, 0x06, 0x6f, 0x83, 0x2e, 0xa2, 0x1d, 0x78, 0xcb, 0x4a, 0x5e, 0x42, 0xe6, 0xc0,
	0xfa, 0xe1, 0x78, 0x4b, 0xd8, 0x48, 0x6c, 0xaa, 0x4d, 0x8f, 0x86, 0xcb, 0x7a, 0xa1, 0xd8, 0xf9,
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

pub(crate) async fn verify_runtime(client: &Client) -> Result<(), StoreError> {
	verify_forbidden_authority(client).await?;
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

fn canonical_safety_function_source(function_name: &str) -> Option<&'static str> {
	if !SAFETY_FUNCTIONS.contains(&function_name) {
		return None;
	}

	let contract = FUNCTION_CONTRACTS.iter().find(|contract| contract.name == function_name)?;

	canonical_function_source(contract)
}

fn canonical_function_source(contract: &FunctionContract) -> Option<&'static str> {
	let declaration = format!("CREATE FUNCTION decodex.{}", contract.migration_signature);
	let migration = [FOUNDATION_MIGRATION, CONVERSATION_MIGRATION]
		.into_iter()
		.find(|migration| migration.contains(&declaration))?;
	let (_, declaration_and_tail) = migration.split_once(&declaration)?;
	let (_, source_and_tail) = declaration_and_tail.split_once("\nAS $$")?;
	let (source, _) = source_and_tail.split_once("$$;")?;

	Some(source)
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

async fn verify_execution_path_contract(client: &Client) -> Result<(), StoreError> {
	let allowed_functions =
		FUNCTION_CONTRACTS.iter().map(|contract| contract.lookup_signature).collect::<Vec<_>>();
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
		let expected_security_definer = matches!(
			contract.name,
			"issue_history_cursor" | "prune_history_snapshots" | "capture_history_item_version"
		);
		let expected_executable = contract.name != "capture_history_item_version";
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
		if executable != expected_executable {
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
		CONVERSATION_MIGRATION, FOUNDATION_MIGRATION, FUNCTION_CONTRACTS, OWNED_OBJECT_CATALOGS,
		ROLE_AUTHORITY_SQL, SAFETY_FUNCTIONS, SCHEMA_CONTRACT_SHA256, SCHEMA_CONTRACT_SQL,
	};

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
	fn canonical_inventory_covers_every_shipped_decodex_function_once() {
		assert_eq!(
			[FOUNDATION_MIGRATION, CONVERSATION_MIGRATION]
				.into_iter()
				.map(|migration| migration.matches("CREATE FUNCTION decodex.").count())
				.sum::<usize>(),
			FUNCTION_CONTRACTS.len()
		);

		let mut lookup_signatures = HashSet::new();

		for contract in FUNCTION_CONTRACTS {
			assert!(lookup_signatures.insert(contract.lookup_signature));
			assert_eq!(
				[FOUNDATION_MIGRATION, CONVERSATION_MIGRATION]
					.into_iter()
					.map(|migration| migration
						.matches(&format!(
							"CREATE FUNCTION decodex.{}",
							contract.migration_signature
						))
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
