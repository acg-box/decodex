-- XY-1338 inert ManagedRuns and fail-closed effect barriers.
-- V12 creates no scheduler, acquisition, progress, completion, or dispatch authority.
CREATE TYPE decodex.managed_run_lifecycle AS ENUM ('queued', 'active', 'waiting', 'terminal');
CREATE TYPE decodex.managed_run_phase AS ENUM (
	'prepare', 'execute', 'validate', 'review', 'repair', 'land', 'close'
);
CREATE TYPE decodex.managed_run_wait_reason AS ENUM (
	'usage', 'auth', 'plugin', 'dependency', 'approval', 'user', 'external',
	'reviewer_unavailable', 'reviewer_failed'
);
CREATE TYPE decodex.execution_assignment_role AS ENUM ('task', 'reviewer');
CREATE TYPE decodex.effect_barrier_state AS ENUM ('guarded', 'closed');
CREATE TYPE decodex.managed_run_effect_kind AS ENUM ('tool', 'repository', 'git', 'artifact');
CREATE TYPE decodex.managed_run_effect_state AS ENUM ('recorded');
CREATE TYPE decodex.managed_run_safety_input_kind AS ENUM (
	'positively_observed_unknown_turn', 'submitted_turn_receipt', 'inconclusive_observation'
);

ALTER TABLE decodex.runtime_sessions
	ADD CONSTRAINT runtime_sessions_identity_revision_unique
	UNIQUE (runtime_session_id, revision);

