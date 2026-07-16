-- XY-1281 persists open-ended Program and finite Objective authority only.
CREATE TYPE decodex.program_state AS ENUM (
	'active', 'needs_attention', 'blocked', 'paused', 'retired'
);
CREATE TYPE decodex.objective_state AS ENUM (
	'proposed', 'active', 'blocked', 'achieved', 'abandoned'
);

CREATE FUNCTION decodex.program_timestamp(value bigint)
RETURNS timestamptz
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT CASE WHEN value BETWEEN 0 AND 253402300799999999 THEN
		TIMESTAMPTZ 'epoch'
			+ (value / 1000000) * INTERVAL '1 second'
			+ (value % 1000000) * INTERVAL '1 microsecond'
	END
$$;

CREATE FUNCTION decodex.is_program_metrics(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE item jsonb;
DECLARE item_count bigint;
DECLARE metric_keys text[] := ARRAY[]::text[];
BEGIN
	IF pg_catalog.jsonb_typeof(document) <> 'array' THEN RETURN false; END IF;
	SELECT pg_catalog.count(*) INTO item_count FROM pg_catalog.jsonb_array_elements(document);
	IF item_count > 64 THEN RETURN false; END IF;
	FOR item IN SELECT value FROM pg_catalog.jsonb_array_elements(document)
	LOOP
		IF pg_catalog.jsonb_typeof(item) <> 'object'
			OR (SELECT pg_catalog.count(*) FROM pg_catalog.jsonb_object_keys(item)) <> 4
			OR NOT item OPERATOR(pg_catalog.?&) ARRAY['key','value','unit','provenance']
			OR pg_catalog.jsonb_typeof(item->'key') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'value') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'unit') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'provenance') <> 'object'
			OR (SELECT pg_catalog.count(*) FROM pg_catalog.jsonb_object_keys(item->'provenance')) <> 2
			OR NOT (item->'provenance') OPERATOR(pg_catalog.?&) ARRAY['source','observed_at_microseconds']
			OR pg_catalog.jsonb_typeof(item->'provenance'->'source') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'provenance'->'observed_at_microseconds') <> 'number'
			OR (item->>'key') COLLATE pg_catalog."C" !~ '^[a-z][a-z0-9_]{0,63}$'
			OR pg_catalog.octet_length(item->>'value') NOT BETWEEN 1 AND 256
			OR pg_catalog.octet_length(item->>'unit') NOT BETWEEN 1 AND 64
			OR pg_catalog.octet_length(item->'provenance'->>'source') NOT BETWEEN 1 AND 256
			OR (item->>'value') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR (item->>'unit') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR (item->'provenance'->>'source') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR (item->>'value') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR (item->>'unit') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR (item->'provenance'->>'source') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR (item->'provenance'->>'observed_at_microseconds') COLLATE pg_catalog."C" !~ '^(0|[1-9][0-9]{0,17})$'
			OR (item->'provenance'->>'observed_at_microseconds')::numeric > 253402300799999999
			OR (item->>'key') = ANY(metric_keys)
			OR decodex.has_credential_material(item)
		THEN RETURN false; END IF;
		metric_keys := pg_catalog.array_append(metric_keys, item->>'key');
	END LOOP;
	RETURN true;
END
$$;

CREATE FUNCTION decodex.is_program_signals(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE item jsonb;
DECLARE item_count bigint;
DECLARE signal_ids text[] := ARRAY[]::text[];
BEGIN
	IF pg_catalog.jsonb_typeof(document) <> 'array' THEN RETURN false; END IF;
	SELECT pg_catalog.count(*) INTO item_count FROM pg_catalog.jsonb_array_elements(document);
	IF item_count > 64 THEN RETURN false; END IF;
	FOR item IN SELECT value FROM pg_catalog.jsonb_array_elements(document)
	LOOP
		IF pg_catalog.jsonb_typeof(item) <> 'object'
			OR (SELECT pg_catalog.count(*) FROM pg_catalog.jsonb_object_keys(item)) <> 4
			OR NOT item OPERATOR(pg_catalog.?&) ARRAY['id','kind','summary','provenance']
			OR pg_catalog.jsonb_typeof(item->'id') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'kind') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'summary') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'provenance') <> 'object'
			OR (SELECT pg_catalog.count(*) FROM pg_catalog.jsonb_object_keys(item->'provenance')) <> 2
			OR NOT (item->'provenance') OPERATOR(pg_catalog.?&) ARRAY['source','observed_at_microseconds']
			OR pg_catalog.jsonb_typeof(item->'provenance'->'source') <> 'string'
			OR pg_catalog.jsonb_typeof(item->'provenance'->'observed_at_microseconds') <> 'number'
			OR (item->>'id') COLLATE pg_catalog."C" !~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
			OR (item->>'kind') COLLATE pg_catalog."C" !~ '^[a-z][a-z0-9_]{0,63}$'
			OR pg_catalog.octet_length(item->>'summary') NOT BETWEEN 1 AND 4096
			OR pg_catalog.octet_length(item->'provenance'->>'source') NOT BETWEEN 1 AND 256
			OR (item->>'summary') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR (item->'provenance'->>'source') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR (item->>'summary') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR (item->'provenance'->>'source') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR (item->'provenance'->>'observed_at_microseconds') COLLATE pg_catalog."C" !~ '^(0|[1-9][0-9]{0,17})$'
			OR (item->'provenance'->>'observed_at_microseconds')::numeric > 253402300799999999
			OR (item->>'id') = ANY(signal_ids)
			OR decodex.has_credential_material(item)
		THEN RETURN false; END IF;
		signal_ids := pg_catalog.array_append(signal_ids, item->>'id');
	END LOOP;
	RETURN true;
END
$$;

