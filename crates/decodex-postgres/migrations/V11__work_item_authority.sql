-- XY-1343 canonical WorkItems, transactional readiness, and Lead acceptance.
-- Keep the final V11 schema at the PostgreSQL 18 pg_dump/pg_restore fixed point.
ALTER TABLE decodex.account_snapshots
	DROP CONSTRAINT account_snapshots_facts,
	ADD CONSTRAINT account_snapshots_facts CHECK (
		pg_catalog.octet_length(display_label) >= 1
		AND pg_catalog.octet_length(display_label) <= 128
		AND NOT decodex.has_credential_material(display_label)
	);

ALTER TABLE decodex.exact_command_receipts
	DROP CONSTRAINT exact_command_receipts_identity_bounded,
	ADD CONSTRAINT exact_command_receipts_identity_bounded CHECK (
		pg_catalog.octet_length(protocol_version) >= 1
		AND pg_catalog.octet_length(protocol_version) <= 64
		AND protocol_version COLLATE pg_catalog."C" ~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		AND pg_catalog.octet_length(idempotency_key) >= 1
		AND pg_catalog.octet_length(idempotency_key) <= 256
		AND NOT decodex.has_credential_material(idempotency_key)
	);

ALTER TABLE decodex.role_profile_revisions
	DROP CONSTRAINT role_profile_revisions_configuration,
	ADD CONSTRAINT role_profile_revisions_configuration CHECK (
		pg_catalog.octet_length(model) >= 1
		AND pg_catalog.octet_length(model) <= 128
		AND pg_catalog.octet_length(reasoning_effort) >= 1
		AND pg_catalog.octet_length(reasoning_effort) <= 32
		AND reasoning_effort COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
		AND pg_catalog.octet_length(service_tier) >= 1
		AND pg_catalog.octet_length(service_tier) <= 32
		AND service_tier COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_-]{0,31}$'
		AND pg_catalog.octet_length(instructions) >= 1
		AND pg_catalog.octet_length(instructions) <= 65536
		AND (provenance IS NULL OR (
			pg_catalog.octet_length(provenance) >= 1
			AND pg_catalog.octet_length(provenance) <= 4096
		))
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
	);

CREATE TYPE decodex.work_item_priority AS ENUM ('urgent', 'high', 'medium', 'low', 'none');
CREATE TYPE decodex.work_item_state AS ENUM (
	'inbox', 'planned', 'ready', 'running', 'review', 'blocked', 'done', 'canceled'
);
CREATE TYPE decodex.work_item_edge_kind AS ENUM ('depends_on', 'blocked_by');
CREATE TYPE decodex.work_item_blocker_kind AS ENUM (
	'project_inactive', 'lead_inactive', 'program_inactive', 'objective_inactive',
	'dependency_incomplete', 'blocker_active', 'dependency_cycle'
);

CREATE FUNCTION decodex.is_work_item_text(value text, maximum_bytes integer)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
SET search_path = pg_catalog, decodex
AS $$
SELECT maximum_bytes > 0
	AND pg_catalog.octet_length(value) BETWEEN 1 AND maximum_bytes
	AND value COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	AND value COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	AND NOT decodex.has_credential_material(value)
$$;

CREATE FUNCTION decodex.is_work_item_criteria(document text[])
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
SET search_path = pg_catalog, decodex
AS $$
DECLARE item text;
BEGIN
	IF pg_catalog.array_ndims(document) <> 1
		OR pg_catalog.cardinality(document) NOT BETWEEN 1 AND 32
	THEN RETURN false; END IF;
	FOREACH item IN ARRAY document LOOP
		IF item IS NULL OR NOT decodex.is_work_item_text(item, 4096) THEN RETURN false; END IF;
	END LOOP;
	RETURN pg_catalog.cardinality(document) = (
		SELECT pg_catalog.count(DISTINCT criterion)
		FROM pg_catalog.unnest(document) AS criterion
	);
END
$$;

