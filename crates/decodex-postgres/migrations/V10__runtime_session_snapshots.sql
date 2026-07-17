-- XY-1337 authoritative RuntimeSession snapshots and exact commands.
-- V10 is an atomic zero-state cutover: no V3 caller-authored session state is accepted.
LOCK TABLE decodex.command_receipts, decodex.exact_command_receipts,
	decodex.conversations, decodex.role_profiles, decodex.role_profile_revisions,
	decodex.profile_snapshots, decodex.account_snapshots, decodex.runtime_sessions,
	decodex.turns, decodex.activity, decodex.outbox IN ACCESS EXCLUSIVE MODE;

DO $$
BEGIN
	IF EXISTS (SELECT 1 FROM decodex.profile_snapshots)
		OR EXISTS (SELECT 1 FROM decodex.account_snapshots)
		OR EXISTS (SELECT 1 FROM decodex.runtime_sessions)
		OR EXISTS (SELECT 1 FROM decodex.turns)
		OR EXISTS (
			SELECT 1 FROM decodex.command_receipts
			WHERE operation IN ('create_runtime_session', 'transition_runtime_session')
		)
		OR EXISTS (
			SELECT 1 FROM decodex.exact_command_receipts
			WHERE request_envelope->>'operation'
				IN ('create_runtime_session', 'transition_runtime_session')
		)
		OR EXISTS (
			SELECT 1 FROM decodex.activity
			WHERE aggregate_kind = 'runtime_session'
				OR event_kind IN ('runtime_session_recorded', 'runtime_session_created',
					'runtime_session_transitioned')
				OR pg_catalog.jsonb_path_exists(payload, '$.** ? (
					@.aggregate_kind == "runtime_session" ||
					@.kind == "runtime_session" ||
					@.event_kind == "runtime_session_recorded" ||
					@.event_kind == "runtime_session_created" ||
					@.event_kind == "runtime_session_transitioned" ||
					exists(@.runtime_session) || exists(@.runtime_session_id) ||
					exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
					exists(@.account_snapshot) || exists(@.account_snapshot_id)
				)')
		)
		OR EXISTS (
			SELECT 1 FROM decodex.outbox AS message
			WHERE message.aggregate_kind = 'runtime_session'
				OR pg_catalog.jsonb_path_exists(message.payload, '$.** ? (
					@.aggregate_kind == "runtime_session" ||
					@.kind == "runtime_session" ||
					@.event_kind == "runtime_session_recorded" ||
					@.event_kind == "runtime_session_created" ||
					@.event_kind == "runtime_session_transitioned" ||
					exists(@.runtime_session) || exists(@.runtime_session_id) ||
					exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
					exists(@.account_snapshot) || exists(@.account_snapshot_id)
				)')
				OR EXISTS (
					SELECT 1
					FROM pg_catalog.jsonb_path_query(
						message.payload, '$.**.activity_sequence'
					) AS link(value)
					JOIN decodex.activity AS activity ON activity.sequence = CASE
						WHEN pg_catalog.jsonb_typeof(link.value) IN ('number', 'string')
							AND link.value #>> '{}' ~ '^[0-9]+$'
						THEN (link.value #>> '{}')::bigint
					END
					WHERE (
							activity.aggregate_kind = 'runtime_session'
							OR activity.event_kind IN ('runtime_session_recorded',
								'runtime_session_created', 'runtime_session_transitioned')
							OR pg_catalog.jsonb_path_exists(activity.payload, '$.** ? (
								@.aggregate_kind == "runtime_session" ||
								@.kind == "runtime_session" ||
								@.event_kind == "runtime_session_recorded" ||
								@.event_kind == "runtime_session_created" ||
								@.event_kind == "runtime_session_transitioned" ||
								exists(@.runtime_session) || exists(@.runtime_session_id) ||
								exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
								exists(@.account_snapshot) || exists(@.account_snapshot_id)
							)')
						)
				)
		)
	THEN
		RAISE EXCEPTION 'V10 RuntimeSession authority requires zero incompatible state'
			USING ERRCODE = '55000', CONSTRAINT = 'runtime_session_v10_zero_state';
	END IF;
END
$$;

ALTER TABLE decodex.profile_snapshots
	DROP CONSTRAINT profile_snapshots_no_credentials,
	DROP CONSTRAINT profile_snapshots_role_check,
	ALTER COLUMN role TYPE decodex.role_profile_role
		USING role::decodex.role_profile_role,
	ADD COLUMN instructions text NOT NULL,
	ADD COLUMN provenance text,
	ADD CONSTRAINT profile_snapshots_source_identity CHECK (source_profile_id = role::text),
	ADD CONSTRAINT profile_snapshots_instruction_digest CHECK (
		instructions_digest = pg_catalog.encode(
			public.digest(pg_catalog.convert_to(instructions, 'UTF8'), 'sha256'), 'hex'
		)
	),
	ADD CONSTRAINT profile_snapshots_configuration CHECK (
		decodex.is_role_profile_configuration(
			model, reasoning_effort, service_tier, instructions, provenance
		)
	),
	ADD CONSTRAINT profile_snapshots_source_revision_fk
		FOREIGN KEY (role, source_revision)
		REFERENCES decodex.role_profile_revisions(role, revision) ON DELETE RESTRICT;

ALTER TABLE decodex.account_snapshots
	DROP CONSTRAINT account_snapshots_no_credentials,
	DROP CONSTRAINT account_snapshots_source_account_id_check,
	DROP CONSTRAINT account_snapshots_observed_state_check,
	ALTER COLUMN source_account_id TYPE uuid USING source_account_id::uuid,
	ALTER COLUMN observed_state TYPE decodex.account_state
		USING observed_state::decodex.account_state,
	ADD CONSTRAINT account_snapshots_facts CHECK (
		pg_catalog.octet_length(display_label) BETWEEN 1 AND 128
		AND NOT decodex.has_credential_material(display_label)
	);

ALTER TABLE decodex.runtime_sessions
	DROP CONSTRAINT runtime_sessions_codex_thread_id_check,
	DROP CONSTRAINT runtime_sessions_no_credentials,
	ALTER COLUMN codex_thread_id TYPE uuid USING codex_thread_id::uuid,
	ADD CONSTRAINT runtime_sessions_no_credentials CHECK (
		(codex_thread_id IS NULL
			OR NOT decodex.has_credential_material(codex_thread_id::text))
		AND (last_known_turn_id IS NULL
			OR NOT decodex.has_credential_material(last_known_turn_id))
	),
	ADD CONSTRAINT runtime_sessions_initial_turn_null CHECK (
		revision <> 1 OR last_known_turn_id IS NULL
	);

-- Preserve readback while atomically removing every previously granted direct writer.
-- V9 readiness proves there are no column grants or grant options before this cutover.
DO $$
DECLARE target record;
BEGIN
	FOR target IN
		SELECT DISTINCT class.relname, role.rolname
		FROM pg_catalog.pg_class AS class
		JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace
		CROSS JOIN LATERAL pg_catalog.aclexplode(
			COALESCE(class.relacl, pg_catalog.acldefault('r', class.relowner))
		) AS privilege
		JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee
		WHERE namespace.nspname = 'decodex'
			AND class.relname IN ('profile_snapshots', 'account_snapshots', 'runtime_sessions')
			AND privilege.grantee <> class.relowner
			AND privilege.privilege_type IN (
				'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER', 'MAINTAIN'
			)
	LOOP
		EXECUTE pg_catalog.format(
			'REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN ON TABLE decodex.%I FROM %I',
			target.relname, target.rolname
		);
	END LOOP;
END
$$;

CREATE FUNCTION decodex.enforce_runtime_session_command_owner()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;

	IF current_user::name <> owner_name THEN
		RAISE EXCEPTION 'RuntimeSession state is command-owned'
			USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_command_owner';
	END IF;

	RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;

CREATE TRIGGER profile_snapshots_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.profile_snapshots
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_runtime_session_command_owner();
CREATE TRIGGER account_snapshots_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.account_snapshots
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_runtime_session_command_owner();
CREATE TRIGGER runtime_sessions_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.runtime_sessions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_runtime_session_command_owner();

CREATE FUNCTION decodex.forbid_runtime_snapshot_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'RuntimeSession snapshots are immutable'
		USING ERRCODE = '23514', CONSTRAINT = 'runtime_session_snapshots_immutable';
END
$$;

CREATE TRIGGER profile_snapshots_immutable
BEFORE UPDATE OR DELETE ON decodex.profile_snapshots
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_runtime_snapshot_mutation();
CREATE TRIGGER account_snapshots_immutable
BEFORE UPDATE OR DELETE ON decodex.account_snapshots
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_runtime_snapshot_mutation();

CREATE FUNCTION decodex.enforce_runtime_session_event_namespace()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
DECLARE linked_runtime_session boolean := false;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;

	IF TG_TABLE_NAME = 'activity' THEN
		linked_runtime_session := NEW.aggregate_kind = 'runtime_session'
			OR NEW.event_kind IN ('runtime_session_recorded', 'runtime_session_created',
				'runtime_session_transitioned')
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "runtime_session" ||
				@.kind == "runtime_session" ||
				@.event_kind == "runtime_session_recorded" ||
				@.event_kind == "runtime_session_created" ||
				@.event_kind == "runtime_session_transitioned" ||
				exists(@.runtime_session) || exists(@.runtime_session_id) ||
				exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
				exists(@.account_snapshot) || exists(@.account_snapshot_id)
			)');
		IF TG_OP = 'UPDATE' THEN
			linked_runtime_session := linked_runtime_session
				OR OLD.aggregate_kind = 'runtime_session'
				OR OLD.event_kind IN ('runtime_session_recorded', 'runtime_session_created',
					'runtime_session_transitioned')
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "runtime_session" ||
					@.kind == "runtime_session" ||
					@.event_kind == "runtime_session_recorded" ||
					@.event_kind == "runtime_session_created" ||
					@.event_kind == "runtime_session_transitioned" ||
					exists(@.runtime_session) || exists(@.runtime_session_id) ||
					exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
					exists(@.account_snapshot) || exists(@.account_snapshot_id)
				)');
		END IF;
	ELSE
		linked_runtime_session := NEW.aggregate_kind = 'runtime_session'
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "runtime_session" ||
				@.kind == "runtime_session" ||
				@.event_kind == "runtime_session_recorded" ||
				@.event_kind == "runtime_session_created" ||
				@.event_kind == "runtime_session_transitioned" ||
				exists(@.runtime_session) || exists(@.runtime_session_id) ||
				exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
				exists(@.account_snapshot) || exists(@.account_snapshot_id)
			)')
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
						OR pg_catalog.jsonb_path_exists(activity.payload, '$.** ? (
							@.aggregate_kind == "runtime_session" || @.kind == "runtime_session" ||
							@.event_kind == "runtime_session_recorded" ||
							@.event_kind == "runtime_session_created" ||
							@.event_kind == "runtime_session_transitioned" ||
							exists(@.runtime_session) || exists(@.runtime_session_id) ||
							exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
							exists(@.account_snapshot) || exists(@.account_snapshot_id)
						)'))
			);
		IF TG_OP = 'UPDATE' THEN
			linked_runtime_session := linked_runtime_session
				OR OLD.aggregate_kind = 'runtime_session'
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "runtime_session" ||
					@.kind == "runtime_session" ||
					@.event_kind == "runtime_session_recorded" ||
					@.event_kind == "runtime_session_created" ||
					@.event_kind == "runtime_session_transitioned" ||
					exists(@.runtime_session) || exists(@.runtime_session_id) ||
					exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
					exists(@.account_snapshot) || exists(@.account_snapshot_id)
				)')
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
						OR pg_catalog.jsonb_path_exists(activity.payload, '$.** ? (
							@.aggregate_kind == "runtime_session" || @.kind == "runtime_session" ||
							@.event_kind == "runtime_session_recorded" ||
							@.event_kind == "runtime_session_created" ||
							@.event_kind == "runtime_session_transitioned" ||
							exists(@.runtime_session) || exists(@.runtime_session_id) ||
							exists(@.profile_snapshot) || exists(@.profile_snapshot_id) ||
							exists(@.account_snapshot) || exists(@.account_snapshot_id)
						)')
					)
				);
		END IF;
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
	WHEN invalid_text_representation THEN
		RAISE EXCEPTION 'RuntimeSession activity/outbox link is malformed'
			USING ERRCODE = '42501', CONSTRAINT = 'runtime_session_event_namespace';
