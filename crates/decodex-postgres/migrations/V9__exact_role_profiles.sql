-- XY-1346 exact in-transaction receipts and immutable global RoleProfiles.
CREATE TYPE decodex.exact_receipt_state AS ENUM (
	'executing', 'completed_success', 'completed_rejected'
);
CREATE TYPE decodex.role_profile_role AS ENUM ('advisor', 'lead', 'task', 'reviewer');

CREATE TABLE decodex.exact_command_receipts (
	protocol_version text NOT NULL,
	idempotency_key text NOT NULL,
	request_envelope jsonb NOT NULL,
	request_digest bytea NOT NULL,
	receipt_state decodex.exact_receipt_state NOT NULL,
	outcome_class text,
	effect_envelope jsonb,
	response_bytes bytea,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	completed_at timestamptz,
	CONSTRAINT exact_command_receipts_pkey PRIMARY KEY (protocol_version, idempotency_key),
	CONSTRAINT exact_command_receipts_identity_bounded CHECK (
		pg_catalog.octet_length(protocol_version) BETWEEN 1 AND 64
		AND protocol_version COLLATE pg_catalog."C" ~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		AND pg_catalog.octet_length(idempotency_key) BETWEEN 1 AND 256
		AND NOT decodex.has_credential_material(idempotency_key)
	),
	CONSTRAINT exact_command_receipts_request_digest CHECK (
		request_digest = public.digest(
			pg_catalog.convert_to(request_envelope::text, 'UTF8'), 'sha256'
		)
	),
	CONSTRAINT exact_command_receipts_no_credentials CHECK (
		NOT decodex.has_credential_material(request_envelope)
		AND (effect_envelope IS NULL OR NOT decodex.has_credential_material(effect_envelope))
	),
	CONSTRAINT exact_command_receipts_shape CHECK (
		(receipt_state = 'executing'
			AND outcome_class IS NULL AND effect_envelope IS NULL
			AND response_bytes IS NULL AND completed_at IS NULL)
		OR
		(receipt_state = 'completed_success' AND outcome_class = 'success'
			AND effect_envelope IS NOT NULL AND response_bytes IS NOT NULL
			AND completed_at IS NOT NULL)
		OR
		(receipt_state = 'completed_rejected' AND outcome_class = 'stable_domain_rejection'
			AND effect_envelope IS NOT NULL AND response_bytes IS NOT NULL
			AND completed_at IS NOT NULL)
	),
	CONSTRAINT exact_command_receipts_response_authority CHECK (
		response_bytes IS NULL OR (
			pg_catalog.convert_from(response_bytes, 'UTF8')::jsonb->>'classification'
				IS NOT DISTINCT FROM outcome_class
			AND pg_catalog.convert_from(response_bytes, 'UTF8')::jsonb->'effect'
				IS NOT DISTINCT FROM effect_envelope
		)
	),
	CONSTRAINT exact_command_receipts_finite_times CHECK (
		pg_catalog.isfinite(created_at)
		AND (completed_at IS NULL OR pg_catalog.isfinite(completed_at))
		AND (completed_at IS NULL OR completed_at >= created_at)
	)
);

CREATE TABLE decodex.role_profiles (
	role decodex.role_profile_role PRIMARY KEY,
	current_revision bigint NOT NULL CHECK (current_revision > 0),
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT role_profiles_finite_times CHECK (
		pg_catalog.isfinite(created_at) AND pg_catalog.isfinite(updated_at)
		AND updated_at >= created_at
	)
);

