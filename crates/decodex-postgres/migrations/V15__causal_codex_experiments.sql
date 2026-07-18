-- XY-1358 causal, positive-only Codex experiment authority.
-- This migration creates no account selection, continuation, wake, scheduler, dispatch, retry,
-- thread adoption, negative observation, or live-routing authority.

CREATE TYPE decodex.codex_experiment_state AS ENUM (
	'prepared', 'creation_possible', 'thread_bound'
);
CREATE TYPE decodex.codex_experiment_observation_kind AS ENUM (
	'thread_list_item', 'thread_read_item', 'turn_started_event',
	'turn_terminal_event', 'message_item'
);

CREATE TABLE decodex.codex_experiments (
	experiment_id uuid PRIMARY KEY,
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 0),
	routing_snapshot_id uuid NOT NULL REFERENCES decodex.routing_snapshots(snapshot_id),
	account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	account_revision bigint NOT NULL CHECK (account_revision > 0),
	role_profile_revision bigint NOT NULL CHECK (role_profile_revision > 0),
	build_id text NOT NULL CHECK (build_id <> '' AND octet_length(build_id) <= 512),
	repository_cwd text NOT NULL CHECK (repository_cwd <> '' AND octet_length(repository_cwd) <= 4096),
	thread_title text NOT NULL CHECK (thread_title <> '' AND octet_length(thread_title) <= 512),
	marker text GENERATED ALWAYS AS ('decodex.experiment.v1:' || experiment_id::text) STORED,
	revision bigint NOT NULL CHECK (revision > 0),
	state decodex.codex_experiment_state NOT NULL,
	prepared_at timestamptz NOT NULL,
	updated_at timestamptz NOT NULL,
	CONSTRAINT codex_experiments_marker_unique UNIQUE (marker),
	CONSTRAINT codex_experiments_marker_retained CHECK (position(marker IN thread_title) > 0),
	CONSTRAINT codex_experiments_time_order CHECK (updated_at >= prepared_at),
	CONSTRAINT codex_experiments_run_fk FOREIGN KEY (managed_run_id, managed_run_revision)
		REFERENCES decodex.managed_runs(managed_run_id, revision)
);

CREATE TABLE decodex.codex_experiment_revisions (
	experiment_id uuid NOT NULL REFERENCES decodex.codex_experiments(experiment_id),
	revision bigint NOT NULL CHECK (revision > 0),
	state decodex.codex_experiment_state NOT NULL,
	recorded_at timestamptz NOT NULL,
	PRIMARY KEY (experiment_id, revision)
);

CREATE TABLE decodex.codex_experiment_creation_attempts (
	experiment_id uuid PRIMARY KEY REFERENCES decodex.codex_experiments(experiment_id),
	attempt_id uuid NOT NULL UNIQUE,
	experiment_revision bigint NOT NULL CHECK (experiment_revision = 2),
	fenced_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_attempt_revision_fk
		FOREIGN KEY (experiment_id, experiment_revision)
		REFERENCES decodex.codex_experiment_revisions(experiment_id, revision)
);

CREATE TABLE decodex.codex_experiment_thread_bindings (
	experiment_id uuid PRIMARY KEY REFERENCES decodex.codex_experiments(experiment_id),
	experiment_revision bigint NOT NULL CHECK (experiment_revision = 3),
	attempt_id uuid NOT NULL UNIQUE REFERENCES decodex.codex_experiment_creation_attempts(attempt_id),
	thread_id text NOT NULL UNIQUE CHECK (
		thread_id <> '' AND octet_length(thread_id) <= 1024
		AND thread_id !~ '[[:cntrl:]]'
	),
	response_id text NOT NULL UNIQUE CHECK (
		response_id <> '' AND octet_length(response_id) <= 1024
		AND response_id !~ '[[:cntrl:]]'
	),
	bound_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_binding_scope_unique UNIQUE (experiment_id, thread_id),
	CONSTRAINT codex_experiment_binding_revision_fk
		FOREIGN KEY (experiment_id, experiment_revision)
		REFERENCES decodex.codex_experiment_revisions(experiment_id, revision)
);

