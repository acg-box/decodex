-- XY-1400 durable provider-process generations.
-- ProcessSupervisor is the only product caller of these commands. Runtime receives no direct
-- relation authority. Negative search, PID or group absence, timeout, lease expiry, and row
-- absence have no transition path to dead.

CREATE TYPE decodex.process_generation_state AS ENUM (
	'starting', 'ready', 'stopping', 'dead', 'death_unknown'
);
CREATE TYPE decodex.process_generation_control_kind AS ENUM (
	'stdio_only_best_effort_eof', 'parent_death_signal_and_stdio_eof'
);
CREATE TYPE decodex.process_generation_isolation_kind AS ENUM ('session');
CREATE TYPE decodex.process_generation_loss_reason AS ENUM (
	'supervisor_restarted',
	'identity_persistence_failed',
	'readiness_persistence_failed',
	'termination_unproved',
	'control_authority_lost'
);
CREATE TYPE decodex.process_generation_death_evidence_kind AS ENUM (
	'spawn_not_created',
	'owned_child_exit',
	'linux_pidfd_exit',
	'macos_kqueue_exit_and_group_quiescence',
	'exact_termination_exit',
	'prior_boot_ended'
);

-- The backup and restore owner writes this relation through migration/operations authority.
-- ProcessSupervisor must receive the matching digest from outside the restored database. No
-- runtime read API returns that digest, so a rollback cannot recreate launch permission.
CREATE TABLE decodex.process_generation_execution_epochs (
	execution_epoch_id uuid PRIMARY KEY,
	authorization_digest text NOT NULL CHECK (
		authorization_digest ~ '^[0-9a-f]{64}$'
	),
	authorized_at timestamptz NOT NULL CHECK (isfinite(authorized_at)),
	retired_at timestamptz CHECK (
		retired_at IS NULL OR (isfinite(retired_at) AND retired_at >= authorized_at)
	)
);
CREATE UNIQUE INDEX process_generation_one_active_execution_epoch
	ON decodex.process_generation_execution_epochs ((retired_at IS NULL))
	WHERE retired_at IS NULL;

CREATE TABLE decodex.process_generations (
	generation_id uuid PRIMARY KEY,
	account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	execution_epoch_id uuid NOT NULL
		REFERENCES decodex.process_generation_execution_epochs(execution_epoch_id),
	runner_identity text NOT NULL CHECK (
		runner_identity ~ '^sha256:[0-9a-f]{64}$'
	),
	intended_boot_id text NOT NULL CHECK (
		intended_boot_id <> ''
		AND octet_length(intended_boot_id) <= 128
		AND intended_boot_id !~ '[[:cntrl:][:space:]]'
	),
	control_kind decodex.process_generation_control_kind NOT NULL,
	isolation_kind decodex.process_generation_isolation_kind NOT NULL,
	bound_boot_id text,
	process_id bigint,
	process_start_id text,
	process_group_id bigint,
	session_id bigint,
	state decodex.process_generation_state NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	authority_loss_reason decodex.process_generation_loss_reason,
	death_evidence_id uuid,
	created_at timestamptz NOT NULL,
	updated_at timestamptz NOT NULL,
	CONSTRAINT process_generation_finite_timestamps CHECK (
		isfinite(created_at) AND isfinite(updated_at) AND updated_at >= created_at
	),
	CONSTRAINT process_generation_identity_shape CHECK (
		(
			bound_boot_id IS NULL
			AND process_id IS NULL
			AND process_start_id IS NULL
			AND process_group_id IS NULL
			AND session_id IS NULL
		) OR (
			bound_boot_id IS NOT NULL
			AND bound_boot_id <> ''
			AND octet_length(bound_boot_id) <= 128
			AND bound_boot_id !~ '[[:cntrl:][:space:]]'
			AND bound_boot_id = intended_boot_id
			AND process_id BETWEEN 1 AND 4294967295
			AND process_start_id IS NOT NULL
			AND process_start_id <> ''
			AND octet_length(process_start_id) <= 128
			AND process_start_id !~ '[[:cntrl:][:space:]]'
			AND process_group_id = process_id
			AND session_id = process_id
		)
	),
	CONSTRAINT process_generation_ready_identity CHECK (
		state NOT IN ('ready', 'stopping') OR process_id IS NOT NULL
	),
	CONSTRAINT process_generation_loss_shape CHECK (
		(state = 'death_unknown') = (authority_loss_reason IS NOT NULL)
	),
	CONSTRAINT process_generation_death_shape CHECK (
		(state = 'dead') = (death_evidence_id IS NOT NULL)
	)
);

