-- XY-1316 persists inert Project-owned policy identity and immutable accepted revisions only.
CREATE FUNCTION decodex.is_policy_snapshot(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE entry record;
DECLARE field_count bigint;
BEGIN
	IF pg_catalog.jsonb_typeof(document) <> 'object' THEN
		RETURN false;
	END IF;
	SELECT count(*) INTO field_count FROM pg_catalog.jsonb_object_keys(document);
	IF field_count > 32 THEN RETURN false; END IF;
	FOR entry IN SELECT key, value FROM pg_catalog.jsonb_each(document)
	LOOP
		IF pg_catalog.octet_length(entry.key) NOT BETWEEN 1 AND 64
			OR entry.key COLLATE pg_catalog."C" !~ '^[a-z][a-z0-9_]*$'
			OR pg_catalog.jsonb_typeof(entry.value) NOT IN ('string', 'boolean')
			OR (
				pg_catalog.jsonb_typeof(entry.value) = 'string'
				AND (
					pg_catalog.octet_length(entry.value OPERATOR(pg_catalog.#>>) '{}') > 256
					OR (entry.value OPERATOR(pg_catalog.#>>) '{}') COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
					OR (entry.value OPERATOR(pg_catalog.#>>) '{}') COLLATE pg_catalog."C" ~ U&'[\0080-\009F]'
				)
			)
		THEN
			RETURN false;
		END IF;
	END LOOP;
	RETURN true;
END
$$;

ALTER TABLE decodex.agents
	ADD CONSTRAINT agents_id_project_unique UNIQUE (agent_id, project_id);

CREATE TABLE decodex.policies (
	policy_id uuid PRIMARY KEY,
	project_id uuid NOT NULL REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	current_revision bigint CHECK (current_revision > 0),
	CONSTRAINT policies_id_canonical_uuid_v4 CHECK (
		policy_id::pg_catalog.text COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT policies_identity_project_unique UNIQUE (policy_id, project_id),
	CONSTRAINT policies_finite_timestamp CHECK (pg_catalog.isfinite(created_at))
);

CREATE TABLE decodex.policy_revisions (
	policy_id uuid NOT NULL,
	project_id uuid NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	provenance text NOT NULL,
	snapshot jsonb NOT NULL,
	accepted_by uuid NOT NULL,
	accepted_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	supersedes_revision bigint,
	CONSTRAINT policy_revisions_primary PRIMARY KEY (policy_id, revision),
	CONSTRAINT policy_revisions_project_identity_unique UNIQUE (policy_id, project_id, revision),
	CONSTRAINT policy_revisions_policy_project_fk FOREIGN KEY (policy_id, project_id)
		REFERENCES decodex.policies(policy_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT policy_revisions_accepting_agent_project_fk FOREIGN KEY (accepted_by, project_id)
		REFERENCES decodex.agents(agent_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT policy_revisions_supersedes_fk FOREIGN KEY (policy_id, supersedes_revision)
		REFERENCES decodex.policy_revisions(policy_id, revision) ON DELETE RESTRICT,
	CONSTRAINT policy_revisions_lineage CHECK (
		(revision = 1 AND supersedes_revision IS NULL)
		OR (revision > 1 AND supersedes_revision = revision - 1)
	),
	CONSTRAINT policy_revisions_provenance_bounded CHECK (
		pg_catalog.octet_length(provenance) >= 1
		AND pg_catalog.octet_length(provenance) <= 256
		AND provenance COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND provenance COLLATE pg_catalog."C" !~ U&'[\0080-\009F]'
	),
	CONSTRAINT policy_revisions_provenance_no_credentials CHECK (
		NOT decodex.has_credential_material(provenance)
	),
	CONSTRAINT policy_revisions_snapshot_bounded CHECK (decodex.is_policy_snapshot(snapshot)),
	CONSTRAINT policy_revisions_snapshot_no_credentials CHECK (
		NOT decodex.has_credential_material(snapshot)
	),
	CONSTRAINT policy_revisions_finite_timestamp CHECK (pg_catalog.isfinite(accepted_at))
);

ALTER TABLE decodex.policies
	ADD CONSTRAINT policies_current_revision_fk
	FOREIGN KEY (policy_id, project_id, current_revision)
	REFERENCES decodex.policy_revisions(policy_id, project_id, revision)
	ON DELETE RESTRICT;

CREATE FUNCTION decodex.enforce_policy_identity_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	IF TG_OP = 'TRUNCATE' OR TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'Policy identities are retained'
			USING ERRCODE = '55000', CONSTRAINT = 'policies_retained';
	END IF;
	IF NEW.policy_id IS DISTINCT FROM OLD.policy_id
		OR NEW.project_id IS DISTINCT FROM OLD.project_id
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.current_revision IS NULL
		OR NEW.current_revision IS DISTINCT FROM coalesce(OLD.current_revision, 0) + 1
	THEN
		RAISE EXCEPTION 'Policy identity mutation is not a legal revision advance'
			USING ERRCODE = '23514', CONSTRAINT = 'policies_revision_advance';
	END IF;
	RETURN NEW;
END
$$;

CREATE FUNCTION decodex.forbid_policy_revision_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	RAISE EXCEPTION 'accepted Policy revisions are immutable'
		USING ERRCODE = '55000', CONSTRAINT = 'policy_revisions_immutable';
END
$$;

CREATE TRIGGER policies_state_guard
BEFORE UPDATE OR DELETE ON decodex.policies
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_policy_identity_state();
CREATE TRIGGER policies_truncate_forbidden
BEFORE TRUNCATE ON decodex.policies
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_policy_identity_state();
CREATE TRIGGER policy_revisions_immutable
BEFORE UPDATE OR DELETE ON decodex.policy_revisions
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_policy_revision_mutation();
CREATE TRIGGER policy_revisions_truncate_forbidden
BEFORE TRUNCATE ON decodex.policy_revisions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_policy_revision_mutation();

CREATE FUNCTION decodex.create_policy(
	p_policy_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text
)
RETURNS TABLE (
	policy_id uuid, project_id uuid, created_at timestamptz, current_revision bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_policy_id pg_catalog.uuid;
DECLARE canonical_project_id pg_catalog.uuid;
DECLARE stored_project_id pg_catalog.uuid;
BEGIN
	IF p_policy_id IS NULL OR p_project_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_policy_id := p_policy_id::pg_catalog.text::pg_catalog.uuid;
	canonical_project_id := p_project_id::pg_catalog.text::pg_catalog.uuid;
	PERFORM 1 FROM decodex.projects AS project
		WHERE project.project_id = canonical_project_id FOR KEY SHARE;
	IF NOT FOUND THEN
		RAISE EXCEPTION 'Policy requires an authoritative Project'
			USING ERRCODE = '23503', CONSTRAINT = 'policies_project_id_fkey';
	END IF;
	INSERT INTO decodex.policies (policy_id, project_id)
	VALUES (canonical_policy_id, canonical_project_id)
	ON CONFLICT ON CONSTRAINT policies_pkey DO NOTHING;
	SELECT stored.project_id INTO stored_project_id
	FROM decodex.policies AS stored
	WHERE stored.policy_id = canonical_policy_id
	FOR SHARE;
	IF stored_project_id IS DISTINCT FROM canonical_project_id THEN
		RAISE EXCEPTION 'Policy identity is already bound to another Project'
			USING ERRCODE = '23505', CONSTRAINT = 'policies_identity_project';
	END IF;
	RETURN QUERY
	SELECT stored.policy_id, stored.project_id, stored.created_at, stored.current_revision
	FROM decodex.policies AS stored
	WHERE stored.policy_id = canonical_policy_id;
END
$$;

CREATE FUNCTION decodex.accept_policy_revision(
	p_policy_id decodex.canonical_uuid_v4_text,
	p_project_id decodex.canonical_uuid_v4_text,
	p_revision bigint,
	p_provenance text,
	p_snapshot jsonb,
	p_accepted_by decodex.canonical_uuid_v4_text,
	p_supersedes_revision bigint
)
RETURNS TABLE (
	policy_id uuid, project_id uuid, revision bigint, provenance text, snapshot jsonb,
	accepted_by uuid, policy_created_at timestamptz, accepted_at timestamptz,
	supersedes_revision bigint, revision_accepted boolean, actual_revision bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_policy_id pg_catalog.uuid;
DECLARE canonical_project_id pg_catalog.uuid;
DECLARE canonical_accepting_agent_id pg_catalog.uuid;
DECLARE policy_row decodex.policies%ROWTYPE;
DECLARE existing decodex.policy_revisions%ROWTYPE;
DECLARE acceptance_time pg_catalog.timestamptz;
BEGIN
	IF p_policy_id IS NULL OR p_project_id IS NULL OR p_accepted_by IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_policy_id := p_policy_id::pg_catalog.text::pg_catalog.uuid;
	canonical_project_id := p_project_id::pg_catalog.text::pg_catalog.uuid;
	canonical_accepting_agent_id := p_accepted_by::pg_catalog.text::pg_catalog.uuid;
	SELECT stored.* INTO policy_row
	FROM decodex.policies AS stored
	WHERE stored.policy_id = canonical_policy_id
	FOR UPDATE;
	IF NOT FOUND THEN
		RAISE EXCEPTION 'Policy identity does not exist'
			USING ERRCODE = '23503', CONSTRAINT = 'policy_revisions_policy_project_fk';
	END IF;
	IF policy_row.project_id IS DISTINCT FROM canonical_project_id THEN
		RAISE EXCEPTION 'Policy revision cannot attach across Projects'
			USING ERRCODE = '23514', CONSTRAINT = 'policy_revisions_project_scope';
	END IF;
	SELECT stored.* INTO existing
	FROM decodex.policy_revisions AS stored
	WHERE stored.policy_id = canonical_policy_id AND stored.revision = p_revision;
	IF FOUND THEN
		IF existing.project_id = canonical_project_id
			AND existing.provenance = p_provenance
			AND existing.snapshot = p_snapshot
			AND existing.accepted_by = canonical_accepting_agent_id
			AND existing.supersedes_revision IS NOT DISTINCT FROM p_supersedes_revision
		THEN
			RETURN QUERY
			SELECT existing.policy_id, existing.project_id, existing.revision,
				existing.provenance, existing.snapshot, existing.accepted_by,
				policy_row.created_at, existing.accepted_at, existing.supersedes_revision,
				true, NULL::bigint;
			RETURN;
		END IF;
		RAISE EXCEPTION 'Policy revision identity was reused with conflicting immutable bytes'
			USING ERRCODE = '23505', CONSTRAINT = 'policy_revisions_conflicting_replay';
	END IF;
	IF p_revision IS NULL OR p_revision <= 0
		OR p_revision IS DISTINCT FROM coalesce(policy_row.current_revision, 0) + 1
		OR p_supersedes_revision IS DISTINCT FROM policy_row.current_revision
	THEN
		RETURN QUERY
		SELECT canonical_policy_id, canonical_project_id, NULL::bigint, NULL::text,
			NULL::jsonb, NULL::uuid, policy_row.created_at, NULL::timestamptz,
			NULL::bigint, false, policy_row.current_revision;
		RETURN;
	END IF;
	PERFORM 1 FROM decodex.projects AS project
	WHERE project.project_id = canonical_project_id AND project.status = 'active'
	FOR KEY SHARE;
	IF NOT FOUND THEN
		RAISE EXCEPTION 'Policy acceptance requires an active Project'
			USING ERRCODE = '23514', CONSTRAINT = 'policy_revisions_active_project';
	END IF;
	PERFORM 1 FROM decodex.agents AS accepting_agent
	WHERE accepting_agent.agent_id = canonical_accepting_agent_id
		AND accepting_agent.project_id = canonical_project_id
		AND accepting_agent.role = 'lead'
		AND accepting_agent.status = 'active'
	FOR KEY SHARE;
	IF NOT FOUND THEN
		RAISE EXCEPTION 'Policy acceptance requires the active canonical Project Lead'
			USING ERRCODE = '23514', CONSTRAINT = 'policy_revisions_accepting_authority';
	END IF;
	acceptance_time := pg_catalog.clock_timestamp();
	IF acceptance_time < policy_row.created_at THEN
		RAISE EXCEPTION 'Policy acceptance chronology is invalid'
			USING ERRCODE = '23514', CONSTRAINT = 'policy_revisions_chronology';
	END IF;
	INSERT INTO decodex.policy_revisions (
		policy_id, project_id, revision, provenance, snapshot, accepted_by,
		accepted_at, supersedes_revision
	) VALUES (
		canonical_policy_id, canonical_project_id, p_revision, p_provenance, p_snapshot,
		canonical_accepting_agent_id, acceptance_time, p_supersedes_revision
	);
	UPDATE decodex.policies AS stored
	SET current_revision = p_revision
	WHERE stored.policy_id = canonical_policy_id;
	RETURN QUERY
	SELECT stored.policy_id, stored.project_id, stored.revision, stored.provenance,
		stored.snapshot, stored.accepted_by, policy_row.created_at, stored.accepted_at,
		stored.supersedes_revision, true, NULL::bigint
	FROM decodex.policy_revisions AS stored
	WHERE stored.policy_id = canonical_policy_id AND stored.revision = p_revision;
END
$$;

ALTER FUNCTION decodex.is_policy_snapshot(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_policy_identity_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.forbid_policy_revision_mutation() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.create_policy(decodex.canonical_uuid_v4_text, decodex.canonical_uuid_v4_text)
	SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.accept_policy_revision(
	decodex.canonical_uuid_v4_text, decodex.canonical_uuid_v4_text, bigint, text, jsonb,
	decodex.canonical_uuid_v4_text, bigint
) SET search_path = pg_catalog, decodex;

REVOKE ALL ON TABLE decodex.policies, decodex.policy_revisions FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
