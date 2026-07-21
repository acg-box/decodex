-- XY-1367 retained-title causal authority for the pinned two-effect Codex protocol.
-- V15 remains immutable history. This bridge never authorizes creation retry, search, adoption,
-- title-set retry, turn submission, routing, scheduling, or production dispatch.

CREATE TABLE decodex.codex_experiment_start_receipts (
	experiment_id uuid PRIMARY KEY REFERENCES decodex.codex_experiments(experiment_id),
	attempt_id uuid NOT NULL UNIQUE REFERENCES decodex.codex_experiment_creation_attempts(attempt_id),
	experiment_revision bigint NOT NULL CHECK (experiment_revision = 3),
	thread_id text NOT NULL UNIQUE CHECK (
		thread_id <> '' AND octet_length(thread_id) <= 1024
		AND thread_id !~ '[[:cntrl:]]'
	),
	start_request_id bigint NOT NULL CHECK (start_request_id > 0),
	start_request_digest text NOT NULL CHECK (start_request_digest ~ '^[0-9a-f]{64}$'),
	request_cwd text NOT NULL CHECK (
		request_cwd <> '' AND octet_length(request_cwd) <= 4096
		AND request_cwd !~ '[[:cntrl:]]'
	),
	request_marker text NOT NULL CHECK (
		request_marker <> '' AND octet_length(request_marker) <= 256
		AND request_marker !~ '[[:cntrl:]]'
	),
	request_ephemeral boolean NOT NULL CHECK (request_ephemeral = false),
	start_response_id bigint NOT NULL CHECK (start_response_id > 0),
	start_response_digest text NOT NULL CHECK (start_response_digest ~ '^[0-9a-f]{64}$'),
	response_cwd text NOT NULL CHECK (
		response_cwd <> '' AND octet_length(response_cwd) <= 4096
		AND response_cwd !~ '[[:cntrl:]]'
	),
	response_marker text NOT NULL CHECK (
		response_marker <> '' AND octet_length(response_marker) <= 256
		AND response_marker !~ '[[:cntrl:]]'
	),
	response_ephemeral boolean NOT NULL CHECK (response_ephemeral = false),
	returned_name text CHECK (returned_name IS NULL),
	bound_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_start_scope_unique UNIQUE (experiment_id, thread_id),
	CONSTRAINT codex_experiment_start_receipt_revision_fk
		FOREIGN KEY (experiment_id, experiment_revision)
		REFERENCES decodex.codex_experiment_revisions(experiment_id, revision),
	CONSTRAINT codex_experiment_start_receipt_binding_fk
		FOREIGN KEY (experiment_id, thread_id)
		REFERENCES decodex.codex_experiment_thread_bindings(experiment_id, thread_id),
	CONSTRAINT codex_experiment_start_response_identity
		CHECK (start_response_id = start_request_id)
);

CREATE TABLE decodex.codex_experiment_title_set_attempts (
	experiment_id uuid PRIMARY KEY REFERENCES decodex.codex_experiments(experiment_id),
	title_attempt_id uuid NOT NULL UNIQUE,
	thread_id text NOT NULL,
	request_id bigint NOT NULL CHECK (request_id > 0),
	request_digest text NOT NULL CHECK (request_digest ~ '^[0-9a-f]{64}$'),
	requested_title text NOT NULL CHECK (
		requested_title <> '' AND octet_length(requested_title) <= 512
		AND requested_title !~ '[[:cntrl:]]'
	),
	fenced_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_title_attempt_scope_unique UNIQUE (experiment_id, thread_id),
	CONSTRAINT codex_experiment_title_attempt_start_fk
		FOREIGN KEY (experiment_id, thread_id)
		REFERENCES decodex.codex_experiment_start_receipts(experiment_id, thread_id)
);