CREATE UNIQUE INDEX process_generation_one_unresolved_per_account
	ON decodex.process_generations(account_id)
	WHERE state <> 'dead';
CREATE INDEX process_generation_reconciliation_order
	ON decodex.process_generations(state, updated_at, generation_id)
	WHERE state = 'death_unknown';

CREATE TABLE decodex.process_generation_death_evidence (
	evidence_id uuid PRIMARY KEY,
	generation_id uuid NOT NULL
		REFERENCES decodex.process_generations(generation_id),
	generation_revision bigint NOT NULL CHECK (generation_revision > 0),
	kind decodex.process_generation_death_evidence_kind NOT NULL,
	observed_boot_id text NOT NULL CHECK (
		observed_boot_id <> ''
		AND octet_length(observed_boot_id) <= 128
		AND observed_boot_id !~ '[[:cntrl:][:space:]]'
	),
	process_id bigint,
	process_start_id text,
	process_group_id bigint,
	session_id bigint,
	witness_digest text NOT NULL CHECK (witness_digest ~ '^[0-9a-f]{64}$'),
	observed_at timestamptz NOT NULL CHECK (isfinite(observed_at)),
	CONSTRAINT process_generation_death_evidence_identity_shape CHECK (
		(
			kind IN ('spawn_not_created', 'prior_boot_ended', 'owned_child_exit')
			AND process_id IS NULL
			AND process_start_id IS NULL
			AND process_group_id IS NULL
			AND session_id IS NULL
		) OR (
			kind NOT IN ('spawn_not_created', 'prior_boot_ended')
			AND process_id BETWEEN 1 AND 4294967295
			AND process_start_id IS NOT NULL
			AND process_start_id <> ''
			AND octet_length(process_start_id) <= 128
			AND process_start_id !~ '[[:cntrl:][:space:]]'
			AND process_group_id = process_id
			AND session_id = process_id
		)
	),
	CONSTRAINT process_generation_death_evidence_scope
		UNIQUE (evidence_id, generation_id)
);

ALTER TABLE decodex.process_generations
	ADD CONSTRAINT process_generation_death_evidence_fk
	FOREIGN KEY (death_evidence_id, generation_id)
	REFERENCES decodex.process_generation_death_evidence(evidence_id, generation_id)
	DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE decodex.process_generation_transitions (
	generation_id uuid NOT NULL
		REFERENCES decodex.process_generations(generation_id),
	revision bigint NOT NULL CHECK (revision > 0),
	previous_state decodex.process_generation_state,
	state decodex.process_generation_state NOT NULL,
	authority_loss_reason decodex.process_generation_loss_reason,
	death_evidence_id uuid,
	transitioned_at timestamptz NOT NULL CHECK (isfinite(transitioned_at)),
	PRIMARY KEY (generation_id, revision),
	CONSTRAINT process_generation_transition_first CHECK (
		(revision = 1) = (previous_state IS NULL)
	),
	CONSTRAINT process_generation_transition_loss_shape CHECK (
		(state = 'death_unknown') = (authority_loss_reason IS NOT NULL)
	),
	CONSTRAINT process_generation_transition_death_shape CHECK (
		(state = 'dead') = (death_evidence_id IS NOT NULL)
	),
	CONSTRAINT process_generation_transition_evidence_fk
		FOREIGN KEY (death_evidence_id, generation_id)
		REFERENCES decodex.process_generation_death_evidence(evidence_id, generation_id)
		DEFERRABLE INITIALLY DEFERRED
	);