CREATE TABLE decodex.codex_experiment_observations (
	observation_id uuid PRIMARY KEY,
	experiment_id uuid NOT NULL REFERENCES decodex.codex_experiments(experiment_id),
	experiment_revision bigint NOT NULL CHECK (experiment_revision = 3),
	thread_id text NOT NULL,
	marker text NOT NULL,
	kind decodex.codex_experiment_observation_kind NOT NULL,
	source_id text NOT NULL CHECK (
		source_id <> '' AND octet_length(source_id) <= 1024
		AND source_id !~ '[[:cntrl:]]'
	),
	fact_digest text NOT NULL CHECK (fact_digest ~ '^[0-9a-f]{64}$'),
	observed_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_observation_revision_fk
		FOREIGN KEY (experiment_id, experiment_revision)
		REFERENCES decodex.codex_experiment_revisions(experiment_id, revision),
	CONSTRAINT codex_experiment_observation_thread_fk
		FOREIGN KEY (experiment_id, thread_id)
		REFERENCES decodex.codex_experiment_thread_bindings(experiment_id, thread_id),
	CONSTRAINT codex_experiment_observation_exact_unique
		UNIQUE (experiment_id, kind, source_id, fact_digest)
);

CREATE FUNCTION decodex.codex_experiment_marker(p_experiment_id uuid)
RETURNS text LANGUAGE sql IMMUTABLE STRICT
SET search_path = pg_catalog, decodex AS $$
	SELECT 'decodex.experiment.v1:' || p_experiment_id::text
$$;

CREATE FUNCTION decodex.forbid_codex_experiment_history_mutation()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
BEGIN
	RAISE EXCEPTION 'V15 Codex experiment history is immutable'
		USING ERRCODE='55000', CONSTRAINT='codex_experiment_history_immutable';
END
$$;

DO $$
DECLARE relation_name text;
BEGIN
	FOREACH relation_name IN ARRAY ARRAY[
		'codex_experiment_revisions', 'codex_experiment_creation_attempts',
		'codex_experiment_thread_bindings', 'codex_experiment_observations'
	] LOOP
		EXECUTE pg_catalog.format(
			'CREATE TRIGGER %I_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.%I '
			|| 'FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_codex_experiment_history_mutation()',
			relation_name, relation_name);
	END LOOP;
END
$$;

CREATE FUNCTION decodex.enforce_codex_experiment_command_owner()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE owner_name name;
BEGIN
	SELECT role.rolname INTO owner_name
	FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner
	WHERE class.oid=TG_RELID;
	IF current_user::name <> owner_name THEN
		RAISE EXCEPTION 'V15 Codex experiment state is writable only by its command owner'
			USING ERRCODE='42501', CONSTRAINT='codex_experiment_command_owner';
	END IF;
	IF TG_OP='DELETE' OR TG_OP='TRUNCATE' THEN
		RAISE EXCEPTION 'V15 Codex experiment state is retained'
			USING ERRCODE='55000', CONSTRAINT='codex_experiment_history_retained';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER codex_experiments_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.codex_experiments
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_codex_experiment_command_owner();