CREATE TABLE decodex.role_profile_revisions (
	role decodex.role_profile_role NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	model text NOT NULL,
	reasoning_effort text NOT NULL,
	service_tier text NOT NULL,
	instructions text NOT NULL,
	provenance text,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT role_profile_revisions_pkey PRIMARY KEY (role, revision),
	CONSTRAINT role_profile_revisions_profile_fk FOREIGN KEY (role)
		REFERENCES decodex.role_profiles(role) ON DELETE RESTRICT,
	CONSTRAINT role_profile_revisions_configuration CHECK (
		pg_catalog.octet_length(model) BETWEEN 1 AND 128
		AND pg_catalog.octet_length(reasoning_effort) BETWEEN 1 AND 32
		AND reasoning_effort COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
		AND pg_catalog.octet_length(service_tier) BETWEEN 1 AND 32
		AND service_tier COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
		AND pg_catalog.octet_length(instructions) BETWEEN 1 AND 65536
		AND (provenance IS NULL OR pg_catalog.octet_length(provenance) BETWEEN 1 AND 4096)
		AND model COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND reasoning_effort COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND service_tier COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND instructions COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND (provenance IS NULL OR provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]')
		AND NOT decodex.has_credential_material(model)
		AND NOT decodex.has_credential_material(reasoning_effort)
		AND NOT decodex.has_credential_material(service_tier)
		AND NOT decodex.has_credential_material(instructions)
		AND (provenance IS NULL OR NOT decodex.has_credential_material(provenance))
	),
	CONSTRAINT role_profile_revisions_finite_time CHECK (pg_catalog.isfinite(created_at))
);

ALTER TABLE decodex.role_profiles
	ADD CONSTRAINT role_profiles_current_revision_fk
	FOREIGN KEY (role, current_revision)
	REFERENCES decodex.role_profile_revisions(role, revision)
	DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION decodex.enforce_exact_receipt_completion()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE current_state decodex.exact_receipt_state;
BEGIN
	SELECT receipt_state INTO current_state
	FROM decodex.exact_command_receipts
	WHERE protocol_version = NEW.protocol_version
		AND idempotency_key = NEW.idempotency_key;

	IF current_state = 'executing' THEN
		RAISE EXCEPTION 'exact command receipt must be completed before commit'
			USING ERRCODE = '23514', CONSTRAINT = 'exact_receipts_complete_at_commit';
	END IF;

	RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER exact_receipts_complete_at_commit
AFTER INSERT OR UPDATE ON decodex.exact_command_receipts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_exact_receipt_completion();

CREATE FUNCTION decodex.forbid_exact_receipt_rewrite()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'exact command receipts cannot be deleted'
			USING ERRCODE = '23514', CONSTRAINT = 'exact_receipts_immutable';
	END IF;

	IF OLD.receipt_state <> 'executing'
		OR NEW.receipt_state = 'executing'
		OR OLD.protocol_version <> NEW.protocol_version
		OR OLD.idempotency_key <> NEW.idempotency_key
		OR OLD.request_envelope <> NEW.request_envelope
		OR OLD.request_digest <> NEW.request_digest
		OR OLD.created_at <> NEW.created_at
	THEN
		RAISE EXCEPTION 'exact command receipt rewrite is forbidden'
			USING ERRCODE = '23514', CONSTRAINT = 'exact_receipts_immutable';
	END IF;

	RETURN NEW;
END
$$;

CREATE TRIGGER exact_receipts_immutable
BEFORE UPDATE OR DELETE ON decodex.exact_command_receipts
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_exact_receipt_rewrite();

CREATE FUNCTION decodex.forbid_exact_receipt_truncate()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'exact command receipts cannot be truncated'
		USING ERRCODE = '23514', CONSTRAINT = 'exact_receipts_untruncatable';
END
$$;

CREATE TRIGGER exact_receipts_untruncatable
BEFORE TRUNCATE ON decodex.exact_command_receipts
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_exact_receipt_truncate();

CREATE FUNCTION decodex.enforce_complete_role_profile_set()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE profile_count bigint;
BEGIN
	SELECT pg_catalog.count(*) INTO profile_count FROM decodex.role_profiles;

	IF profile_count NOT IN (0, 4)
		OR (profile_count = 4 AND EXISTS (
			SELECT expected.role
			FROM (VALUES
				('advisor'::decodex.role_profile_role), ('lead'::decodex.role_profile_role),
				('task'::decodex.role_profile_role), ('reviewer'::decodex.role_profile_role)
			) AS expected(role)
			EXCEPT SELECT role FROM decodex.role_profiles
		))
	THEN
		RAISE EXCEPTION 'RoleProfiles must contain exactly the four global roles'
			USING ERRCODE = '23514', CONSTRAINT = 'role_profiles_exact_global_set';
	END IF;

	RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER role_profiles_exact_global_set
AFTER INSERT OR UPDATE OR DELETE ON decodex.role_profiles
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_complete_role_profile_set();