CREATE TABLE decodex.work_items (
	work_item_id uuid PRIMARY KEY,
	project_id uuid NOT NULL REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	lead_agent_id uuid NOT NULL,
	program_id uuid,
	title text NOT NULL,
	description text NOT NULL,
	priority decodex.work_item_priority NOT NULL,
	acceptance_criteria text[] NOT NULL,
	validation_criteria text[] NOT NULL,
	state decodex.work_item_state NOT NULL DEFAULT 'inbox',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	last_changed_by uuid NOT NULL,
	last_correlation_id uuid NOT NULL,
	last_provenance text NOT NULL,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT work_items_identity_project_revision_unique
		UNIQUE (work_item_id, project_id, revision),
	CONSTRAINT work_items_identity_project_unique UNIQUE (work_item_id, project_id),
	CONSTRAINT work_items_lead_project_fk FOREIGN KEY (lead_agent_id, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_items_program_project_fk FOREIGN KEY (program_id, project_id)
		REFERENCES decodex.programs(program_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_items_actor_project_fk FOREIGN KEY (last_changed_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_items_ids_canonical CHECK (
		work_item_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND last_correlation_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT work_items_text_bounded CHECK (
		decodex.is_work_item_text(title, 256)
		AND decodex.is_work_item_text(description, 4096)
		AND decodex.is_work_item_text(last_provenance, 4096)
	),
	CONSTRAINT work_items_criteria_bounded CHECK (
		decodex.is_work_item_criteria(acceptance_criteria)
		AND decodex.is_work_item_criteria(validation_criteria)
	),
	CONSTRAINT work_items_finite_times CHECK (
		pg_catalog.isfinite(created_at) AND pg_catalog.isfinite(updated_at)
		AND created_at >= TIMESTAMPTZ 'epoch'
		AND updated_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND updated_at >= created_at
	)
);

CREATE TABLE decodex.work_item_objectives (
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	objective_id uuid NOT NULL,
	CONSTRAINT work_item_objectives_pkey PRIMARY KEY (work_item_id, objective_id),
	CONSTRAINT work_item_objectives_item_project_fk FOREIGN KEY (work_item_id, project_id)
		REFERENCES decodex.work_items(work_item_id, project_id) ON DELETE CASCADE,
	CONSTRAINT work_item_objectives_objective_project_fk FOREIGN KEY (objective_id, project_id)
		REFERENCES decodex.objectives(objective_id, project_id) ON DELETE RESTRICT
);

CREATE TABLE decodex.work_item_edges (
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	related_work_item_id uuid NOT NULL,
	kind decodex.work_item_edge_kind NOT NULL,
	CONSTRAINT work_item_edges_pkey PRIMARY KEY (work_item_id, related_work_item_id),
	CONSTRAINT work_item_edges_item_project_fk FOREIGN KEY (work_item_id, project_id)
		REFERENCES decodex.work_items(work_item_id, project_id) ON DELETE CASCADE,
	CONSTRAINT work_item_edges_related_project_fk FOREIGN KEY (related_work_item_id, project_id)
		REFERENCES decodex.work_items(work_item_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_item_edges_not_self CHECK (work_item_id <> related_work_item_id)
);
CREATE INDEX work_item_edges_related_idx
	ON decodex.work_item_edges(project_id, related_work_item_id, work_item_id);

CREATE TABLE decodex.work_item_readiness_blockers (
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	work_item_revision bigint NOT NULL CHECK (work_item_revision > 0),
	ordinal integer NOT NULL CHECK (ordinal BETWEEN 1 AND 16384),
	kind decodex.work_item_blocker_kind NOT NULL,
	subject_id uuid,
	observed_state text,
	recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT work_item_readiness_blockers_pkey
		PRIMARY KEY (work_item_id, work_item_revision, ordinal),
	CONSTRAINT work_item_readiness_blockers_item_revision_fk
		FOREIGN KEY (work_item_id, project_id, work_item_revision)
		REFERENCES decodex.work_items(work_item_id, project_id, revision) ON DELETE CASCADE,
	CONSTRAINT work_item_readiness_blockers_shape CHECK (
		(kind IN ('project_inactive', 'lead_inactive', 'dependency_cycle')
			AND subject_id IS NULL AND observed_state IS NOT NULL)
		OR (kind IN ('program_inactive', 'objective_inactive',
			'dependency_incomplete', 'blocker_active')
			AND subject_id IS NOT NULL AND observed_state IS NOT NULL)
	),
	CONSTRAINT work_item_readiness_blockers_state_bounded CHECK (
		pg_catalog.octet_length(observed_state) >= 1
		AND pg_catalog.octet_length(observed_state) <= 64
		AND observed_state COLLATE pg_catalog."C" ~ '^[a-z][a-z0-9_]{0,63}$'
	),
	CONSTRAINT work_item_readiness_blockers_finite_time CHECK (
		pg_catalog.isfinite(recorded_at)
		AND recorded_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.work_item_acceptances (
	acceptance_id uuid PRIMARY KEY,
	project_id uuid NOT NULL,
	work_item_id uuid NOT NULL,
	work_item_revision bigint NOT NULL CHECK (work_item_revision > 0),
	work_item_updated_at timestamptz NOT NULL,
	accepted_by uuid NOT NULL,
	acceptance_criteria text[] NOT NULL,
	validation_criteria text[] NOT NULL,
	criteria_provenance text NOT NULL,
	evidence_summary text NOT NULL,
	evidence_provenance text NOT NULL,
	correlation_id uuid NOT NULL,
	accepted_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT work_item_acceptances_exact_revision_unique
		UNIQUE (work_item_id, work_item_revision),
	CONSTRAINT work_item_acceptances_item_project_fk FOREIGN KEY (work_item_id, project_id)
		REFERENCES decodex.work_items(work_item_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_item_acceptances_lead_project_fk FOREIGN KEY (accepted_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT work_item_acceptances_ids_canonical CHECK (
		acceptance_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND correlation_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT work_item_acceptances_criteria_bounded CHECK (
		decodex.is_work_item_criteria(acceptance_criteria)
		AND decodex.is_work_item_criteria(validation_criteria)
	),
	CONSTRAINT work_item_acceptances_provenance_bounded CHECK (
		decodex.is_work_item_text(criteria_provenance, 4096)
		AND decodex.is_work_item_text(evidence_summary, 4096)
		AND decodex.is_work_item_text(evidence_provenance, 4096)
	),
	CONSTRAINT work_item_acceptances_chronology CHECK (
		pg_catalog.isfinite(work_item_updated_at) AND pg_catalog.isfinite(accepted_at)
		AND work_item_updated_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND accepted_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND accepted_at >= work_item_updated_at
	)
);

CREATE FUNCTION decodex.enforce_work_item_state()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.state <> 'inbox' OR NEW.revision <> 1 OR NEW.updated_at <> NEW.created_at THEN
			RAISE EXCEPTION 'new WorkItem must be inbox revision one'
				USING ERRCODE = '23514', CONSTRAINT = 'work_items_state_guard';
		END IF;
		RETURN NEW;
	END IF;
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'WorkItems cannot be deleted'
			USING ERRCODE = '23514', CONSTRAINT = 'work_items_state_guard';
	END IF;
	IF NEW.work_item_id <> OLD.work_item_id OR NEW.project_id <> OLD.project_id
		OR NEW.lead_agent_id <> OLD.lead_agent_id OR NEW.created_at <> OLD.created_at
		OR NEW.revision <> OLD.revision + 1 OR NEW.updated_at < OLD.updated_at
		OR NOT (
			(OLD.state = 'inbox' AND NEW.state IN ('inbox', 'planned', 'canceled'))
			OR (OLD.state = 'planned' AND NEW.state IN ('inbox', 'planned', 'ready', 'blocked', 'canceled'))
			OR (OLD.state = 'ready' AND NEW.state IN ('planned', 'blocked', 'canceled'))
			OR (OLD.state = 'running' AND NEW.state IN ('review', 'blocked', 'canceled'))
			OR (OLD.state = 'review' AND NEW.state IN ('blocked', 'canceled'))
			OR (OLD.state = 'blocked' AND NEW.state IN ('planned', 'ready', 'blocked', 'canceled'))
		)
	THEN
		RAISE EXCEPTION 'invalid WorkItem revision or lifecycle mutation'
			USING ERRCODE = '23514', CONSTRAINT = 'work_items_state_guard';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER work_items_state_guard
BEFORE INSERT OR UPDATE OR DELETE ON decodex.work_items
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_work_item_state();

CREATE FUNCTION decodex.enforce_work_item_command_owner()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
BEGIN
	SELECT pg_catalog.pg_get_userbyid(class.relowner) INTO owner_name
	FROM pg_catalog.pg_class AS class WHERE class.oid = TG_RELID;
	IF current_user::name <> owner_name THEN
		RAISE EXCEPTION 'WorkItem state is command-owned'
			USING ERRCODE = '42501', CONSTRAINT = 'work_item_command_owner';
	END IF;
	RETURN NULL;
END
$$;
CREATE TRIGGER work_items_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.work_items
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_work_item_command_owner();
CREATE TRIGGER work_item_objectives_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.work_item_objectives
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_work_item_command_owner();
CREATE TRIGGER work_item_edges_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.work_item_edges
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_work_item_command_owner();
CREATE TRIGGER work_item_readiness_blockers_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.work_item_readiness_blockers
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_work_item_command_owner();
CREATE TRIGGER work_item_acceptances_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.work_item_acceptances
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_work_item_command_owner();

CREATE FUNCTION decodex.forbid_work_item_acceptance_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'WorkItem acceptances are immutable'
		USING ERRCODE = '23514', CONSTRAINT = 'work_item_acceptances_immutable';
END
$$;
CREATE TRIGGER work_item_acceptances_immutable
BEFORE UPDATE OR DELETE ON decodex.work_item_acceptances
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_work_item_acceptance_mutation();

CREATE FUNCTION decodex.enforce_work_item_acceptance_coherence()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE item record;
BEGIN
	SELECT revision, updated_at, state, lead_agent_id, acceptance_criteria, validation_criteria
	INTO item FROM decodex.work_items
	WHERE work_item_id = NEW.work_item_id AND project_id = NEW.project_id;
	IF NOT FOUND OR item.revision <> NEW.work_item_revision OR item.updated_at <> NEW.work_item_updated_at
		OR item.state <> 'review' OR item.lead_agent_id <> NEW.accepted_by
		OR item.acceptance_criteria <> NEW.acceptance_criteria
		OR item.validation_criteria <> NEW.validation_criteria
	THEN
		RAISE EXCEPTION 'WorkItem acceptance does not bind the exact review revision'
			USING ERRCODE = '23514', CONSTRAINT = 'work_item_acceptance_coherence';
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER work_item_acceptance_coherence
AFTER INSERT ON decodex.work_item_acceptances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_work_item_acceptance_coherence();

CREATE FUNCTION decodex.enforce_work_item_event_namespace()
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
		linked := NEW.aggregate_kind = 'work_item'
			OR NEW.event_kind IN ('work_item_created', 'work_item_updated',
				'work_item_readiness_blocked', 'work_item_ready', 'work_item_accepted')
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "work_item" || @.kind == "work_item" ||
				exists(@.work_item) || exists(@.work_item_id) ||
				exists(@.readiness_blockers) || exists(@.acceptance)
			)');
		IF TG_OP = 'UPDATE' THEN
			linked := linked OR OLD.aggregate_kind = 'work_item'
				OR OLD.event_kind IN ('work_item_created', 'work_item_updated',
					'work_item_readiness_blocked', 'work_item_ready', 'work_item_accepted')
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "work_item" || @.kind == "work_item" ||
					exists(@.work_item) || exists(@.work_item_id) ||
					exists(@.readiness_blockers) || exists(@.acceptance)
				)');
		END IF;
	ELSE
		linked := NEW.aggregate_kind = 'work_item'
			OR pg_catalog.jsonb_path_exists(NEW.payload, '$.** ? (
				@.aggregate_kind == "work_item" || @.kind == "work_item" ||
				exists(@.work_item) || exists(@.work_item_id) ||
				exists(@.readiness_blockers) || exists(@.acceptance)
			)') OR EXISTS (
				SELECT 1 FROM pg_catalog.jsonb_path_query(
					NEW.payload, '$.**.activity_sequence'
				) AS link(value)
				JOIN decodex.activity AS activity ON activity.sequence = CASE
					WHEN link.value #>> '{}' ~ '^[0-9]+$' THEN (link.value #>> '{}')::bigint
				END WHERE activity.aggregate_kind = 'work_item'
			);
		IF TG_OP = 'UPDATE' THEN
			linked := linked OR OLD.aggregate_kind = 'work_item'
				OR pg_catalog.jsonb_path_exists(OLD.payload, '$.** ? (
					@.aggregate_kind == "work_item" || @.kind == "work_item" ||
					exists(@.work_item) || exists(@.work_item_id) ||
					exists(@.readiness_blockers) || exists(@.acceptance)
				)') OR EXISTS (
					SELECT 1 FROM pg_catalog.jsonb_path_query(
						OLD.payload, '$.**.activity_sequence'
					) AS link(value)
					JOIN decodex.activity AS activity ON activity.sequence = CASE
						WHEN link.value #>> '{}' ~ '^[0-9]+$' THEN (link.value #>> '{}')::bigint
					END WHERE activity.aggregate_kind = 'work_item'
				);
		END IF;
	END IF;
	IF linked AND current_user::name <> owner_name THEN
		IF TG_TABLE_NAME = 'activity' OR TG_OP = 'INSERT' THEN
			RAISE EXCEPTION 'WorkItem activity/outbox namespace is command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'work_item_event_namespace';
		ELSIF NEW.id IS DISTINCT FROM OLD.id
			OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload
			OR NEW.created_at IS DISTINCT FROM OLD.created_at
		THEN
			RAISE EXCEPTION 'WorkItem outbox authority fields are command-owned'
				USING ERRCODE = '42501', CONSTRAINT = 'work_item_event_namespace';
		END IF;
	END IF;
	RETURN NEW;
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
	RAISE EXCEPTION 'WorkItem activity/outbox link is malformed'
		USING ERRCODE = '42501', CONSTRAINT = 'work_item_event_namespace';
END
$$;
CREATE TRIGGER activity_work_item_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_work_item_event_namespace();
CREATE TRIGGER outbox_work_item_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_work_item_event_namespace();

CREATE FUNCTION decodex.work_item_document(p_work_item_id uuid)
RETURNS jsonb
LANGUAGE sql
STABLE
SET search_path = pg_catalog, decodex
AS $$
SELECT pg_catalog.jsonb_build_object(
	'work_item_id', item.work_item_id, 'project_id', item.project_id,
	'lead_agent_id', item.lead_agent_id, 'program_id', item.program_id,
	'objective_ids', COALESCE((SELECT pg_catalog.jsonb_agg(link.objective_id ORDER BY link.objective_id)
		FROM decodex.work_item_objectives AS link WHERE link.work_item_id = item.work_item_id), '[]'::jsonb),
	'edges', COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
		'kind', edge.kind, 'related_work_item_id', edge.related_work_item_id
	) ORDER BY edge.related_work_item_id) FROM decodex.work_item_edges AS edge
		WHERE edge.work_item_id = item.work_item_id), '[]'::jsonb),
	'title', item.title, 'description', item.description, 'priority', item.priority,
	'acceptance_criteria', item.acceptance_criteria,
	'validation_criteria', item.validation_criteria, 'state', item.state,
	'revision', item.revision, 'last_changed_by', item.last_changed_by,
	'last_correlation_id', item.last_correlation_id,
	'last_provenance', item.last_provenance,
	'created_at_microseconds', (EXTRACT(EPOCH FROM item.created_at)*1000000)::bigint,
	'updated_at_microseconds', (EXTRACT(EPOCH FROM item.updated_at)*1000000)::bigint,
	'accepted_revision', (SELECT pg_catalog.max(acceptance.work_item_revision)
		FROM decodex.work_item_acceptances AS acceptance
		WHERE acceptance.work_item_id = item.work_item_id)
) FROM decodex.work_items AS item WHERE item.work_item_id = p_work_item_id
$$;

CREATE FUNCTION decodex.complete_exact_work_item_rejection(
	p_protocol text, p_idempotency_key text, p_code text
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE effect_value jsonb;
DECLARE response_value bytea;
DECLARE request_value jsonb;
BEGIN
	SELECT request_envelope INTO STRICT request_value FROM decodex.exact_command_receipts
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;
	effect_value := pg_catalog.jsonb_build_object(
		'changed', false, 'code', p_code, 'request', request_value
	);
	response_value := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification', 'stable_domain_rejection', 'code', p_code, 'effect', effect_value
	)::text, 'UTF8');
	UPDATE decodex.exact_command_receipts
	SET receipt_state = 'completed_rejected', outcome_class = 'stable_domain_rejection',
		effect_envelope = effect_value, response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;
	RETURN response_value;
END
$$;

CREATE FUNCTION decodex.complete_exact_work_item_success(
	p_protocol text, p_idempotency_key text, p_event_kind text,
	p_work_item_id uuid, p_effect jsonb
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE item_value jsonb;
DECLARE activity_sequence bigint;
DECLARE activity_payload jsonb;
DECLARE outbox_id bigint;
DECLARE outbox_payload jsonb;
DECLARE effect_value jsonb;
DECLARE response_value bytea;
BEGIN
	item_value := decodex.work_item_document(p_work_item_id);
	activity_payload := pg_catalog.jsonb_build_object(
		'kind', 'work_item', 'work_item', item_value
	) || p_effect;
	INSERT INTO decodex.activity(
		aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload
	) VALUES (
		'work_item', p_work_item_id::text, (item_value->>'revision')::bigint,
		p_event_kind, p_idempotency_key, activity_payload
	) RETURNING sequence, payload INTO activity_sequence, activity_payload;
	outbox_payload := pg_catalog.jsonb_build_object(
		'activity_sequence', activity_sequence, 'event_kind', p_event_kind,
		'aggregate_kind', 'work_item', 'aggregate_id', p_work_item_id,
		'revision', (item_value->>'revision')::bigint, 'payload', activity_payload
	);
	INSERT INTO decodex.outbox(
		effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload
	) VALUES (
		'activity/' || activity_sequence::text, 'work_item', p_work_item_id::text,
		(item_value->>'revision')::bigint, outbox_payload
	) RETURNING id, payload INTO outbox_id, outbox_payload;
	effect_value := pg_catalog.jsonb_build_object(
		'work_item', item_value, 'activity_sequence', activity_sequence,
		'activity_payload', activity_payload, 'outbox_id', outbox_id,
		'outbox_payload', outbox_payload
	) || p_effect;
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

CREATE FUNCTION decodex.reserve_exact_work_item_command(
	p_protocol text, p_idempotency_key text, p_request jsonb
) RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE existing_request jsonb;
DECLARE existing_response bytea;
DECLARE inserted_count integer;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_request IS NULL THEN
		RAISE EXCEPTION 'exact WorkItem command identity is incomplete' USING ERRCODE = '22004';
	END IF;
	IF pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key)
	THEN RAISE EXCEPTION 'exact WorkItem command identity is invalid' USING ERRCODE = '22023';
	END IF;
	INSERT INTO decodex.exact_command_receipts(
		protocol_version, idempotency_key, request_envelope, request_digest, receipt_state
	) VALUES (
		p_protocol, p_idempotency_key, p_request,
		public.digest(pg_catalog.convert_to(p_request::text, 'UTF8'), 'sha256'), 'executing'
	) ON CONFLICT (protocol_version, idempotency_key) DO NOTHING;
	GET DIAGNOSTICS inserted_count = ROW_COUNT;
	IF inserted_count = 0 THEN
		SELECT request_envelope, response_bytes INTO existing_request, existing_response
		FROM decodex.exact_command_receipts
		WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key FOR UPDATE;
		IF existing_request <> p_request THEN
			RAISE EXCEPTION 'exact idempotency conflict' USING ERRCODE = 'DX001';
		END IF;
		IF existing_response IS NULL THEN
			RAISE EXCEPTION 'incomplete exact receipt is not replayable' USING ERRCODE = 'DX002';
		END IF;
		RETURN existing_response;
	END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.work_item_graph_cycle(p_project_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SET search_path = pg_catalog, decodex
AS $$
WITH RECURSIVE reach(origin, node) AS (
	SELECT edge.work_item_id, edge.related_work_item_id
	FROM decodex.work_item_edges AS edge WHERE edge.project_id = p_project_id
	UNION
	SELECT reach.origin, edge.related_work_item_id
	FROM reach JOIN decodex.work_item_edges AS edge
		ON edge.project_id = p_project_id AND edge.work_item_id = reach.node
)
SELECT EXISTS (SELECT 1 FROM reach WHERE origin = node)
$$;

CREATE FUNCTION decodex.work_item_readiness(p_work_item_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, decodex
AS $$
DECLARE item record;
DECLARE blockers jsonb := '[]'::jsonb;
BEGIN
	SELECT work.project_id, work.lead_agent_id, work.program_id,
		project.status AS project_state, lead.status AS lead_state, lead.role AS lead_role
	INTO STRICT item
	FROM decodex.work_items AS work
	JOIN decodex.projects AS project USING (project_id)
	JOIN decodex.agents AS lead ON lead.agent_id = work.lead_agent_id
	WHERE work.work_item_id = p_work_item_id;
	IF item.project_state <> 'active' THEN blockers := blockers || pg_catalog.jsonb_build_array(
		pg_catalog.jsonb_build_object('kind','project_inactive','subject_id',NULL,
			'observed_state',item.project_state)); END IF;
	IF item.lead_state <> 'active' OR item.lead_role <> 'lead' THEN
		blockers := blockers || pg_catalog.jsonb_build_array(
		pg_catalog.jsonb_build_object('kind','lead_inactive','subject_id',NULL,
			'observed_state',CASE WHEN item.lead_state <> 'active' THEN item.lead_state
				ELSE 'not_lead' END)); END IF;
	IF item.program_id IS NOT NULL THEN blockers := blockers || COALESCE((
		SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'kind','program_inactive','subject_id',program.program_id,
			'observed_state',program.state) ORDER BY program.program_id)
		FROM decodex.programs AS program WHERE program.program_id = item.program_id
			AND program.state <> 'active'), '[]'::jsonb); END IF;
	blockers := blockers || COALESCE((
		SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'kind','objective_inactive','subject_id',objective.objective_id,
			'observed_state',objective.state) ORDER BY objective.objective_id)
		FROM decodex.work_item_objectives AS link
		JOIN decodex.objectives AS objective USING (objective_id)
		WHERE link.work_item_id = p_work_item_id AND objective.state <> 'active'
	), '[]'::jsonb);
	blockers := blockers || COALESCE((
		SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'kind', CASE edge.kind WHEN 'depends_on' THEN 'dependency_incomplete'
				ELSE 'blocker_active' END,
			'subject_id', related.work_item_id, 'observed_state', related.state
		) ORDER BY edge.related_work_item_id)
		FROM decodex.work_item_edges AS edge
		JOIN decodex.work_items AS related ON related.work_item_id = edge.related_work_item_id
		WHERE edge.work_item_id = p_work_item_id AND (
			(edge.kind = 'depends_on' AND related.state <> 'done') OR
			(edge.kind = 'blocked_by' AND related.state NOT IN ('done','canceled'))
		)
	), '[]'::jsonb);
	IF decodex.work_item_graph_cycle(item.project_id) THEN
		blockers := blockers || pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'kind','dependency_cycle','subject_id',NULL,'observed_state','cycle'));
	END IF;
	RETURN blockers;