ALTER TABLE decodex.process_generation_death_evidence
	ADD CONSTRAINT process_generation_death_evidence_revision_fk
	FOREIGN KEY (generation_id, generation_revision)
	REFERENCES decodex.process_generation_transitions(generation_id, revision)
	DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION decodex.enforce_process_generation_transition()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.revision <> 1 OR NEW.state <> 'starting'
			OR NEW.process_id IS NOT NULL
			OR NEW.authority_loss_reason IS NOT NULL
			OR NEW.death_evidence_id IS NOT NULL
		THEN
			RAISE EXCEPTION 'process generation must begin as unbound revision-one starting'
				USING ERRCODE='23514',
					CONSTRAINT='process_generation_initial_state';
		END IF;
		RETURN NEW;
	END IF;

	IF (NEW.generation_id, NEW.account_id, NEW.execution_epoch_id, NEW.runner_identity,
		NEW.intended_boot_id, NEW.control_kind, NEW.isolation_kind, NEW.created_at)
		IS DISTINCT FROM
		(OLD.generation_id, OLD.account_id, OLD.execution_epoch_id, OLD.runner_identity,
		OLD.intended_boot_id, OLD.control_kind, OLD.isolation_kind, OLD.created_at)
		OR NEW.revision <> OLD.revision + 1
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'process generation immutable identity or revision changed'
			USING ERRCODE='23514',
				CONSTRAINT='process_generation_immutable_identity';
	END IF;

	IF (NEW.bound_boot_id, NEW.process_id, NEW.process_start_id,
		NEW.process_group_id, NEW.session_id)
		IS DISTINCT FROM
		(OLD.bound_boot_id, OLD.process_id, OLD.process_start_id,
		OLD.process_group_id, OLD.session_id)
		AND NOT (
			OLD.state = 'starting'
			AND NEW.state = 'starting'
			AND OLD.process_id IS NULL
			AND NEW.process_id IS NOT NULL
		)
	THEN
		RAISE EXCEPTION 'process generation exact process identity changed'
			USING ERRCODE='23514',
				CONSTRAINT='process_generation_immutable_process_identity';
	END IF;

	IF NOT (
		(OLD.state = 'starting' AND NEW.state = 'starting'
			AND OLD.process_id IS NULL AND NEW.process_id IS NOT NULL)
		OR (OLD.state = 'starting' AND NEW.state IN ('ready', 'death_unknown'))
		OR (OLD.state = 'starting' AND NEW.state = 'stopping'
			AND NEW.process_id IS NOT NULL)
		OR (OLD.state = 'ready' AND NEW.state IN ('stopping', 'death_unknown'))
		OR (OLD.state = 'stopping' AND NEW.state IN ('dead', 'death_unknown'))
		OR (OLD.state = 'death_unknown' AND NEW.state IN ('stopping', 'dead'))
	) THEN
		RAISE EXCEPTION 'illegal process generation transition'
			USING ERRCODE='23514',
				CONSTRAINT='process_generation_legal_transition';
	END IF;

	IF NEW.state = 'dead' AND NOT EXISTS (
		SELECT 1
		FROM decodex.process_generation_death_evidence AS evidence
		WHERE (evidence.evidence_id, evidence.generation_id) =
			(NEW.death_evidence_id, NEW.generation_id)
	) THEN
		RAISE EXCEPTION 'dead process generation lacks positive evidence'
			USING ERRCODE='23514',
				CONSTRAINT='process_generation_positive_death';
	END IF;

	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.record_process_generation_transition()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	INSERT INTO decodex.process_generation_transitions(
		generation_id, revision, previous_state, state,
		authority_loss_reason, death_evidence_id, transitioned_at
	) VALUES (
		NEW.generation_id, NEW.revision,
		CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.state END,
		NEW.state, NEW.authority_loss_reason, NEW.death_evidence_id, NEW.updated_at
	);
	RETURN NULL;
END;
$$;

CREATE FUNCTION decodex.forbid_process_generation_history_mutation()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	RAISE EXCEPTION 'process generation history is append-only'
		USING ERRCODE='23514',
			CONSTRAINT='process_generation_history_immutable';
END;
$$;

CREATE TRIGGER process_generation_transition_guard
	BEFORE INSERT OR UPDATE
	ON decodex.process_generations
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_process_generation_transition();
CREATE TRIGGER process_generation_delete_immutable
	BEFORE DELETE OR TRUNCATE
	ON decodex.process_generations
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_process_generation_history_mutation();
CREATE TRIGGER process_generation_transition_record
	AFTER INSERT OR UPDATE
	ON decodex.process_generations
	FOR EACH ROW EXECUTE FUNCTION decodex.record_process_generation_transition();
CREATE TRIGGER process_generation_death_evidence_immutable
	BEFORE UPDATE OR DELETE OR TRUNCATE
	ON decodex.process_generation_death_evidence
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_process_generation_history_mutation();
CREATE TRIGGER process_generation_transitions_immutable
	BEFORE UPDATE OR DELETE OR TRUNCATE
	ON decodex.process_generation_transitions
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_process_generation_history_mutation();