END
$$;

CREATE TRIGGER activity_runtime_session_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_runtime_session_event_namespace();
CREATE TRIGGER outbox_runtime_session_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_runtime_session_event_namespace();

CREATE FUNCTION decodex.build_runtime_session_create_request(
	p_protocol text, p_session_id uuid, p_conversation_id uuid,
	p_role decodex.role_profile_role, p_account_snapshot_id uuid,
	p_source_account_id uuid, p_display_label text,
	p_observed_state decodex.account_state, p_account_source_revision bigint,
	p_codex_thread_id uuid, p_initial_state decodex.runtime_session_state
) RETURNS jsonb
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
SELECT pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'create_runtime_session',
	'runtime_session_id', p_session_id, 'conversation_id', p_conversation_id,
	'role', p_role, 'account_snapshot_id', p_account_snapshot_id,
	'source_account_id', p_source_account_id, 'display_label', p_display_label,
	'observed_state', p_observed_state,
	'account_source_revision', p_account_source_revision,
	'codex_thread_id', p_codex_thread_id, 'initial_state', p_initial_state
)
$$;

CREATE FUNCTION decodex.build_runtime_session_transition_request(
	p_protocol text, p_session_id uuid, p_expected_revision bigint,
	p_target_state decodex.runtime_session_state
) RETURNS jsonb
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
SELECT pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'transition_runtime_session',
	'runtime_session_id', p_session_id, 'expected_revision', p_expected_revision,
	'target_state', p_target_state
)
$$;

