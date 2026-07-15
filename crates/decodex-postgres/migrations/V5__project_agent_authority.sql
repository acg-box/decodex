-- XY-1315 establishes inert Project and Agent identity authority only.
CREATE DOMAIN decodex.canonical_uuid_v4_text AS pg_catalog.text
COLLATE pg_catalog."C"
CONSTRAINT canonical_uuid_v4_text_exact CHECK (
	VALUE COLLATE pg_catalog."C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
);

CREATE TYPE decodex.project_status AS ENUM ('active', 'paused', 'archived');
CREATE TYPE decodex.agent_role AS ENUM ('advisor', 'lead');
CREATE TYPE decodex.agent_status AS ENUM ('active', 'paused', 'retired');

CREATE FUNCTION decodex.is_project_metadata(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE entry record;
DECLARE field_count bigint;
BEGIN
	IF jsonb_typeof(document) <> 'object' THEN
		RETURN false;
	END IF;
	SELECT count(*) INTO field_count FROM jsonb_object_keys(document);
	IF field_count > 32 THEN RETURN false; END IF;
	FOR entry IN SELECT key, value FROM jsonb_each(document)
	LOOP
		IF pg_catalog.octet_length(entry.key) NOT BETWEEN 1 AND 64
			OR entry.key COLLATE "C" !~ '^[a-z][a-z0-9_]*$'
			OR pg_catalog.jsonb_typeof(entry.value) NOT IN ('string', 'boolean')
			OR (
				pg_catalog.jsonb_typeof(entry.value) = 'string'
				AND (
					pg_catalog.octet_length(entry.value OPERATOR(pg_catalog.#>>) '{}') > 256
					OR (entry.value OPERATOR(pg_catalog.#>>) '{}') COLLATE "C" ~ '[[:cntrl:]]'
					OR (entry.value OPERATOR(pg_catalog.#>>) '{}') COLLATE "C" ~ U&'[\0080-\009F]'
				)
			)
		THEN
			RETURN false;
		END IF;
	END LOOP;
	RETURN true;
END
$$;

CREATE TABLE decodex.projects (
	project_id uuid PRIMARY KEY,
	repository_identity text NOT NULL UNIQUE,
	repository_root text NOT NULL UNIQUE,
	default_cwd text NOT NULL,
	status decodex.project_status NOT NULL DEFAULT 'active',
	metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT projects_repository_identity_canonical CHECK (
		length(repository_identity) >= 1
		AND length(repository_identity) <= 128
		AND repository_identity COLLATE "C" ~ '^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?(?:/[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?)*$'
	),
	CONSTRAINT projects_id_canonical_uuid_v4 CHECK (
		project_id::text COLLATE "C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT projects_paths_bounded_absolute CHECK (
		pg_catalog.octet_length(repository_root) >= 2
		AND pg_catalog.octet_length(repository_root) <= 4096
		AND pg_catalog.octet_length(default_cwd) >= 2
		AND pg_catalog.octet_length(default_cwd) <= 4096
		AND left(repository_root, 1) = '/'
		AND left(default_cwd, 1) = '/'
		AND repository_root !~ '(^|/)(\.|\.\.)(/|$)'
		AND default_cwd !~ '(^|/)(\.|\.\.)(/|$)'
		AND repository_root NOT LIKE '%//%'
		AND default_cwd NOT LIKE '%//%'
		AND pg_catalog.strpos(repository_root, E'\\') = 0
		AND pg_catalog.strpos(default_cwd, E'\\') = 0
		AND repository_root COLLATE "C" !~ '[[:cntrl:]]'
		AND default_cwd COLLATE "C" !~ '[[:cntrl:]]'
		AND repository_root COLLATE "C" !~ U&'[\0080-\009F]'
		AND default_cwd COLLATE "C" !~ U&'[\0080-\009F]'
		AND right(repository_root, 1) <> '/'
		AND right(default_cwd, 1) <> '/'
	),
	CONSTRAINT projects_default_cwd_contained CHECK (
		default_cwd = repository_root
		OR (
			left(default_cwd, length(repository_root)) = repository_root
			AND substring(default_cwd FROM length(repository_root) + 1 FOR 1) = '/'
		)
	),
	CONSTRAINT projects_metadata_bounded CHECK (decodex.is_project_metadata(metadata)),
	CONSTRAINT projects_metadata_no_credentials CHECK (NOT decodex.has_credential_material(metadata)),
	CONSTRAINT projects_finite_timestamps CHECK (isfinite(created_at) AND isfinite(updated_at))
);

CREATE TABLE decodex.agents (
	agent_id uuid PRIMARY KEY,
	role decodex.agent_role NOT NULL,
	project_id uuid REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	status decodex.agent_status NOT NULL DEFAULT 'active',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT agents_role_project_shape CHECK (
		(role = 'advisor' AND project_id IS NULL)
		OR (role = 'lead' AND project_id IS NOT NULL)
	),
	CONSTRAINT agents_id_canonical_uuid_v4 CHECK (
		agent_id::text COLLATE "C" ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT agents_finite_timestamps CHECK (isfinite(created_at) AND isfinite(updated_at))
);

CREATE UNIQUE INDEX agents_one_global_advisor_idx ON decodex.agents (role) WHERE role = 'advisor';
CREATE UNIQUE INDEX agents_one_lead_per_project_idx ON decodex.agents (project_id) WHERE role = 'lead';

CREATE FUNCTION decodex.bootstrap_advisor(p_agent_id decodex.canonical_uuid_v4_text)
RETURNS TABLE (agent_id uuid, role decodex.agent_role, project_id uuid, status decodex.agent_status, revision bigint)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_agent_id pg_catalog.uuid;
BEGIN
	IF p_agent_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_agent_id := p_agent_id::pg_catalog.text::pg_catalog.uuid;
	PERFORM pg_catalog.pg_advisory_xact_lock(1315, 1);
	INSERT INTO decodex.agents (agent_id, role) VALUES (canonical_agent_id, 'advisor')
	ON CONFLICT DO NOTHING;
	RETURN QUERY
	SELECT stored.agent_id, stored.role, stored.project_id, stored.status, stored.revision
	FROM decodex.agents AS stored WHERE stored.role = 'advisor';
END
$$;

CREATE FUNCTION decodex.create_project(
	p_project_id decodex.canonical_uuid_v4_text,
	p_repository_identity text,
	p_repository_root text,
	p_default_cwd text,
	p_metadata jsonb,
	p_lead_id decodex.canonical_uuid_v4_text
)
RETURNS TABLE (
	project_id uuid, repository_identity text, repository_root text, default_cwd text,
	project_status decodex.project_status, metadata jsonb, project_revision bigint,
	agent_id uuid, agent_role decodex.agent_role, agent_status decodex.agent_status, agent_revision bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_project_id pg_catalog.uuid;
DECLARE canonical_lead_id pg_catalog.uuid;
DECLARE identity_project_id uuid;
DECLARE repository_project_id uuid;
DECLARE selected_project_id uuid;
DECLARE lead_count bigint;
BEGIN
	IF p_project_id IS NULL OR p_lead_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_project_id := p_project_id::pg_catalog.text::pg_catalog.uuid;
	canonical_lead_id := p_lead_id::pg_catalog.text::pg_catalog.uuid;
	PERFORM pg_catalog.pg_advisory_xact_lock(1315, pg_catalog.hashtext(p_repository_identity));
	INSERT INTO decodex.projects (
		project_id, repository_identity, repository_root, default_cwd, metadata
	) VALUES (
		canonical_project_id, p_repository_identity, p_repository_root, p_default_cwd, p_metadata
	) ON CONFLICT DO NOTHING;

	SELECT stored.project_id INTO identity_project_id
	FROM decodex.projects AS stored
	WHERE stored.project_id = canonical_project_id;
	SELECT stored.project_id INTO repository_project_id
	FROM decodex.projects AS stored
	WHERE stored.repository_identity = p_repository_identity;

	IF identity_project_id IS NOT NULL AND identity_project_id = repository_project_id THEN
		selected_project_id := identity_project_id;
	ELSIF identity_project_id IS NULL AND repository_project_id IS NOT NULL THEN
		selected_project_id := repository_project_id;
	ELSE
		RAISE EXCEPTION 'Project and repository identities are already bound differently'
			USING ERRCODE = '23505', CONSTRAINT = 'projects_identity_pair';
	END IF;

	IF selected_project_id = canonical_project_id THEN
		INSERT INTO decodex.agents (agent_id, role, project_id)
		VALUES (canonical_lead_id, 'lead', selected_project_id)
		ON CONFLICT DO NOTHING;
	END IF;
	SELECT count(*) INTO lead_count
	FROM decodex.agents AS lead
	WHERE lead.project_id = selected_project_id AND lead.role = 'lead';
	IF lead_count <> 1 THEN
		RAISE EXCEPTION 'Project requires exactly one canonical Lead'
			USING ERRCODE = '23514', CONSTRAINT = 'project_canonical_lead';
	END IF;

	RETURN QUERY
	SELECT project.project_id, project.repository_identity, project.repository_root,
		project.default_cwd, project.status, project.metadata, project.revision,
		lead.agent_id, lead.role, lead.status, lead.revision
	FROM decodex.projects AS project
	JOIN decodex.agents AS lead ON lead.project_id = project.project_id AND lead.role = 'lead'
	WHERE project.project_id = selected_project_id;
END
$$;

CREATE FUNCTION decodex.transition_project(
	p_project_id decodex.canonical_uuid_v4_text,
	p_expected_revision bigint,
	p_status decodex.project_status
)
RETURNS TABLE (
	project_id uuid, repository_identity text, repository_root text, default_cwd text,
	project_status decodex.project_status, metadata jsonb, project_revision bigint,
	agent_id uuid, agent_role decodex.agent_role, agent_status decodex.agent_status, agent_revision bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
CALLED ON NULL INPUT
AS $$
DECLARE canonical_project_id pg_catalog.uuid;
DECLARE changed bigint;
BEGIN
	IF p_project_id IS NULL THEN
		RAISE EXCEPTION 'identity ingress requires canonical UUID-v4 text'
			USING ERRCODE = '23514', CONSTRAINT = 'canonical_uuid_v4_text_ingress';
	END IF;
	canonical_project_id := p_project_id::pg_catalog.text::pg_catalog.uuid;
	UPDATE decodex.projects AS project
	SET status = p_status, revision = project.revision + 1, updated_at = clock_timestamp()
	WHERE project.project_id = canonical_project_id
		AND project.revision = p_expected_revision
		AND (
			(project.status = 'active' AND p_status IN ('paused', 'archived'))
			OR (project.status = 'paused' AND p_status IN ('active', 'archived'))
		);
	GET DIAGNOSTICS changed = ROW_COUNT;
	IF changed <> 1 THEN RETURN; END IF;

	UPDATE decodex.agents AS lead
	SET status = CASE p_status
		WHEN 'active' THEN 'active'::decodex.agent_status
		WHEN 'paused' THEN 'paused'::decodex.agent_status
		WHEN 'archived' THEN 'retired'::decodex.agent_status
	END,
		revision = lead.revision + 1,
		updated_at = clock_timestamp()
	WHERE lead.project_id = canonical_project_id AND lead.role = 'lead' AND lead.revision = p_expected_revision;
	GET DIAGNOSTICS changed = ROW_COUNT;
	IF changed <> 1 THEN
		RAISE EXCEPTION 'Project canonical Lead revision differs'
			USING ERRCODE = '40001';
	END IF;

	RETURN QUERY
	SELECT project.project_id, project.repository_identity, project.repository_root,
		project.default_cwd, project.status, project.metadata, project.revision,
		lead.agent_id, lead.role, lead.status, lead.revision
	FROM decodex.projects AS project
	JOIN decodex.agents AS lead ON lead.project_id = project.project_id AND lead.role = 'lead'
	WHERE project.project_id = canonical_project_id;
END
$$;

ALTER FUNCTION decodex.is_project_metadata(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.create_project(decodex.canonical_uuid_v4_text, text, text, text, jsonb, decodex.canonical_uuid_v4_text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.transition_project(decodex.canonical_uuid_v4_text, bigint, decodex.project_status) SET search_path = pg_catalog, decodex;

REVOKE ALL ON TABLE decodex.projects, decodex.agents FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
REVOKE ALL ON TYPE
	decodex.account_state,
	decodex.outbox_state,
	decodex.effect_state,
	decodex.conversation_status,
	decodex.runtime_session_state,
	decodex.turn_role,
	decodex.side_effect_state,
	decodex.history_item_kind,
	decodex.history_item_status,
	decodex.turn_status,
	decodex.artifact_status,
	decodex.context_source_kind,
	decodex.transition_kind,
	decodex.context_source_disposition,
	decodex.command_receipt_state,
	decodex.canonical_uuid_v4_text,
	decodex.project_status,
	decodex.agent_role,
	decodex.agent_status
FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