CREATE TABLE decodex.codex_experiment_retained_title_attestations (
	attestation_id uuid PRIMARY KEY,
	experiment_id uuid NOT NULL UNIQUE REFERENCES decodex.codex_experiments(experiment_id),
	title_attempt_id uuid NOT NULL UNIQUE
		REFERENCES decodex.codex_experiment_title_set_attempts(title_attempt_id),
	thread_id text NOT NULL,
	read_request_id bigint NOT NULL CHECK (read_request_id > 0),
	read_request_digest text NOT NULL CHECK (read_request_digest ~ '^[0-9a-f]{64}$'),
	read_response_id bigint NOT NULL CHECK (read_response_id > 0),
	read_response_digest text NOT NULL CHECK (read_response_digest ~ '^[0-9a-f]{64}$'),
	returned_title text NOT NULL CHECK (
		returned_title <> '' AND octet_length(returned_title) <= 512
		AND returned_title !~ '[[:cntrl:]]'
	),
	returned_cwd text NOT NULL CHECK (
		returned_cwd <> '' AND octet_length(returned_cwd) <= 4096
		AND returned_cwd !~ '[[:cntrl:]]'
	),
	returned_marker text NOT NULL CHECK (
		returned_marker <> '' AND octet_length(returned_marker) <= 256
		AND returned_marker !~ '[[:cntrl:]]'
	),
	attested_at timestamptz NOT NULL,
	CONSTRAINT codex_experiment_retained_title_attempt_fk
		FOREIGN KEY (experiment_id, thread_id)
		REFERENCES decodex.codex_experiment_title_set_attempts(experiment_id, thread_id),
	CONSTRAINT codex_experiment_read_response_identity
		CHECK (read_response_id = read_request_id),
	CONSTRAINT codex_experiment_retained_title_scope_unique
		UNIQUE (attestation_id, experiment_id, thread_id)
);

CREATE TABLE decodex.codex_experiment_attested_observations (
	observation_id uuid PRIMARY KEY
		REFERENCES decodex.codex_experiment_observations(observation_id),
	attestation_id uuid NOT NULL
		REFERENCES decodex.codex_experiment_retained_title_attestations(attestation_id),
	experiment_id uuid NOT NULL,
	thread_id text NOT NULL,
	CONSTRAINT codex_experiment_attested_observation_scope_unique
		UNIQUE (observation_id, attestation_id, experiment_id, thread_id),
	CONSTRAINT codex_experiment_attested_observation_attestation_fk
		FOREIGN KEY (attestation_id, experiment_id, thread_id)
		REFERENCES decodex.codex_experiment_retained_title_attestations(
			attestation_id, experiment_id, thread_id
		)
);

DO $$
DECLARE relation_name text;
BEGIN
	FOREACH relation_name IN ARRAY ARRAY[
		'codex_experiment_start_receipts',
		'codex_experiment_title_set_attempts',
		'codex_experiment_retained_title_attestations',
		'codex_experiment_attested_observations'
	] LOOP
		EXECUTE pg_catalog.format(
			'CREATE TRIGGER %I_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.%I '
			|| 'FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_codex_experiment_history_mutation()',
			relation_name, relation_name);
	END LOOP;
END
$$;