CREATE FUNCTION decodex.complete_exact_runtime_session_rejection(
	p_protocol text, p_idempotency_key text, p_code text
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE effect_value jsonb;
DECLARE response_value bytea;
DECLARE request_value jsonb;
BEGIN
	SELECT request_envelope INTO STRICT request_value
	FROM decodex.exact_command_receipts
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;
	effect_value := pg_catalog.jsonb_build_object(
		'changed', false, 'code', p_code, 'request', request_value
	);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification', 'stable_domain_rejection', 'code', p_code,
		'effect', effect_value
	)::text, 'UTF8');

	UPDATE decodex.exact_command_receipts
	SET receipt_state = 'completed_rejected', outcome_class = 'stable_domain_rejection',
		effect_envelope = effect_value, response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;

	RETURN response_value;
END
$$;

CREATE FUNCTION decodex.create_runtime_session_exact(
	p_protocol text, p_idempotency_key text,
	p_session_id uuid, p_conversation_id uuid, p_role decodex.role_profile_role,
	p_account_snapshot_id uuid, p_source_account_id uuid, p_display_label text,
	p_observed_state decodex.account_state, p_account_source_revision bigint,
	p_codex_thread_id uuid, p_initial_state decodex.runtime_session_state
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
DECLARE inserted_count integer;
DECLARE conversation_status decodex.conversation_status;
DECLARE identity_lock bigint;
DECLARE thread_lock bigint;
DECLARE profile_snapshot_id uuid;
DECLARE profile_value jsonb;
DECLARE account_value jsonb;
DECLARE session_value jsonb;
DECLARE activity_sequence bigint;
DECLARE activity_aggregate_kind text;
DECLARE activity_aggregate_id text;
DECLARE activity_revision bigint;
DECLARE activity_event_kind text;
DECLARE activity_payload jsonb;
DECLARE outbox_id bigint;
DECLARE outbox_effect_key text;
DECLARE outbox_aggregate_kind text;
DECLARE outbox_aggregate_id text;
DECLARE outbox_aggregate_revision bigint;
DECLARE outbox_payload jsonb;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_session_id IS NULL
		OR p_conversation_id IS NULL OR p_role IS NULL OR p_account_snapshot_id IS NULL
		OR p_source_account_id IS NULL OR p_display_label IS NULL
		OR p_observed_state IS NULL OR p_account_source_revision IS NULL
		OR p_initial_state IS NULL
	THEN
		RAISE EXCEPTION 'exact RuntimeSession creation is incomplete' USING ERRCODE = '22004';
	END IF;
	IF pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN
		RAISE EXCEPTION 'exact RuntimeSession command identity is invalid' USING ERRCODE = '22023';
	END IF;

	request_value := decodex.build_runtime_session_create_request(
		p_protocol, p_session_id, p_conversation_id, p_role,
		p_account_snapshot_id, p_source_account_id, p_display_label,
		p_observed_state, p_account_source_revision, p_codex_thread_id, p_initial_state
	);
	INSERT INTO decodex.exact_command_receipts(
		protocol_version, idempotency_key, request_envelope, request_digest, receipt_state
	) VALUES (
		p_protocol, p_idempotency_key, request_value,
		public.digest(pg_catalog.convert_to(request_value::text, 'UTF8'), 'sha256'), 'executing'
	) ON CONFLICT (protocol_version, idempotency_key) DO NOTHING;
	GET DIAGNOSTICS inserted_count = ROW_COUNT;

	IF inserted_count = 0 THEN
		SELECT request_envelope, response_bytes INTO existing_request, existing_response
		FROM decodex.exact_command_receipts
		WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key
		FOR UPDATE;
		IF existing_request <> request_value THEN
			RAISE EXCEPTION 'exact idempotency conflict' USING ERRCODE = 'DX001';
		END IF;
		IF existing_response IS NULL THEN
			RAISE EXCEPTION 'incomplete exact receipt is not replayable' USING ERRCODE = 'DX002';
		END IF;
		RETURN existing_response;
	END IF;

	IF p_initial_state NOT IN ('starting', 'active') THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'illegal_transition'
		);
	END IF;
	IF p_account_source_revision < 1
		OR pg_catalog.octet_length(p_display_label) NOT BETWEEN 1 AND 128
		OR decodex.has_credential_material(p_display_label)
	THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'invalid_account_snapshot'
		);
	END IF;

	-- Match the statement-level hierarchy trigger order: acquire the outer coordinator
	-- before selecting or locking a Conversation or RuntimeSession tuple.
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);

	-- The row lock is both the Conversation eligibility proof and the canonical create order.
	SELECT status INTO conversation_status FROM decodex.conversations
	WHERE conversation_id = p_conversation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'missing_target'
		);
	END IF;
	IF conversation_status <> 'open' THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'illegal_transition'
		);
	END IF;

	-- Serialize both unique RuntimeSession identities in canonical order even when
	-- hostile callers name different Conversations. Hash collisions only add safe
	-- serialization; they cannot weaken uniqueness or rejection classification.
	identity_lock := pg_catalog.hashtextextended(
		'runtime_session/id/' || p_session_id::text, 0
	);
	thread_lock := CASE WHEN p_codex_thread_id IS NULL THEN identity_lock ELSE
		pg_catalog.hashtextextended('runtime_session/thread/' || p_codex_thread_id::text, 0)
	END;
	PERFORM pg_catalog.pg_advisory_xact_lock(
		CASE WHEN identity_lock <= thread_lock THEN identity_lock ELSE thread_lock END
	);
	IF thread_lock <> identity_lock THEN
		PERFORM pg_catalog.pg_advisory_xact_lock(
			CASE WHEN identity_lock > thread_lock THEN identity_lock ELSE thread_lock END
		);
	END IF;
	IF EXISTS (
		SELECT 1 FROM decodex.runtime_sessions
		WHERE runtime_session_id = p_session_id
			OR (p_codex_thread_id IS NOT NULL AND codex_thread_id = p_codex_thread_id)
	) THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'duplicate_target'
		);
	END IF;

	-- FOR SHARE prevents a concurrent profile advance until the complete selected
	-- revision has been copied, yielding either the old or new immutable revision.
	SELECT pg_catalog.jsonb_build_object(
		'role', revision.role, 'revision', revision.revision,
		'model', revision.model, 'reasoning_effort', revision.reasoning_effort,
		'service_tier', revision.service_tier, 'instructions', revision.instructions,
		'provenance', revision.provenance, 'created_at', revision.created_at
	) INTO profile_value
	FROM decodex.role_profiles AS profile
	JOIN decodex.role_profile_revisions AS revision
		ON (revision.role, revision.revision) = (profile.role, profile.current_revision)
	WHERE profile.role = p_role
	FOR SHARE OF profile;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'missing_target'
		);
	END IF;

	INSERT INTO decodex.account_snapshots(
		account_snapshot_id, source_account_id, display_label, observed_state, source_revision
	) VALUES (
		p_account_snapshot_id, p_source_account_id, p_display_label,
		p_observed_state, p_account_source_revision
	) ON CONFLICT (account_snapshot_id) DO NOTHING;
	SELECT pg_catalog.jsonb_build_object(
		'account_snapshot_id', account_snapshot_id,
		'source_account_id', source_account_id, 'display_label', display_label,
		'observed_state', observed_state, 'source_revision', source_revision,
		'created_at', created_at
	) INTO account_value
	FROM decodex.account_snapshots WHERE account_snapshot_id = p_account_snapshot_id;
	IF account_value->>'source_account_id' <> p_source_account_id::text
		OR account_value->>'display_label' <> p_display_label
		OR account_value->>'observed_state' <> p_observed_state::text
		OR (account_value->>'source_revision')::bigint <> p_account_source_revision
	THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'account_snapshot_conflict'
		);
	END IF;

	profile_snapshot_id := pg_catalog.gen_random_uuid();
	INSERT INTO decodex.profile_snapshots(
		profile_snapshot_id, source_profile_id, role, model, reasoning_effort,
		service_tier, instructions_digest, instructions, provenance, source_revision
	) VALUES (
		profile_snapshot_id, profile_value->>'role', p_role,
		profile_value->>'model', profile_value->>'reasoning_effort',
		profile_value->>'service_tier',
		pg_catalog.encode(public.digest(
			pg_catalog.convert_to(profile_value->>'instructions', 'UTF8'), 'sha256'
		), 'hex'), profile_value->>'instructions', profile_value->>'provenance',
		(profile_value->>'revision')::bigint
	) RETURNING pg_catalog.jsonb_build_object(
		'profile_snapshot_id', profile_snapshot_id,
		'source_profile_id', source_profile_id, 'role', role,
		'model', model, 'reasoning_effort', reasoning_effort,
		'service_tier', service_tier, 'instructions_digest', instructions_digest,
		'instructions', instructions, 'provenance', provenance,
		'source_revision', source_revision, 'created_at', created_at
	) INTO profile_value;

	INSERT INTO decodex.runtime_sessions(
		runtime_session_id, conversation_id, profile_snapshot_id,
		account_snapshot_id, codex_thread_id, state, last_known_turn_id
	) VALUES (
		p_session_id, p_conversation_id, profile_snapshot_id,
		p_account_snapshot_id, p_codex_thread_id, p_initial_state, NULL
	) RETURNING pg_catalog.jsonb_build_object(
		'runtime_session_id', runtime_session_id, 'conversation_id', conversation_id,
		'profile_snapshot_id', profile_snapshot_id,
		'account_snapshot_id', account_snapshot_id,
		'codex_thread_id', codex_thread_id, 'last_known_turn_id', last_known_turn_id,
		'state', state, 'revision', revision, 'created_at', created_at,
		'updated_at', updated_at, 'ended_at', ended_at
	) INTO session_value;

	activity_payload := pg_catalog.jsonb_build_object(
		'kind', 'runtime_session', 'runtime_session', session_value,
		'profile_snapshot', profile_value, 'account_snapshot', account_value
	);
	INSERT INTO decodex.activity(
		aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload
	) VALUES (
		'runtime_session', p_session_id::text, 1, 'runtime_session_created',
		p_idempotency_key, activity_payload
	) RETURNING sequence, aggregate_kind, aggregate_id, revision, event_kind, payload
	INTO activity_sequence, activity_aggregate_kind, activity_aggregate_id,
		activity_revision, activity_event_kind, activity_payload;
	outbox_payload := pg_catalog.jsonb_build_object(
		'activity_sequence', activity_sequence, 'event_kind', 'runtime_session_created',
		'aggregate_kind', 'runtime_session', 'aggregate_id', p_session_id,
		'revision', 1, 'payload', activity_payload
	);
	INSERT INTO decodex.outbox(
		effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	) VALUES (
		'activity/' || activity_sequence::text, 'runtime_session', p_session_id::text,
		1, outbox_payload
	) RETURNING id, effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	INTO outbox_id, outbox_effect_key, outbox_aggregate_kind,
		outbox_aggregate_id, outbox_aggregate_revision, outbox_payload;

	effect_value := pg_catalog.jsonb_build_object(
		'request', request_value,
		'runtime_session', session_value, 'profile_snapshot', profile_value,
		'account_snapshot', account_value, 'prior_state', NULL,
		'new_state', p_initial_state, 'prior_revision', NULL, 'new_revision', 1,
		'activity_sequence', activity_sequence,
		'activity_aggregate_kind', activity_aggregate_kind,
		'activity_aggregate_id', activity_aggregate_id,
		'activity_revision', activity_revision,
		'activity_event_kind', activity_event_kind,
		'activity_payload', activity_payload,
		'outbox_id', outbox_id, 'outbox_effect_key', outbox_effect_key,
		'outbox_aggregate_kind', outbox_aggregate_kind,
		'outbox_aggregate_id', outbox_aggregate_id,
		'outbox_aggregate_revision', outbox_aggregate_revision,
		'outbox_payload', outbox_payload
	);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification', 'success', 'effect', effect_value
	)::text, 'UTF8');
	UPDATE decodex.exact_command_receipts
	SET receipt_state = 'completed_success', outcome_class = 'success',
		effect_envelope = effect_value, response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;

	RETURN response_value;