CREATE TABLE decodex.managed_runs (
	managed_run_id uuid PRIMARY KEY,
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	runtime_session_id uuid NOT NULL,
	runtime_session_revision bigint NOT NULL CHECK (runtime_session_revision > 0),
	lifecycle decodex.managed_run_lifecycle NOT NULL DEFAULT 'waiting',
	phase decodex.managed_run_phase NOT NULL,
	wait_reason decodex.managed_run_wait_reason NOT NULL,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	diverged boolean NOT NULL DEFAULT false,
	blocked boolean NOT NULL DEFAULT true,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT managed_runs_identity_project_unique UNIQUE (managed_run_id, project_id),
	CONSTRAINT managed_runs_identity_project_work_item_unique
		UNIQUE (managed_run_id, project_id, work_item_id),
	CONSTRAINT managed_runs_work_item_unique UNIQUE (work_item_id),
	CONSTRAINT managed_runs_work_item_project_fk FOREIGN KEY (work_item_id, project_id)
		REFERENCES decodex.work_items(work_item_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT managed_runs_runtime_session_revision_fk
		FOREIGN KEY (runtime_session_id, runtime_session_revision)
		REFERENCES decodex.runtime_sessions(runtime_session_id, revision) ON DELETE RESTRICT
		DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT managed_runs_identity_canonical CHECK (
		managed_run_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT managed_runs_inert_waiting_only CHECK (
		lifecycle = 'waiting' AND wait_reason IS NOT NULL AND blocked
	),
	CONSTRAINT managed_runs_finite_times CHECK (
		pg_catalog.isfinite(created_at) AND pg_catalog.isfinite(updated_at)
		AND created_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND updated_at BETWEEN created_at
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.managed_run_assignments (
	managed_run_id uuid NOT NULL,
	project_id uuid NOT NULL,
	runtime_session_id uuid NOT NULL,
	role decodex.execution_assignment_role NOT NULL,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT managed_run_assignments_pkey PRIMARY KEY (managed_run_id, role),
	CONSTRAINT managed_run_assignments_runtime_session_unique UNIQUE (runtime_session_id),
	CONSTRAINT managed_run_assignments_run_project_fk FOREIGN KEY (managed_run_id, project_id)
		REFERENCES decodex.managed_runs(managed_run_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_assignments_session_fk FOREIGN KEY (runtime_session_id)
		REFERENCES decodex.runtime_sessions(runtime_session_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_assignments_finite_time CHECK (
		pg_catalog.isfinite(created_at)
		AND created_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.managed_run_effect_barriers (
	managed_run_id uuid PRIMARY KEY,
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	state decodex.effect_barrier_state NOT NULL DEFAULT 'guarded',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	closure_input_id uuid,
	closure_input_kind decodex.managed_run_safety_input_kind,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	closed_at timestamptz,
	CONSTRAINT managed_run_effect_barriers_run_scope_fk
		FOREIGN KEY (managed_run_id, project_id, work_item_id)
		REFERENCES decodex.managed_runs(managed_run_id, project_id, work_item_id)
		ON DELETE RESTRICT,
	CONSTRAINT managed_run_effect_barriers_shape CHECK (
		(state = 'guarded' AND revision = 1 AND closure_input_id IS NULL
			AND closure_input_kind IS NULL AND closed_at IS NULL)
		OR (state = 'closed' AND revision = 2 AND closure_input_id IS NOT NULL
			AND closure_input_kind IS NOT NULL AND closed_at IS NOT NULL)
	),
	CONSTRAINT managed_run_effect_barriers_finite_times CHECK (
		pg_catalog.isfinite(created_at) AND (closed_at IS NULL OR pg_catalog.isfinite(closed_at))
		AND created_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND (closed_at IS NULL OR closed_at BETWEEN created_at
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00')
	)
);

CREATE TABLE decodex.managed_run_effects (
	effect_id uuid PRIMARY KEY,
	managed_run_id uuid NOT NULL,
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	ordinal integer NOT NULL CHECK (ordinal BETWEEN 1 AND 16384),
	kind decodex.managed_run_effect_kind NOT NULL,
	effect_key text NOT NULL,
	state decodex.managed_run_effect_state NOT NULL DEFAULT 'recorded',
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT managed_run_effects_run_ordinal_unique UNIQUE (managed_run_id, ordinal),
	CONSTRAINT managed_run_effects_run_scope_fk
		FOREIGN KEY (managed_run_id, project_id, work_item_id)
		REFERENCES decodex.managed_runs(managed_run_id, project_id, work_item_id)
		ON DELETE RESTRICT,
	CONSTRAINT managed_run_effects_barrier_fk FOREIGN KEY (managed_run_id)
		REFERENCES decodex.managed_run_effect_barriers(managed_run_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_effects_identity_canonical CHECK (
		effect_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT managed_run_effects_key_bounded CHECK (
		pg_catalog.octet_length(effect_key) >= 1
		AND pg_catalog.octet_length(effect_key) <= 256
		AND effect_key COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND NOT decodex.has_credential_material(effect_key)
	),
	CONSTRAINT managed_run_effects_finite_time CHECK (
		pg_catalog.isfinite(created_at)
		AND created_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.managed_run_submitted_turn_receipts (
	receipt_id uuid PRIMARY KEY,
	managed_run_id uuid NOT NULL,
	project_id uuid NOT NULL,
	runtime_session_id uuid NOT NULL,
	runtime_session_revision bigint NOT NULL CHECK (runtime_session_revision > 0),
	turn_id uuid NOT NULL,
	submitted_at timestamptz NOT NULL,
	CONSTRAINT managed_run_submitted_turn_receipts_exact_turn_unique
		UNIQUE (managed_run_id, runtime_session_id, turn_id),
	CONSTRAINT managed_run_submitted_turn_receipts_run_project_fk
		FOREIGN KEY (managed_run_id, project_id)
		REFERENCES decodex.managed_runs(managed_run_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_submitted_turn_receipts_session_fk FOREIGN KEY (runtime_session_id)
		REFERENCES decodex.runtime_sessions(runtime_session_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_submitted_turn_receipts_ids_canonical CHECK (
		receipt_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT managed_run_submitted_turn_receipts_finite_time CHECK (
		pg_catalog.isfinite(submitted_at)
		AND submitted_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.managed_run_safety_inputs (
	input_id uuid PRIMARY KEY,
	managed_run_id uuid NOT NULL,
	project_id uuid NOT NULL,
	runtime_session_id uuid NOT NULL,
	kind decodex.managed_run_safety_input_kind NOT NULL,
	turn_id uuid,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 1),
	runtime_session_revision bigint NOT NULL CHECK (runtime_session_revision > 0),
	barrier_revision bigint NOT NULL CHECK (barrier_revision > 0),
	stale_receipt boolean NOT NULL,
	request_envelope jsonb NOT NULL,
	effect_envelope jsonb NOT NULL,
	response_bytes bytea NOT NULL,
	applied_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT managed_run_safety_inputs_run_project_fk
		FOREIGN KEY (managed_run_id, project_id)
		REFERENCES decodex.managed_runs(managed_run_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_safety_inputs_session_fk FOREIGN KEY (runtime_session_id)
		REFERENCES decodex.runtime_sessions(runtime_session_id) ON DELETE RESTRICT,
	CONSTRAINT managed_run_safety_inputs_shape CHECK (
		(kind IN ('positively_observed_unknown_turn', 'submitted_turn_receipt') AND turn_id IS NOT NULL)
		OR (kind = 'inconclusive_observation' AND turn_id IS NULL)
	),
	CONSTRAINT managed_run_safety_inputs_ids_canonical CHECK (
		input_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT managed_run_safety_inputs_response_bounded CHECK (
		pg_catalog.octet_length(response_bytes) BETWEEN 1 AND 1048576
	),
	CONSTRAINT managed_run_safety_inputs_finite_time CHECK (
		pg_catalog.isfinite(applied_at)
		AND applied_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE FUNCTION decodex.enforce_managed_run_command_owner()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;
	IF current_user::name <> owner_name THEN
		RAISE EXCEPTION 'ManagedRun state is command-owned'
			USING ERRCODE = '42501', CONSTRAINT = 'managed_run_command_owner';
	END IF;
	RETURN NULL;
END
$$;

CREATE TRIGGER managed_runs_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_runs
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();
CREATE TRIGGER managed_run_assignments_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_run_assignments
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();
CREATE TRIGGER managed_run_effect_barriers_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_run_effect_barriers
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();
CREATE TRIGGER managed_run_effects_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_run_effects
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();
CREATE TRIGGER managed_run_submitted_turn_receipts_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_run_submitted_turn_receipts
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();
CREATE TRIGGER managed_run_safety_inputs_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.managed_run_safety_inputs
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_managed_run_command_owner();

CREATE FUNCTION decodex.forbid_managed_run_immutable_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'ManagedRun immutable evidence cannot be changed'
		USING ERRCODE = '23514', CONSTRAINT = 'managed_run_immutable_evidence';
END
$$;
CREATE TRIGGER managed_run_assignments_immutable
BEFORE UPDATE OR DELETE ON decodex.managed_run_assignments
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_managed_run_immutable_mutation();
CREATE TRIGGER managed_run_effects_immutable
BEFORE UPDATE OR DELETE ON decodex.managed_run_effects
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_managed_run_immutable_mutation();
CREATE TRIGGER managed_run_submitted_turn_receipts_immutable
BEFORE UPDATE OR DELETE ON decodex.managed_run_submitted_turn_receipts
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_managed_run_immutable_mutation();
CREATE TRIGGER managed_run_safety_inputs_immutable
BEFORE UPDATE OR DELETE ON decodex.managed_run_safety_inputs
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_managed_run_immutable_mutation();

CREATE FUNCTION decodex.enforce_managed_run_assignment_scope()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE run_session uuid;
DECLARE snapshot_role decodex.role_profile_role;
BEGIN
	SELECT run.runtime_session_id INTO run_session FROM decodex.managed_runs AS run
	WHERE run.managed_run_id = NEW.managed_run_id AND run.project_id = NEW.project_id;
	SELECT profile.role INTO snapshot_role
	FROM decodex.runtime_sessions AS session
	JOIN decodex.profile_snapshots AS profile USING (profile_snapshot_id)
	WHERE session.runtime_session_id = NEW.runtime_session_id;
	IF snapshot_role::text <> NEW.role::text
		OR (NEW.role = 'task' AND NEW.runtime_session_id <> run_session)
	THEN
		RAISE EXCEPTION 'execution assignment is not exact-run Task/Reviewer identity'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_run_assignment_scope';
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER managed_run_assignment_scope
AFTER INSERT ON decodex.managed_run_assignments
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_managed_run_assignment_scope();

CREATE FUNCTION decodex.enforce_managed_run_state()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.lifecycle <> 'waiting' OR NOT NEW.blocked OR NEW.revision <> 1
			OR NEW.updated_at <> NEW.created_at
		THEN
			RAISE EXCEPTION 'new ManagedRun must be inert waiting revision one'
				USING ERRCODE = '23514', CONSTRAINT = 'managed_runs_inert_state';
		END IF;
		RETURN NEW;
	END IF;
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'ManagedRuns cannot be deleted'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_runs_inert_state';
	END IF;
	IF NEW.managed_run_id <> OLD.managed_run_id OR NEW.project_id <> OLD.project_id
		OR NEW.work_item_id <> OLD.work_item_id
		OR NEW.runtime_session_id <> OLD.runtime_session_id
		OR NEW.lifecycle <> 'waiting' OR NOT NEW.blocked
		OR NEW.phase <> OLD.phase OR NEW.created_at <> OLD.created_at
		OR NEW.revision <> OLD.revision + 1 OR NEW.updated_at < OLD.updated_at
		OR (OLD.diverged AND NOT NEW.diverged)
	THEN
		RAISE EXCEPTION 'ManagedRun update must remain monotonic and inert'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_runs_inert_state';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER managed_runs_inert_state
BEFORE INSERT OR UPDATE OR DELETE ON decodex.managed_runs
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_managed_run_state();

CREATE FUNCTION decodex.enforce_effect_barrier_state()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.state <> 'guarded' OR NEW.revision <> 1 OR NEW.closed_at IS NOT NULL THEN
			RAISE EXCEPTION 'new effect barrier must be guarded and fail closed'
				USING ERRCODE = '23514', CONSTRAINT = 'managed_run_effect_barrier_state';
		END IF;
		RETURN NEW;
	END IF;
	IF TG_OP = 'DELETE' OR OLD.state = 'closed'
		OR NEW.managed_run_id <> OLD.managed_run_id
		OR NEW.project_id <> OLD.project_id OR NEW.work_item_id <> OLD.work_item_id
		OR NEW.created_at <> OLD.created_at OR NEW.state <> 'closed'
		OR NEW.revision <> 2 OR NEW.closure_input_id IS NULL
		OR NEW.closure_input_kind IS NULL OR NEW.closed_at IS NULL
	THEN
		RAISE EXCEPTION 'effect barrier may only close exactly once'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_run_effect_barrier_state';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER managed_run_effect_barriers_state
BEFORE INSERT OR UPDATE OR DELETE ON decodex.managed_run_effect_barriers
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_effect_barrier_state();

-- Divergence is the fail-closed exception to the V3 active-turn terminal guard. An active
-- turn remains active and therefore cannot be mistaken for completion or replay authority.
CREATE OR REPLACE FUNCTION decodex.enforce_runtime_session_state()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE parent_status decodex.conversation_status;
DECLARE transition_time timestamptz;
BEGIN
	SELECT status INTO parent_status FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	IF parent_status <> 'open' THEN RAISE EXCEPTION 'runtime session requires an open conversation'; END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.state NOT IN ('starting', 'active') OR NEW.revision <> 1 THEN
			RAISE EXCEPTION 'illegal initial runtime session state';
		END IF;
		NEW.created_at := pg_catalog.clock_timestamp(); NEW.updated_at := NEW.created_at;
		NEW.ended_at := NULL; RETURN NEW;
	END IF;
	IF OLD.state IN ('ended', 'diverged') THEN RAISE EXCEPTION 'terminal runtime session is immutable'; END IF;
	IF NEW.runtime_session_id <> OLD.runtime_session_id
		OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.profile_snapshot_id <> OLD.profile_snapshot_id
		OR NEW.account_snapshot_id <> OLD.account_snapshot_id
		OR NEW.codex_thread_id IS DISTINCT FROM OLD.codex_thread_id
		OR NEW.last_known_turn_id IS DISTINCT FROM OLD.last_known_turn_id
		OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision <> OLD.revision + 1
		OR NOT ((OLD.state = 'starting' AND NEW.state IN ('active', 'ended', 'diverged'))
			OR (OLD.state = 'active' AND NEW.state IN ('ended', 'diverged'))) THEN
		RAISE EXCEPTION 'illegal runtime session transition';
	END IF;
	IF NEW.state = 'ended' AND EXISTS (SELECT 1 FROM decodex.turns
		WHERE runtime_session_id = NEW.runtime_session_id AND status = 'active')
	THEN RAISE EXCEPTION 'runtime session has active turns'; END IF;
	transition_time := pg_catalog.clock_timestamp(); NEW.updated_at := transition_time;
	NEW.ended_at := CASE WHEN NEW.state IN ('ended', 'diverged') THEN transition_time ELSE NULL END;
	RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION decodex.create_runtime_session_exact(
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
DECLARE created_profile_snapshot_id uuid;
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

	created_profile_snapshot_id := pg_catalog.gen_random_uuid();
	INSERT INTO decodex.profile_snapshots(
		profile_snapshot_id, source_profile_id, role, model, reasoning_effort,
		service_tier, instructions_digest, instructions, provenance, source_revision
	) VALUES (
		created_profile_snapshot_id, profile_value->>'role', p_role,
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
		p_session_id, p_conversation_id, created_profile_snapshot_id,
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
		'kind', 'runtime_session', 'runtime_session_snapshot', session_value,
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
		'runtime_session_snapshot', session_value, 'profile_snapshot', profile_value,
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

CREATE OR REPLACE FUNCTION decodex.transition_runtime_session_exact(
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
		'kind', 'runtime_session', 'runtime_session_snapshot', session_value,
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
		'runtime_session_snapshot', session_value, 'profile_snapshot', profile_value,
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

-- V3 locked RuntimeSessions from invoker-rights Turn and HistoryItem triggers. V10 made
-- RuntimeSessions SELECT-only for runtime, so V12 rolls those triggers forward without an
-- authority-expanding UPDATE grant. The statement-level hierarchy coordinator remains the
-- serialization owner; unsupported transaction isolation fails closed before hierarchy reads.
CREATE OR REPLACE FUNCTION decodex.enforce_turn_state() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE parent_state decodex.runtime_session_state;
DECLARE parent_status decodex.conversation_status;
DECLARE transition_time timestamptz;
BEGIN
	IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
		RAISE EXCEPTION 'turn hierarchy writes require READ COMMITTED'
			USING ERRCODE = '40001';
	END IF;
	SELECT c.status, rs.state INTO parent_status, parent_state
		FROM decodex.conversations c JOIN decodex.runtime_sessions rs
		ON rs.conversation_id = c.conversation_id
		WHERE c.conversation_id = NEW.conversation_id
			AND rs.runtime_session_id = NEW.runtime_session_id FOR UPDATE OF c;
	IF TG_OP = 'INSERT' THEN
		IF parent_status <> 'open' OR parent_state <> 'active'
			OR NEW.status <> 'active' OR NEW.revision <> 1 THEN
			RAISE EXCEPTION 'turn requires an active parent';
		END IF;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		NEW.completed_at := NULL;
		RETURN NEW;
	END IF;
	IF OLD.status IN ('completed', 'failed') THEN RAISE EXCEPTION 'terminal turn is immutable'; END IF;
	IF NEW.turn_id <> OLD.turn_id OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.runtime_session_id <> OLD.runtime_session_id OR NEW.sequence <> OLD.sequence
		OR NEW.role <> OLD.role OR NEW.possible_side_effects <> OLD.possible_side_effects
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1 OR NEW.status NOT IN ('completed', 'failed') THEN
		RAISE EXCEPTION 'illegal turn transition';
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.history_items WHERE turn_id = NEW.turn_id
		AND status = 'streaming') THEN RAISE EXCEPTION 'turn has streaming items'; END IF;
	transition_time := pg_catalog.clock_timestamp();
	NEW.updated_at := transition_time;
	NEW.completed_at := transition_time;
	RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION decodex.enforce_history_item_state() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE turn_state decodex.turn_status;
DECLARE session_state decodex.runtime_session_state;
DECLARE conversation_state decodex.conversation_status;
DECLARE artifact_state decodex.artifact_status;
DECLARE next_position bigint;
BEGIN
	IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
		RAISE EXCEPTION 'history hierarchy writes require READ COMMITTED'
			USING ERRCODE = '40001';
	END IF;
	SELECT t.status, rs.state, c.status INTO turn_state, session_state, conversation_state
		FROM decodex.turns t JOIN decodex.runtime_sessions rs
		ON (rs.runtime_session_id, rs.conversation_id) = (t.runtime_session_id, t.conversation_id)
		JOIN decodex.conversations c ON c.conversation_id = t.conversation_id
		WHERE (t.turn_id, t.conversation_id) = (NEW.turn_id, NEW.conversation_id)
			FOR UPDATE OF c, t;
	IF turn_state <> 'active' OR session_state <> 'active' OR conversation_state <> 'open' THEN
		RAISE EXCEPTION 'history write requires active parents';
	END IF;
	IF NEW.kind = 'artifact' THEN
		SELECT status INTO artifact_state FROM decodex.artifacts
			WHERE (artifact_id, conversation_id) = (NEW.artifact_id, NEW.conversation_id)
			FOR UPDATE;
		IF artifact_state <> 'active' THEN
			RAISE EXCEPTION 'history Artifact reference requires active Artifact';
		END IF;
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.revision <> 1 THEN RAISE EXCEPTION 'history item must start at revision 1'; END IF;
		SELECT COALESCE(max(history_position), 0) + 1 INTO next_position
			FROM decodex.history_items WHERE conversation_id = NEW.conversation_id;
		NEW.history_position := next_position;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		RETURN NEW;
	END IF;
	IF OLD.status IN ('completed', 'failed') THEN RAISE EXCEPTION 'terminal history item is immutable'; END IF;
	IF NEW.history_item_id <> OLD.history_item_id OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.history_position <> OLD.history_position OR NEW.turn_id <> OLD.turn_id
		OR NEW.ordinal <> OLD.ordinal OR NEW.kind <> OLD.kind
		OR NEW.artifact_id IS DISTINCT FROM OLD.artifact_id
		OR NEW.artifact_revision IS DISTINCT FROM OLD.artifact_revision
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1 THEN
		RAISE EXCEPTION 'illegal history item transition';
	END IF;
	NEW.updated_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_managed_run_event_namespace()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
DECLARE linked boolean := false;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;
	IF TG_TABLE_NAME = 'activity' THEN
		linked := NEW.aggregate_kind = 'managed_run'
			OR NEW.event_kind LIKE 'managed_run_%'
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
				@.event_kind like_regex "^managed_run_" ||
				exists(@.managed_run) || exists(@.managed_run_id) ||
				exists(@.effect_barrier) || exists(@.effect_barrier_id)
			)');
		IF TG_OP = 'UPDATE' THEN
			linked := linked OR OLD.aggregate_kind = 'managed_run'
				OR OLD.event_kind LIKE 'managed_run_%'
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
					@.event_kind like_regex "^managed_run_" ||
					exists(@.managed_run) || exists(@.managed_run_id) ||
					exists(@.effect_barrier) || exists(@.effect_barrier_id)
				)');
		END IF;
	ELSE
		IF EXISTS (
			SELECT 1 FROM pg_catalog.jsonb_path_query(
				NEW.payload, '$.**.activity_sequence'
			) AS link(value)
			WHERE pg_catalog.jsonb_typeof(link.value) NOT IN ('number', 'string')
				OR link.value #>> '{}' !~ '^[0-9]+$'
		) THEN
			RAISE EXCEPTION 'ManagedRun activity/outbox link is malformed'
				USING ERRCODE = '42501', CONSTRAINT = 'managed_run_event_namespace';
		END IF;
		linked := NEW.aggregate_kind = 'managed_run'
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
				@.event_kind like_regex "^managed_run_" ||
				exists(@.managed_run) || exists(@.managed_run_id) ||
				exists(@.effect_barrier) || exists(@.effect_barrier_id)
			)') OR EXISTS (
				SELECT 1 FROM pg_catalog.jsonb_path_query(
					NEW.payload, '$.**.activity_sequence'
				) AS link(value)
				JOIN decodex.activity AS activity ON activity.sequence = CASE
					WHEN pg_catalog.jsonb_typeof(link.value) IN ('number', 'string')
						AND link.value #>> '{}' ~ '^[0-9]+$'
					THEN (link.value #>> '{}')::bigint
				END WHERE activity.aggregate_kind = 'managed_run'
					OR activity.event_kind LIKE 'managed_run_%'
					OR pg_catalog.jsonb_path_exists(activity.payload, '$.** ? (
						@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
						@.event_kind like_regex "^managed_run_" ||
						exists(@.managed_run) || exists(@.managed_run_id) ||
						exists(@.effect_barrier) || exists(@.effect_barrier_id)
					)')
			);
		IF TG_OP = 'UPDATE' THEN
			IF EXISTS (
				SELECT 1 FROM pg_catalog.jsonb_path_query(
					OLD.payload, '$.**.activity_sequence'
				) AS link(value)
				WHERE pg_catalog.jsonb_typeof(link.value) NOT IN ('number', 'string')
					OR link.value #>> '{}' !~ '^[0-9]+$'
			) THEN
				RAISE EXCEPTION 'ManagedRun activity/outbox link is malformed'
					USING ERRCODE = '42501', CONSTRAINT = 'managed_run_event_namespace';
			END IF;
			linked := linked OR OLD.aggregate_kind = 'managed_run'
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
					@.event_kind like_regex "^managed_run_" ||
					exists(@.managed_run) || exists(@.managed_run_id) ||
					exists(@.effect_barrier) || exists(@.effect_barrier_id)
				)') OR EXISTS (
					SELECT 1 FROM pg_catalog.jsonb_path_query(
						OLD.payload, '$.**.activity_sequence'
					) AS link(value)
					JOIN decodex.activity AS activity ON activity.sequence = CASE
						WHEN pg_catalog.jsonb_typeof(link.value) IN ('number', 'string')
							AND link.value #>> '{}' ~ '^[0-9]+$'
						THEN (link.value #>> '{}')::bigint
					END WHERE activity.aggregate_kind = 'managed_run'
						OR activity.event_kind LIKE 'managed_run_%'
						OR pg_catalog.jsonb_path_exists(activity.payload, '$.** ? (
							@.aggregate_kind == "managed_run" || @.kind == "managed_run" ||
							@.event_kind like_regex "^managed_run_" ||
							exists(@.managed_run) || exists(@.managed_run_id) ||
							exists(@.effect_barrier) || exists(@.effect_barrier_id)
						)')
				);
		END IF;
	END IF;
	IF linked AND current_user::name <> owner_name THEN
		IF TG_TABLE_NAME = 'activity' OR TG_OP = 'INSERT' THEN
			RAISE EXCEPTION 'ManagedRun activity/outbox namespace is command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'managed_run_event_namespace';
		ELSIF NEW.id IS DISTINCT FROM OLD.id
			OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload
			OR NEW.created_at IS DISTINCT FROM OLD.created_at
		THEN
			RAISE EXCEPTION 'ManagedRun outbox authority fields are command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'managed_run_event_namespace';
		END IF;
	END IF;
	RETURN NEW;
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
	RAISE EXCEPTION 'ManagedRun activity/outbox link is malformed'
		USING ERRCODE = '42501', CONSTRAINT = 'managed_run_event_namespace';
END
$$;
CREATE TRIGGER activity_managed_run_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_managed_run_event_namespace();
CREATE TRIGGER outbox_managed_run_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_managed_run_event_namespace();

CREATE FUNCTION decodex.reserve_exact_managed_run_safety_command(
	p_protocol text, p_idempotency_key text, p_request jsonb
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE inserted_count bigint;
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_request IS NULL
		OR pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN RAISE EXCEPTION 'exact ManagedRun safety command identity is invalid' USING ERRCODE='22023'; END IF;
	INSERT INTO decodex.exact_command_receipts(
		protocol_version,idempotency_key,request_envelope,request_digest,receipt_state
	) VALUES (p_protocol,p_idempotency_key,p_request,
		public.digest(pg_catalog.convert_to(p_request::text,'UTF8'),'sha256'),'executing')
	ON CONFLICT (protocol_version,idempotency_key) DO NOTHING;
	GET DIAGNOSTICS inserted_count = ROW_COUNT;
	IF inserted_count = 0 THEN
		SELECT request_envelope,response_bytes INTO existing_request,existing_response
		FROM decodex.exact_command_receipts
		WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key FOR UPDATE;
		IF existing_request <> p_request THEN RAISE EXCEPTION 'exact idempotency conflict' USING ERRCODE='DX001'; END IF;
		IF existing_response IS NULL THEN RAISE EXCEPTION 'incomplete exact receipt is not replayable' USING ERRCODE='DX002'; END IF;
		RETURN existing_response;
	END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.complete_exact_managed_run_safety_rejection(
	p_protocol text, p_idempotency_key text, p_reason text, p_request jsonb
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	effect_value := pg_catalog.jsonb_build_object('request',p_request,'reason',p_reason);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','stable_domain_rejection','effect',effect_value)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_rejected',
		outcome_class='stable_domain_rejection',effect_envelope=effect_value,response_bytes=response_value,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response_value;
END
$$;

CREATE FUNCTION decodex.apply_managed_run_safety_input_exact(
	p_protocol text, p_idempotency_key text, p_managed_run_id uuid, p_project_id uuid,
	p_expected_run_revision bigint, p_input_kind decodex.managed_run_safety_input_kind,
	p_input_id uuid, p_runtime_session_id uuid, p_turn_id uuid
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE replay bytea;
DECLARE prior_input record;
DECLARE run_row record;
DECLARE session_row record;
DECLARE barrier_row record;
DECLARE submitted_row record;
DECLARE stale_receipt boolean := false;
DECLARE unknown_turn boolean := false;
DECLARE barrier_closed_now boolean := false;
DECLARE new_run_revision bigint;
DECLARE new_session_revision bigint;
DECLARE new_barrier_revision bigint;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	request_value := pg_catalog.jsonb_build_object(
		'protocol_version',p_protocol,'operation','apply_managed_run_safety_input',
		'managed_run_id',p_managed_run_id,'project_id',p_project_id,
		'expected_run_revision',p_expected_run_revision,'input_kind',p_input_kind,
		'input_id',p_input_id,'runtime_session_id',p_runtime_session_id,'turn_id',p_turn_id);
	replay := decodex.reserve_exact_managed_run_safety_command(
		p_protocol,p_idempotency_key,request_value);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_managed_run_id IS NULL OR p_project_id IS NULL OR p_expected_run_revision IS NULL
		OR p_input_kind IS NULL OR p_input_id IS NULL OR p_runtime_session_id IS NULL
		OR p_expected_run_revision <= 0
		OR (p_input_kind IN ('positively_observed_unknown_turn','submitted_turn_receipt')
			AND p_turn_id IS NULL)
		OR (p_input_kind='inconclusive_observation' AND p_turn_id IS NOT NULL)
	THEN RETURN decodex.complete_exact_managed_run_safety_rejection(
		p_protocol,p_idempotency_key,'invalid_input',request_value); END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(p_managed_run_id::text));
	SELECT * INTO prior_input FROM decodex.managed_run_safety_inputs WHERE input_id=p_input_id FOR UPDATE;
	IF FOUND THEN
		IF prior_input.request_envelope<>request_value
			OR prior_input.managed_run_id<>p_managed_run_id OR prior_input.project_id<>p_project_id
			OR prior_input.runtime_session_id<>p_runtime_session_id OR prior_input.kind<>p_input_kind
			OR prior_input.turn_id IS DISTINCT FROM p_turn_id
		THEN RETURN decodex.complete_exact_managed_run_safety_rejection(
			p_protocol,p_idempotency_key,'input_identity_conflict',request_value); END IF;
		UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
			outcome_class='success',effect_envelope=prior_input.effect_envelope,
			response_bytes=prior_input.response_bytes,completed_at=pg_catalog.clock_timestamp()
		WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
		RETURN prior_input.response_bytes;
	END IF;

	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=p_managed_run_id AND project_id=p_project_id FOR UPDATE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_managed_run_safety_rejection(
		p_protocol,p_idempotency_key,'missing_target',request_value); END IF;
	IF run_row.revision<>p_expected_run_revision THEN
		RETURN decodex.complete_exact_managed_run_safety_rejection(
			p_protocol,p_idempotency_key,'stale_revision',request_value); END IF;
	IF run_row.runtime_session_id<>p_runtime_session_id THEN
		RETURN decodex.complete_exact_managed_run_safety_rejection(
			p_protocol,p_idempotency_key,'wrong_runtime_session',request_value); END IF;
	SELECT * INTO session_row FROM decodex.runtime_sessions
	WHERE runtime_session_id=p_runtime_session_id FOR UPDATE;
	SELECT * INTO barrier_row FROM decodex.managed_run_effect_barriers
	WHERE managed_run_id=p_managed_run_id FOR UPDATE;
	IF session_row.runtime_session_id IS NULL OR barrier_row.managed_run_id IS NULL THEN
		RETURN decodex.complete_exact_managed_run_safety_rejection(
			p_protocol,p_idempotency_key,'missing_target',request_value); END IF;

	IF p_input_kind='submitted_turn_receipt' THEN
		SELECT * INTO submitted_row FROM decodex.managed_run_submitted_turn_receipts
		WHERE receipt_id=p_input_id AND managed_run_id=p_managed_run_id
			AND project_id=p_project_id AND runtime_session_id=p_runtime_session_id
			AND turn_id=p_turn_id FOR SHARE;
		IF NOT FOUND THEN RETURN decodex.complete_exact_managed_run_safety_rejection(
			p_protocol,p_idempotency_key,'missing_submitted_turn_receipt',request_value); END IF;
		stale_receipt := submitted_row.runtime_session_revision<>session_row.revision;
	ELSIF p_input_kind='positively_observed_unknown_turn' THEN
		unknown_turn := NOT EXISTS (SELECT 1 FROM decodex.managed_run_submitted_turn_receipts
			WHERE managed_run_id=p_managed_run_id AND runtime_session_id=p_runtime_session_id
				AND turn_id=p_turn_id)
			AND NOT EXISTS (SELECT 1 FROM decodex.turns WHERE turn_id=p_turn_id);
		IF NOT unknown_turn THEN
			RETURN decodex.complete_exact_managed_run_safety_rejection(
				p_protocol,p_idempotency_key,'turn_already_owned_or_known',request_value);
		END IF;
	END IF;

	IF unknown_turn AND session_row.state IN ('starting','active') THEN
		UPDATE decodex.runtime_sessions SET state='diverged',revision=revision+1
		WHERE runtime_session_id=p_runtime_session_id;
	END IF;
	SELECT revision INTO new_session_revision FROM decodex.runtime_sessions
	WHERE runtime_session_id=p_runtime_session_id;

	UPDATE decodex.managed_runs SET runtime_session_revision=new_session_revision,
		revision=revision+1,diverged=diverged OR unknown_turn,blocked=true,
		wait_reason='external',updated_at=pg_catalog.clock_timestamp()
	WHERE managed_run_id=p_managed_run_id RETURNING revision INTO new_run_revision;

	IF barrier_row.state='guarded' THEN
		UPDATE decodex.managed_run_effect_barriers SET state='closed',revision=2,
			closure_input_id=p_input_id,closure_input_kind=p_input_kind,
			closed_at=pg_catalog.clock_timestamp()
		WHERE managed_run_id=p_managed_run_id RETURNING revision INTO new_barrier_revision;
		barrier_closed_now := true;
	ELSE
		new_barrier_revision := barrier_row.revision;
	END IF;

	effect_value := pg_catalog.jsonb_build_object(
		'request',request_value,'managed_run_id',p_managed_run_id,
		'managed_run_revision',new_run_revision,'runtime_session_id',p_runtime_session_id,
		'runtime_session_revision',new_session_revision,
		'runtime_session_diverged',unknown_turn,'managed_run_blocked',true,
		'effect_barrier_state','closed','effect_barrier_revision',new_barrier_revision,
		'effect_barrier_closed_now',barrier_closed_now,'stale_receipt',stale_receipt);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect_value)::text,'UTF8');
	INSERT INTO decodex.managed_run_safety_inputs(
		input_id,managed_run_id,project_id,runtime_session_id,kind,turn_id,
		managed_run_revision,runtime_session_revision,barrier_revision,stale_receipt,
		request_envelope,effect_envelope,response_bytes
	) VALUES (p_input_id,p_managed_run_id,p_project_id,p_runtime_session_id,p_input_kind,p_turn_id,
		new_run_revision,new_session_revision,new_barrier_revision,stale_receipt,
		request_value,effect_value,response_value);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect_value,response_bytes=response_value,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response_value;
END
$$;

REVOKE ALL ON TABLE decodex.managed_runs, decodex.managed_run_assignments,
	decodex.managed_run_effect_barriers, decodex.managed_run_effects,
	decodex.managed_run_submitted_turn_receipts, decodex.managed_run_safety_inputs FROM PUBLIC;
REVOKE ALL ON TYPE decodex.managed_run_lifecycle, decodex.managed_run_phase,
	decodex.managed_run_wait_reason, decodex.execution_assignment_role,
	decodex.effect_barrier_state, decodex.managed_run_effect_kind,
	decodex.managed_run_effect_state, decodex.managed_run_safety_input_kind FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