END
$$;

CREATE FUNCTION decodex.create_work_item_exact(
	p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,
	p_lead_agent_id uuid, p_program_id uuid, p_objective_ids uuid[],
	p_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text,
	p_priority decodex.work_item_priority, p_acceptance_criteria text[],
	p_validation_criteria text[], p_actor_id uuid, p_correlation_id uuid, p_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE replay bytea;
DECLARE project_ok boolean;
DECLARE invalid_count bigint;
DECLARE operation_time timestamptz := pg_catalog.clock_timestamp();
BEGIN
	request_value := pg_catalog.jsonb_build_object(
		'protocol_version',p_protocol,'operation','create_work_item','work_item_id',p_work_item_id,
		'project_id',p_project_id,'lead_agent_id',p_lead_agent_id,'program_id',p_program_id,
		'objective_ids',p_objective_ids,'depends_on_ids',p_depends_on_ids,
		'blocked_by_ids',p_blocked_by_ids,'title',p_title,'description',p_description,
		'priority',p_priority,'acceptance_criteria',p_acceptance_criteria,
		'validation_criteria',p_validation_criteria,'actor_id',p_actor_id,
		'correlation_id',p_correlation_id,'provenance',p_provenance
	);
	replay := decodex.reserve_exact_work_item_command(p_protocol,p_idempotency_key,request_value);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_work_item_id IS NULL OR p_project_id IS NULL OR p_lead_agent_id IS NULL
		OR p_objective_ids IS NULL OR p_depends_on_ids IS NULL OR p_blocked_by_ids IS NULL
		OR p_title IS NULL OR p_description IS NULL OR p_priority IS NULL
		OR p_acceptance_criteria IS NULL OR p_validation_criteria IS NULL
		OR p_actor_id IS NULL OR p_correlation_id IS NULL OR p_provenance IS NULL
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	IF NOT decodex.is_work_item_text(p_title,256)
		OR NOT decodex.is_work_item_text(p_description,4096)
		OR NOT decodex.is_work_item_text(p_provenance,4096)
		OR NOT decodex.is_work_item_criteria(p_acceptance_criteria)
		OR NOT decodex.is_work_item_criteria(p_validation_criteria)
		OR pg_catalog.cardinality(p_objective_ids) > 32
		OR pg_catalog.cardinality(p_depends_on_ids) + pg_catalog.cardinality(p_blocked_by_ids) > 256
		OR COALESCE(pg_catalog.array_ndims(p_objective_ids),1) <> 1
		OR COALESCE(pg_catalog.array_ndims(p_depends_on_ids),1) <> 1
		OR COALESCE(pg_catalog.array_ndims(p_blocked_by_ids),1) <> 1
		OR pg_catalog.array_position(p_objective_ids,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_depends_on_ids,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_blocked_by_ids,NULL) IS NOT NULL
		OR p_work_item_id = ANY(p_depends_on_ids) OR p_work_item_id = ANY(p_blocked_by_ids)
		OR p_work_item_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		OR p_correlation_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	IF (SELECT pg_catalog.count(*) FROM pg_catalog.unnest(p_objective_ids) value)
		<> (SELECT pg_catalog.count(DISTINCT value) FROM pg_catalog.unnest(p_objective_ids) value)
		OR (SELECT pg_catalog.count(*) FROM pg_catalog.unnest(p_depends_on_ids || p_blocked_by_ids) value)
		<> (SELECT pg_catalog.count(DISTINCT value) FROM pg_catalog.unnest(p_depends_on_ids || p_blocked_by_ids) value)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'duplicate_relation'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1343,pg_catalog.hashtext(p_project_id::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1344,pg_catalog.hashtext(p_work_item_id::text));
	IF EXISTS (SELECT 1 FROM decodex.work_items WHERE work_item_id=p_work_item_id) THEN
		RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'duplicate_target'); END IF;
	IF (SELECT pg_catalog.count(*) FROM decodex.work_items WHERE project_id=p_project_id) >= 4096
		OR (SELECT pg_catalog.count(*) FROM decodex.work_item_edges WHERE project_id=p_project_id)
			+ pg_catalog.cardinality(p_depends_on_ids) + pg_catalog.cardinality(p_blocked_by_ids) > 16384
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	IF EXISTS (SELECT 1 FROM pg_catalog.unnest(p_objective_ids) id LEFT JOIN decodex.objectives objective
		ON objective.objective_id=id AND objective.project_id=p_project_id WHERE objective.objective_id IS NULL)
		OR (p_program_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM decodex.programs
			WHERE program_id=p_program_id AND project_id=p_project_id))
		OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_depends_on_ids||p_blocked_by_ids) id
			LEFT JOIN decodex.work_items related ON related.work_item_id=id AND related.project_id=p_project_id
			WHERE related.work_item_id IS NULL)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_relation'); END IF;
	PERFORM 1 FROM decodex.programs WHERE program_id=p_program_id FOR SHARE;
	PERFORM 1 FROM decodex.objectives WHERE objective_id=ANY(p_objective_ids)
		ORDER BY objective_id FOR SHARE;
	PERFORM 1 FROM decodex.work_items WHERE work_item_id=ANY(p_depends_on_ids||p_blocked_by_ids)
		ORDER BY work_item_id FOR SHARE;
	SELECT project.status = 'active' AND lead.role = 'lead' AND lead.status = 'active'
		AND lead.project_id = project.project_id AND lead.agent_id = p_lead_agent_id
		AND p_actor_id = lead.agent_id INTO project_ok
	FROM decodex.projects AS project JOIN decodex.agents AS lead ON lead.project_id=project.project_id
		AND lead.role='lead' WHERE project.project_id=p_project_id FOR SHARE OF project,lead;
	IF project_ok IS DISTINCT FROM true THEN
		RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_authority'); END IF;
	SELECT pg_catalog.count(*) INTO invalid_count FROM pg_catalog.unnest(p_objective_ids) id
	LEFT JOIN decodex.objectives objective ON objective.objective_id=id AND objective.project_id=p_project_id
	WHERE objective.objective_id IS NULL;
	IF invalid_count > 0 OR (p_program_id IS NOT NULL AND NOT EXISTS (
		SELECT 1 FROM decodex.programs WHERE program_id=p_program_id AND project_id=p_project_id
	)) OR EXISTS (
		SELECT 1 FROM pg_catalog.unnest(p_depends_on_ids || p_blocked_by_ids) id
		LEFT JOIN decodex.work_items related ON related.work_item_id=id AND related.project_id=p_project_id
		WHERE related.work_item_id IS NULL
	) THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_relation'); END IF;
	INSERT INTO decodex.work_items(work_item_id,project_id,lead_agent_id,program_id,title,
		description,priority,acceptance_criteria,validation_criteria,last_changed_by,
		last_correlation_id,last_provenance,created_at,updated_at)
	VALUES(p_work_item_id,p_project_id,p_lead_agent_id,p_program_id,p_title,p_description,
		p_priority,p_acceptance_criteria,p_validation_criteria,p_actor_id,p_correlation_id,
		p_provenance,operation_time,operation_time);
	INSERT INTO decodex.work_item_objectives(project_id,work_item_id,objective_id)
	SELECT p_project_id,p_work_item_id,id FROM pg_catalog.unnest(p_objective_ids) id ORDER BY id;
	INSERT INTO decodex.work_item_edges(project_id,work_item_id,related_work_item_id,kind)
	SELECT p_project_id,p_work_item_id,id,'depends_on'::decodex.work_item_edge_kind
	FROM pg_catalog.unnest(p_depends_on_ids) id
	UNION ALL SELECT p_project_id,p_work_item_id,id,'blocked_by'::decodex.work_item_edge_kind
	FROM pg_catalog.unnest(p_blocked_by_ids) id ORDER BY 3;
	RETURN decodex.complete_exact_work_item_success(p_protocol,p_idempotency_key,
		'work_item_created',p_work_item_id,pg_catalog.jsonb_build_object('request',request_value));