CREATE FUNCTION decodex.bind_codex_experiment_start_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_attempt_id uuid, p_thread_id text,
	p_start_request_id bigint, p_start_request_digest text,
	p_request_cwd text, p_request_marker text, p_request_ephemeral boolean,
	p_start_response_id bigint, p_start_response_digest text,
	p_response_cwd text, p_response_marker text, p_response_ephemeral boolean,
	p_returned_name text
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; bound timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','bind_codex_experiment_start',
		'protocol',p_protocol,'experiment_id',p_experiment_id,
		'expected_revision',p_expected_revision,'attempt_id',p_attempt_id,
		'thread_id',p_thread_id,'start_request_id',p_start_request_id,
		'start_request_digest',p_start_request_digest,'request_cwd',p_request_cwd,
		'request_marker',p_request_marker,'request_ephemeral',p_request_ephemeral,
		'start_response_id',p_start_response_id,
		'start_response_digest',p_start_response_digest,'response_cwd',p_response_cwd,
		'response_marker',p_response_marker,'response_ephemeral',p_response_ephemeral,
		'returned_name',p_returned_name);
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
		OR p_thread_id IS NULL OR p_thread_id='' OR octet_length(p_thread_id)>1024
		OR p_thread_id~'[[:cntrl:]]'
		OR p_start_request_id IS NULL OR p_start_request_id<=0
		OR p_start_response_id IS DISTINCT FROM p_start_request_id
		OR p_start_request_digest IS NULL OR p_start_request_digest!~'^[0-9a-f]{64}$'
		OR p_request_cwd IS DISTINCT FROM experiment_row.repository_cwd
		OR p_request_marker IS DISTINCT FROM experiment_row.marker
		OR p_request_ephemeral IS DISTINCT FROM false
		OR p_start_response_digest IS NULL OR p_start_response_digest!~'^[0-9a-f]{64}$'
		OR p_response_cwd IS DISTINCT FROM experiment_row.repository_cwd
		OR p_response_marker IS DISTINCT FROM experiment_row.marker
		OR p_response_ephemeral IS DISTINCT FROM false
		OR p_returned_name IS NOT NULL THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'bind_codex_experiment_start',
			'start_response_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_thread_bindings
		WHERE thread_id=p_thread_id
			OR response_id=p_attempt_id::text||':'||p_start_response_id::text
			OR attempt_id=p_attempt_id)
		OR EXISTS (SELECT 1 FROM decodex.codex_experiment_start_receipts
			WHERE experiment_id=p_experiment_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'bind_codex_experiment_start',
			'start_identity_conflict');
	END IF;
	bound:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_revisions VALUES(p_experiment_id,3,'thread_bound',bound);
	INSERT INTO decodex.codex_experiment_thread_bindings VALUES(
		p_experiment_id,3,p_attempt_id,p_thread_id,
		p_attempt_id::text||':'||p_start_response_id::text,bound);
	INSERT INTO decodex.codex_experiment_start_receipts(
		experiment_id,attempt_id,experiment_revision,thread_id,
		start_request_id,start_request_digest,request_cwd,request_marker,request_ephemeral,
		start_response_id,start_response_digest,
		response_cwd,response_marker,response_ephemeral,returned_name,bound_at
	) VALUES (
		p_experiment_id,p_attempt_id,3,p_thread_id,
		p_start_request_id,p_start_request_digest,p_request_cwd,p_request_marker,p_request_ephemeral,
		p_start_response_id,p_start_response_digest,
		p_response_cwd,p_response_marker,p_response_ephemeral,p_returned_name,bound
	);
	UPDATE decodex.codex_experiments SET revision=3,state='thread_bound',updated_at=bound
	WHERE experiment_id=p_experiment_id;
	core:=pg_catalog.jsonb_build_object('operation','bind_codex_experiment_start',
		'experiment_id',p_experiment_id,'revision',3,'state','thread_bound',
		'attempt_id',p_attempt_id,'thread_id',p_thread_id,
		'start_request_id',p_start_request_id,'start_request_digest',p_start_request_digest,
		'request_cwd',p_request_cwd,'request_marker',p_request_marker,
		'request_ephemeral',p_request_ephemeral,
		'start_response_id',p_start_response_id,'start_response_digest',p_start_response_digest,
		'response_cwd',p_response_cwd,'response_marker',p_response_marker,
		'response_ephemeral',p_response_ephemeral,'returned_name',p_returned_name,
		'bound_at_micros',(extract(epoch FROM bound)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.read_codex_experiment_start_exact(
	p_experiment_id uuid, p_attempt_id uuid
) RETURNS TABLE(
	experiment_id uuid, attempt_id uuid, experiment_revision bigint, thread_id text,
	start_request_id bigint, start_request_digest text,
	request_cwd text, request_marker text, request_ephemeral boolean,
	start_response_id bigint, start_response_digest text,
	response_cwd text, response_marker text, response_ephemeral boolean,
	returned_name text, bound_at_micros bigint
) LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
	SELECT receipt.experiment_id,receipt.attempt_id,receipt.experiment_revision,
		receipt.thread_id,receipt.start_request_id,receipt.start_request_digest,
		receipt.request_cwd,receipt.request_marker,receipt.request_ephemeral,
		receipt.start_response_id,receipt.start_response_digest,
		receipt.response_cwd,receipt.response_marker,receipt.response_ephemeral,
		receipt.returned_name,
		(extract(epoch FROM receipt.bound_at)*1000000)::bigint
	FROM decodex.codex_experiment_start_receipts AS receipt
	WHERE receipt.experiment_id=p_experiment_id AND receipt.attempt_id=p_attempt_id
$$;

CREATE FUNCTION decodex.mark_codex_experiment_title_set_possible_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_title_attempt_id uuid, p_thread_id text,
	p_request_id bigint, p_request_digest text, p_requested_title text
) RETURNS TABLE(response_bytes bytea, replayed boolean) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; fenced timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','mark_codex_experiment_title_set_possible','protocol',p_protocol,
		'experiment_id',p_experiment_id,'expected_revision',p_expected_revision,
		'title_attempt_id',p_title_attempt_id,'thread_id',p_thread_id,
		'request_id',p_request_id,'request_digest',p_request_digest,
		'requested_title',p_requested_title);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN QUERY SELECT replay,true; RETURN; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT experiment.*,receipt.start_request_id AS bound_start_request_id
	INTO experiment_row FROM decodex.codex_experiments AS experiment
	JOIN decodex.codex_experiment_start_receipts AS receipt USING (experiment_id)
	WHERE experiment.experiment_id=p_experiment_id AND receipt.thread_id=p_thread_id
	FOR SHARE OF experiment,receipt;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>3
		OR experiment_row.state<>'thread_bound' OR p_title_attempt_id IS NULL
		OR p_request_id IS NULL OR p_request_id<=0
		OR p_request_id<=experiment_row.bound_start_request_id
		OR p_request_digest IS NULL OR p_request_digest!~'^[0-9a-f]{64}$'
		OR p_requested_title IS DISTINCT FROM experiment_row.thread_title THEN
		response:=decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'mark_codex_experiment_title_set_possible',
			'title_set_not_authorized');
		RETURN QUERY SELECT response,false; RETURN;
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_title_set_attempts
		WHERE experiment_id=p_experiment_id OR title_attempt_id=p_title_attempt_id) THEN
		response:=decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'mark_codex_experiment_title_set_possible',
			'title_attempt_identity_conflict');
		RETURN QUERY SELECT response,false; RETURN;
	END IF;
	fenced:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_title_set_attempts(
		experiment_id,title_attempt_id,thread_id,request_id,request_digest,requested_title,fenced_at
	) VALUES (
		p_experiment_id,p_title_attempt_id,p_thread_id,p_request_id,p_request_digest,
		p_requested_title,fenced
	);
	core:=pg_catalog.jsonb_build_object(
		'operation','mark_codex_experiment_title_set_possible',
		'experiment_id',p_experiment_id,'experiment_revision',3,
		'title_attempt_id',p_title_attempt_id,'thread_id',p_thread_id,
		'request_id',p_request_id,'request_digest',p_request_digest,
		'requested_title',p_requested_title,
		'fenced_at_micros',(extract(epoch FROM fenced)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN QUERY SELECT response,false;
END
$$;

CREATE FUNCTION decodex.attest_codex_experiment_retained_title_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_attestation_id uuid, p_title_attempt_id uuid,
	p_thread_id text, p_read_request_id bigint, p_read_request_digest text,
	p_read_response_id bigint, p_read_response_digest text,
	p_returned_title text, p_returned_cwd text, p_returned_marker text
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; attested timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','attest_codex_experiment_retained_title','protocol',p_protocol,
		'experiment_id',p_experiment_id,'expected_revision',p_expected_revision,
		'attestation_id',p_attestation_id,'title_attempt_id',p_title_attempt_id,
		'thread_id',p_thread_id,'read_request_id',p_read_request_id,
		'read_request_digest',p_read_request_digest,'read_response_id',p_read_response_id,
		'read_response_digest',p_read_response_digest,'returned_title',p_returned_title,
		'returned_cwd',p_returned_cwd,'returned_marker',p_returned_marker);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT experiment.*,title_attempt.request_id AS title_request_id
	INTO experiment_row FROM decodex.codex_experiments AS experiment
	JOIN decodex.codex_experiment_start_receipts AS start_receipt USING (experiment_id)
	JOIN decodex.codex_experiment_title_set_attempts AS title_attempt USING (experiment_id,thread_id)
	WHERE experiment.experiment_id=p_experiment_id
		AND title_attempt.title_attempt_id=p_title_attempt_id
		AND start_receipt.thread_id=p_thread_id
	FOR SHARE OF experiment,start_receipt,title_attempt;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>3
		OR experiment_row.state<>'thread_bound' OR p_attestation_id IS NULL
		OR p_read_request_id IS NULL OR p_read_request_id<=0
		OR p_read_request_id<=experiment_row.title_request_id
		OR p_read_response_id IS DISTINCT FROM p_read_request_id
		OR p_read_request_digest IS NULL OR p_read_request_digest!~'^[0-9a-f]{64}$'
		OR p_read_response_digest IS NULL OR p_read_response_digest!~'^[0-9a-f]{64}$'
		OR p_returned_title IS DISTINCT FROM experiment_row.thread_title
		OR p_returned_cwd IS DISTINCT FROM experiment_row.repository_cwd
		OR p_returned_marker IS DISTINCT FROM experiment_row.marker THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'attest_codex_experiment_retained_title',
			'retained_title_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_retained_title_attestations
		WHERE attestation_id=p_attestation_id OR experiment_id=p_experiment_id
			OR title_attempt_id=p_title_attempt_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'attest_codex_experiment_retained_title',
			'attestation_identity_conflict');
	END IF;
	attested:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_retained_title_attestations(
		attestation_id,experiment_id,title_attempt_id,thread_id,
		read_request_id,read_request_digest,read_response_id,read_response_digest,
		returned_title,returned_cwd,returned_marker,attested_at
	) VALUES (
		p_attestation_id,p_experiment_id,p_title_attempt_id,p_thread_id,
		p_read_request_id,p_read_request_digest,p_read_response_id,p_read_response_digest,
		p_returned_title,p_returned_cwd,p_returned_marker,attested
	);
	core:=pg_catalog.jsonb_build_object(
		'operation','attest_codex_experiment_retained_title',
		'experiment_id',p_experiment_id,'experiment_revision',3,
		'attestation_id',p_attestation_id,'title_attempt_id',p_title_attempt_id,
		'thread_id',p_thread_id,'read_request_id',p_read_request_id,
		'read_request_digest',p_read_request_digest,'read_response_id',p_read_response_id,
		'read_response_digest',p_read_response_digest,'retained_title',p_returned_title,
		'returned_cwd',p_returned_cwd,'marker',p_returned_marker,
		'attested_at_micros',(extract(epoch FROM attested)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.record_attested_codex_experiment_observation_exact(
	p_protocol text, p_idempotency_key text, p_experiment_id uuid,
	p_expected_revision bigint, p_attestation_id uuid, p_observation_id uuid,
	p_kind decodex.codex_experiment_observation_kind, p_thread_id text,
	p_marker text, p_source_id text, p_fact_digest text
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; observed timestamptz; experiment_row record;
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','record_attested_codex_experiment_observation','protocol',p_protocol,
		'experiment_id',p_experiment_id,'expected_revision',p_expected_revision,
		'attestation_id',p_attestation_id,'observation_id',p_observation_id,
		'kind',p_kind,'thread_id',p_thread_id,'marker',p_marker,
		'source_id',p_source_id,'fact_digest',p_fact_digest);
	replay:=decodex.reserve_exact_codex_experiment_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1358);
	PERFORM pg_catalog.pg_advisory_xact_lock(1358,pg_catalog.hashtext(p_experiment_id::text));
	SELECT experiment.* INTO experiment_row FROM decodex.codex_experiments AS experiment
	JOIN decodex.codex_experiment_retained_title_attestations AS attestation
		USING (experiment_id)
	WHERE experiment.experiment_id=p_experiment_id
		AND attestation.attestation_id=p_attestation_id
		AND attestation.thread_id=p_thread_id
	FOR SHARE OF experiment,attestation;
	IF NOT FOUND OR experiment_row.revision<>p_expected_revision OR p_expected_revision<>3
		OR experiment_row.state<>'thread_bound' OR p_observation_id IS NULL
		OR p_kind IS NULL OR p_marker IS DISTINCT FROM experiment_row.marker
		OR p_source_id IS NULL OR p_source_id='' OR octet_length(p_source_id)>1024
		OR p_source_id~'[[:cntrl:]]' OR p_fact_digest IS NULL
		OR p_fact_digest!~'^[0-9a-f]{64}$' THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_attested_codex_experiment_observation',
			'attested_observation_lineage_mismatch');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_observations
		WHERE observation_id=p_observation_id) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_attested_codex_experiment_observation',
			'observation_identity_conflict');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.codex_experiment_observations
		WHERE experiment_id=p_experiment_id AND kind=p_kind
			AND source_id=p_source_id AND fact_digest=p_fact_digest) THEN
		RETURN decodex.complete_exact_codex_experiment_rejection(
			p_protocol,p_idempotency_key,'record_attested_codex_experiment_observation',
			'observation_fact_conflict');
	END IF;
	observed:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.codex_experiment_observations(
		observation_id,experiment_id,experiment_revision,thread_id,marker,
		kind,source_id,fact_digest,observed_at
	) VALUES (
		p_observation_id,p_experiment_id,3,p_thread_id,p_marker,
		p_kind,p_source_id,p_fact_digest,observed
	);
	INSERT INTO decodex.codex_experiment_attested_observations(
		observation_id,attestation_id,experiment_id,thread_id
	) VALUES (p_observation_id,p_attestation_id,p_experiment_id,p_thread_id);
	core:=pg_catalog.jsonb_build_object(
		'operation','record_attested_codex_experiment_observation',
		'experiment_id',p_experiment_id,'experiment_revision',3,
		'attestation_id',p_attestation_id,'observation_id',p_observation_id,
		'kind',p_kind,'thread_id',p_thread_id,'marker',p_marker,
		'source_id',p_source_id,'fact_digest',p_fact_digest,
		'observed_at_micros',(extract(epoch FROM observed)*1000000)::bigint);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