CREATE FUNCTION decodex.forbid_role_profile_identity_rewrite()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'RoleProfile identities cannot be deleted'
			USING ERRCODE = '23514', CONSTRAINT = 'role_profiles_identity_immutable';
	END IF;

	IF NEW.role <> OLD.role OR NEW.created_at <> OLD.created_at
		OR NEW.current_revision <> OLD.current_revision + 1
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'RoleProfile identity or current revision rewrite is forbidden'
			USING ERRCODE = '23514', CONSTRAINT = 'role_profiles_identity_immutable';
	END IF;

	RETURN NEW;
END
$$;

CREATE TRIGGER role_profiles_identity_immutable
BEFORE UPDATE OR DELETE ON decodex.role_profiles
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_role_profile_identity_rewrite();

CREATE FUNCTION decodex.forbid_role_profile_revision_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'RoleProfile revisions are immutable'
		USING ERRCODE = '23514', CONSTRAINT = 'role_profile_revisions_immutable';
END
$$;

CREATE TRIGGER role_profile_revisions_immutable
BEFORE UPDATE OR DELETE ON decodex.role_profile_revisions
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_role_profile_revision_mutation();

CREATE FUNCTION decodex.forbid_role_profile_truncate()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'RoleProfile authority cannot be truncated'
		USING ERRCODE = '23514', CONSTRAINT = 'role_profiles_untruncatable';
END
$$;

CREATE TRIGGER role_profiles_untruncatable
BEFORE TRUNCATE ON decodex.role_profiles
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_role_profile_truncate();
CREATE TRIGGER role_profile_revisions_untruncatable
BEFORE TRUNCATE ON decodex.role_profile_revisions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_role_profile_truncate();

CREATE FUNCTION decodex.enforce_role_profile_event_namespace()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
DECLARE linked_role_profile boolean := false;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class
	WHERE class.oid = TG_RELID;

	IF TG_TABLE_NAME = 'activity' THEN
		linked_role_profile := NEW.aggregate_kind = 'role_profile'
			OR NEW.event_kind IN ('role_profile_bootstrapped', 'role_profile_updated')
			OR NEW.payload->>'aggregate_kind' = 'role_profile'
			OR NEW.payload->>'event_kind' IN ('role_profile_bootstrapped', 'role_profile_updated')
			OR NEW.payload->>'kind' = 'role_profile'
			OR NEW.payload OPERATOR(pg_catalog.?&) ARRAY[
				'role', 'model', 'reasoning_effort', 'service_tier', 'instructions', 'revision'
			];
		IF TG_OP = 'UPDATE' THEN
			linked_role_profile := linked_role_profile
				OR OLD.aggregate_kind = 'role_profile'
				OR OLD.event_kind IN ('role_profile_bootstrapped', 'role_profile_updated')
				OR OLD.payload->>'aggregate_kind' = 'role_profile'
				OR OLD.payload->>'event_kind' IN ('role_profile_bootstrapped', 'role_profile_updated')
				OR OLD.payload->>'kind' = 'role_profile'
				OR OLD.payload OPERATOR(pg_catalog.?&) ARRAY[
					'role', 'model', 'reasoning_effort', 'service_tier', 'instructions', 'revision'
				];
		END IF;
	ELSE
		linked_role_profile := NEW.aggregate_kind = 'role_profile'
			OR NEW.payload->>'aggregate_kind' = 'role_profile'
			OR NEW.payload->>'event_kind' IN ('role_profile_bootstrapped', 'role_profile_updated')
			OR NEW.payload->'payload'->>'kind' = 'role_profile'
			OR EXISTS (
				SELECT 1 FROM decodex.activity AS activity
				WHERE activity.sequence = (NEW.payload->>'activity_sequence')::bigint
					AND activity.aggregate_kind = 'role_profile'
			);
		IF TG_OP = 'UPDATE' THEN
			linked_role_profile := linked_role_profile
				OR OLD.aggregate_kind = 'role_profile'
				OR OLD.payload->>'aggregate_kind' = 'role_profile'
				OR OLD.payload->>'event_kind' IN ('role_profile_bootstrapped', 'role_profile_updated')
				OR OLD.payload->'payload'->>'kind' = 'role_profile';
		END IF;
	END IF;

	IF linked_role_profile AND current_user::name <> owner_name THEN
		IF TG_TABLE_NAME = 'activity' OR TG_OP = 'INSERT' THEN
			RAISE EXCEPTION 'RoleProfile activity/outbox namespace is command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'role_profile_event_namespace';
		ELSIF NEW.id IS DISTINCT FROM OLD.id
			OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload
			OR NEW.created_at IS DISTINCT FROM OLD.created_at
		THEN
			RAISE EXCEPTION 'RoleProfile outbox authority fields are command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'role_profile_event_namespace';
		END IF;
	END IF;

	RETURN NEW;