END
$$;

CREATE FUNCTION decodex.transition_runtime_session_exact(
	p_protocol text, p_idempotency_key text, p_session_id uuid,
	p_expected_revision bigint, p_target_state decodex.runtime_session_state
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
DECLARE inserted_count integer;
DECLARE prior_state decodex.runtime_session_state;
DECLARE actual_revision bigint;
DECLARE profile_value jsonb;
DECLARE account_value jsonb;
DECLARE session_value jsonb;
DECLARE activity_sequence bigint;
DECLARE activity_aggregate_kind text;
DECLARE activity_aggregate_id text;
DECLARE activity_revision bigint;
DECLARE activity_event_kind text;
DECLARE activity_payload jsonb;
DECLARE outbox_id bigint;
DECLARE outbox_effect_key text;
DECLARE outbox_aggregate_kind text;
DECLARE outbox_aggregate_id text;
DECLARE outbox_aggregate_revision bigint;
DECLARE outbox_payload jsonb;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_session_id IS NULL
		OR p_expected_revision IS NULL OR p_target_state IS NULL
	THEN
		RAISE EXCEPTION 'exact RuntimeSession transition is incomplete' USING ERRCODE = '22004';
	END IF;
	IF pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN
		RAISE EXCEPTION 'exact RuntimeSession command identity is invalid' USING ERRCODE = '22023';
	END IF;

	request_value := decodex.build_runtime_session_transition_request(
		p_protocol, p_session_id, p_expected_revision, p_target_state
	);
	INSERT INTO decodex.exact_command_receipts(
		protocol_version, idempotency_key, request_envelope, request_digest, receipt_state
	) VALUES (
		p_protocol, p_idempotency_key, request_value,
		public.digest(pg_catalog.convert_to(request_value::text, 'UTF8'), 'sha256'), 'executing'
	) ON CONFLICT (protocol_version, idempotency_key) DO NOTHING;
	GET DIAGNOSTICS inserted_count = ROW_COUNT;

	IF inserted_count = 0 THEN
		SELECT request_envelope, response_bytes INTO existing_request, existing_response
		FROM decodex.exact_command_receipts
		WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key
		FOR UPDATE;
		IF existing_request <> request_value THEN
			RAISE EXCEPTION 'exact idempotency conflict' USING ERRCODE = 'DX001';
		END IF;
		IF existing_response IS NULL THEN
			RAISE EXCEPTION 'incomplete exact receipt is not replayable' USING ERRCODE = 'DX002';
		END IF;
		RETURN existing_response;
	END IF;

	-- The following UPDATE has the same outer coordinator as every hierarchy mutation.
	-- Acquire it before the executor can select and lock the RuntimeSession tuple below.
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);

	SELECT session.state, session.revision,
		pg_catalog.jsonb_build_object(
			'profile_snapshot_id', profile.profile_snapshot_id,
			'source_profile_id', profile.source_profile_id, 'role', profile.role,
			'model', profile.model, 'reasoning_effort', profile.reasoning_effort,
			'service_tier', profile.service_tier,
			'instructions_digest', profile.instructions_digest,
			'instructions', profile.instructions, 'provenance', profile.provenance,
			'source_revision', profile.source_revision, 'created_at', profile.created_at
		),
		pg_catalog.jsonb_build_object(
			'account_snapshot_id', account.account_snapshot_id,
			'source_account_id', account.source_account_id,
			'display_label', account.display_label, 'observed_state', account.observed_state,
			'source_revision', account.source_revision, 'created_at', account.created_at
		)
	INTO prior_state, actual_revision, profile_value, account_value
	FROM decodex.runtime_sessions AS session
	JOIN decodex.profile_snapshots AS profile USING (profile_snapshot_id)
	JOIN decodex.account_snapshots AS account USING (account_snapshot_id)
	WHERE session.runtime_session_id = p_session_id
	FOR UPDATE OF session;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'missing_target'
		);
	END IF;
	IF actual_revision <> p_expected_revision THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'stale_revision'
		);
	END IF;
	IF NOT (
		(prior_state = 'starting' AND p_target_state IN ('active', 'ended', 'diverged'))
		OR (prior_state = 'active' AND p_target_state IN ('ended', 'diverged'))
	) OR (p_target_state IN ('ended', 'diverged') AND EXISTS (
		SELECT 1 FROM decodex.turns
		WHERE runtime_session_id = p_session_id AND status = 'active'
	)) THEN
		RETURN decodex.complete_exact_runtime_session_rejection(
			p_protocol, p_idempotency_key, 'illegal_transition'
		);
	END IF;

	UPDATE decodex.runtime_sessions
	SET state = p_target_state, revision = revision + 1
	WHERE runtime_session_id = p_session_id AND revision = p_expected_revision
	RETURNING pg_catalog.jsonb_build_object(
		'runtime_session_id', runtime_session_id, 'conversation_id', conversation_id,
		'profile_snapshot_id', profile_snapshot_id,
		'account_snapshot_id', account_snapshot_id,
		'codex_thread_id', codex_thread_id, 'last_known_turn_id', last_known_turn_id,
		'state', state, 'revision', revision, 'created_at', created_at,
		'updated_at', updated_at, 'ended_at', ended_at
	) INTO session_value;
	IF session_value IS NULL THEN
		RAISE EXCEPTION 'RuntimeSession compare-and-swap lost after row lock' USING ERRCODE = '40001';
	END IF;

	activity_payload := pg_catalog.jsonb_build_object(
		'kind', 'runtime_session', 'runtime_session', session_value,
		'profile_snapshot', profile_value, 'account_snapshot', account_value,
		'prior_state', prior_state, 'new_state', p_target_state,
		'prior_revision', p_expected_revision,
		'new_revision', (session_value->>'revision')::bigint
	);
	INSERT INTO decodex.activity(
		aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload
	) VALUES (
		'runtime_session', p_session_id::text, (session_value->>'revision')::bigint,
		'runtime_session_transitioned', p_idempotency_key, activity_payload
	) RETURNING sequence, aggregate_kind, aggregate_id, revision, event_kind, payload
	INTO activity_sequence, activity_aggregate_kind, activity_aggregate_id,
		activity_revision, activity_event_kind, activity_payload;
	outbox_payload := pg_catalog.jsonb_build_object(
		'activity_sequence', activity_sequence, 'event_kind', 'runtime_session_transitioned',
		'aggregate_kind', 'runtime_session', 'aggregate_id', p_session_id,
		'revision', (session_value->>'revision')::bigint, 'payload', activity_payload
	);
	INSERT INTO decodex.outbox(
		effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	) VALUES (
		'activity/' || activity_sequence::text, 'runtime_session', p_session_id::text,
		(session_value->>'revision')::bigint, outbox_payload
	) RETURNING id, effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	INTO outbox_id, outbox_effect_key, outbox_aggregate_kind,
		outbox_aggregate_id, outbox_aggregate_revision, outbox_payload;

	effect_value := pg_catalog.jsonb_build_object(
		'request', request_value,
		'runtime_session', session_value, 'profile_snapshot', profile_value,
		'account_snapshot', account_value, 'prior_state', prior_state,
		'new_state', p_target_state, 'prior_revision', p_expected_revision,
		'new_revision', (session_value->>'revision')::bigint,
		'activity_sequence', activity_sequence,
		'activity_aggregate_kind', activity_aggregate_kind,
		'activity_aggregate_id', activity_aggregate_id,
		'activity_revision', activity_revision,
		'activity_event_kind', activity_event_kind,
		'activity_payload', activity_payload,
		'outbox_id', outbox_id, 'outbox_effect_key', outbox_effect_key,
		'outbox_aggregate_kind', outbox_aggregate_kind,
		'outbox_aggregate_id', outbox_aggregate_id,
		'outbox_aggregate_revision', outbox_aggregate_revision,
		'outbox_payload', outbox_payload
	);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification', 'success', 'effect', effect_value
	)::text, 'UTF8');
	UPDATE decodex.exact_command_receipts
	SET receipt_state = 'completed_success', outcome_class = 'success',
		effect_envelope = effect_value, response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;

	RETURN response_value;
END
$$;

REVOKE ALL ON TABLE decodex.profile_snapshots,
	decodex.account_snapshots, decodex.runtime_sessions FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