CREATE FUNCTION decodex.prepare_process_generation_exact(
	p_generation_id uuid,
	p_account_id uuid,
	p_execution_epoch_id uuid,
	p_authorization_digest text,
	p_runner_identity text,
	p_intended_boot_id text,
	p_control_kind decodex.process_generation_control_kind,
	p_isolation_kind decodex.process_generation_isolation_kind
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	created_at_micros bigint,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE existing decodex.process_generations%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	-- The generation namespace makes same-ID/different-account races deterministic. The
	-- separate account namespace then serializes replacement authority without a cross-key
	-- deadlock order. The shared restore-projection gate permits unrelated account launches
	-- concurrently but excludes the startup-wide loss projection.
	PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1400, pg_catalog.hashtext(p_generation_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1401, pg_catalog.hashtext(p_account_id::text)
	);
	SELECT * INTO existing
	FROM decodex.process_generations
	WHERE generation_id = p_generation_id
	FOR UPDATE;

	IF FOUND THEN
		IF (existing.account_id, existing.execution_epoch_id, existing.runner_identity,
			existing.intended_boot_id, existing.control_kind, existing.isolation_kind)
			IS DISTINCT FROM
			(p_account_id, p_execution_epoch_id, p_runner_identity,
			p_intended_boot_id, p_control_kind, p_isolation_kind)
		THEN
			RETURN QUERY SELECT 'identity_conflict', existing.revision, existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT 'replayed', existing.revision, existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;

	IF p_authorization_digest IS NULL
		OR p_authorization_digest !~ '^[0-9a-f]{64}$'
	THEN
		RETURN QUERY SELECT 'restore_authority_unavailable', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;
	PERFORM 1
	FROM decodex.process_generation_execution_epochs AS epoch
	WHERE epoch.execution_epoch_id = p_execution_epoch_id
		AND epoch.authorization_digest = p_authorization_digest
		AND epoch.retired_at IS NULL
	FOR SHARE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'restore_authority_unavailable', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;
	PERFORM 1 FROM decodex.accounts
	WHERE account_id = p_account_id
	FOR KEY SHARE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'account_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;
	IF EXISTS (
		SELECT 1 FROM decodex.process_generations
		WHERE account_id = p_account_id AND state <> 'dead'
	) THEN
		RETURN QUERY SELECT 'account_quarantined', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	now_value := pg_catalog.clock_timestamp();
	INSERT INTO decodex.process_generations(
		generation_id, account_id, execution_epoch_id, runner_identity,
		intended_boot_id, control_kind, isolation_kind, state, revision,
		created_at, updated_at
	) VALUES (
		p_generation_id, p_account_id, p_execution_epoch_id, p_runner_identity,
		p_intended_boot_id, p_control_kind, p_isolation_kind, 'starting', 1,
		now_value, now_value
	);
	RETURN QUERY SELECT 'prepared', 1::bigint,
		'starting'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.bind_process_generation_identity_exact(
	p_generation_id uuid,
	p_expected_revision bigint,
	p_bound_boot_id text,
	p_process_id bigint,
	p_process_start_id text,
	p_process_group_id bigint,
	p_session_id bigint
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'generation_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.process_id IS NOT NULL THEN
		IF (generation.bound_boot_id, generation.process_id, generation.process_start_id,
			generation.process_group_id, generation.session_id)
			IS NOT DISTINCT FROM
			(p_bound_boot_id, p_process_id, p_process_start_id,
			p_process_group_id, p_session_id)
			AND generation.state = 'starting'
			AND p_expected_revision = generation.revision - 1
		THEN
			RETURN QUERY SELECT 'replayed', generation.revision, generation.state,
				(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		ELSIF (generation.bound_boot_id, generation.process_id, generation.process_start_id,
			generation.process_group_id, generation.session_id)
			IS NOT DISTINCT FROM
			(p_bound_boot_id, p_process_id, p_process_start_id,
			p_process_group_id, p_session_id)
		THEN
			RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
				(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT 'process_identity_conflict', generation.revision, generation.state,
				(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;
	IF generation.revision <> p_expected_revision
		OR generation.state <> 'starting'
	THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF p_bound_boot_id IS DISTINCT FROM generation.intended_boot_id
		OR p_process_id IS NULL OR p_process_id NOT BETWEEN 1 AND 4294967295
		OR p_process_group_id IS DISTINCT FROM p_process_id
		OR p_session_id IS DISTINCT FROM p_process_id
		OR p_process_start_id IS NULL OR p_process_start_id = ''
		OR octet_length(p_process_start_id) > 128
		OR p_process_start_id ~ '[[:cntrl:][:space:]]'
	THEN
		RETURN QUERY SELECT 'invalid_process_identity', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.process_generations AS target
	SET bound_boot_id = p_bound_boot_id,
		process_id = p_process_id,
		process_start_id = p_process_start_id,
		process_group_id = p_process_group_id,
		session_id = p_session_id,
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.generation_id = p_generation_id;
	RETURN QUERY SELECT 'bound', p_expected_revision + 1,
		'starting'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.mark_process_generation_ready_exact(
	p_generation_id uuid,
	p_expected_revision bigint
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'generation_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.state = 'ready' THEN
		RETURN QUERY SELECT
			CASE WHEN p_expected_revision = generation.revision - 1
				THEN 'replayed' ELSE 'stale_generation' END,
			generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.revision <> p_expected_revision
		OR generation.state <> 'starting'
		OR generation.process_id IS NULL
	THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.process_generations AS target
	SET state = 'ready', revision = target.revision + 1, updated_at = now_value
	WHERE target.generation_id = p_generation_id;
	RETURN QUERY SELECT 'ready', p_expected_revision + 1,
		'ready'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.mark_process_generation_stopping_exact(
	p_generation_id uuid,
	p_expected_revision bigint
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'generation_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.state = 'stopping' THEN
		RETURN QUERY SELECT
			CASE WHEN p_expected_revision IN (generation.revision - 1, generation.revision)
				THEN 'replayed' ELSE 'stale_generation' END,
			generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.revision <> p_expected_revision
		OR generation.state NOT IN ('starting', 'ready', 'death_unknown')
		OR generation.process_id IS NULL
	THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.process_generations AS target
	SET state = 'stopping',
		authority_loss_reason = NULL,
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.generation_id = p_generation_id;
	RETURN QUERY SELECT 'stopping', p_expected_revision + 1,
		'stopping'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.mark_process_generation_death_unknown_exact(
	p_generation_id uuid,
	p_expected_revision bigint,
	p_reason decodex.process_generation_loss_reason
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'generation_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL OR p_reason IS NULL THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.state = 'death_unknown' THEN
		RETURN QUERY SELECT
			CASE WHEN p_expected_revision = generation.revision - 1
					AND p_reason = generation.authority_loss_reason
				THEN 'replayed' ELSE 'stale_generation' END,
			generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF generation.state = 'dead'
		OR generation.revision <> p_expected_revision
	THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state,
			(extract(epoch FROM generation.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.process_generations AS target
	SET state = 'death_unknown',
		authority_loss_reason = p_reason,
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.generation_id = p_generation_id;
	RETURN QUERY SELECT 'death_unknown', p_expected_revision + 1,
		'death_unknown'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.record_process_generation_death_exact(
	p_generation_id uuid,
	p_expected_revision bigint,
	p_evidence_id uuid,
	p_kind decodex.process_generation_death_evidence_kind,
	p_observed_boot_id text,
	p_process_id bigint,
	p_process_start_id text,
	p_process_group_id bigint,
	p_session_id bigint,
	p_witness_digest text
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.process_generation_state,
	observed_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE evidence decodex.process_generation_death_evidence%ROWTYPE;
DECLARE observed timestamptz;
BEGIN
	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'generation_missing', 0::bigint,
			'starting'::decodex.process_generation_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state, 0::bigint;
		RETURN;
	END IF;
	SELECT * INTO evidence
	FROM decodex.process_generation_death_evidence
	WHERE evidence_id = p_evidence_id;
	IF FOUND THEN
		IF (evidence.generation_id, evidence.generation_revision,
			evidence.kind, evidence.observed_boot_id,
			evidence.process_id, evidence.process_start_id, evidence.process_group_id,
			evidence.session_id, evidence.witness_digest)
			IS NOT DISTINCT FROM
			(p_generation_id, p_expected_revision, p_kind, p_observed_boot_id,
			p_process_id, p_process_start_id, p_process_group_id,
			p_session_id, p_witness_digest)
			AND generation.state = 'dead'
			AND generation.death_evidence_id = p_evidence_id
		THEN
			RETURN QUERY SELECT 'replayed', generation.revision, generation.state,
				(extract(epoch FROM evidence.observed_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT 'evidence_conflict', generation.revision, generation.state,
				(extract(epoch FROM evidence.observed_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;
	IF generation.state = 'dead'
		OR generation.revision <> p_expected_revision
	THEN
		RETURN QUERY SELECT 'stale_generation', generation.revision, generation.state, 0::bigint;
		RETURN;
	END IF;
	IF p_witness_digest IS NULL OR p_witness_digest !~ '^[0-9a-f]{64}$'
		OR p_observed_boot_id IS NULL OR p_observed_boot_id = ''
		OR octet_length(p_observed_boot_id) > 128
		OR p_observed_boot_id ~ '[[:cntrl:][:space:]]'
	THEN
		RETURN QUERY SELECT 'invalid_evidence', generation.revision, generation.state, 0::bigint;
		RETURN;
	END IF;
	IF p_kind = 'spawn_not_created' THEN
		IF generation.state <> 'starting'
			OR generation.process_id IS NOT NULL
			OR p_observed_boot_id IS DISTINCT FROM generation.intended_boot_id
			OR p_process_id IS NOT NULL OR p_process_start_id IS NOT NULL
			OR p_process_group_id IS NOT NULL OR p_session_id IS NOT NULL
		THEN
			RETURN QUERY SELECT 'evidence_mismatch', generation.revision, generation.state,
				0::bigint;
			RETURN;
		END IF;
	ELSIF p_kind = 'prior_boot_ended' THEN
		IF generation.state <> 'death_unknown'
			OR p_observed_boot_id = generation.intended_boot_id
			OR p_process_id IS NOT NULL OR p_process_start_id IS NOT NULL
			OR p_process_group_id IS NOT NULL OR p_session_id IS NOT NULL
		THEN
			RETURN QUERY SELECT 'evidence_mismatch', generation.revision, generation.state,
				0::bigint;
			RETURN;
		END IF;
	ELSIF p_kind = 'owned_child_exit' AND generation.process_id IS NULL THEN
		IF generation.state NOT IN ('starting', 'death_unknown')
			OR p_observed_boot_id IS DISTINCT FROM generation.intended_boot_id
			OR p_process_id IS NOT NULL OR p_process_start_id IS NOT NULL
			OR p_process_group_id IS NOT NULL OR p_session_id IS NOT NULL
		THEN
			RETURN QUERY SELECT 'evidence_mismatch', generation.revision, generation.state,
				0::bigint;
			RETURN;
		END IF;
	ELSIF generation.process_id IS NULL
		OR p_observed_boot_id IS DISTINCT FROM generation.bound_boot_id
		OR (p_process_id, p_process_start_id, p_process_group_id, p_session_id)
			IS DISTINCT FROM
			(generation.process_id, generation.process_start_id,
			generation.process_group_id, generation.session_id)
	THEN
		RETURN QUERY SELECT 'evidence_mismatch', generation.revision, generation.state,
			0::bigint;
		RETURN;
	END IF;

	observed := pg_catalog.clock_timestamp();
	INSERT INTO decodex.process_generation_death_evidence(
		evidence_id, generation_id, generation_revision, kind, observed_boot_id,
		process_id, process_start_id, process_group_id, session_id,
		witness_digest, observed_at
	) VALUES (
		p_evidence_id, p_generation_id, p_expected_revision, p_kind, p_observed_boot_id,
		p_process_id, p_process_start_id, p_process_group_id, p_session_id,
		p_witness_digest, observed
	);

	IF generation.state = 'ready' THEN
		UPDATE decodex.process_generations AS target
		SET state = 'stopping', revision = target.revision + 1, updated_at = observed
		WHERE target.generation_id = p_generation_id;
	ELSIF generation.state = 'starting' THEN
		UPDATE decodex.process_generations AS target
		SET state = 'death_unknown',
			authority_loss_reason = 'control_authority_lost',
			revision = target.revision + 1,
			updated_at = observed
		WHERE target.generation_id = p_generation_id;
	END IF;

	UPDATE decodex.process_generations AS target
	SET state = 'dead',
		authority_loss_reason = NULL,
		death_evidence_id = p_evidence_id,
		revision = target.revision + 1,
		updated_at = observed
	WHERE target.generation_id = p_generation_id;

	SELECT * INTO generation FROM decodex.process_generations
	WHERE generation_id = p_generation_id;
	RETURN QUERY SELECT 'dead', generation.revision, generation.state,
		(extract(epoch FROM observed)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.project_process_generations_after_supervisor_loss_exact()
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE changed bigint;
DECLARE now_value timestamptz;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock(1400);
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.process_generations
	SET state = 'death_unknown',
		authority_loss_reason = 'supervisor_restarted',
		revision = revision + 1,
		updated_at = now_value
	WHERE state IN ('starting', 'ready', 'stopping');
	GET DIAGNOSTICS changed = ROW_COUNT;
	RETURN changed;
END;
$$;

CREATE FUNCTION decodex.read_process_generations_exact(
	p_account_id uuid,
	p_include_dead boolean,
	p_after_generation_id uuid,
	p_limit bigint
) RETURNS TABLE(
	generation_id uuid,
	account_id uuid,
	execution_epoch_id uuid,
	runner_identity text,
	intended_boot_id text,
	control_kind decodex.process_generation_control_kind,
	isolation_kind decodex.process_generation_isolation_kind,
	bound_boot_id text,
	process_id bigint,
	process_start_id text,
	process_group_id bigint,
	session_id bigint,
	state decodex.process_generation_state,
	revision bigint,
	authority_loss_reason decodex.process_generation_loss_reason,
	death_evidence_id uuid,
	created_at_micros bigint,
	updated_at_micros bigint
) LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF p_limit IS NULL OR p_limit < 1 OR p_limit > 256 THEN
		RAISE EXCEPTION 'process generation read limit is invalid'
			USING ERRCODE='22023';
	END IF;
	RETURN QUERY
	SELECT
		generation.generation_id,
		generation.account_id,
		generation.execution_epoch_id,
		generation.runner_identity,
		generation.intended_boot_id,
		generation.control_kind,
		generation.isolation_kind,
		generation.bound_boot_id,
		generation.process_id,
		generation.process_start_id,
		generation.process_group_id,
		generation.session_id,
		generation.state,
		generation.revision,
		generation.authority_loss_reason,
		generation.death_evidence_id,
		(extract(epoch FROM generation.created_at)*1000000)::bigint,
		(extract(epoch FROM generation.updated_at)*1000000)::bigint
	FROM decodex.process_generations AS generation
	WHERE (p_account_id IS NULL OR generation.account_id = p_account_id)
		AND (p_include_dead OR generation.state <> 'dead')
		AND (p_after_generation_id IS NULL
			OR generation.generation_id > p_after_generation_id)
	ORDER BY generation.generation_id
	LIMIT p_limit;
END;
$$;

REVOKE ALL ON TYPE
	decodex.process_generation_state,
	decodex.process_generation_control_kind,
	decodex.process_generation_isolation_kind,
	decodex.process_generation_loss_reason,
	decodex.process_generation_death_evidence_kind
	FROM PUBLIC;
REVOKE ALL ON TABLE
	decodex.process_generation_execution_epochs,
	decodex.process_generations,
	decodex.process_generation_death_evidence,
	decodex.process_generation_transitions
	FROM PUBLIC;
REVOKE ALL ON FUNCTION
	decodex.enforce_process_generation_transition(),
	decodex.record_process_generation_transition(),
	decodex.forbid_process_generation_history_mutation(),
	decodex.prepare_process_generation_exact(uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind),
	decodex.bind_process_generation_identity_exact(uuid,bigint,text,bigint,text,bigint,bigint),
	decodex.mark_process_generation_ready_exact(uuid,bigint),
	decodex.mark_process_generation_stopping_exact(uuid,bigint),
	decodex.mark_process_generation_death_unknown_exact(uuid,bigint,decodex.process_generation_loss_reason),
	decodex.record_process_generation_death_exact(uuid,bigint,uuid,decodex.process_generation_death_evidence_kind,text,bigint,text,bigint,bigint,text),
	decodex.project_process_generations_after_supervisor_loss_exact(),
	decodex.read_process_generations_exact(uuid,boolean,uuid,bigint)
	FROM PUBLIC;

-- Derive the one configured runtime principal from the existing migration-owned anchor. The
-- migration identity retains ownership; runtime receives only the eight ProcessSupervisor
-- entrypoints and enum USAGE.
DO $$
DECLARE anchor_oid pg_catalog.oid;
DECLARE migration_role_oid pg_catalog.oid;
DECLARE owner_execute_count pg_catalog.int8;
DECLARE runtime_role_count pg_catalog.int8;
DECLARE invalid_execute_count pg_catalog.int8;
DECLARE runtime_role pg_catalog.name;
BEGIN
	SELECT role.oid INTO migration_role_oid FROM pg_catalog.pg_roles AS role
	WHERE role.rolname = current_user;
	anchor_oid := pg_catalog.to_regprocedure(
		'decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)'
	);
	IF anchor_oid IS NULL OR NOT EXISTS (
		SELECT 1 FROM pg_catalog.pg_proc AS procedure
		WHERE procedure.oid = anchor_oid AND procedure.proowner = migration_role_oid
	) THEN
		RAISE EXCEPTION 'V23 runtime principal anchor is missing or not migration-owned'
			USING ERRCODE='42501';
	END IF;
	SELECT
		pg_catalog.count(*) FILTER (WHERE privilege.grantee = migration_role_oid),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee <> migration_role_oid AND role.oid IS NOT NULL
		),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee = 0
				OR privilege.grantor <> migration_role_oid
				OR (
					privilege.grantee <> migration_role_oid
					AND (privilege.is_grantable OR role.oid IS NULL)
				)
		),
		pg_catalog.min(role.rolname) FILTER (
			WHERE privilege.grantee <> migration_role_oid AND role.oid IS NOT NULL
		)
	INTO owner_execute_count, runtime_role_count, invalid_execute_count, runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(
		COALESCE(procedure.proacl, pg_catalog.acldefault('f', procedure.proowner))
	) AS privilege
	LEFT JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee
	WHERE procedure.oid = anchor_oid AND privilege.privilege_type = 'EXECUTE';
	IF owner_execute_count <> 1 OR runtime_role_count > 1 OR invalid_execute_count <> 0 THEN
		RAISE EXCEPTION 'V23 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count = 1 THEN
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON TABLE decodex.process_generation_execution_epochs, '
			|| 'decodex.process_generations, '
			|| 'decodex.process_generation_death_evidence, '
			|| 'decodex.process_generation_transitions FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON TYPE decodex.process_generation_state, '
			|| 'decodex.process_generation_control_kind, '
			|| 'decodex.process_generation_isolation_kind, '
			|| 'decodex.process_generation_loss_reason, '
			|| 'decodex.process_generation_death_evidence_kind FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON FUNCTION '
			|| 'decodex.enforce_process_generation_transition(), '
			|| 'decodex.record_process_generation_transition(), '
			|| 'decodex.forbid_process_generation_history_mutation(), '
			|| 'decodex.prepare_process_generation_exact(uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind), '
			|| 'decodex.bind_process_generation_identity_exact(uuid,bigint,text,bigint,text,bigint,bigint), '
			|| 'decodex.mark_process_generation_ready_exact(uuid,bigint), '
			|| 'decodex.mark_process_generation_stopping_exact(uuid,bigint), '
			|| 'decodex.mark_process_generation_death_unknown_exact(uuid,bigint,decodex.process_generation_loss_reason), '
			|| 'decodex.record_process_generation_death_exact(uuid,bigint,uuid,decodex.process_generation_death_evidence_kind,text,bigint,text,bigint,bigint,text), '
			|| 'decodex.project_process_generations_after_supervisor_loss_exact(), '
			|| 'decodex.read_process_generations_exact(uuid,boolean,uuid,bigint) FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT USAGE ON TYPE decodex.process_generation_state, '
			|| 'decodex.process_generation_control_kind, '
			|| 'decodex.process_generation_isolation_kind, '
			|| 'decodex.process_generation_loss_reason, '
			|| 'decodex.process_generation_death_evidence_kind TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.prepare_process_generation_exact(uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.bind_process_generation_identity_exact(uuid,bigint,text,bigint,text,bigint,bigint) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_process_generation_ready_exact(uuid,bigint) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_process_generation_stopping_exact(uuid,bigint) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_process_generation_death_unknown_exact(uuid,bigint,decodex.process_generation_loss_reason) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.record_process_generation_death_exact(uuid,bigint,uuid,decodex.process_generation_death_evidence_kind,text,bigint,text,bigint,bigint,text) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.project_process_generations_after_supervisor_loss_exact() TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_process_generations_exact(uuid,boolean,uuid,bigint) TO %I',
			runtime_role
		);
	END IF;
END;
$$;