-- A same-thread V17 plan can consume only a V22 observation linked to one exact retained-title
-- attestation. Historical V15 observations remain immutable but no longer satisfy this gate.
CREATE OR REPLACE FUNCTION decodex.enforce_continuation_plan_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.routing_decisions AS decision
		JOIN decodex.routing_snapshots AS snapshot ON snapshot.snapshot_id=decision.snapshot_id
		JOIN decodex.managed_runs AS run
			ON (run.managed_run_id,run.revision)=(decision.managed_run_id,decision.managed_run_revision)
		JOIN decodex.runtime_sessions AS session
			ON (session.runtime_session_id,session.revision)=
				(snapshot.runtime_session_id,snapshot.runtime_session_revision)
		WHERE decision.decision_id=NEW.routing_decision_id AND decision.kind='selected'
			AND decision.selected_account_id=NEW.selected_account_id
			AND (decision.managed_run_id,decision.managed_run_revision)=
				(NEW.managed_run_id,NEW.managed_run_revision)
			AND (snapshot.runtime_session_id,snapshot.runtime_session_revision)=
				(NEW.source_runtime_session_id,NEW.source_runtime_session_revision)
			AND session.conversation_id=NEW.conversation_id
	) THEN
		RAISE EXCEPTION 'continuation plan has forged V16 lineage'
			USING ERRCODE='23514', CONSTRAINT='continuation_plan_complete';
	END IF;
	IF NEW.kind='same_thread' AND NOT EXISTS (
		SELECT 1
		FROM decodex.routing_decisions AS decision
		JOIN decodex.routing_snapshots AS snapshot ON snapshot.snapshot_id=decision.snapshot_id
		JOIN decodex.routing_snapshot_members AS member
			ON member.snapshot_id=snapshot.snapshot_id AND member.account_id=decision.selected_account_id
		JOIN decodex.managed_runs AS run ON run.managed_run_id=NEW.managed_run_id
		JOIN decodex.runtime_sessions AS source_session
			ON source_session.runtime_session_id=NEW.source_runtime_session_id
		JOIN decodex.routing_compatibility_evidence AS evidence
			ON evidence.evidence_id=member.evidence_id
		JOIN decodex.codex_experiments AS experiment
			ON experiment.experiment_id=NEW.codex_experiment_id
		JOIN decodex.codex_experiment_start_receipts AS start_receipt
			ON start_receipt.experiment_id=experiment.experiment_id
		JOIN decodex.codex_experiment_thread_bindings AS binding
			ON binding.experiment_id=experiment.experiment_id
		JOIN decodex.codex_experiment_observations AS observation
			ON observation.observation_id=NEW.codex_observation_id
		JOIN decodex.codex_experiment_attested_observations AS attested_observation
			ON attested_observation.observation_id=observation.observation_id
		JOIN decodex.codex_experiment_retained_title_attestations AS attestation
			ON attestation.attestation_id=attested_observation.attestation_id
		WHERE decision.decision_id=NEW.routing_decision_id
			AND (evidence.evidence_id,evidence.evidence_revision,evidence.schema_fingerprint)=
				(NEW.routing_evidence_id,NEW.routing_evidence_revision,NEW.schema_fingerprint)
			AND (experiment.managed_run_id,experiment.managed_run_revision,
				experiment.routing_snapshot_id,experiment.account_id,experiment.account_revision,
				experiment.role_profile_revision,experiment.build_id,experiment.revision,experiment.state)=
				(NEW.managed_run_id,NEW.managed_run_revision,snapshot.snapshot_id,
				 decision.selected_account_id,member.account_revision,
				 snapshot.required_role_profile_revision,snapshot.required_build_id,3,'thread_bound')
			AND binding.thread_id=NEW.codex_thread_id::text
			AND start_receipt.thread_id=binding.thread_id
			AND start_receipt.returned_name IS NULL
			AND observation.experiment_id=experiment.experiment_id
			AND observation.experiment_revision=3 AND observation.thread_id=binding.thread_id
			AND observation.kind='thread_read_item'
			AND (attested_observation.experiment_id,attested_observation.thread_id)=
				(experiment.experiment_id,binding.thread_id)
			AND (attestation.experiment_id,attestation.thread_id)=
				(experiment.experiment_id,binding.thread_id)
			AND (attestation.returned_title,attestation.returned_cwd,
				attestation.returned_marker)=
				(experiment.thread_title,experiment.repository_cwd,experiment.marker)
			AND evidence.account_id=NEW.selected_account_id
			AND evidence.account_revision=member.account_revision
			AND evidence.role=snapshot.required_role
			AND evidence.role_profile_revision=snapshot.required_role_profile_revision
			AND evidence.build_id=snapshot.required_build_id
			AND evidence.process_account_id=NEW.selected_account_id
			AND member.disposition='included' AND pg_catalog.cardinality(member.blockers)=0
			AND source_session.state='active' AND NOT run.diverged
			AND evidence.ingested_at<=NEW.planned_at
			AND NEW.planned_at-evidence.ingested_at<=INTERVAL '300 seconds'
			AND experiment.updated_at<=NEW.planned_at
			AND NEW.planned_at-experiment.updated_at<=INTERVAL '300 seconds'
			AND attestation.attested_at<=observation.observed_at
			AND observation.observed_at<=NEW.planned_at
			AND NEW.planned_at-observation.observed_at<=INTERVAL '300 seconds'
			AND 4=(SELECT pg_catalog.count(DISTINCT capability.capability)
				FROM decodex.routing_capability_evidence AS capability
				WHERE capability.evidence_id=evidence.evidence_id
					AND capability.capability IN
						('initialize','account_read','thread_read','paginated_history')
					AND capability.state='supported')
	) THEN
		RAISE EXCEPTION 'same-thread plan lacks exact retained-title attestation'
			USING ERRCODE='23514', CONSTRAINT='continuation_plan_complete';
	ELSIF NEW.kind='context_pack_fallback' AND NOT EXISTS (
		SELECT 1 FROM decodex.context_packs AS pack
		JOIN decodex.routing_decisions AS decision
			ON decision.decision_id=NEW.routing_decision_id
		JOIN decodex.routing_snapshot_members AS member
			ON member.snapshot_id=decision.snapshot_id AND member.account_id=NEW.selected_account_id
		JOIN decodex.runtime_sessions AS source_session
			ON source_session.runtime_session_id=NEW.source_runtime_session_id
		JOIN decodex.runtime_sessions AS session
			ON session.runtime_session_id=NEW.fallback_runtime_session_id
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id=session.account_snapshot_id
		WHERE pack.context_pack_id=NEW.fallback_context_pack_id
			AND pack.conversation_id=NEW.conversation_id
			AND session.conversation_id=NEW.conversation_id
			AND session.codex_thread_id IS NULL AND session.state='starting' AND session.revision=1
			AND account.source_account_id=NEW.selected_account_id
			AND (account.source_revision,account.display_label,account.observed_state)=
				(member.account_revision,member.display_label,member.account_state)
			AND session.profile_snapshot_id=source_session.profile_snapshot_id
	) THEN
		RAISE EXCEPTION 'fallback plan has incomplete Context Pack or RuntimeSession linkage'
			USING ERRCODE='23514', CONSTRAINT='continuation_plan_complete';
	END IF;
	RETURN NULL;