END
$$;

CREATE FUNCTION decodex.update_work_item_exact(
	p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,
	p_expected_revision bigint, p_program_id uuid, p_objective_ids uuid[],
	p_depends_on_ids uuid[], p_blocked_by_ids uuid[], p_title text, p_description text,
	p_priority decodex.work_item_priority, p_acceptance_criteria text[],
	p_validation_criteria text[], p_target_state decodex.work_item_state,
	p_actor_id uuid, p_correlation_id uuid, p_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE replay bytea;
DECLARE item record;
DECLARE project_state text;
DECLARE lead_state text;
DECLARE lead_role text;
BEGIN
	request_value := pg_catalog.jsonb_build_object(
		'protocol_version',p_protocol,'operation','update_work_item','work_item_id',p_work_item_id,
		'project_id',p_project_id,'expected_revision',p_expected_revision,'program_id',p_program_id,
		'objective_ids',p_objective_ids,'depends_on_ids',p_depends_on_ids,'blocked_by_ids',p_blocked_by_ids,
		'title',p_title,'description',p_description,'priority',p_priority,
		'acceptance_criteria',p_acceptance_criteria,'validation_criteria',p_validation_criteria,
		'target_state',p_target_state,'actor_id',p_actor_id,'correlation_id',p_correlation_id,
		'provenance',p_provenance);
	replay := decodex.reserve_exact_work_item_command(p_protocol,p_idempotency_key,request_value);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_work_item_id IS NULL OR p_project_id IS NULL OR p_expected_revision IS NULL
		OR p_objective_ids IS NULL OR p_depends_on_ids IS NULL OR p_blocked_by_ids IS NULL
		OR p_title IS NULL OR p_description IS NULL OR p_priority IS NULL
		OR p_acceptance_criteria IS NULL OR p_validation_criteria IS NULL
		OR p_target_state IS NULL OR p_actor_id IS NULL OR p_correlation_id IS NULL
		OR p_provenance IS NULL OR p_expected_revision <= 0
		OR p_work_item_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1343,pg_catalog.hashtext(p_project_id::text));
	SELECT work.* INTO item FROM decodex.work_items work
	WHERE work.work_item_id=p_work_item_id AND work.project_id=p_project_id FOR UPDATE OF work;
	IF NOT FOUND THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'missing_target'); END IF;
	IF item.revision <> p_expected_revision THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'stale_revision'); END IF;
	IF p_target_state IN ('ready','running','done') OR NOT (
		(item.state='inbox' AND p_target_state IN ('inbox','planned','canceled')) OR
		(item.state='planned' AND p_target_state IN ('inbox','planned','blocked','canceled')) OR
		(item.state='ready' AND p_target_state IN ('planned','blocked','canceled')) OR
		(item.state='running' AND p_target_state IN ('review','blocked','canceled')) OR
		(item.state='review' AND p_target_state IN ('blocked','canceled')) OR
		(item.state='blocked' AND p_target_state IN ('planned','canceled'))
	) THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'illegal_transition'); END IF;
	IF NOT decodex.is_work_item_text(p_title,256) OR NOT decodex.is_work_item_text(p_description,4096)
		OR NOT decodex.is_work_item_text(p_provenance,4096)
		OR NOT decodex.is_work_item_criteria(p_acceptance_criteria)
		OR NOT decodex.is_work_item_criteria(p_validation_criteria)
		OR pg_catalog.cardinality(p_objective_ids)>32
		OR pg_catalog.cardinality(p_depends_on_ids)+pg_catalog.cardinality(p_blocked_by_ids)>256
		OR COALESCE(pg_catalog.array_ndims(p_objective_ids),1) <> 1
		OR COALESCE(pg_catalog.array_ndims(p_depends_on_ids),1) <> 1
		OR COALESCE(pg_catalog.array_ndims(p_blocked_by_ids),1) <> 1
		OR pg_catalog.array_position(p_objective_ids,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_depends_on_ids,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_blocked_by_ids,NULL) IS NOT NULL
		OR p_work_item_id=ANY(p_depends_on_ids) OR p_work_item_id=ANY(p_blocked_by_ids)
		OR p_correlation_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	IF EXISTS (SELECT 1 FROM pg_catalog.unnest(p_objective_ids) id LEFT JOIN decodex.objectives objective
		ON objective.objective_id=id AND objective.project_id=p_project_id WHERE objective.objective_id IS NULL)
		OR (p_program_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM decodex.programs
			WHERE program_id=p_program_id AND project_id=p_project_id))
		OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_depends_on_ids||p_blocked_by_ids) id
			LEFT JOIN decodex.work_items related ON related.work_item_id=id AND related.project_id=p_project_id
			WHERE related.work_item_id IS NULL)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_relation'); END IF;
	PERFORM 1 FROM decodex.programs WHERE program_id=p_program_id FOR SHARE;
	PERFORM 1 FROM decodex.objectives WHERE objective_id=ANY(p_objective_ids)
		ORDER BY objective_id FOR SHARE;
	PERFORM 1 FROM decodex.work_items WHERE work_item_id=ANY(p_depends_on_ids||p_blocked_by_ids)
		ORDER BY work_item_id FOR SHARE;
	SELECT project.status,lead.status,lead.role INTO project_state,lead_state,lead_role
	FROM decodex.projects project JOIN decodex.agents lead
		ON lead.agent_id=item.lead_agent_id AND lead.project_id=project.project_id
	WHERE project.project_id=p_project_id FOR SHARE OF project,lead;
	IF project_state <> 'active' OR lead_state <> 'active' OR lead_role <> 'lead'
		OR p_actor_id <> item.lead_agent_id THEN
		RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_authority'); END IF;
	IF (SELECT pg_catalog.count(*) FROM pg_catalog.unnest(p_objective_ids) value)
		<> (SELECT pg_catalog.count(DISTINCT value) FROM pg_catalog.unnest(p_objective_ids) value)
		OR (SELECT pg_catalog.count(*) FROM pg_catalog.unnest(p_depends_on_ids||p_blocked_by_ids) value)
		<> (SELECT pg_catalog.count(DISTINCT value) FROM pg_catalog.unnest(p_depends_on_ids||p_blocked_by_ids) value)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'duplicate_relation'); END IF;
	IF EXISTS (SELECT 1 FROM pg_catalog.unnest(p_objective_ids) id LEFT JOIN decodex.objectives objective
		ON objective.objective_id=id AND objective.project_id=p_project_id WHERE objective.objective_id IS NULL)
		OR (p_program_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM decodex.programs
			WHERE program_id=p_program_id AND project_id=p_project_id))
		OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_depends_on_ids||p_blocked_by_ids) id
			LEFT JOIN decodex.work_items related ON related.work_item_id=id AND related.project_id=p_project_id
			WHERE related.work_item_id IS NULL)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_relation'); END IF;
	IF (SELECT pg_catalog.count(*) FROM decodex.work_items WHERE project_id=p_project_id) > 4096
		OR (SELECT pg_catalog.count(*) FROM decodex.work_item_edges
			WHERE project_id=p_project_id AND work_item_id<>p_work_item_id)
			+ pg_catalog.cardinality(p_depends_on_ids) + pg_catalog.cardinality(p_blocked_by_ids) > 16384
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	BEGIN
		DELETE FROM decodex.work_item_readiness_blockers WHERE work_item_id=p_work_item_id;
		DELETE FROM decodex.work_item_objectives WHERE work_item_id=p_work_item_id;
		DELETE FROM decodex.work_item_edges WHERE work_item_id=p_work_item_id;
		INSERT INTO decodex.work_item_objectives(project_id,work_item_id,objective_id)
		SELECT p_project_id,p_work_item_id,id FROM pg_catalog.unnest(p_objective_ids) id ORDER BY id;
		INSERT INTO decodex.work_item_edges(project_id,work_item_id,related_work_item_id,kind)
		SELECT p_project_id,p_work_item_id,id,'depends_on'::decodex.work_item_edge_kind
		FROM pg_catalog.unnest(p_depends_on_ids) id
		UNION ALL SELECT p_project_id,p_work_item_id,id,'blocked_by'::decodex.work_item_edge_kind
		FROM pg_catalog.unnest(p_blocked_by_ids) id ORDER BY 3;
		IF decodex.work_item_graph_cycle(p_project_id) THEN
			RAISE EXCEPTION 'candidate WorkItem graph contains a cycle' USING ERRCODE='P1343';
		END IF;
	EXCEPTION WHEN SQLSTATE 'P1343' THEN
		RETURN decodex.complete_exact_work_item_rejection(
			p_protocol,p_idempotency_key,'dependency_cycle');
	END;
	UPDATE decodex.work_items SET program_id=p_program_id,title=p_title,description=p_description,
		priority=p_priority,acceptance_criteria=p_acceptance_criteria,
		validation_criteria=p_validation_criteria,state=p_target_state,revision=revision+1,
		last_changed_by=p_actor_id,last_correlation_id=p_correlation_id,last_provenance=p_provenance,
		updated_at=pg_catalog.clock_timestamp() WHERE work_item_id=p_work_item_id;
	RETURN decodex.complete_exact_work_item_success(p_protocol,p_idempotency_key,
		'work_item_updated',p_work_item_id,pg_catalog.jsonb_build_object('request',request_value));