EXCEPTION
	WHEN invalid_text_representation THEN
		RAISE EXCEPTION 'RoleProfile activity/outbox link is malformed'
			USING ERRCODE = '42501', CONSTRAINT = 'role_profile_event_namespace';
END
$$;

CREATE TRIGGER activity_role_profile_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_role_profile_event_namespace();
CREATE TRIGGER outbox_role_profile_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_role_profile_event_namespace();

CREATE FUNCTION decodex.is_role_profile_configuration(
	p_model text, p_reasoning_effort text, p_service_tier text,
	p_instructions text, p_provenance text
) RETURNS boolean
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
SELECT p_model IS NOT NULL AND pg_catalog.octet_length(p_model) BETWEEN 1 AND 128
	AND p_reasoning_effort IS NOT NULL
	AND pg_catalog.octet_length(p_reasoning_effort) BETWEEN 1 AND 32
	AND p_reasoning_effort COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
	AND p_service_tier IS NOT NULL
	AND pg_catalog.octet_length(p_service_tier) BETWEEN 1 AND 32
	AND p_service_tier COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
	AND p_instructions IS NOT NULL
	AND pg_catalog.octet_length(p_instructions) BETWEEN 1 AND 65536
	AND (p_provenance IS NULL OR pg_catalog.octet_length(p_provenance) BETWEEN 1 AND 4096)
	AND p_model COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	AND p_instructions COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	AND (p_provenance IS NULL OR p_provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]')
	AND NOT decodex.has_credential_material(p_model)
	AND NOT decodex.has_credential_material(p_reasoning_effort)
	AND NOT decodex.has_credential_material(p_service_tier)
	AND NOT decodex.has_credential_material(p_instructions)
	AND (p_provenance IS NULL OR NOT decodex.has_credential_material(p_provenance))
$$;

CREATE FUNCTION decodex.build_role_profile_bootstrap_request(
	p_protocol text,
	p_advisor_model text, p_advisor_reasoning_effort text,
	p_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text,
	p_lead_model text, p_lead_reasoning_effort text,
	p_lead_service_tier text, p_lead_instructions text, p_lead_provenance text,
	p_task_model text, p_task_reasoning_effort text,
	p_task_service_tier text, p_task_instructions text, p_task_provenance text,
	p_reviewer_model text, p_reviewer_reasoning_effort text,
	p_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text
) RETURNS jsonb
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF p_protocol IS NULL OR p_advisor_model IS NULL OR p_advisor_reasoning_effort IS NULL
		OR p_advisor_service_tier IS NULL OR p_advisor_instructions IS NULL
		OR p_lead_model IS NULL OR p_lead_reasoning_effort IS NULL
		OR p_lead_service_tier IS NULL OR p_lead_instructions IS NULL
		OR p_task_model IS NULL OR p_task_reasoning_effort IS NULL
		OR p_task_service_tier IS NULL OR p_task_instructions IS NULL
		OR p_reviewer_model IS NULL OR p_reviewer_reasoning_effort IS NULL
		OR p_reviewer_service_tier IS NULL OR p_reviewer_instructions IS NULL
	THEN
		RAISE EXCEPTION 'RoleProfile bootstrap request is incomplete' USING ERRCODE = '22004';
	END IF;

	RETURN pg_catalog.jsonb_build_object(
		'protocol_version', p_protocol, 'operation', 'bootstrap_role_profiles',
		'profiles', pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object('role', 'advisor', 'model', p_advisor_model,
				'reasoning_effort', p_advisor_reasoning_effort, 'service_tier', p_advisor_service_tier,
				'instructions', p_advisor_instructions, 'provenance', p_advisor_provenance),
			pg_catalog.jsonb_build_object('role', 'lead', 'model', p_lead_model,
				'reasoning_effort', p_lead_reasoning_effort, 'service_tier', p_lead_service_tier,
				'instructions', p_lead_instructions, 'provenance', p_lead_provenance),
			pg_catalog.jsonb_build_object('role', 'task', 'model', p_task_model,
				'reasoning_effort', p_task_reasoning_effort, 'service_tier', p_task_service_tier,
				'instructions', p_task_instructions, 'provenance', p_task_provenance),
			pg_catalog.jsonb_build_object('role', 'reviewer', 'model', p_reviewer_model,
				'reasoning_effort', p_reviewer_reasoning_effort, 'service_tier', p_reviewer_service_tier,
				'instructions', p_reviewer_instructions, 'provenance', p_reviewer_provenance)
		)
	);