CREATE FUNCTION decodex.complete_exact_codex_experiment_rejection(
	p_protocol text, p_idempotency_key text, p_operation text, p_code text
) RETURNS bytea LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	core := pg_catalog.jsonb_build_object('operation',p_operation,'rejection',p_code);
	effect := core || pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
			public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','stable_domain_rejection','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_rejected',
		outcome_class='stable_domain_rejection',effect_envelope=effect,
		response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.reserve_exact_codex_experiment_command(
	p_protocol text, p_idempotency_key text, p_request jsonb
) RETURNS bytea LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
DECLARE stored record;
BEGIN
	IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
		RAISE EXCEPTION 'exact commands require READ COMMITTED' USING ERRCODE='40001';
	END IF;
	INSERT INTO decodex.exact_command_receipts(
		protocol_version,idempotency_key,request_envelope,request_digest,receipt_state
	) VALUES (p_protocol,p_idempotency_key,p_request,
		public.digest(pg_catalog.convert_to(p_request::text,'UTF8'),'sha256'),'executing')
	ON CONFLICT DO NOTHING;
	SELECT request_envelope,response_bytes,receipt_state INTO stored
	FROM decodex.exact_command_receipts
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key FOR UPDATE;
	IF stored.request_envelope <> p_request THEN
		RAISE EXCEPTION 'idempotency key reused for another Codex experiment command'
			USING ERRCODE='DX001';
	END IF;
	IF stored.receipt_state <> 'executing' THEN RETURN stored.response_bytes; END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.prepare_codex_experiment_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_managed_run_id uuid, p_managed_run_revision bigint, p_routing_snapshot_id uuid,
	p_account_id uuid, p_account_revision bigint, p_role_profile_revision bigint,
	p_build_id text, p_repository_cwd text, p_thread_title text
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; prepared timestamptz; marker_value text;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','prepare_codex_experiment',
		'protocol',p_protocol,'experiment_id',p_experiment_id,
		'managed_run_id',p_managed_run_id,'managed_run_revision',p_managed_run_revision,
		'routing_snapshot_id',p_routing_snapshot_id,'account_id',p_account_id,
		'account_revision',p_account_revision,'role_profile_revision',p_role_profile_revision,
		'build_id',p_build_id,'repository_cwd',p_repository_cwd,'thread_title',p_thread_title);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	marker_value:=decodex.codex_experiment_marker(p_experiment_id);
	IF p_experiment_id IS NULL OR p_managed_run_id IS NULL OR p_routing_snapshot_id IS NULL
		OR p_account_id IS NULL OR p_managed_run_revision<=0 OR p_account_revision<=0
		OR p_role_profile_revision<=0 OR p_build_id IS NULL OR p_build_id=''
		OR p_repository_cwd IS NULL OR p_repository_cwd=''
		OR p_thread_title IS NULL OR position(marker_value IN p_thread_title)=0 THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'prepare_codex_experiment','invalid_identity');
	END IF;
	PERFORM 1 FROM decodex.routing_snapshots AS snapshot
	JOIN decodex.routing_snapshot_members AS member USING (snapshot_id)
	JOIN decodex.managed_runs AS run ON run.managed_run_id=snapshot.managed_run_id
	WHERE snapshot.snapshot_id=p_routing_snapshot_id
		AND snapshot.managed_run_id=p_managed_run_id
		AND snapshot.managed_run_revision=p_managed_run_revision
		AND run.revision=p_managed_run_revision
		AND member.account_id=p_account_id
		AND member.account_revision=p_account_revision
		AND snapshot.required_role_profile_revision=p_role_profile_revision
		AND snapshot.required_build_id=p_build_id FOR SHARE OF snapshot,member,run;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'prepare_codex_experiment','lineage_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiments WHERE experiment_id=p_experiment_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'prepare_codex_experiment','experiment_exists');
	END IF;
	prepared:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiments(experiment_id,managed_run_id,managed_run_revision,
		routing_snapshot_id,account_id,account_revision,role_profile_revision,build_id,
		repository_cwd,thread_title,revision,state,prepared_at,updated_at)
	VALUES(p_experiment_id,p_managed_run_id,p_managed_run_revision,p_routing_snapshot_id,
		p_account_id,p_account_revision,p_role_profile_revision,p_build_id,p_repository_cwd,
		p_thread_title,1,'prepared',prepared,prepared);
	INSERT INTO decodex.codex_experiment_revisions VALUES(p_experiment_id,1,'prepared',prepared);
	core:=pg_catalog.jsonb_build_object('operation','prepare_codex_experiment',
		'experiment_id',p_experiment_id,'revision',1,'state','prepared','marker',marker_value,
		'managed_run_id',p_managed_run_id,'managed_run_revision',p_managed_run_revision,
		'routing_snapshot_id',p_routing_snapshot_id,'account_id',p_account_id,
		'account_revision',p_account_revision,'role_profile_revision',p_role_profile_revision,
		'build_id',p_build_id,'repository_cwd',p_repository_cwd,'thread_title',p_thread_title,
		'prepared_at_micros',(pg_catalog.extract(epoch FROM prepared)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.mark_codex_experiment_creation_possible_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_attempt_id uuid
) RETURNS TABLE(response_bytes bytea, replayed boolean) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; fenced timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','mark_codex_experiment_creation_possible',
		'protocol',p_protocol,'experiment_id',p_experiment_id,
		'expected_revision',p_expected_revision,'attempt_id',p_attempt_id);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN QUERY SELECT replay,true; RETURN; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT * INTO experiment_row FROM decodex.codex_experiments
	WHERE experiment_id=p_experiment_id FOR UPDATE;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>1
		OR experiment_row.state<>'prepared' OR p_attempt_id IS NULL THEN
		response:=decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'mark_codex_experiment_creation_possible',
			'creation_not_authorized');
		RETURN QUERY SELECT response,false; RETURN;
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_creation_attempts
		WHERE attempt_id=p_attempt_id) THEN
		response:=decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'mark_codex_experiment_creation_possible',
			'attempt_identity_conflict');
		RETURN QUERY SELECT response,false; RETURN;
	END IF;
	fenced:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_revisions VALUES(
		p_experiment_id,2,'creation_possible',fenced);
	INSERT INTO decodex.codex_experiment_creation_attempts VALUES(
		p_experiment_id,p_attempt_id,2,fenced);
	UPDATE decodex.codex_experiments SET revision=2,state='creation_possible',updated_at=fenced
	WHERE experiment_id=p_experiment_id;
	core:=pg_catalog.jsonb_build_object('operation','mark_codex_experiment_creation_possible',
		'experiment_id',p_experiment_id,'revision',2,'state','creation_possible',
		'attempt_id',p_attempt_id,
		'fenced_at_micros',(pg_catalog.extract(epoch FROM fenced)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN QUERY SELECT response,false;
END
$$;

CREATE FUNCTION decodex.bind_codex_experiment_thread_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_attempt_id uuid, p_thread_id text, p_response_id text,
	p_response_title text, p_response_cwd text, p_response_marker text, p_ephemeral boolean
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; bound timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','bind_codex_experiment_thread',
		'protocol',p_protocol,'experiment_id',p_experiment_id,
		'expected_revision',p_expected_revision,'attempt_id',p_attempt_id,
		'thread_id',p_thread_id,'response_id',p_response_id,'response_title',p_response_title,
		'response_cwd',p_response_cwd,'response_marker',p_response_marker,'ephemeral',p_ephemeral);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT * INTO experiment_row FROM decodex.codex_experiments
	WHERE experiment_id=p_experiment_id FOR UPDATE;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>2
		OR experiment_row.state<>'creation_possible'
		OR NOT EXISTS (SELECT 1 FROM decodex.codex_experiment_creation_attempts
			WHERE experiment_id=p_experiment_id AND attempt_id=p_attempt_id)
		OR p_thread_id IS NULL OR p_thread_id='' OR p_response_id IS NULL OR p_response_id=''
		OR p_ephemeral IS DISTINCT FROM false
		OR p_response_title IS DISTINCT FROM experiment_row.thread_title
		OR p_response_cwd IS DISTINCT FROM experiment_row.repository_cwd
		OR p_response_marker IS DISTINCT FROM experiment_row.marker THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'bind_codex_experiment_thread','typed_response_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_thread_bindings
		WHERE thread_id=p_thread_id OR response_id=p_response_id OR attempt_id=p_attempt_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'bind_codex_experiment_thread','thread_identity_conflict');
	END IF;
	bound:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_revisions VALUES(p_experiment_id,3,'thread_bound',bound);
	INSERT INTO decodex.codex_experiment_thread_bindings VALUES(
		p_experiment_id,3,p_attempt_id,p_thread_id,p_response_id,bound);
	UPDATE decodex.codex_experiments SET revision=3,state='thread_bound',updated_at=bound
	WHERE experiment_id=p_experiment_id;
	core:=pg_catalog.jsonb_build_object('operation','bind_codex_experiment_thread',
		'experiment_id',p_experiment_id,'revision',3,'state','thread_bound',
		'attempt_id',p_attempt_id,'thread_id',p_thread_id,'response_id',p_response_id,
		'marker',experiment_row.marker,
		'bound_at_micros',(pg_catalog.extract(epoch FROM bound)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.record_codex_experiment_observation_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_observation_id uuid,
	p_kind decodex.codex_experiment_observation_kind, p_thread_id text,
	p_marker text, p_source_id text, p_fact_digest text
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; observed timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','record_codex_experiment_observation',
		'protocol',p_protocol,'experiment_id',p_experiment_id,
		'expected_revision',p_expected_revision,'observation_id',p_observation_id,
		'kind',p_kind,'thread_id',p_thread_id,'marker',p_marker,
		'source_id',p_source_id,'fact_digest',p_fact_digest);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT experiment.* INTO experiment_row FROM decodex.codex_experiments AS experiment
	JOIN decodex.codex_experiment_thread_bindings AS binding USING (experiment_id)
	WHERE experiment.experiment_id=p_experiment_id
		AND binding.thread_id=p_thread_id FOR SHARE OF experiment,binding;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>3
		OR experiment_row.state<>'thread_bound' OR p_observation_id IS NULL
		OR p_kind IS NULL OR p_marker IS DISTINCT FROM experiment_row.marker
		OR p_source_id IS NULL OR p_source_id='' OR p_fact_digest !~ '^[0-9a-f]{64}$' THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_codex_experiment_observation',
			'observation_lineage_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_observations
		WHERE observation_id=p_observation_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_codex_experiment_observation',
			'observation_identity_conflict');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_observations
		WHERE experiment_id=p_experiment_id AND kind=p_kind
			AND source_id=p_source_id AND fact_digest=p_fact_digest) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_codex_experiment_observation',
			'observation_fact_conflict');
	END IF;
	observed:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_observations(observation_id,experiment_id,
		experiment_revision,thread_id,marker,kind,source_id,fact_digest,observed_at)
	VALUES(p_observation_id,p_experiment_id,3,p_thread_id,p_marker,p_kind,
		p_source_id,p_fact_digest,observed);
	core:=pg_catalog.jsonb_build_object('operation','record_codex_experiment_observation',
		'experiment_id',p_experiment_id,'experiment_revision',3,
		'observation_id',p_observation_id,'kind',p_kind,'thread_id',p_thread_id,
		'marker',p_marker,'source_id',p_source_id,'fact_digest',p_fact_digest,
		'observed_at_micros',(pg_catalog.extract(epoch FROM observed)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON TABLE decodex.codex_experiments, decodex.codex_experiment_revisions,
	decodex.codex_experiment_creation_attempts, decodex.codex_experiment_thread_bindings,
	decodex.codex_experiment_observations FROM PUBLIC;
REVOKE ALL ON TYPE decodex.codex_experiment_state,
	decodex.codex_experiment_observation_kind FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;

-- Extend exactly the accepted V12 runtime command principals. Relations and helpers remain sealed.
DO $$
DECLARE runtime_role name;
BEGIN
	FOR runtime_role IN
		SELECT role.rolname
		FROM pg_catalog.pg_proc AS procedure
		JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=procedure.pronamespace
		CROSS JOIN LATERAL pg_catalog.aclexplode(procedure.proacl) AS privilege
		JOIN pg_catalog.pg_roles AS role ON role.oid=privilege.grantee
		WHERE namespace.nspname='decodex'
			AND procedure.proname='apply_managed_run_safety_input_exact'
			AND privilege.privilege_type='EXECUTE' AND NOT privilege.is_grantable
	LOOP
		EXECUTE pg_catalog.format(
			'GRANT USAGE ON TYPE decodex.codex_experiment_observation_kind TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.prepare_codex_experiment_exact(text,text,uuid,uuid,bigint,uuid,uuid,bigint,bigint,text,text,text) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_codex_experiment_creation_possible_exact(text,text,uuid,bigint,uuid) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.bind_codex_experiment_thread_exact(text,text,uuid,bigint,uuid,text,text,text,text,text,boolean) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.record_codex_experiment_observation_exact(text,text,uuid,bigint,uuid,decodex.codex_experiment_observation_kind,text,text,text,text) TO %I',runtime_role);
	END LOOP;
END
$$;