END
$$;

CREATE FUNCTION decodex.assess_work_item_readiness_exact(
	p_protocol text, p_idempotency_key text, p_work_item_id uuid, p_project_id uuid,
	p_expected_revision bigint, p_actor_id uuid, p_correlation_id uuid, p_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE replay bytea;
DECLARE item record;
DECLARE blockers jsonb;
DECLARE target_state decodex.work_item_state;
DECLARE event_kind text;
DECLARE project_state text;
DECLARE lead_state text;
DECLARE lead_role text;
BEGIN
	request_value := pg_catalog.jsonb_build_object('protocol_version',p_protocol,
		'operation','assess_work_item_readiness','work_item_id',p_work_item_id,
		'project_id',p_project_id,'expected_revision',p_expected_revision,
		'actor_id',p_actor_id,'correlation_id',p_correlation_id,'provenance',p_provenance);
	replay := decodex.reserve_exact_work_item_command(p_protocol,p_idempotency_key,request_value);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_work_item_id IS NULL OR p_project_id IS NULL OR p_expected_revision IS NULL
		OR p_actor_id IS NULL OR p_correlation_id IS NULL OR p_provenance IS NULL
		OR p_expected_revision <= 0 OR NOT decodex.is_work_item_text(p_provenance,4096)
		OR p_work_item_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		OR p_correlation_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1343,pg_catalog.hashtext(p_project_id::text));
	SELECT work.* INTO item FROM decodex.work_items work
	WHERE work.work_item_id=p_work_item_id AND work.project_id=p_project_id FOR UPDATE OF work;
	IF NOT FOUND THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'missing_target'); END IF;
	IF item.revision<>p_expected_revision THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'stale_revision'); END IF;
	IF item.state NOT IN ('planned','blocked') THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'illegal_transition'); END IF;
	IF p_actor_id<>item.lead_agent_id THEN
		RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_authority'); END IF;
	-- All authority inputs are selected again inside this transaction after the exact row lock.
	PERFORM 1 FROM decodex.programs WHERE program_id=item.program_id FOR SHARE;
	PERFORM 1 FROM decodex.objectives objective JOIN decodex.work_item_objectives link USING(objective_id)
		WHERE link.work_item_id=p_work_item_id ORDER BY objective.objective_id FOR SHARE OF objective;
	PERFORM 1 FROM decodex.work_items related JOIN decodex.work_item_edges edge
		ON edge.related_work_item_id=related.work_item_id WHERE edge.work_item_id=p_work_item_id
		ORDER BY related.work_item_id FOR SHARE OF related;
	SELECT project.status,lead.status,lead.role INTO project_state,lead_state,lead_role
	FROM decodex.projects project JOIN decodex.agents lead
		ON lead.agent_id=item.lead_agent_id AND lead.project_id=project.project_id
	WHERE project.project_id=p_project_id FOR SHARE OF project,lead;
	blockers := decodex.work_item_readiness(p_work_item_id);
	DELETE FROM decodex.work_item_readiness_blockers WHERE work_item_id=p_work_item_id;
	target_state := CASE WHEN pg_catalog.jsonb_array_length(blockers)=0 THEN 'ready' ELSE 'blocked' END;
	event_kind := CASE WHEN target_state='ready' THEN 'work_item_ready' ELSE 'work_item_readiness_blocked' END;
	UPDATE decodex.work_items SET state=target_state,revision=revision+1,last_changed_by=p_actor_id,
		last_correlation_id=p_correlation_id,last_provenance=p_provenance,
		updated_at=pg_catalog.clock_timestamp() WHERE work_item_id=p_work_item_id;
	IF target_state='blocked' THEN
		INSERT INTO decodex.work_item_readiness_blockers(project_id,work_item_id,work_item_revision,
			ordinal,kind,subject_id,observed_state)
		SELECT p_project_id,p_work_item_id,p_expected_revision+1,entry.ordinality,
			(entry.value->>'kind')::decodex.work_item_blocker_kind,
			(entry.value->>'subject_id')::uuid,entry.value->>'observed_state'
		FROM pg_catalog.jsonb_array_elements(blockers) WITH ORDINALITY AS entry(value,ordinality);
	END IF;
	RETURN decodex.complete_exact_work_item_success(p_protocol,p_idempotency_key,event_kind,
		p_work_item_id,pg_catalog.jsonb_build_object('request',request_value,
			'readiness_blockers',blockers,'ready',target_state='ready'));