END
$$;

CREATE FUNCTION decodex.build_role_profile_update_request(
	p_protocol text, p_role decodex.role_profile_role, p_expected_revision bigint,
	p_model text, p_reasoning_effort text, p_service_tier text,
	p_instructions text, p_provenance text
) RETURNS jsonb
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
SELECT pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'update_role_profile',
	'role', p_role, 'expected_revision', p_expected_revision,
	'model', p_model, 'reasoning_effort', p_reasoning_effort,
	'service_tier', p_service_tier, 'instructions', p_instructions,
	'provenance', p_provenance
)
$$;

CREATE FUNCTION decodex.complete_exact_role_profile_rejection(
	p_protocol text, p_idempotency_key text, p_code text
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	effect_value := pg_catalog.jsonb_build_object('changed', false, 'code', p_code);
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

CREATE FUNCTION decodex.bootstrap_role_profiles_exact(
	p_protocol text, p_idempotency_key text,
	p_advisor_model text, p_advisor_reasoning_effort text,
	p_advisor_service_tier text, p_advisor_instructions text, p_advisor_provenance text,
	p_lead_model text, p_lead_reasoning_effort text,
	p_lead_service_tier text, p_lead_instructions text, p_lead_provenance text,
	p_task_model text, p_task_reasoning_effort text,
	p_task_service_tier text, p_task_instructions text, p_task_provenance text,
	p_reviewer_model text, p_reviewer_reasoning_effort text,
	p_reviewer_service_tier text, p_reviewer_instructions text, p_reviewer_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
DECLARE inserted_count integer;
DECLARE profiles_value jsonb;
DECLARE profile_value jsonb;
DECLARE activity_sequence bigint;
DECLARE activity_payload jsonb;
DECLARE outbox_id bigint;
DECLARE outbox_payload jsonb;
DECLARE effects_value jsonb := '[]'::jsonb;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL THEN
		RAISE EXCEPTION 'exact RoleProfile command identity is incomplete' USING ERRCODE = '22004';
	END IF;
	IF pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN
		RAISE EXCEPTION 'exact RoleProfile command identity is invalid' USING ERRCODE = '22023';
	END IF;

	request_value := decodex.build_role_profile_bootstrap_request(
		p_protocol,
		p_advisor_model, p_advisor_reasoning_effort, p_advisor_service_tier,
		p_advisor_instructions, p_advisor_provenance,
		p_lead_model, p_lead_reasoning_effort, p_lead_service_tier,
		p_lead_instructions, p_lead_provenance,
		p_task_model, p_task_reasoning_effort, p_task_service_tier,
		p_task_instructions, p_task_provenance,
		p_reviewer_model, p_reviewer_reasoning_effort, p_reviewer_service_tier,
		p_reviewer_instructions, p_reviewer_provenance
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

	IF NOT decodex.is_role_profile_configuration(p_advisor_model, p_advisor_reasoning_effort,
		p_advisor_service_tier, p_advisor_instructions, p_advisor_provenance)
		OR NOT decodex.is_role_profile_configuration(p_lead_model, p_lead_reasoning_effort,
			p_lead_service_tier, p_lead_instructions, p_lead_provenance)
		OR NOT decodex.is_role_profile_configuration(p_task_model, p_task_reasoning_effort,
			p_task_service_tier, p_task_instructions, p_task_provenance)
		OR NOT decodex.is_role_profile_configuration(p_reviewer_model, p_reviewer_reasoning_effort,
			p_reviewer_service_tier, p_reviewer_instructions, p_reviewer_provenance)
	THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'invalid_profile'
		);
	END IF;

	LOCK TABLE decodex.role_profiles IN SHARE ROW EXCLUSIVE MODE;
	IF EXISTS (SELECT 1 FROM decodex.role_profiles) THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'already_bootstrapped'
		);
	END IF;

	WITH inputs(role, model, reasoning_effort, service_tier, instructions, provenance) AS (
		VALUES
			('advisor'::decodex.role_profile_role, p_advisor_model,
				p_advisor_reasoning_effort, p_advisor_service_tier,
				p_advisor_instructions, p_advisor_provenance),
			('lead'::decodex.role_profile_role, p_lead_model,
				p_lead_reasoning_effort, p_lead_service_tier,
				p_lead_instructions, p_lead_provenance),
			('task'::decodex.role_profile_role, p_task_model,
				p_task_reasoning_effort, p_task_service_tier,
				p_task_instructions, p_task_provenance),
			('reviewer'::decodex.role_profile_role, p_reviewer_model,
				p_reviewer_reasoning_effort, p_reviewer_service_tier,
				p_reviewer_instructions, p_reviewer_provenance)
	), inserted_profiles AS (
		INSERT INTO decodex.role_profiles(role, current_revision)
		SELECT role, 1 FROM inputs ORDER BY role
		RETURNING role, current_revision, created_at, updated_at
	), inserted_revisions AS (
		INSERT INTO decodex.role_profile_revisions(
			role, revision, model, reasoning_effort, service_tier, instructions, provenance
		)
		SELECT input.role, profile.current_revision, input.model, input.reasoning_effort,
			input.service_tier, input.instructions, input.provenance
		FROM inputs AS input JOIN inserted_profiles AS profile USING (role)
		RETURNING role, revision, model, reasoning_effort, service_tier,
			instructions, provenance, created_at
	)
	SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
		'role', revision.role, 'revision', revision.revision,
		'model', revision.model, 'reasoning_effort', revision.reasoning_effort,
		'service_tier', revision.service_tier, 'instructions', revision.instructions,
		'provenance', revision.provenance, 'created_at', revision.created_at,
		'updated_at', profile.updated_at
	) ORDER BY revision.role)
	INTO profiles_value
	FROM inserted_revisions AS revision JOIN inserted_profiles AS profile USING (role);

	FOR profile_value IN SELECT value FROM pg_catalog.jsonb_array_elements(profiles_value)
	LOOP
		activity_payload := profile_value || pg_catalog.jsonb_build_object('kind', 'role_profile');
		INSERT INTO decodex.activity(
			aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload
		) VALUES (
			'role_profile', profile_value->>'role', (profile_value->>'revision')::bigint,
			'role_profile_bootstrapped', p_idempotency_key, activity_payload
		) RETURNING sequence, payload INTO activity_sequence, activity_payload;

		outbox_payload := pg_catalog.jsonb_build_object(
			'activity_sequence', activity_sequence,
			'event_kind', 'role_profile_bootstrapped', 'aggregate_kind', 'role_profile',
			'aggregate_id', profile_value->>'role',
			'revision', (profile_value->>'revision')::bigint, 'payload', activity_payload
		);
		INSERT INTO decodex.outbox(
			effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
		) VALUES (
			'activity/' || activity_sequence::text, 'role_profile', profile_value->>'role',
			(profile_value->>'revision')::bigint, outbox_payload
		) RETURNING id, payload INTO outbox_id, outbox_payload;

		effects_value := effects_value || pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object(
				'profile', profile_value, 'activity_sequence', activity_sequence,
				'activity_payload', activity_payload, 'outbox_id', outbox_id,
				'outbox_payload', outbox_payload
			)
		);
	END LOOP;

	effect_value := pg_catalog.jsonb_build_object('profiles', effects_value);
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

