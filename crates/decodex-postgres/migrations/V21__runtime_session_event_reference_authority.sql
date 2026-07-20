-- XY-1364 separates cross-domain RuntimeSession identity provenance from event ownership.
-- Scalar *_id references are not ownership. Aggregate/event/kind markers, complete RuntimeSession
-- or snapshot field sets under any key, and links to activity with those shapes remain owner-only.
CREATE OR REPLACE FUNCTION decodex.enforce_runtime_session_event_namespace()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
DECLARE linked_runtime_session boolean := false;
-- Complete shapes are object-local: lax array access must not combine fields from siblings.
DECLARE ownership_path CONSTANT pg_catalog.jsonpath := '$.** ? (
	@.aggregate_kind == "runtime_session" ||
	@.kind == "runtime_session" ||
	@.event_kind == "runtime_session_recorded" ||
	@.event_kind == "runtime_session_created" ||
	@.event_kind == "runtime_session_transitioned" ||
	(
		@.type() == "object" &&
		exists(@.runtime_session_id) && exists(@.conversation_id) &&
		exists(@.profile_snapshot_id) && exists(@.account_snapshot_id) &&
		exists(@.codex_thread_id) && exists(@.last_known_turn_id) &&
		exists(@.state) && exists(@.revision) && exists(@.created_at) &&
		exists(@.updated_at) && exists(@.ended_at)
	) ||
	(
		@.type() == "object" &&
		exists(@.profile_snapshot_id) && exists(@.source_profile_id) &&
		exists(@.role) && exists(@.model) && exists(@.reasoning_effort) &&
		exists(@.service_tier) && exists(@.instructions_digest) &&
		exists(@.instructions) && exists(@.provenance) &&
		exists(@.source_revision) && exists(@.created_at)
	) ||
	(
		@.type() == "object" &&
		exists(@.account_snapshot_id) && exists(@.source_account_id) &&
		exists(@.display_label) && exists(@.observed_state) &&
		exists(@.source_revision) && exists(@.created_at)
	)
)';
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;

	IF TG_TABLE_NAME = 'activity' THEN
		linked_runtime_session := NEW.aggregate_kind = 'runtime_session'
			OR NEW.event_kind IN ('runtime_session_recorded', 'runtime_session_created',
				'runtime_session_transitioned')
			OR pg_catalog.jsonb_path_exists(NEW.payload, ownership_path);
		IF TG_OP = 'UPDATE' THEN
			linked_runtime_session := linked_runtime_session
				OR OLD.aggregate_kind = 'runtime_session'
				OR OLD.event_kind IN ('runtime_session_recorded', 'runtime_session_created',
					'runtime_session_transitioned')
				OR pg_catalog.jsonb_path_exists(OLD.payload, ownership_path);
		END IF;
	ELSIF TG_TABLE_NAME = 'outbox' THEN
		linked_runtime_session := NEW.aggregate_kind = 'runtime_session'
			OR pg_catalog.jsonb_path_exists(NEW.payload, ownership_path)
			OR EXISTS (
				SELECT 1
				FROM pg_catalog.jsonb_path_query(
					NEW.payload, '$.**.activity_sequence'
				) AS link(value)
				JOIN decodex.activity AS activity ON activity.sequence = CASE
					WHEN pg_catalog.jsonb_typeof(link.value) IN ('number', 'string')
						AND link.value #>> '{}' ~ '^[0-9]+$'
					THEN (link.value #>> '{}')::bigint
				END
				WHERE (activity.aggregate_kind = 'runtime_session'
						OR activity.event_kind IN ('runtime_session_recorded',
							'runtime_session_created', 'runtime_session_transitioned')
						OR pg_catalog.jsonb_path_exists(activity.payload, ownership_path))
			);
		IF TG_OP = 'UPDATE' THEN
			linked_runtime_session := linked_runtime_session
				OR OLD.aggregate_kind = 'runtime_session'
				OR pg_catalog.jsonb_path_exists(OLD.payload, ownership_path)
				OR EXISTS (
					SELECT 1
					FROM pg_catalog.jsonb_path_query(
						OLD.payload, '$.**.activity_sequence'
					) AS link(value)
					JOIN decodex.activity AS activity ON activity.sequence = CASE
						WHEN pg_catalog.jsonb_typeof(link.value) IN ('number', 'string')
							AND link.value #>> '{}' ~ '^[0-9]+$'
						THEN (link.value #>> '{}')::bigint
					END
					WHERE (activity.aggregate_kind = 'runtime_session'
						OR activity.event_kind IN ('runtime_session_recorded',
							'runtime_session_created', 'runtime_session_transitioned')
						OR pg_catalog.jsonb_path_exists(activity.payload, ownership_path))
				);
		END IF;
	ELSE
		RAISE EXCEPTION 'RuntimeSession event namespace has unexpected trigger relation'
			USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_event_namespace';
	END IF;

	IF linked_runtime_session AND current_user::name <> owner_name THEN
		IF TG_TABLE_NAME = 'activity' OR TG_OP = 'INSERT' THEN
			RAISE EXCEPTION 'RuntimeSession activity/outbox namespace is command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_event_namespace';
		ELSIF NEW.id IS DISTINCT FROM OLD.id
			OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload
			OR NEW.created_at IS DISTINCT FROM OLD.created_at
		THEN
			RAISE EXCEPTION 'RuntimeSession outbox authority fields are command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_event_namespace';
		END IF;
	END IF;

	RETURN NEW;
EXCEPTION
	WHEN invalid_text_representation OR numeric_value_out_of_range THEN
		RAISE EXCEPTION 'RuntimeSession activity/outbox link is malformed'
			USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_event_namespace';
END
$$;