END
$$;

CREATE FUNCTION decodex.accept_work_item_exact(
	p_protocol text, p_idempotency_key text, p_acceptance_id uuid,
	p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint,
	p_actor_id uuid, p_correlation_id uuid, p_provenance text, p_criteria_provenance text,
	p_evidence_summary text, p_evidence_provenance text
) RETURNS bytea
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request_value jsonb;
DECLARE replay bytea;
DECLARE item record;
DECLARE accepted_revision bigint;
BEGIN
	request_value := pg_catalog.jsonb_build_object('protocol_version',p_protocol,
		'operation','accept_work_item','acceptance_id',p_acceptance_id,
		'work_item_id',p_work_item_id,'project_id',p_project_id,
		'expected_revision',p_expected_revision,'actor_id',p_actor_id,
		'correlation_id',p_correlation_id,'criteria_provenance',p_criteria_provenance,
		'provenance',p_provenance,'evidence_summary',p_evidence_summary,
		'evidence_provenance',p_evidence_provenance);
	replay := decodex.reserve_exact_work_item_command(p_protocol,p_idempotency_key,request_value);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_acceptance_id IS NULL OR p_work_item_id IS NULL OR p_project_id IS NULL
		OR p_expected_revision IS NULL OR p_actor_id IS NULL OR p_correlation_id IS NULL
		OR p_provenance IS NULL OR p_criteria_provenance IS NULL OR p_evidence_summary IS NULL
		OR p_evidence_provenance IS NULL OR p_expected_revision <= 0
		OR p_acceptance_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		OR p_work_item_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		OR p_correlation_id::text COLLATE pg_catalog."C" !~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1345,pg_catalog.hashtext(p_acceptance_id::text));
	SELECT work.*,project.status AS project_state,lead.status AS lead_state,lead.role AS lead_role
	INTO item FROM decodex.work_items work JOIN decodex.projects project USING(project_id)
	JOIN decodex.agents lead ON lead.agent_id=work.lead_agent_id
	WHERE work.work_item_id=p_work_item_id AND work.project_id=p_project_id
	FOR UPDATE OF work FOR SHARE OF project,lead;
	IF NOT FOUND THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'missing_target'); END IF;
	IF item.revision<>p_expected_revision THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'stale_revision'); END IF;
	IF item.state<>'review' THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'illegal_transition'); END IF;
	IF item.project_state<>'active' OR item.lead_state<>'active' OR item.lead_role<>'lead'
		OR p_actor_id<>item.lead_agent_id THEN
		RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_authority'); END IF;
	IF NOT decodex.is_work_item_text(p_provenance,4096)
		OR NOT decodex.is_work_item_text(p_criteria_provenance,4096)
		OR NOT decodex.is_work_item_text(p_evidence_summary,4096)
		OR NOT decodex.is_work_item_text(p_evidence_provenance,4096)
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'invalid_input'); END IF;
	IF EXISTS (SELECT 1 FROM decodex.work_item_acceptances
		WHERE acceptance_id=p_acceptance_id OR (work_item_id=p_work_item_id AND work_item_revision=p_expected_revision))
	THEN RETURN decodex.complete_exact_work_item_rejection(p_protocol,p_idempotency_key,'duplicate_acceptance'); END IF;
	INSERT INTO decodex.work_item_acceptances(acceptance_id,project_id,work_item_id,
		work_item_revision,work_item_updated_at,accepted_by,acceptance_criteria,
		validation_criteria,criteria_provenance,evidence_summary,evidence_provenance,correlation_id)
	VALUES(p_acceptance_id,p_project_id,p_work_item_id,p_expected_revision,item.updated_at,
		p_actor_id,item.acceptance_criteria,item.validation_criteria,p_criteria_provenance,
		p_evidence_summary,p_evidence_provenance,p_correlation_id)
	RETURNING work_item_revision INTO accepted_revision;
	-- Acceptance intentionally does not update WorkItem state or revision. Completion is unowned.
	RETURN decodex.complete_exact_work_item_success(p_protocol,p_idempotency_key,
		'work_item_accepted',p_work_item_id,pg_catalog.jsonb_build_object(
			'request',request_value,'acceptance_recorded',pg_catalog.jsonb_build_object(
				'acceptance_id',p_acceptance_id,'work_item_revision',accepted_revision)));