CREATE FUNCTION decodex.update_role_profile_exact(
	p_protocol text, p_idempotency_key text,
	p_role decodex.role_profile_role, p_expected_revision bigint,
	p_model text, p_reasoning_effort text, p_service_tier text,
	p_instructions text, p_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
DECLARE inserted_count integer;
DECLARE actual_revision bigint;
DECLARE profile_value jsonb;
DECLARE activity_sequence bigint;
DECLARE activity_payload jsonb;
DECLARE outbox_id bigint;
DECLARE outbox_payload jsonb;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_role IS NULL
		OR p_expected_revision IS NULL OR p_model IS NULL OR p_reasoning_effort IS NULL
		OR p_service_tier IS NULL OR p_instructions IS NULL
	THEN
		RAISE EXCEPTION 'exact RoleProfile update is incomplete' USING ERRCODE = '22004';
	END IF;
	IF pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN
		RAISE EXCEPTION 'exact RoleProfile command identity is invalid' USING ERRCODE = '22023';
	END IF;

	request_value := decodex.build_role_profile_update_request(
		p_protocol, p_role, p_expected_revision, p_model, p_reasoning_effort,
		p_service_tier, p_instructions, p_provenance
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

	IF p_expected_revision < 1 THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'invalid_expected_revision'
		);
	END IF;
	IF NOT decodex.is_role_profile_configuration(
		p_model, p_reasoning_effort, p_service_tier, p_instructions, p_provenance
	) THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'invalid_profile'
		);
	END IF;

	SELECT current_revision INTO actual_revision
	FROM decodex.role_profiles WHERE role = p_role FOR UPDATE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'not_bootstrapped'
		);
	END IF;
	IF actual_revision <> p_expected_revision THEN
		RETURN decodex.complete_exact_role_profile_rejection(
			p_protocol, p_idempotency_key, 'stale_revision'
		);
	END IF;

	WITH inserted_revision AS (
		INSERT INTO decodex.role_profile_revisions(
			role, revision, model, reasoning_effort, service_tier, instructions, provenance
		) VALUES (
			p_role, actual_revision + 1, p_model, p_reasoning_effort,
			p_service_tier, p_instructions, p_provenance
		) RETURNING role, revision, model, reasoning_effort, service_tier,
			instructions, provenance, created_at
	), advanced_profile AS (
		UPDATE decodex.role_profiles
		SET current_revision = actual_revision + 1, updated_at = pg_catalog.clock_timestamp()
		WHERE role = p_role AND current_revision = actual_revision
		RETURNING role, current_revision, created_at, updated_at
	)
	SELECT pg_catalog.jsonb_build_object(
		'role', revision.role, 'revision', revision.revision,
		'model', revision.model, 'reasoning_effort', revision.reasoning_effort,
		'service_tier', revision.service_tier, 'instructions', revision.instructions,
		'provenance', revision.provenance, 'created_at', revision.created_at,
		'updated_at', profile.updated_at
	) INTO profile_value
	FROM inserted_revision AS revision JOIN advanced_profile AS profile USING (role);

	IF profile_value IS NULL THEN
		RAISE EXCEPTION 'RoleProfile compare-and-swap lost after row lock' USING ERRCODE = '40001';
	END IF;

	activity_payload := profile_value || pg_catalog.jsonb_build_object('kind', 'role_profile');
	INSERT INTO decodex.activity(
		aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload
	) VALUES (
		'role_profile', p_role::text, (profile_value->>'revision')::bigint,
		'role_profile_updated', p_idempotency_key, activity_payload
	) RETURNING sequence, payload INTO activity_sequence, activity_payload;
	outbox_payload := pg_catalog.jsonb_build_object(
		'activity_sequence', activity_sequence, 'event_kind', 'role_profile_updated',
		'aggregate_kind', 'role_profile', 'aggregate_id', p_role,
		'revision', (profile_value->>'revision')::bigint, 'payload', activity_payload
	);
	INSERT INTO decodex.outbox(
		effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	) VALUES (
		'activity/' || activity_sequence::text, 'role_profile', p_role::text,
		(profile_value->>'revision')::bigint, outbox_payload
	) RETURNING id, payload INTO outbox_id, outbox_payload;

	effect_value := pg_catalog.jsonb_build_object(
		'profile', profile_value, 'activity_sequence', activity_sequence,
		'activity_payload', activity_payload, 'outbox_id', outbox_id,
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

REVOKE ALL ON TABLE decodex.exact_command_receipts,
	decodex.role_profiles, decodex.role_profile_revisions FROM PUBLIC;
REVOKE ALL ON TYPE decodex.exact_receipt_state, decodex.role_profile_role FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