END
$$;

REVOKE ALL ON TABLE decodex.codex_experiment_start_receipts,
	decodex.codex_experiment_title_set_attempts,
	decodex.codex_experiment_retained_title_attestations,
	decodex.codex_experiment_attested_observations FROM PUBLIC;
REVOKE ALL ON FUNCTION
	decodex.bind_codex_experiment_start_exact(text,text,uuid,bigint,uuid,text,bigint,text,text,text,boolean,bigint,text,text,text,boolean,text),
	decodex.read_codex_experiment_start_exact(uuid,uuid),
	decodex.mark_codex_experiment_title_set_possible_exact(text,text,uuid,bigint,uuid,text,bigint,text,text),
	decodex.attest_codex_experiment_retained_title_exact(text,text,uuid,bigint,uuid,uuid,text,bigint,text,bigint,text,text,text,text),
	decodex.record_attested_codex_experiment_observation_exact(text,text,uuid,bigint,uuid,uuid,decodex.codex_experiment_observation_kind,text,text,text,text)
	FROM PUBLIC;

-- Replace V15 runtime entrypoints with the V22 bridge. Existing tables, functions, rows, and
-- exact-command receipts remain immutable historical authority.
DO $$
DECLARE anchor_oid pg_catalog.oid;
DECLARE migration_role_oid pg_catalog.oid;
DECLARE owner_execute_count pg_catalog.int8;
DECLARE runtime_role_count pg_catalog.int8;
DECLARE invalid_execute_count pg_catalog.int8;
DECLARE runtime_role pg_catalog.name;
BEGIN
	SELECT role.oid INTO migration_role_oid FROM pg_catalog.pg_roles AS role
	WHERE role.rolname=current_user;
	anchor_oid:=pg_catalog.to_regprocedure(
		'decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)');
	IF anchor_oid IS NULL OR NOT EXISTS (
		SELECT 1 FROM pg_catalog.pg_proc AS procedure
		WHERE procedure.oid=anchor_oid AND procedure.proowner=migration_role_oid
	) THEN
		RAISE EXCEPTION 'V22 runtime principal anchor is missing or not migration-owned'
			USING ERRCODE='42501';
	END IF;
	SELECT
		pg_catalog.count(*) FILTER (WHERE privilege.grantee=migration_role_oid),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee<>migration_role_oid AND role.oid IS NOT NULL),
		pg_catalog.count(*) FILTER (WHERE privilege.grantee=0
			OR privilege.grantor<>migration_role_oid
			OR (privilege.grantee<>migration_role_oid
				AND (privilege.is_grantable OR role.oid IS NULL))),
		pg_catalog.min(role.rolname) FILTER (
			WHERE privilege.grantee<>migration_role_oid AND role.oid IS NOT NULL)
	INTO owner_execute_count,runtime_role_count,invalid_execute_count,runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(
		COALESCE(procedure.proacl,pg_catalog.acldefault('f',procedure.proowner))) AS privilege
	LEFT JOIN pg_catalog.pg_roles AS role ON role.oid=privilege.grantee
	WHERE procedure.oid=anchor_oid AND privilege.privilege_type='EXECUTE';
	IF owner_execute_count<>1 OR runtime_role_count>1 OR invalid_execute_count<>0 THEN
		RAISE EXCEPTION 'V22 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count=1 THEN
		EXECUTE pg_catalog.format(
			'REVOKE EXECUTE ON FUNCTION decodex.bind_codex_experiment_thread_exact(text,text,uuid,bigint,uuid,text,text,text,text,text,boolean) FROM %I',runtime_role);
		EXECUTE pg_catalog.format(
			'REVOKE EXECUTE ON FUNCTION decodex.record_codex_experiment_observation_exact(text,text,uuid,bigint,uuid,decodex.codex_experiment_observation_kind,text,text,text,text) FROM %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.bind_codex_experiment_start_exact(text,text,uuid,bigint,uuid,text,bigint,text,text,text,boolean,bigint,text,text,text,boolean,text) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_codex_experiment_start_exact(uuid,uuid) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_codex_experiment_title_set_possible_exact(text,text,uuid,bigint,uuid,text,bigint,text,text) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.attest_codex_experiment_retained_title_exact(text,text,uuid,bigint,uuid,uuid,text,bigint,text,bigint,text,text,text,text) TO %I',runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.record_attested_codex_experiment_observation_exact(text,text,uuid,bigint,uuid,uuid,decodex.codex_experiment_observation_kind,text,text,text,text) TO %I',runtime_role);
	END IF;
END
$$;