END
$$;

CREATE FUNCTION decodex.guard_work_item_running_resume(
	p_work_item_id uuid, p_project_id uuid, p_expected_revision bigint
) RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE item record;
DECLARE blockers jsonb;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock(1343,pg_catalog.hashtext(p_project_id::text));
	SELECT work.revision,work.state,work.program_id,work.lead_agent_id INTO item
	FROM decodex.work_items work
	WHERE work.work_item_id=p_work_item_id AND work.project_id=p_project_id FOR UPDATE OF work;
	IF NOT FOUND OR item.revision<>p_expected_revision OR item.state NOT IN ('ready','running') THEN
		RAISE EXCEPTION 'future WorkItem running/resume guard rejected current state'
			USING ERRCODE='55000',CONSTRAINT='work_item_running_resume_guard';
	END IF;
	PERFORM 1 FROM decodex.programs WHERE program_id=item.program_id FOR SHARE;
	PERFORM 1 FROM decodex.objectives objective JOIN decodex.work_item_objectives link USING(objective_id)
		WHERE link.work_item_id=p_work_item_id ORDER BY objective.objective_id FOR SHARE OF objective;
	PERFORM 1 FROM decodex.work_items related JOIN decodex.work_item_edges edge
		ON edge.related_work_item_id=related.work_item_id WHERE edge.work_item_id=p_work_item_id
		ORDER BY related.work_item_id FOR SHARE OF related;
	PERFORM 1 FROM decodex.projects project JOIN decodex.agents lead
		ON lead.agent_id=item.lead_agent_id AND lead.project_id=project.project_id
		WHERE project.project_id=p_project_id FOR SHARE OF project,lead;
	blockers := decodex.work_item_readiness(p_work_item_id);
	IF pg_catalog.jsonb_array_length(blockers)<>0 THEN
		RAISE EXCEPTION 'future WorkItem running/resume guard found current blockers'
			USING ERRCODE='55000',CONSTRAINT='work_item_running_resume_guard';
	END IF;
	-- Intentionally no transition, receipt, lease, assignment, dispatch, or side effect.
END
$$;

REVOKE ALL ON TABLE decodex.work_items, decodex.work_item_objectives,
	decodex.work_item_edges, decodex.work_item_readiness_blockers,
	decodex.work_item_acceptances FROM PUBLIC;
REVOKE ALL ON TYPE decodex.work_item_priority, decodex.work_item_state,
	decodex.work_item_edge_kind, decodex.work_item_blocker_kind FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