CREATE FUNCTION decodex.is_objective_criteria(document text[])
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE item text;
BEGIN
	IF pg_catalog.array_ndims(document) <> 1
		OR pg_catalog.cardinality(document) NOT BETWEEN 1 AND 32
	THEN RETURN false; END IF;
	FOREACH item IN ARRAY document
	LOOP
		IF item IS NULL
			OR pg_catalog.octet_length(item) NOT BETWEEN 1 AND 4096
			OR item COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			OR item COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
			OR decodex.has_credential_material(item)
		THEN RETURN false; END IF;
	END LOOP;
	RETURN pg_catalog.cardinality(document) = (
		SELECT pg_catalog.count(DISTINCT criterion)
		FROM pg_catalog.unnest(document) AS criterion
	);
END
$$;

CREATE TABLE decodex.programs (
	program_id uuid PRIMARY KEY,
	project_id uuid NOT NULL REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	owner_agent_id uuid NOT NULL,
	name text NOT NULL,
	responsibility text NOT NULL,
	state decodex.program_state NOT NULL DEFAULT 'active',
	policy_id uuid NOT NULL,
	policy_revision bigint NOT NULL CHECK (policy_revision > 0),
	review_interval_days integer NOT NULL CHECK (review_interval_days BETWEEN 1 AND 365),
	next_review_at timestamptz NOT NULL,
	metrics jsonb NOT NULL DEFAULT '[]'::jsonb,
	signals jsonb NOT NULL DEFAULT '[]'::jsonb,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	last_changed_by uuid NOT NULL,
	last_correlation_id uuid NOT NULL,
	last_provenance text NOT NULL,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT programs_identity_project_unique UNIQUE (program_id, project_id),
	CONSTRAINT programs_owner_project_fk FOREIGN KEY (owner_agent_id, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT programs_policy_revision_fk FOREIGN KEY (policy_id, project_id, policy_revision)
		REFERENCES decodex.policy_revisions(policy_id, project_id, revision) ON DELETE RESTRICT,
	CONSTRAINT programs_last_actor_project_fk FOREIGN KEY (last_changed_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT programs_ids_canonical CHECK (
		program_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND last_correlation_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT programs_text_bounded CHECK (
		pg_catalog.octet_length(name) >= 1 AND pg_catalog.octet_length(name) <= 256
		AND pg_catalog.octet_length(responsibility) >= 1
		AND pg_catalog.octet_length(responsibility) <= 4096
		AND pg_catalog.octet_length(last_provenance) >= 1
		AND pg_catalog.octet_length(last_provenance) <= 4096
		AND name COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND responsibility COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND last_provenance COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND name COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND responsibility COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND last_provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	),
	CONSTRAINT programs_observations_bounded CHECK (
		decodex.is_program_metrics(metrics) AND decodex.is_program_signals(signals)
	),
	CONSTRAINT programs_finite_timestamps CHECK (
		pg_catalog.isfinite(next_review_at) AND pg_catalog.isfinite(created_at)
		AND pg_catalog.isfinite(updated_at) AND updated_at >= created_at
		AND next_review_at >= TIMESTAMPTZ 'epoch'
		AND next_review_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND created_at >= TIMESTAMPTZ 'epoch'
		AND created_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND updated_at >= TIMESTAMPTZ 'epoch'
		AND updated_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	),
	CONSTRAINT programs_no_credentials CHECK (
		NOT decodex.has_credential_material(name)
		AND NOT decodex.has_credential_material(responsibility)
		AND NOT decodex.has_credential_material(last_provenance)
		AND NOT decodex.has_credential_material(metrics)
		AND NOT decodex.has_credential_material(signals)
	)
);

CREATE TABLE decodex.objectives (
	objective_id uuid PRIMARY KEY,
	project_id uuid NOT NULL REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	program_id uuid,
	outcome text NOT NULL,
	acceptance_criteria text[] NOT NULL,
	validation_criteria text[] NOT NULL,
	target_at timestamptz NOT NULL,
	state decodex.objective_state NOT NULL DEFAULT 'proposed',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	completion_evidence_id uuid,
	last_changed_by uuid NOT NULL,
	last_correlation_id uuid NOT NULL,
	last_provenance text NOT NULL,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT objectives_identity_project_unique UNIQUE (objective_id, project_id),
	CONSTRAINT objectives_program_project_fk FOREIGN KEY (program_id, project_id)
		REFERENCES decodex.programs(program_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT objectives_last_actor_project_fk FOREIGN KEY (last_changed_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT objectives_ids_canonical CHECK (
		objective_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND last_correlation_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT objectives_text_bounded CHECK (
		pg_catalog.octet_length(outcome) >= 1 AND pg_catalog.octet_length(outcome) <= 4096
		AND pg_catalog.octet_length(last_provenance) >= 1
		AND pg_catalog.octet_length(last_provenance) <= 4096
		AND outcome COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND last_provenance COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND outcome COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND last_provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	),
	CONSTRAINT objectives_criteria_bounded CHECK (
		decodex.is_objective_criteria(acceptance_criteria)
		AND decodex.is_objective_criteria(validation_criteria)
	),
	CONSTRAINT objectives_completion_state CHECK (
		(state = 'achieved') = (completion_evidence_id IS NOT NULL)
	),
	CONSTRAINT objectives_finite_timestamps CHECK (
		pg_catalog.isfinite(target_at) AND pg_catalog.isfinite(created_at)
		AND pg_catalog.isfinite(updated_at) AND updated_at >= created_at
		AND target_at >= TIMESTAMPTZ 'epoch'
		AND target_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND created_at >= TIMESTAMPTZ 'epoch'
		AND created_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND updated_at >= TIMESTAMPTZ 'epoch'
		AND updated_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	),
	CONSTRAINT objectives_no_credentials CHECK (
		NOT decodex.has_credential_material(outcome)
		AND NOT decodex.has_credential_material(pg_catalog.to_jsonb(acceptance_criteria))
		AND NOT decodex.has_credential_material(pg_catalog.to_jsonb(validation_criteria))
		AND NOT decodex.has_credential_material(last_provenance)
	)
);

CREATE TABLE decodex.objective_completion_evidence (
	evidence_id uuid PRIMARY KEY,
	objective_id uuid NOT NULL,
	project_id uuid NOT NULL,
	objective_revision bigint NOT NULL CHECK (objective_revision > 0),
	objective_updated_at timestamptz NOT NULL,
	acceptance_result text NOT NULL,
	accepted_by uuid NOT NULL,
	accepted_at timestamptz NOT NULL,
	acceptance_provenance text NOT NULL,
	validation_result text NOT NULL,
	validated_by uuid NOT NULL,
	validated_at timestamptz NOT NULL,
	validation_provenance text NOT NULL,
	correlation_id uuid NOT NULL,
	recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT objective_evidence_identity_scope_unique UNIQUE (evidence_id, objective_id, project_id),
	CONSTRAINT objective_evidence_one_per_objective UNIQUE (objective_id),
	CONSTRAINT objective_evidence_objective_project_fk FOREIGN KEY (objective_id, project_id)
		REFERENCES decodex.objectives(objective_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT objective_evidence_accepting_agent_project_fk FOREIGN KEY (accepted_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT objective_evidence_validating_agent_project_fk FOREIGN KEY (validated_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT objective_evidence_ids_canonical CHECK (
		evidence_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND correlation_id::text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT objective_evidence_text_bounded CHECK (
		pg_catalog.octet_length(acceptance_result) >= 1
		AND pg_catalog.octet_length(acceptance_result) <= 4096
		AND pg_catalog.octet_length(acceptance_provenance) >= 1
		AND pg_catalog.octet_length(acceptance_provenance) <= 4096
		AND pg_catalog.octet_length(validation_result) >= 1
		AND pg_catalog.octet_length(validation_result) <= 4096
		AND pg_catalog.octet_length(validation_provenance) >= 1
		AND pg_catalog.octet_length(validation_provenance) <= 4096
		AND acceptance_result COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND acceptance_provenance COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND validation_result COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND validation_provenance COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND acceptance_result COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND acceptance_provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND validation_result COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
		AND validation_provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	),
	CONSTRAINT objective_evidence_chronology CHECK (
		pg_catalog.isfinite(objective_updated_at) AND pg_catalog.isfinite(accepted_at)
		AND pg_catalog.isfinite(validated_at) AND pg_catalog.isfinite(recorded_at)
		AND objective_updated_at >= TIMESTAMPTZ 'epoch'
		AND recorded_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND objective_updated_at <= accepted_at AND accepted_at <= validated_at
		AND validated_at <= recorded_at
	),
	CONSTRAINT objective_evidence_no_credentials CHECK (
		NOT decodex.has_credential_material(acceptance_result)
		AND NOT decodex.has_credential_material(acceptance_provenance)
		AND NOT decodex.has_credential_material(validation_result)
		AND NOT decodex.has_credential_material(validation_provenance)
	)
);

ALTER TABLE decodex.objectives
	ADD CONSTRAINT objectives_completion_evidence_fk
	FOREIGN KEY (completion_evidence_id, objective_id, project_id)
	REFERENCES decodex.objective_completion_evidence(evidence_id, objective_id, project_id)
	ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION decodex.enforce_program_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	IF TG_OP = 'TRUNCATE' OR TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'Programs are retained'
			USING ERRCODE = '55000', CONSTRAINT = 'programs_retained';
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.state <> 'active' OR NEW.revision <> 1 OR NEW.updated_at <> NEW.created_at THEN
			RAISE EXCEPTION 'Program insertion must be canonical active revision one'
				USING ERRCODE = '23514', CONSTRAINT = 'programs_canonical_insert';
		END IF;
		RETURN NEW;
	END IF;
	IF OLD.state = 'retired' THEN
		RAISE EXCEPTION 'retired Program is immutable'
			USING ERRCODE = '55000', CONSTRAINT = 'programs_retired_terminal';
	END IF;
	IF NEW.program_id IS DISTINCT FROM OLD.program_id
		OR NEW.project_id IS DISTINCT FROM OLD.project_id
		OR NEW.owner_agent_id IS DISTINCT FROM OLD.owner_agent_id
		OR NEW.name IS DISTINCT FROM OLD.name
		OR NEW.responsibility IS DISTINCT FROM OLD.responsibility
		OR NEW.policy_id IS DISTINCT FROM OLD.policy_id
		OR NEW.policy_revision IS DISTINCT FROM OLD.policy_revision
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision IS DISTINCT FROM OLD.revision + 1
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'Program identity or revision mutation is invalid'
			USING ERRCODE = '23514', CONSTRAINT = 'programs_identity_revision';
	END IF;
	IF NEW.state IS DISTINCT FROM OLD.state THEN
		IF NOT (
			(OLD.state = 'active' AND NEW.state IN ('needs_attention','blocked','paused','retired'))
			OR (OLD.state = 'needs_attention' AND NEW.state IN ('active','blocked','paused','retired'))
			OR (OLD.state = 'blocked' AND NEW.state IN ('active','needs_attention','paused','retired'))
			OR (OLD.state = 'paused' AND NEW.state IN ('active','retired'))
		) OR NEW.review_interval_days IS DISTINCT FROM OLD.review_interval_days
			OR NEW.next_review_at IS DISTINCT FROM OLD.next_review_at
			OR NEW.metrics IS DISTINCT FROM OLD.metrics OR NEW.signals IS DISTINCT FROM OLD.signals
		THEN
			RAISE EXCEPTION 'Program lifecycle transition is invalid'
				USING ERRCODE = '23514', CONSTRAINT = 'programs_invalid_transition';
		END IF;
	ELSIF NEW.review_interval_days IS NOT DISTINCT FROM OLD.review_interval_days
		AND NEW.next_review_at IS NOT DISTINCT FROM OLD.next_review_at
		AND NEW.metrics IS NOT DISTINCT FROM OLD.metrics
		AND NEW.signals IS NOT DISTINCT FROM OLD.signals
	THEN
		RAISE EXCEPTION 'Program context update is empty'
			USING ERRCODE = '23514', CONSTRAINT = 'programs_noop_update';
	END IF;
	RETURN NEW;
END
$$;

CREATE FUNCTION decodex.enforce_objective_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE evidence_row decodex.objective_completion_evidence%ROWTYPE;
BEGIN
	IF TG_OP = 'TRUNCATE' OR TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'Objectives are retained'
			USING ERRCODE = '55000', CONSTRAINT = 'objectives_retained';
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.state <> 'proposed' OR NEW.revision <> 1
			OR NEW.completion_evidence_id IS NOT NULL OR NEW.updated_at <> NEW.created_at
		THEN
			RAISE EXCEPTION 'Objective insertion must be canonical proposed revision one'
				USING ERRCODE = '23514', CONSTRAINT = 'objectives_canonical_insert';
		END IF;
		RETURN NEW;
	END IF;
	IF OLD.state IN ('achieved','abandoned') THEN
		RAISE EXCEPTION 'terminal Objective is immutable'
			USING ERRCODE = '55000', CONSTRAINT = 'objectives_terminal';
	END IF;
	IF NEW.objective_id IS DISTINCT FROM OLD.objective_id
		OR NEW.project_id IS DISTINCT FROM OLD.project_id
		OR NEW.program_id IS DISTINCT FROM OLD.program_id
		OR NEW.outcome IS DISTINCT FROM OLD.outcome
		OR NEW.acceptance_criteria IS DISTINCT FROM OLD.acceptance_criteria
		OR NEW.validation_criteria IS DISTINCT FROM OLD.validation_criteria
		OR NEW.target_at IS DISTINCT FROM OLD.target_at
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision IS DISTINCT FROM OLD.revision + 1
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'Objective identity or revision mutation is invalid'
			USING ERRCODE = '23514', CONSTRAINT = 'objectives_identity_revision';
	END IF;
	IF NEW.state = 'achieved' THEN
		SELECT stored.* INTO evidence_row
		FROM decodex.objective_completion_evidence AS stored
		WHERE stored.evidence_id = NEW.completion_evidence_id;
		IF NOT FOUND OR evidence_row.objective_id <> OLD.objective_id
			OR evidence_row.project_id <> OLD.project_id
			OR evidence_row.objective_revision <> OLD.revision
			OR evidence_row.objective_updated_at <> OLD.updated_at
			OR evidence_row.accepted_at < OLD.updated_at
		THEN
			RAISE EXCEPTION 'Objective achievement evidence must follow its exact prior revision'
				USING ERRCODE = '23514', CONSTRAINT = 'objective_evidence_prior_revision_time';
		END IF;
	END IF;
	IF NOT (
		(OLD.state = 'proposed' AND NEW.state IN ('active','abandoned'))
		OR (OLD.state = 'active' AND NEW.state IN ('blocked','achieved','abandoned'))
		OR (OLD.state = 'blocked' AND NEW.state IN ('active','achieved','abandoned'))
	) THEN
		RAISE EXCEPTION 'Objective lifecycle transition is invalid'
			USING ERRCODE = '23514', CONSTRAINT = 'objectives_invalid_transition';
	END IF;
	RETURN NEW;
END
$$;

CREATE FUNCTION decodex.forbid_objective_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	RAISE EXCEPTION 'Objective completion evidence is immutable'
		USING ERRCODE = '55000', CONSTRAINT = 'objective_evidence_immutable';
END
$$;

CREATE FUNCTION decodex.enforce_objective_completion_coherence()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE objective_row decodex.objectives%ROWTYPE;
DECLARE evidence_row decodex.objective_completion_evidence%ROWTYPE;
BEGIN
	IF TG_TABLE_NAME = 'objectives' THEN
		IF NEW.state <> 'achieved' THEN RETURN NEW; END IF;
		SELECT stored.* INTO evidence_row FROM decodex.objective_completion_evidence AS stored
		WHERE stored.evidence_id = NEW.completion_evidence_id;
		IF NOT FOUND OR evidence_row.objective_id <> NEW.objective_id
			OR evidence_row.project_id <> NEW.project_id
			OR evidence_row.objective_revision + 1 <> NEW.revision
		THEN
			RAISE EXCEPTION 'achieved Objective requires exact prior-revision evidence'
				USING ERRCODE = '23514', CONSTRAINT = 'objective_completion_coherence';
		END IF;
		RETURN NEW;
	END IF;
	SELECT stored.* INTO objective_row FROM decodex.objectives AS stored
	WHERE stored.objective_id = NEW.objective_id;
	IF NOT FOUND OR objective_row.project_id <> NEW.project_id
		OR objective_row.state <> 'achieved'
		OR objective_row.completion_evidence_id <> NEW.evidence_id
		OR objective_row.revision <> NEW.objective_revision + 1
	THEN
		RAISE EXCEPTION 'Objective evidence must establish one exact achieved revision'
			USING ERRCODE = '23514', CONSTRAINT = 'objective_completion_coherence';
	END IF;
	RETURN NEW;
END
$$;

CREATE TRIGGER programs_state_guard
BEFORE INSERT OR UPDATE OR DELETE ON decodex.programs
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_program_state();
CREATE TRIGGER programs_truncate_forbidden
BEFORE TRUNCATE ON decodex.programs
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_program_state();
CREATE TRIGGER objectives_state_guard
BEFORE INSERT OR UPDATE OR DELETE ON decodex.objectives
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_objective_state();
CREATE TRIGGER objectives_truncate_forbidden
BEFORE TRUNCATE ON decodex.objectives
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_objective_state();
CREATE TRIGGER objective_evidence_immutable
BEFORE UPDATE OR DELETE ON decodex.objective_completion_evidence
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_objective_evidence_mutation();
CREATE TRIGGER objective_evidence_truncate_forbidden
BEFORE TRUNCATE ON decodex.objective_completion_evidence
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_objective_evidence_mutation();
CREATE CONSTRAINT TRIGGER objectives_completion_coherence
AFTER INSERT OR UPDATE ON decodex.objectives DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_objective_completion_coherence();
CREATE CONSTRAINT TRIGGER objective_evidence_completion_coherence
AFTER INSERT ON decodex.objective_completion_evidence DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_objective_completion_coherence();

CREATE FUNCTION decodex.create_program(
	p_program_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_owner_agent_id decodex.canonical_uuid_v4_text,
	p_name text,
	p_responsibility text,
	p_policy_id decodex.canonical_uuid_v4_text,
	p_policy_revision bigint,
	p_review_interval_days integer,
	p_next_review_at bigint,
	p_metrics jsonb,
	p_signals jsonb,
	p_correlation_id decodex.canonical_uuid_v4_text,
	p_provenance text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_program_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_owner_agent_id uuid;
DECLARE canonical_policy_id uuid;
DECLARE canonical_correlation_id uuid;
DECLARE existing decodex.programs%ROWTYPE;
DECLARE write_time timestamptz;
DECLARE inserted boolean := false;
BEGIN
	IF p_program_id IS NULL OR p_project_id IS NULL OR p_owner_agent_id IS NULL
		OR p_policy_id IS NULL OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_program_id := p_program_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_owner_agent_id := p_owner_agent_id::text::uuid;
	canonical_policy_id := p_policy_id::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	PERFORM 1 FROM decodex.projects AS project
	JOIN decodex.agents AS lead ON lead.project_id=project.project_id AND lead.role='lead'
	WHERE project.project_id=canonical_project_id AND project.status='active'
		AND lead.agent_id=canonical_owner_agent_id AND lead.status='active' FOR KEY SHARE OF project, lead;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', NULL::bigint, false; RETURN; END IF;
	PERFORM 1 FROM decodex.policy_revisions AS revision
	WHERE revision.policy_id=canonical_policy_id AND revision.project_id=canonical_project_id
		AND revision.revision=p_policy_revision FOR KEY SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_policy', NULL::bigint, false; RETURN; END IF;
	write_time := pg_catalog.clock_timestamp();
	INSERT INTO decodex.programs (
		program_id,project_id,owner_agent_id,name,responsibility,policy_id,policy_revision,
		review_interval_days,next_review_at,metrics,signals,last_changed_by,last_correlation_id,
		last_provenance,created_at,updated_at
	) VALUES (
		canonical_program_id,canonical_project_id,canonical_owner_agent_id,p_name,p_responsibility,
		canonical_policy_id,p_policy_revision,p_review_interval_days,
		decodex.program_timestamp(p_next_review_at),p_metrics,p_signals,
		canonical_owner_agent_id,canonical_correlation_id,p_provenance,
		write_time,write_time
	) ON CONFLICT DO NOTHING
	RETURNING true INTO inserted;
	IF inserted IS NULL THEN inserted := false; END IF;
	SELECT stored.* INTO existing FROM decodex.programs AS stored
	WHERE stored.program_id=canonical_program_id FOR UPDATE;
	IF existing.project_id=canonical_project_id AND existing.owner_agent_id=canonical_owner_agent_id
		AND existing.name=p_name AND existing.responsibility=p_responsibility
		AND existing.policy_id=canonical_policy_id AND existing.policy_revision=p_policy_revision
		AND existing.review_interval_days=p_review_interval_days
		AND existing.next_review_at=decodex.program_timestamp(p_next_review_at)
		AND existing.metrics=p_metrics AND existing.signals=p_signals
	THEN RETURN QUERY SELECT 'ok', existing.revision, inserted; RETURN; END IF;
	RETURN QUERY SELECT 'conflicting_identity', existing.revision, false;
END
$$;

CREATE FUNCTION decodex.update_program_context(
	p_program_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_expected_revision bigint,
	p_review_interval_days integer,
	p_next_review_at bigint,
	p_metrics jsonb,
	p_signals jsonb,
	p_actor_id decodex.canonical_uuid_v4_text,
	p_correlation_id decodex.canonical_uuid_v4_text,
	p_provenance text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_program_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_actor_id uuid;
DECLARE canonical_correlation_id uuid;
DECLARE stored decodex.programs%ROWTYPE;
BEGIN
	IF p_program_id IS NULL OR p_project_id IS NULL OR p_actor_id IS NULL
		OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_program_id := p_program_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_actor_id := p_actor_id::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	SELECT current.* INTO stored FROM decodex.programs AS current
	WHERE current.program_id=canonical_program_id FOR UPDATE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'not_found', NULL::bigint, false; RETURN; END IF;
	IF stored.project_id<>canonical_project_id THEN
		RETURN QUERY SELECT 'invalid_project', NULL::bigint, false; RETURN;
	END IF;
	IF stored.revision IS DISTINCT FROM p_expected_revision THEN
		RETURN QUERY SELECT 'revision_conflict', stored.revision, false; RETURN;
	END IF;
	PERFORM 1 FROM decodex.projects AS project JOIN decodex.agents AS lead
		ON lead.project_id=project.project_id AND lead.role='lead'
	WHERE project.project_id=canonical_project_id AND project.status='active'
		AND lead.agent_id=canonical_actor_id AND lead.agent_id=stored.owner_agent_id
		AND lead.status='active' FOR KEY SHARE OF project, lead;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', stored.revision, false; RETURN; END IF;
	IF stored.state='retired' THEN RETURN QUERY SELECT 'invalid_transition', stored.revision, false; RETURN; END IF;
	IF stored.review_interval_days=p_review_interval_days
		AND stored.next_review_at=decodex.program_timestamp(p_next_review_at)
		AND stored.metrics=p_metrics AND stored.signals=p_signals
	THEN RETURN QUERY SELECT 'invalid_transition', stored.revision, false; RETURN; END IF;
	UPDATE decodex.programs SET review_interval_days=p_review_interval_days,
		next_review_at=decodex.program_timestamp(p_next_review_at),metrics=p_metrics,
		signals=p_signals,revision=revision+1,
		last_changed_by=canonical_actor_id,last_correlation_id=canonical_correlation_id,
		last_provenance=p_provenance,updated_at=pg_catalog.clock_timestamp()
	WHERE program_id=canonical_program_id;
	RETURN QUERY SELECT 'ok', stored.revision+1, true;
END
$$;

CREATE FUNCTION decodex.transition_program(
	p_program_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_expected_revision bigint,
	p_state decodex.program_state,
	p_actor_id decodex.canonical_uuid_v4_text,
	p_correlation_id decodex.canonical_uuid_v4_text,
	p_provenance text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_program_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_actor_id uuid;
DECLARE canonical_correlation_id uuid;
DECLARE stored decodex.programs%ROWTYPE;
BEGIN
	IF p_program_id IS NULL OR p_project_id IS NULL OR p_actor_id IS NULL
		OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_program_id := p_program_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_actor_id := p_actor_id::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	SELECT current.* INTO stored FROM decodex.programs AS current
	WHERE current.program_id=canonical_program_id FOR UPDATE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'not_found', NULL::bigint, false; RETURN; END IF;
	IF stored.project_id<>canonical_project_id THEN
		RETURN QUERY SELECT 'invalid_project', NULL::bigint, false; RETURN;
	END IF;
	IF stored.revision IS DISTINCT FROM p_expected_revision THEN
		RETURN QUERY SELECT 'revision_conflict', stored.revision, false; RETURN;
	END IF;
	PERFORM 1 FROM decodex.projects AS project JOIN decodex.agents AS lead
		ON lead.project_id=project.project_id AND lead.role='lead'
	WHERE project.project_id=canonical_project_id AND project.status='active'
		AND lead.agent_id=canonical_actor_id AND lead.agent_id=stored.owner_agent_id
		AND lead.status='active' FOR KEY SHARE OF project, lead;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', stored.revision, false; RETURN; END IF;
	IF NOT (
		(stored.state='active' AND p_state IN ('needs_attention','blocked','paused','retired'))
		OR (stored.state='needs_attention' AND p_state IN ('active','blocked','paused','retired'))
		OR (stored.state='blocked' AND p_state IN ('active','needs_attention','paused','retired'))
		OR (stored.state='paused' AND p_state IN ('active','retired'))
	) THEN RETURN QUERY SELECT 'invalid_transition', stored.revision, false; RETURN; END IF;
	UPDATE decodex.programs SET state=p_state,revision=revision+1,
		last_changed_by=canonical_actor_id,last_correlation_id=canonical_correlation_id,
		last_provenance=p_provenance,updated_at=pg_catalog.clock_timestamp()
	WHERE program_id=canonical_program_id;
	RETURN QUERY SELECT 'ok', stored.revision+1, true;
END
$$;

CREATE FUNCTION decodex.create_objective(
	p_objective_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_program_id decodex.canonical_uuid_v4_text,
	p_outcome text,
	p_acceptance_criteria text[],
	p_validation_criteria text[],
	p_target_at bigint,
	p_actor_id decodex.canonical_uuid_v4_text,
	p_correlation_id decodex.canonical_uuid_v4_text,
	p_provenance text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_objective_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_program_id uuid;
DECLARE canonical_actor_id uuid;
DECLARE canonical_correlation_id uuid;
DECLARE existing decodex.objectives%ROWTYPE;
DECLARE write_time timestamptz;
DECLARE inserted boolean := false;
BEGIN
	IF p_objective_id IS NULL OR p_project_id IS NULL OR p_actor_id IS NULL
		OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_objective_id := p_objective_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_program_id := CASE WHEN p_program_id IS NULL THEN NULL ELSE p_program_id::text::uuid END;
	canonical_actor_id := p_actor_id::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	PERFORM 1 FROM decodex.projects AS project JOIN decodex.agents AS lead
		ON lead.project_id=project.project_id AND lead.role='lead'
	WHERE project.project_id=canonical_project_id AND project.status='active'
		AND lead.agent_id=canonical_actor_id AND lead.status='active' FOR KEY SHARE OF project, lead;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', NULL::bigint, false; RETURN; END IF;
	IF canonical_program_id IS NOT NULL AND NOT EXISTS (
		SELECT 1 FROM decodex.programs AS program
		WHERE program.program_id=canonical_program_id AND program.project_id=canonical_project_id
	) THEN RETURN QUERY SELECT 'invalid_program', NULL::bigint, false; RETURN; END IF;
	IF decodex.program_timestamp(p_target_at) IS NULL
		OR decodex.program_timestamp(p_target_at) <= pg_catalog.clock_timestamp()
	THEN RETURN QUERY SELECT 'invalid_horizon', NULL::bigint, false; RETURN; END IF;
	write_time := pg_catalog.clock_timestamp();
	INSERT INTO decodex.objectives (
		objective_id,project_id,program_id,outcome,acceptance_criteria,validation_criteria,
		target_at,last_changed_by,last_correlation_id,last_provenance,created_at,updated_at
	) VALUES (
		canonical_objective_id,canonical_project_id,canonical_program_id,p_outcome,
		p_acceptance_criteria,p_validation_criteria,decodex.program_timestamp(p_target_at),
		canonical_actor_id,
		canonical_correlation_id,p_provenance,write_time,write_time
	) ON CONFLICT DO NOTHING
	RETURNING true INTO inserted;
	IF inserted IS NULL THEN inserted := false; END IF;
	SELECT stored.* INTO existing FROM decodex.objectives AS stored
	WHERE stored.objective_id=canonical_objective_id FOR UPDATE;
	IF existing.project_id=canonical_project_id
		AND existing.program_id IS NOT DISTINCT FROM canonical_program_id
		AND existing.outcome=p_outcome AND existing.acceptance_criteria=p_acceptance_criteria
		AND existing.validation_criteria=p_validation_criteria
		AND existing.target_at=decodex.program_timestamp(p_target_at)
	THEN RETURN QUERY SELECT 'ok', existing.revision, inserted; RETURN; END IF;
	RETURN QUERY SELECT 'conflicting_identity', existing.revision, false;
END
$$;

CREATE FUNCTION decodex.transition_objective(
	p_objective_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_expected_revision bigint,
	p_state decodex.objective_state,
	p_actor_id decodex.canonical_uuid_v4_text,
	p_correlation_id decodex.canonical_uuid_v4_text,
	p_provenance text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_objective_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_actor_id uuid;
DECLARE canonical_correlation_id uuid;
DECLARE stored decodex.objectives%ROWTYPE;
BEGIN
	IF p_objective_id IS NULL OR p_project_id IS NULL OR p_actor_id IS NULL
		OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_objective_id := p_objective_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_actor_id := p_actor_id::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	SELECT current.* INTO stored FROM decodex.objectives AS current
	WHERE current.objective_id=canonical_objective_id FOR UPDATE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'not_found', NULL::bigint, false; RETURN; END IF;
	IF stored.project_id<>canonical_project_id THEN
		RETURN QUERY SELECT 'invalid_project', NULL::bigint, false; RETURN;
	END IF;
	IF stored.revision IS DISTINCT FROM p_expected_revision THEN
		RETURN QUERY SELECT 'revision_conflict', stored.revision, false; RETURN;
	END IF;
	PERFORM 1 FROM decodex.projects AS project JOIN decodex.agents AS lead
		ON lead.project_id=project.project_id AND lead.role='lead'
	WHERE project.project_id=canonical_project_id AND project.status='active'
		AND lead.agent_id=canonical_actor_id AND lead.status='active' FOR KEY SHARE OF project, lead;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', stored.revision, false; RETURN; END IF;
	IF NOT (
		(stored.state='proposed' AND p_state IN ('active','abandoned'))
		OR (stored.state='active' AND p_state IN ('blocked','abandoned'))
		OR (stored.state='blocked' AND p_state IN ('active','abandoned'))
	) THEN RETURN QUERY SELECT 'invalid_transition', stored.revision, false; RETURN; END IF;
	UPDATE decodex.objectives SET state=p_state,revision=revision+1,
		last_changed_by=canonical_actor_id,last_correlation_id=canonical_correlation_id,
		last_provenance=p_provenance,updated_at=pg_catalog.clock_timestamp()
	WHERE objective_id=canonical_objective_id;
	RETURN QUERY SELECT 'ok', stored.revision+1, true;
END
$$;

CREATE FUNCTION decodex.achieve_objective(
	p_evidence_id decodex.canonical_uuid_v4_text,
	p_objective_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_objective_revision bigint,
	p_acceptance_result text,
	p_accepted_by decodex.canonical_uuid_v4_text,
	p_accepted_at bigint,
	p_acceptance_provenance text,
	p_validation_result text,
	p_validated_by decodex.canonical_uuid_v4_text,
	p_validated_at bigint,
	p_validation_provenance text,
	p_correlation_id decodex.canonical_uuid_v4_text
)
RETURNS TABLE (result_code text, actual_revision bigint, changed boolean)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_evidence_id uuid;
DECLARE canonical_objective_id uuid;
DECLARE canonical_project_id uuid;
DECLARE canonical_accepted_by uuid;
DECLARE canonical_validated_by uuid;
DECLARE canonical_correlation_id uuid;
DECLARE stored decodex.objectives%ROWTYPE;
DECLARE existing decodex.objective_completion_evidence%ROWTYPE;
DECLARE record_time timestamptz;
DECLARE inserted boolean;
BEGIN
	IF p_evidence_id IS NULL OR p_objective_id IS NULL OR p_project_id IS NULL
		OR p_accepted_by IS NULL OR p_validated_by IS NULL OR p_correlation_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_evidence_id := p_evidence_id::text::uuid;
	canonical_objective_id := p_objective_id::text::uuid;
	canonical_project_id := p_project_id::text::uuid;
	canonical_accepted_by := p_accepted_by::text::uuid;
	canonical_validated_by := p_validated_by::text::uuid;
	canonical_correlation_id := p_correlation_id::text::uuid;
	SELECT current.* INTO stored FROM decodex.objectives AS current
	WHERE current.objective_id=canonical_objective_id FOR UPDATE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'not_found', NULL::bigint, false; RETURN; END IF;
	SELECT evidence.* INTO existing FROM decodex.objective_completion_evidence AS evidence
	WHERE evidence.evidence_id=canonical_evidence_id;
	IF FOUND THEN
		IF existing.objective_id=canonical_objective_id AND existing.project_id=canonical_project_id
			AND existing.objective_revision=p_objective_revision
			AND existing.acceptance_result=p_acceptance_result
			AND existing.accepted_by=canonical_accepted_by
			AND existing.accepted_at=decodex.program_timestamp(p_accepted_at)
			AND existing.acceptance_provenance=p_acceptance_provenance
			AND existing.validation_result=p_validation_result
			AND existing.validated_by=canonical_validated_by
			AND existing.validated_at=decodex.program_timestamp(p_validated_at)
			AND existing.validation_provenance=p_validation_provenance
			AND existing.correlation_id=canonical_correlation_id
		THEN RETURN QUERY SELECT 'ok', stored.revision, false; RETURN; END IF;
		RETURN QUERY SELECT 'conflicting_identity', stored.revision, false; RETURN;
	END IF;
	IF stored.project_id<>canonical_project_id THEN
		RETURN QUERY SELECT 'invalid_project', stored.revision, false; RETURN;
	END IF;
	IF stored.revision IS DISTINCT FROM p_objective_revision THEN
		RETURN QUERY SELECT 'revision_conflict', stored.revision, false; RETURN;
	END IF;
	IF stored.state NOT IN ('active','blocked') THEN
		RETURN QUERY SELECT 'invalid_transition', stored.revision, false; RETURN;
	END IF;
	PERFORM 1 FROM decodex.projects AS project WHERE project.project_id=canonical_project_id
		AND project.status='active' FOR KEY SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', stored.revision, false; RETURN; END IF;
	PERFORM 1 FROM decodex.agents AS agent WHERE agent.project_id=canonical_project_id
		AND agent.role='lead' AND agent.status='active'
		AND agent.agent_id IN (canonical_accepted_by,canonical_validated_by)
	GROUP BY agent.project_id HAVING pg_catalog.count(DISTINCT agent.agent_id)
		= CASE WHEN canonical_accepted_by=canonical_validated_by THEN 1 ELSE 2 END;
	IF NOT FOUND THEN RETURN QUERY SELECT 'invalid_authority', stored.revision, false; RETURN; END IF;
	record_time := pg_catalog.clock_timestamp();
	IF decodex.program_timestamp(p_accepted_at) IS NULL
		OR decodex.program_timestamp(p_validated_at) IS NULL
		OR decodex.program_timestamp(p_accepted_at)<stored.updated_at
		OR decodex.program_timestamp(p_accepted_at)>decodex.program_timestamp(p_validated_at)
		OR decodex.program_timestamp(p_validated_at)>record_time
	THEN RETURN QUERY SELECT 'invalid_evidence', stored.revision, false; RETURN; END IF;
	INSERT INTO decodex.objective_completion_evidence (
		evidence_id,objective_id,project_id,objective_revision,objective_updated_at,
		acceptance_result,accepted_by,
		accepted_at,acceptance_provenance,validation_result,validated_by,validated_at,
		validation_provenance,correlation_id,recorded_at
	) VALUES (
		canonical_evidence_id,canonical_objective_id,canonical_project_id,p_objective_revision,
		stored.updated_at,p_acceptance_result,canonical_accepted_by,
		decodex.program_timestamp(p_accepted_at),
		p_acceptance_provenance,p_validation_result,canonical_validated_by,
		decodex.program_timestamp(p_validated_at),p_validation_provenance,
		canonical_correlation_id,record_time
	) ON CONFLICT DO NOTHING
	RETURNING true INTO inserted;
	IF inserted IS DISTINCT FROM true THEN
		SELECT evidence.* INTO existing FROM decodex.objective_completion_evidence AS evidence
		WHERE evidence.evidence_id=canonical_evidence_id;
		IF FOUND AND existing.objective_id=canonical_objective_id
			AND existing.project_id=canonical_project_id
			AND existing.objective_revision=p_objective_revision
			AND existing.acceptance_result=p_acceptance_result
			AND existing.accepted_by=canonical_accepted_by
			AND existing.accepted_at=decodex.program_timestamp(p_accepted_at)
			AND existing.acceptance_provenance=p_acceptance_provenance
			AND existing.validation_result=p_validation_result
			AND existing.validated_by=canonical_validated_by
			AND existing.validated_at=decodex.program_timestamp(p_validated_at)
			AND existing.validation_provenance=p_validation_provenance
			AND existing.correlation_id=canonical_correlation_id
		THEN RETURN QUERY SELECT 'ok', stored.revision, false; RETURN; END IF;
		RETURN QUERY SELECT 'conflicting_identity', stored.revision, false; RETURN;
	END IF;
	UPDATE decodex.objectives SET state='achieved',revision=revision+1,
		completion_evidence_id=canonical_evidence_id,last_changed_by=canonical_validated_by,
		last_correlation_id=canonical_correlation_id,last_provenance=p_validation_provenance,
		updated_at=record_time WHERE objective_id=canonical_objective_id;
	RETURN QUERY SELECT 'ok', stored.revision+1, true;
END
$$;

ALTER FUNCTION decodex.program_timestamp(bigint) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_program_metrics(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_program_signals(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_objective_criteria(text[]) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_program_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_objective_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.forbid_objective_evidence_mutation() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_objective_completion_coherence() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.create_program(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,
	decodex.canonical_uuid_v4_text,text,text,decodex.canonical_uuid_v4_text,bigint,integer,
	bigint,jsonb,jsonb,decodex.canonical_uuid_v4_text,text
) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.update_program_context(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,bigint,integer,bigint,jsonb,jsonb,
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,text
) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.transition_program(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,bigint,decodex.program_state,
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,text
) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.create_objective(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,
	decodex.canonical_uuid_v4_text,text,text[],text[],bigint,
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,text
) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.transition_objective(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,bigint,decodex.objective_state,
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,text
) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.achieve_objective(
	decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,
	decodex.canonical_uuid_v4_text,bigint,text,decodex.canonical_uuid_v4_text,bigint,text,
	text,decodex.canonical_uuid_v4_text,bigint,text,decodex.canonical_uuid_v4_text
) SET search_path = pg_catalog, decodex;

REVOKE ALL ON TABLE decodex.programs, decodex.objectives,
	decodex.objective_completion_evidence FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
REVOKE ALL ON TYPE decodex.program_state, decodex.objective_state FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
