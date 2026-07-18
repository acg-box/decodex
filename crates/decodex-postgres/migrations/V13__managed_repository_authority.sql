-- XY-1349 durable managed-repository authority.
-- External repository effects remain unimplemented. These relations persist only admission,
-- allocation, fenced preparation, transition-specific readback, and terminal reconciliation.
CREATE TYPE decodex.managed_repository_phase AS ENUM (
	'allocated', 'registered', 'ready', 'ambiguous'
);
CREATE TYPE decodex.repository_operation_kind AS ENUM (
	'register', 'worktree_ready', 'commit'
);
CREATE TYPE decodex.repository_operation_state AS ENUM (
	'possibly_effected', 'completed', 'ambiguous'
);
CREATE TYPE decodex.repository_ambiguity AS ENUM (
	'stale', 'foreign', 'replaced', 'dirty', 'rollback', 'no_effect', 'incomplete',
	'inconclusive'
);
CREATE TYPE decodex.repository_authority_transition_kind AS ENUM (
	'allocated', 'register_prepared', 'register_completed',
	'worktree_ready_prepared', 'worktree_ready_completed',
	'commit_prepared', 'commit_completed', 'operation_ambiguous'
);
CREATE TYPE decodex.repository_evidence_kind AS ENUM (
	'allocation', 'registration', 'worktree_ready', 'commit'
);

CREATE TABLE decodex.repository_admissions (
	repository_id uuid PRIMARY KEY,
	project_id uuid NOT NULL REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	admitted_identity text NOT NULL UNIQUE,
	admitted_base text NOT NULL,
	admission_descriptor_schema smallint NOT NULL,
	admission_descriptor_digest text NOT NULL,
	admission_descriptor jsonb NOT NULL,
	repository_absolute_path text NOT NULL UNIQUE,
	admitted_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_admissions_repository_id_canonical CHECK (
		repository_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT repository_admissions_identity_bounded CHECK (
		pg_catalog.octet_length(admitted_identity) BETWEEN 1 AND 256
		AND admitted_identity = pg_catalog.btrim(admitted_identity)
		AND admitted_identity COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	),
	CONSTRAINT repository_admissions_base_bounded CHECK (
		pg_catalog.octet_length(admitted_base) BETWEEN 1 AND 256
		AND admitted_base = pg_catalog.btrim(admitted_base)
		AND admitted_base COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	),
	CONSTRAINT repository_admissions_digest_canonical CHECK (
		admission_descriptor_digest COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
	),
	CONSTRAINT repository_admissions_descriptor_bounded CHECK (
		admission_descriptor_schema = 1
		AND pg_catalog.octet_length(admission_descriptor::text) BETWEEN 2 AND 2097152
		AND pg_catalog.jsonb_typeof(admission_descriptor) = 'object'
		AND pg_catalog.jsonb_typeof(admission_descriptor->'git_layout') = 'object'
		AND pg_catalog.jsonb_typeof(admission_descriptor->'observations') = 'array'
		AND pg_catalog.jsonb_array_length(admission_descriptor->'observations') BETWEEN 1 AND 256
	),
	CONSTRAINT repository_admissions_descriptor_complete CHECK (
		admission_descriptor = pg_catalog.jsonb_build_object(
			'schema', admission_descriptor_schema,
			'project_id', project_id,
			'repository_id', repository_id,
			'admitted_identity', admitted_identity,
			'admitted_base', admitted_base,
			'repository_absolute_path', repository_absolute_path,
			'git_layout', admission_descriptor->'git_layout',
			'observations', admission_descriptor->'observations',
			'digest', admission_descriptor_digest
		)
	),
	CONSTRAINT repository_admissions_git_layout_complete CHECK (COALESCE((
		admission_descriptor->'git_layout' ?& ARRAY[
			'registration_role','registration_id','repository_absolute_path',
			'worktree_git_entry_absolute_path','git_directory_absolute_path',
			'common_directory_absolute_path','objects_directory_absolute_path',
			'refs_directory_absolute_path','common_directory_file_absolute_path',
			'git_directory_backlink_file_absolute_path'
		]
		AND (admission_descriptor->'git_layout') - ARRAY[
			'registration_role','registration_id','repository_absolute_path',
			'worktree_git_entry_absolute_path','git_directory_absolute_path',
			'common_directory_absolute_path','objects_directory_absolute_path',
			'refs_directory_absolute_path','common_directory_file_absolute_path',
			'git_directory_backlink_file_absolute_path'
		] = '{}'::jsonb
		AND admission_descriptor#>>'{git_layout,registration_role}' IN (
			'primary_worktree','linked_worktree'
		)
		AND admission_descriptor#>>'{git_layout,repository_absolute_path}'
			= repository_absolute_path
		AND CASE admission_descriptor#>>'{git_layout,registration_role}'
			WHEN 'primary_worktree' THEN
				admission_descriptor#>'{git_layout,registration_id}' = 'null'::jsonb
			WHEN 'linked_worktree' THEN
				pg_catalog.jsonb_typeof(
					admission_descriptor#>'{git_layout,registration_id}'
				) = 'string'
				AND pg_catalog.octet_length(
					admission_descriptor#>>'{git_layout,registration_id}'
				) BETWEEN 1 AND 128
		END
	), false)),
	CONSTRAINT repository_admissions_path_bounded CHECK (
		pg_catalog.octet_length(repository_absolute_path) BETWEEN 2 AND 4096
		AND pg_catalog.left(repository_absolute_path, 1) = '/'
		AND repository_absolute_path COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND repository_absolute_path COLLATE pg_catalog."C" !~ '(^|/)\.{1,2}(/|$)'
		AND repository_absolute_path NOT LIKE '%//%'
	),
	CONSTRAINT repository_admissions_time_finite CHECK (
		pg_catalog.isfinite(admitted_at)
		AND admitted_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);
ALTER TABLE decodex.repository_admissions
	ADD CONSTRAINT repository_admissions_identity_project_unique UNIQUE (repository_id, project_id);

CREATE TABLE decodex.repository_operations (
	operation_id uuid PRIMARY KEY,
	descriptor_schema smallint NOT NULL,
	project_id uuid NOT NULL,
	repository_id uuid NOT NULL,
	admitted_identity text NOT NULL,
	admitted_base text NOT NULL,
	admission_descriptor_digest text NOT NULL,
	allocation_id uuid NOT NULL,
	worktree_id uuid NOT NULL,
	repository_absolute_path text NOT NULL,
	worktree_absolute_path text NOT NULL,
	expected_generation bigint NOT NULL,
	expected_authority_tip uuid NOT NULL,
	kind decodex.repository_operation_kind NOT NULL,
	payload jsonb NOT NULL,
	executor_contract_version integer NOT NULL,
	descriptor jsonb NOT NULL,
	assigned_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_operations_repository_fk FOREIGN KEY (repository_id)
		REFERENCES decodex.repository_admissions(repository_id) ON DELETE RESTRICT,
	CONSTRAINT repository_operations_ids_canonical CHECK (
		operation_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND allocation_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND worktree_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND expected_authority_tip::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT repository_operations_versions_positive CHECK (
		descriptor_schema = 1 AND expected_generation > 0
		AND executor_contract_version BETWEEN 1 AND 65535
	),
	CONSTRAINT repository_operations_descriptor_bounded CHECK (
		pg_catalog.octet_length(descriptor::text) BETWEEN 2 AND 1048576
		AND pg_catalog.octet_length(payload::text) BETWEEN 2 AND 262144
	),
	CONSTRAINT repository_operations_descriptor_complete CHECK (
		descriptor = pg_catalog.jsonb_build_object(
			'schema', descriptor_schema,
			'operation_id', operation_id,
			'project_id', project_id,
			'repository_id', repository_id,
			'admitted_identity', admitted_identity,
			'admitted_base', admitted_base,
			'admission_descriptor_digest', admission_descriptor_digest,
			'allocation_id', allocation_id,
			'worktree_id', worktree_id,
			'repository_absolute_path', repository_absolute_path,
			'worktree_absolute_path', worktree_absolute_path,
			'expected_generation', expected_generation,
			'expected_authority_tip', expected_authority_tip,
			'kind', kind,
			'payload', payload,
			'executor_contract_version', executor_contract_version
		)
	),
	CONSTRAINT repository_operations_payload_kind CHECK (COALESCE((
		payload->>'kind' = kind::text
		AND pg_catalog.jsonb_typeof(payload) = 'object'
		AND pg_catalog.octet_length(payload->>'expected_head') BETWEEN 1 AND 256
		AND payload->>'expected_head' = pg_catalog.btrim(payload->>'expected_head')
		AND payload->>'expected_head' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND CASE kind
			WHEN 'register' THEN
				payload ?& ARRAY['kind','expected_head','target']
				AND payload - ARRAY['kind','expected_head','target'] = '{}'::jsonb
				AND payload->'target' = pg_catalog.jsonb_build_object(
					'repository_id', repository_id,
					'worktree_id', worktree_id,
					'repository_absolute_path', repository_absolute_path,
					'worktree_absolute_path', worktree_absolute_path
				)
			WHEN 'worktree_ready' THEN
				payload ?& ARRAY['kind','expected_head','policy']
				AND payload - ARRAY['kind','expected_head','policy'] = '{}'::jsonb
				AND payload->>'policy' = 'exact_clean_worktree'
			WHEN 'commit' THEN
				payload ?& ARRAY['kind','expected_head','next_head','intent']
				AND payload - ARRAY['kind','expected_head','next_head','intent'] = '{}'::jsonb
				AND pg_catalog.octet_length(payload->>'next_head') BETWEEN 1 AND 256
				AND payload->>'next_head' = pg_catalog.btrim(payload->>'next_head')
				AND payload->>'next_head' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
				AND payload->>'next_head' <> payload->>'expected_head'
				AND pg_catalog.jsonb_typeof(payload->'intent') = 'object'
				AND payload->'intent' ?& ARRAY[
					'target_reference','tree','message','author','committer'
				]
				AND (payload->'intent') - ARRAY[
					'target_reference','tree','message','author','committer'
				] = '{}'::jsonb
				AND pg_catalog.octet_length(payload#>>'{intent,target_reference}') BETWEEN 1 AND 256
				AND pg_catalog.octet_length(payload#>>'{intent,tree}') BETWEEN 1 AND 256
				AND pg_catalog.octet_length(payload#>>'{intent,message}') BETWEEN 1 AND 16384
				AND payload#>>'{intent,target_reference}' = pg_catalog.btrim(
					payload#>>'{intent,target_reference}'
				)
				AND payload#>>'{intent,tree}' = pg_catalog.btrim(payload#>>'{intent,tree}')
				AND payload#>>'{intent,message}' !~ E'\\r'
				AND pg_catalog.jsonb_typeof(payload#>'{intent,author}') = 'object'
				AND pg_catalog.jsonb_typeof(payload#>'{intent,committer}') = 'object'
				AND payload#>'{intent,author}' ?& ARRAY[
					'name','email','timestamp_seconds','utc_offset_minutes'
				]
				AND (payload#>'{intent,author}') - ARRAY[
					'name','email','timestamp_seconds','utc_offset_minutes'
				] = '{}'::jsonb
				AND payload#>'{intent,committer}' ?& ARRAY[
					'name','email','timestamp_seconds','utc_offset_minutes'
				]
				AND (payload#>'{intent,committer}') - ARRAY[
					'name','email','timestamp_seconds','utc_offset_minutes'
				] = '{}'::jsonb
				AND pg_catalog.octet_length(payload#>>'{intent,author,name}') BETWEEN 1 AND 256
				AND pg_catalog.octet_length(payload#>>'{intent,author,email}') BETWEEN 1 AND 256
				AND pg_catalog.octet_length(payload#>>'{intent,committer,name}') BETWEEN 1 AND 256
				AND pg_catalog.octet_length(payload#>>'{intent,committer,email}') BETWEEN 1 AND 256
				AND payload#>>'{intent,author,name}' = pg_catalog.btrim(
					payload#>>'{intent,author,name}'
				)
				AND payload#>>'{intent,author,email}' = pg_catalog.btrim(
					payload#>>'{intent,author,email}'
				)
				AND payload#>>'{intent,committer,name}' = pg_catalog.btrim(
					payload#>>'{intent,committer,name}'
				)
				AND payload#>>'{intent,committer,email}' = pg_catalog.btrim(
					payload#>>'{intent,committer,email}'
				)
				AND payload#>>'{intent,author,name}' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
				AND payload#>>'{intent,author,email}' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
				AND payload#>>'{intent,committer,name}' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
				AND payload#>>'{intent,committer,email}' COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
				AND pg_catalog.jsonb_typeof(payload#>'{intent,author,timestamp_seconds}') = 'number'
				AND pg_catalog.jsonb_typeof(payload#>'{intent,committer,timestamp_seconds}') = 'number'
				AND payload#>>'{intent,author,timestamp_seconds}' COLLATE pg_catalog."C" ~ '^-?[0-9]+$'
				AND payload#>>'{intent,committer,timestamp_seconds}' COLLATE pg_catalog."C" ~ '^-?[0-9]+$'
				AND payload#>>'{intent,author,utc_offset_minutes}' COLLATE pg_catalog."C" ~ '^-?[0-9]+$'
				AND payload#>>'{intent,committer,utc_offset_minutes}' COLLATE pg_catalog."C" ~ '^-?[0-9]+$'
				AND (payload#>>'{intent,author,timestamp_seconds}')::bigint IS NOT NULL
				AND (payload#>>'{intent,committer,timestamp_seconds}')::bigint IS NOT NULL
				AND (payload#>>'{intent,author,utc_offset_minutes}')::integer BETWEEN -1439 AND 1439
				AND (payload#>>'{intent,committer,utc_offset_minutes}')::integer BETWEEN -1439 AND 1439
		END
	), false)),
	CONSTRAINT repository_operations_time_finite CHECK (
		pg_catalog.isfinite(assigned_at)
		AND assigned_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);
CREATE INDEX repository_operations_repository_assigned_idx
	ON decodex.repository_operations(repository_id, assigned_at, operation_id);

CREATE TABLE decodex.repository_operation_evidence (
	evidence_id uuid PRIMARY KEY,
	repository_id uuid NOT NULL REFERENCES decodex.repository_admissions(repository_id)
		ON DELETE RESTRICT,
	operation_id uuid REFERENCES decodex.repository_operations(operation_id) ON DELETE RESTRICT,
	kind decodex.repository_evidence_kind NOT NULL,
	evidence jsonb NOT NULL,
	recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_operation_evidence_scope CHECK (
		(kind = 'allocation' AND operation_id IS NULL)
		OR (kind <> 'allocation' AND operation_id IS NOT NULL)
	),
	CONSTRAINT repository_operation_evidence_bounded CHECK (
		pg_catalog.octet_length(evidence::text) BETWEEN 2 AND 4194304
	),
	CONSTRAINT repository_operation_evidence_classification CHECK (COALESCE((
		pg_catalog.jsonb_typeof(evidence) = 'object'
		AND CASE kind
			WHEN 'allocation' THEN evidence->>'classification' = 'positive'
				AND evidence->>'evidence_id' = evidence_id::text
				AND evidence ?& ARRAY[
					'classification','evidence_id','admission_descriptor',
					'vacant_worktree_absolute_path'
				]
				AND evidence - ARRAY[
					'classification','evidence_id','admission_descriptor',
					'vacant_worktree_absolute_path'
				] = '{}'::jsonb
				AND pg_catalog.jsonb_typeof(evidence->'admission_descriptor') = 'object'
			WHEN 'registration' THEN evidence->>'classification' IN (
				'exact_reciprocal','no_effect','missing_reciprocal','stale','foreign',
				'replaced','dirty','rollback','inconclusive'
			) AND (evidence->>'classification' <> 'exact_reciprocal'
				OR evidence#>>'{scope,evidence_id}' = evidence_id::text)
			WHEN 'worktree_ready' THEN evidence->>'classification' IN (
				'exact','no_effect','incomplete','stale','foreign','replaced','dirty',
				'rollback','inconclusive'
			) AND (evidence->>'classification' <> 'exact'
				OR evidence#>>'{scope,evidence_id}' = evidence_id::text)
			WHEN 'commit' THEN evidence->>'classification' IN (
				'exact','no_effect','incomplete','stale','foreign','replaced','dirty',
				'rollback','inconclusive'
			) AND (evidence->>'classification' <> 'exact'
				OR evidence#>>'{scope,evidence_id}' = evidence_id::text)
		END
	), false)),
	CONSTRAINT repository_operation_evidence_time_finite CHECK (
		pg_catalog.isfinite(recorded_at)
		AND recorded_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);
CREATE UNIQUE INDEX repository_operation_evidence_allocation_once
	ON decodex.repository_operation_evidence(repository_id) WHERE operation_id IS NULL;
CREATE UNIQUE INDEX repository_operation_evidence_terminal_once
	ON decodex.repository_operation_evidence(operation_id) WHERE operation_id IS NOT NULL;

CREATE TABLE decodex.repository_operation_results (
	operation_id uuid PRIMARY KEY REFERENCES decodex.repository_operations(operation_id)
		ON DELETE RESTRICT,
	repository_id uuid NOT NULL REFERENCES decodex.repository_admissions(repository_id)
		ON DELETE RESTRICT,
	state decodex.repository_operation_state NOT NULL,
	ambiguity decodex.repository_ambiguity,
	result jsonb,
	evidence_id uuid NOT NULL UNIQUE REFERENCES decodex.repository_operation_evidence(evidence_id)
		ON DELETE RESTRICT,
	completed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_operation_results_terminal CHECK (
		(state = 'completed' AND ambiguity IS NULL AND result IS NOT NULL)
		OR (state = 'ambiguous' AND ambiguity IS NOT NULL AND result IS NULL)
	),
	CONSTRAINT repository_operation_results_bounded CHECK (
		result IS NULL OR pg_catalog.octet_length(result::text) BETWEEN 2 AND 262144
	),
	CONSTRAINT repository_operation_results_time_finite CHECK (
		pg_catalog.isfinite(completed_at)
		AND completed_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.repository_operation_events (
	operation_id uuid NOT NULL REFERENCES decodex.repository_operations(operation_id)
		ON DELETE RESTRICT,
	ordinal smallint NOT NULL,
	repository_id uuid NOT NULL REFERENCES decodex.repository_admissions(repository_id)
		ON DELETE RESTRICT,
	state decodex.repository_operation_state NOT NULL,
	evidence_id uuid REFERENCES decodex.repository_operation_evidence(evidence_id)
		ON DELETE RESTRICT,
	recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_operation_events_pkey PRIMARY KEY (operation_id, ordinal),
	CONSTRAINT repository_operation_events_shape CHECK (
		(ordinal = 1 AND state = 'possibly_effected' AND evidence_id IS NULL)
		OR (ordinal = 2 AND state IN ('completed','ambiguous') AND evidence_id IS NOT NULL)
	),
	CONSTRAINT repository_operation_events_time_finite CHECK (
		pg_catalog.isfinite(recorded_at)
		AND recorded_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.repository_authority_transitions (
	repository_id uuid NOT NULL REFERENCES decodex.repository_admissions(repository_id)
		ON DELETE RESTRICT,
	generation bigint NOT NULL CHECK (generation > 0),
	authority_tip uuid NOT NULL UNIQUE,
	prior_generation bigint,
	prior_authority_tip uuid,
	transition_kind decodex.repository_authority_transition_kind NOT NULL,
	operation_id uuid REFERENCES decodex.repository_operations(operation_id) ON DELETE RESTRICT,
	evidence_id uuid REFERENCES decodex.repository_operation_evidence(evidence_id)
		ON DELETE RESTRICT,
	phase decodex.managed_repository_phase NOT NULL,
	ambiguity decodex.repository_ambiguity,
	head text NOT NULL,
	active_operation_id uuid REFERENCES decodex.repository_operations(operation_id)
		ON DELETE RESTRICT,
	recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT repository_authority_transitions_pkey PRIMARY KEY (repository_id, generation),
	CONSTRAINT repository_authority_transitions_tip_scope_unique
		UNIQUE (repository_id, authority_tip),
	CONSTRAINT repository_authority_transitions_generation_tip_unique
		UNIQUE (repository_id, generation, authority_tip),
	CONSTRAINT repository_authority_transitions_predecessor_fk
		FOREIGN KEY (repository_id, prior_generation, prior_authority_tip)
		REFERENCES decodex.repository_authority_transitions(
			repository_id, generation, authority_tip
		) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT repository_authority_transitions_chain CHECK (
		(generation = 1 AND prior_generation IS NULL AND prior_authority_tip IS NULL
			AND transition_kind = 'allocated' AND operation_id IS NULL
			AND evidence_id IS NOT NULL AND active_operation_id IS NULL)
		OR (generation > 1 AND prior_generation = generation - 1
			AND prior_authority_tip IS NOT NULL)
	),
	CONSTRAINT repository_authority_transitions_operation_shape CHECK (
		(transition_kind IN ('register_prepared','worktree_ready_prepared','commit_prepared')
			AND operation_id IS NOT NULL AND evidence_id IS NULL
			AND active_operation_id = operation_id)
		OR (transition_kind IN ('register_completed','worktree_ready_completed','commit_completed',
			'operation_ambiguous') AND operation_id IS NOT NULL AND evidence_id IS NOT NULL
			AND active_operation_id IS NULL)
		OR transition_kind = 'allocated'
	),
	CONSTRAINT repository_authority_transitions_phase_shape CHECK (
		(phase = 'ambiguous' AND ambiguity IS NOT NULL)
		OR (phase <> 'ambiguous' AND ambiguity IS NULL)
	),
	CONSTRAINT repository_authority_transitions_head_bounded CHECK (
		pg_catalog.octet_length(head) BETWEEN 1 AND 256
		AND head = pg_catalog.btrim(head) AND head COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	),
	CONSTRAINT repository_authority_transitions_time_finite CHECK (
		pg_catalog.isfinite(recorded_at)
		AND recorded_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);

CREATE TABLE decodex.managed_repositories (
	repository_id uuid PRIMARY KEY REFERENCES decodex.repository_admissions(repository_id)
		ON DELETE RESTRICT,
	project_id uuid NOT NULL,
	allocation_id uuid NOT NULL UNIQUE,
	worktree_id uuid NOT NULL UNIQUE,
	worktree_absolute_path text NOT NULL UNIQUE,
	phase decodex.managed_repository_phase NOT NULL,
	ambiguity decodex.repository_ambiguity,
	head text NOT NULL,
	generation bigint NOT NULL CHECK (generation > 0),
	authority_tip uuid NOT NULL,
	active_operation_id uuid UNIQUE REFERENCES decodex.repository_operations(operation_id)
		ON DELETE RESTRICT,
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT managed_repositories_admission_project_fk FOREIGN KEY (repository_id, project_id)
		REFERENCES decodex.repository_admissions(repository_id, project_id) ON DELETE RESTRICT,
	CONSTRAINT managed_repositories_ids_canonical CHECK (
		allocation_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND worktree_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT managed_repositories_authority_tip_fk
		FOREIGN KEY (repository_id, generation, authority_tip)
		REFERENCES decodex.repository_authority_transitions(
			repository_id, generation, authority_tip
		) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT managed_repositories_worktree_path_bounded CHECK (
		pg_catalog.octet_length(worktree_absolute_path) BETWEEN 2 AND 4096
		AND pg_catalog.left(worktree_absolute_path, 1) = '/'
		AND worktree_absolute_path COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND worktree_absolute_path COLLATE pg_catalog."C" !~ '(^|/)\.{1,2}(/|$)'
		AND worktree_absolute_path NOT LIKE '%//%'
	),
	CONSTRAINT managed_repositories_head_bounded CHECK (
		pg_catalog.octet_length(head) BETWEEN 1 AND 256
		AND head = pg_catalog.btrim(head) AND head COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	),
	CONSTRAINT managed_repositories_fence_shape CHECK (
		((phase = 'ambiguous' AND ambiguity IS NOT NULL AND active_operation_id IS NULL)
		OR (phase <> 'ambiguous' AND ambiguity IS NULL))
	),
	CONSTRAINT managed_repositories_time_finite CHECK (
		pg_catalog.isfinite(updated_at)
		AND updated_at BETWEEN TIMESTAMPTZ 'epoch'
			AND TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
	)
);
CREATE FUNCTION decodex.forbid_managed_repository_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'managed-repository admission and history are append-only'
		USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_append_only';
END
$$;

CREATE TRIGGER repository_admissions_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_admissions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();
CREATE TRIGGER repository_operations_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_operations
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();
CREATE TRIGGER repository_operation_evidence_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_operation_evidence
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();
CREATE TRIGGER repository_operation_results_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_operation_results
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();
CREATE TRIGGER repository_operation_events_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_operation_events
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();
CREATE TRIGGER repository_authority_transitions_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.repository_authority_transitions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_managed_repository_history_mutation();

CREATE FUNCTION decodex.enforce_managed_repository_projection()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE transition_row decodex.repository_authority_transitions%ROWTYPE;
DECLARE terminal_state decodex.repository_operation_state;
DECLARE operation_row decodex.repository_operations%ROWTYPE;
DECLARE result_row decodex.repository_operation_results%ROWTYPE;
BEGIN
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'managed-repository current authority cannot be deleted'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
	END IF;
	SELECT * INTO transition_row FROM decodex.repository_authority_transitions
	WHERE repository_id = NEW.repository_id AND generation = NEW.generation
		AND authority_tip = NEW.authority_tip;
	IF NOT FOUND OR transition_row.phase <> NEW.phase
		OR transition_row.ambiguity IS DISTINCT FROM NEW.ambiguity
		OR transition_row.head <> NEW.head
		OR transition_row.active_operation_id IS DISTINCT FROM NEW.active_operation_id
	THEN
		RAISE EXCEPTION 'managed-repository projection must equal its authority transition'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.generation <> 1 OR NEW.active_operation_id IS NOT NULL
			OR transition_row.transition_kind <> 'allocated'
		THEN
			RAISE EXCEPTION 'managed-repository projection must begin at allocation generation one'
				USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
		END IF;
		RETURN NEW;
	END IF;
	IF NEW.repository_id <> OLD.repository_id OR NEW.project_id <> OLD.project_id
		OR NEW.allocation_id <> OLD.allocation_id OR NEW.worktree_id <> OLD.worktree_id
		OR NEW.worktree_absolute_path <> OLD.worktree_absolute_path
		OR NEW.generation <> OLD.generation + 1
		OR transition_row.prior_generation <> OLD.generation
		OR transition_row.prior_authority_tip <> OLD.authority_tip
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'managed-repository projection update is not one monotonic transition'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
	END IF;
	IF OLD.active_operation_id IS NULL AND NEW.active_operation_id IS NOT NULL THEN
		SELECT * INTO operation_row FROM decodex.repository_operations
		WHERE operation_id = NEW.active_operation_id;
		IF NOT FOUND OR operation_row.repository_id <> NEW.repository_id
			OR operation_row.expected_generation <> OLD.generation
			OR operation_row.expected_authority_tip <> OLD.authority_tip
			OR operation_row.payload->>'expected_head' <> OLD.head
			OR NOT (CASE operation_row.kind
				WHEN 'register' THEN OLD.phase = 'allocated'
				WHEN 'worktree_ready' THEN OLD.phase = 'registered'
				WHEN 'commit' THEN OLD.phase = 'ready'
			END)
			OR NEW.phase <> OLD.phase OR NEW.head <> OLD.head
			OR transition_row.operation_id <> NEW.active_operation_id
			OR transition_row.transition_kind <> (CASE operation_row.kind
				WHEN 'register' THEN 'register_prepared'::decodex.repository_authority_transition_kind
				WHEN 'worktree_ready' THEN 'worktree_ready_prepared'::decodex.repository_authority_transition_kind
				WHEN 'commit' THEN 'commit_prepared'::decodex.repository_authority_transition_kind
			END) OR EXISTS (
			SELECT 1 FROM decodex.repository_operation_results
			WHERE operation_id = NEW.active_operation_id
		) OR NOT EXISTS (
			SELECT 1 FROM decodex.repository_operation_events
			WHERE operation_id = NEW.active_operation_id AND ordinal = 1
				AND state = 'possibly_effected'
		) THEN
			RAISE EXCEPTION 'managed-repository preparation is incomplete'
				USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
		END IF;
	ELSIF OLD.active_operation_id IS NOT NULL AND NEW.active_operation_id IS NULL THEN
		SELECT * INTO result_row FROM decodex.repository_operation_results
		WHERE operation_id = OLD.active_operation_id;
		terminal_state := result_row.state;
		SELECT * INTO operation_row FROM decodex.repository_operations
		WHERE operation_id = OLD.active_operation_id;
		IF terminal_state IS NULL OR operation_row.operation_id IS NULL
			OR result_row.repository_id <> NEW.repository_id
			OR result_row.evidence_id <> transition_row.evidence_id
			OR NOT EXISTS (
				SELECT 1 FROM decodex.repository_operation_evidence evidence
				WHERE evidence.evidence_id = result_row.evidence_id
					AND evidence.repository_id = NEW.repository_id
					AND evidence.operation_id = OLD.active_operation_id
					AND evidence.kind = (CASE operation_row.kind
						WHEN 'register' THEN 'registration'::decodex.repository_evidence_kind
						WHEN 'worktree_ready' THEN 'worktree_ready'::decodex.repository_evidence_kind
						WHEN 'commit' THEN 'commit'::decodex.repository_evidence_kind
					END)
			)
			OR transition_row.operation_id <> OLD.active_operation_id
			OR NOT EXISTS (SELECT 1 FROM decodex.repository_operation_events
				WHERE operation_id = OLD.active_operation_id AND ordinal = 2
					AND state = terminal_state AND evidence_id = result_row.evidence_id)
			OR (terminal_state = 'ambiguous' AND (
				NEW.phase <> 'ambiguous' OR NEW.ambiguity <> result_row.ambiguity
				OR NEW.head <> OLD.head OR transition_row.transition_kind <> 'operation_ambiguous'
			))
			OR (terminal_state = 'completed' AND (
				NEW.ambiguity IS NOT NULL
				OR transition_row.transition_kind <> (CASE operation_row.kind
					WHEN 'register' THEN 'register_completed'::decodex.repository_authority_transition_kind
					WHEN 'worktree_ready' THEN 'worktree_ready_completed'::decodex.repository_authority_transition_kind
					WHEN 'commit' THEN 'commit_completed'::decodex.repository_authority_transition_kind
				END)
				OR (CASE operation_row.kind
					WHEN 'register' THEN result_row.result <> pg_catalog.jsonb_build_object(
						'kind','registered','head',operation_row.payload->>'expected_head'
					) OR NEW.phase <> 'registered' OR NEW.head <> OLD.head
					WHEN 'worktree_ready' THEN result_row.result <> pg_catalog.jsonb_build_object(
						'kind','worktree_ready','head',operation_row.payload->>'expected_head'
					) OR NEW.phase <> 'ready' OR NEW.head <> OLD.head
					WHEN 'commit' THEN result_row.result <> pg_catalog.jsonb_build_object(
						'kind','committed','from',operation_row.payload->>'expected_head',
						'to',operation_row.payload->>'next_head'
					) OR NEW.phase <> 'ready'
						OR NEW.head <> operation_row.payload->>'next_head'
				END)
			))
		THEN
			RAISE EXCEPTION 'managed-repository reconciliation is incomplete'
				USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
		END IF;
	ELSE
		RAISE EXCEPTION 'managed-repository transition must set or clear one active fence'
			USING ERRCODE = '23514', CONSTRAINT = 'managed_repository_projection';
	END IF;
	RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER managed_repositories_projection_complete
AFTER INSERT OR UPDATE OR DELETE ON decodex.managed_repositories
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_managed_repository_projection();

CREATE FUNCTION decodex.enforce_repository_operation_scope()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1 FROM decodex.repository_admissions admission
		JOIN decodex.managed_repositories repository USING (repository_id)
		WHERE admission.repository_id = NEW.repository_id
			AND admission.project_id = NEW.project_id
			AND admission.admitted_identity = NEW.admitted_identity
			AND admission.admitted_base = NEW.admitted_base
			AND admission.admission_descriptor_digest = NEW.admission_descriptor_digest
			AND admission.repository_absolute_path = NEW.repository_absolute_path
			AND repository.allocation_id = NEW.allocation_id
			AND repository.worktree_id = NEW.worktree_id
			AND repository.worktree_absolute_path = NEW.worktree_absolute_path
			AND repository.generation = NEW.expected_generation + 1
			AND repository.active_operation_id = NEW.operation_id
			AND EXISTS (SELECT 1 FROM decodex.repository_operation_events event
				WHERE event.operation_id = NEW.operation_id AND event.ordinal = 1
					AND event.repository_id = NEW.repository_id
					AND event.state = 'possibly_effected' AND event.evidence_id IS NULL)
			AND EXISTS (SELECT 1 FROM decodex.repository_authority_transitions transition
				WHERE transition.repository_id = NEW.repository_id
					AND transition.generation = repository.generation
					AND transition.authority_tip = repository.authority_tip
					AND transition.operation_id = NEW.operation_id
					AND transition.active_operation_id = NEW.operation_id)
	) THEN
		RAISE EXCEPTION 'repository operation descriptor is outside admitted allocation scope'
			USING ERRCODE = '23514', CONSTRAINT = 'repository_operation_scope';
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER repository_operations_scope_complete
AFTER INSERT ON decodex.repository_operations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_repository_operation_scope();

CREATE FUNCTION decodex.enforce_repository_history_completeness()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
DECLARE target_operation_id uuid;
DECLARE target_repository_id uuid;
DECLARE target_evidence_id uuid;
DECLARE allocation_entry boolean := false;
DECLARE terminal_entry boolean := false;
BEGIN
	IF TG_TABLE_NAME = 'repository_authority_transitions' THEN
		IF NOT EXISTS (SELECT 1 FROM decodex.managed_repositories repository
			WHERE repository.repository_id = NEW.repository_id
				AND repository.generation = NEW.generation
				AND repository.authority_tip = NEW.authority_tip
				AND repository.phase = NEW.phase
				AND repository.ambiguity IS NOT DISTINCT FROM NEW.ambiguity
				AND repository.head = NEW.head
				AND repository.active_operation_id IS NOT DISTINCT FROM NEW.active_operation_id)
		THEN
			RAISE EXCEPTION 'repository authority transition has no exact current projection'
				USING ERRCODE = '23514', CONSTRAINT = 'repository_history_completeness';
		END IF;
		IF NEW.transition_kind = 'allocated' THEN
			allocation_entry := true;
			target_repository_id := NEW.repository_id;
			target_evidence_id := NEW.evidence_id;
		ELSIF NEW.transition_kind IN ('register_completed','worktree_ready_completed',
			'commit_completed','operation_ambiguous')
		THEN
			terminal_entry := true;
			target_operation_id := NEW.operation_id;
			target_repository_id := NEW.repository_id;
			target_evidence_id := NEW.evidence_id;
		ELSE
			RETURN NULL;
		END IF;
	ELSIF TG_TABLE_NAME = 'repository_operation_evidence' THEN
		IF NEW.operation_id IS NULL THEN
			allocation_entry := true;
		ELSE
			terminal_entry := true;
			target_operation_id := NEW.operation_id;
		END IF;
		target_repository_id := NEW.repository_id;
		target_evidence_id := NEW.evidence_id;
	ELSIF TG_TABLE_NAME = 'repository_operation_results' THEN
		terminal_entry := true;
		target_operation_id := NEW.operation_id;
		target_repository_id := NEW.repository_id;
		target_evidence_id := NEW.evidence_id;
	ELSIF TG_TABLE_NAME = 'repository_operation_events' THEN
		IF NEW.ordinal <> 2 THEN
			RETURN NULL;
		END IF;
		terminal_entry := true;
		target_operation_id := NEW.operation_id;
		target_repository_id := NEW.repository_id;
		target_evidence_id := NEW.evidence_id;
	ELSE
		RAISE EXCEPTION 'unexpected repository history completeness trigger source'
			USING ERRCODE = '23514', CONSTRAINT = 'repository_history_completeness';
	END IF;

	IF allocation_entry THEN
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.repository_operation_evidence evidence
			JOIN decodex.repository_admissions admission
				ON admission.repository_id = evidence.repository_id
			JOIN decodex.repository_authority_transitions transition
				ON transition.repository_id = evidence.repository_id
				AND transition.generation = 1
				AND transition.evidence_id = evidence.evidence_id
			JOIN decodex.managed_repositories repository
				ON repository.repository_id = transition.repository_id
				AND repository.generation = transition.generation
				AND repository.authority_tip = transition.authority_tip
			WHERE evidence.evidence_id = target_evidence_id
				AND evidence.repository_id = target_repository_id
				AND evidence.operation_id IS NULL AND evidence.kind = 'allocation'
				AND evidence.evidence = pg_catalog.jsonb_build_object(
					'classification','positive',
					'evidence_id',evidence.evidence_id,
					'admission_descriptor',admission.admission_descriptor,
					'vacant_worktree_absolute_path',repository.worktree_absolute_path
				)
				AND transition.transition_kind = 'allocated'
				AND transition.prior_generation IS NULL
				AND transition.prior_authority_tip IS NULL
				AND transition.operation_id IS NULL
				AND transition.phase = 'allocated' AND transition.ambiguity IS NULL
				AND transition.head = admission.admitted_base
				AND transition.active_operation_id IS NULL
				AND repository.project_id = admission.project_id
				AND repository.phase = 'allocated' AND repository.ambiguity IS NULL
				AND repository.head = admission.admitted_base
				AND repository.active_operation_id IS NULL
				AND pg_catalog.rtrim(repository.worktree_absolute_path, '/')
					<> pg_catalog.rtrim(admission.repository_absolute_path, '/')
		) THEN
			RAISE EXCEPTION 'allocation history has no exact admitted generation-one cluster'
				USING ERRCODE = '23514', CONSTRAINT = 'repository_history_completeness';
		END IF;
		RETURN NULL;
	END IF;

	IF terminal_entry THEN
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.repository_operations operation
			JOIN decodex.repository_operation_evidence evidence
				ON evidence.operation_id = operation.operation_id
				AND evidence.repository_id = operation.repository_id
			JOIN decodex.repository_operation_results result
				ON result.operation_id = operation.operation_id
				AND result.repository_id = operation.repository_id
				AND result.evidence_id = evidence.evidence_id
			JOIN decodex.repository_operation_events initial_event
				ON initial_event.operation_id = operation.operation_id
				AND initial_event.ordinal = 1
				AND initial_event.repository_id = operation.repository_id
			JOIN decodex.repository_operation_events terminal_event
				ON terminal_event.operation_id = operation.operation_id
				AND terminal_event.ordinal = 2
				AND terminal_event.repository_id = operation.repository_id
				AND terminal_event.evidence_id = evidence.evidence_id
				AND terminal_event.state = result.state
			JOIN decodex.repository_authority_transitions preparation
				ON preparation.repository_id = operation.repository_id
				AND preparation.generation = operation.expected_generation + 1
				AND preparation.operation_id = operation.operation_id
			JOIN decodex.repository_authority_transitions terminal_transition
				ON terminal_transition.repository_id = operation.repository_id
				AND terminal_transition.generation = operation.expected_generation + 2
				AND terminal_transition.operation_id = operation.operation_id
				AND terminal_transition.evidence_id = evidence.evidence_id
				AND terminal_transition.prior_generation = preparation.generation
				AND terminal_transition.prior_authority_tip = preparation.authority_tip
			JOIN decodex.managed_repositories repository
				ON repository.repository_id = terminal_transition.repository_id
				AND repository.generation = terminal_transition.generation
				AND repository.authority_tip = terminal_transition.authority_tip
			WHERE operation.operation_id = target_operation_id
				AND operation.repository_id = target_repository_id
				AND evidence.evidence_id = target_evidence_id
				AND evidence.kind = (CASE operation.kind
					WHEN 'register' THEN 'registration'::decodex.repository_evidence_kind
					WHEN 'worktree_ready' THEN 'worktree_ready'::decodex.repository_evidence_kind
					WHEN 'commit' THEN 'commit'::decodex.repository_evidence_kind
				END)
				AND initial_event.state = 'possibly_effected'
				AND initial_event.evidence_id IS NULL
				AND preparation.transition_kind = (CASE operation.kind
					WHEN 'register' THEN 'register_prepared'::decodex.repository_authority_transition_kind
					WHEN 'worktree_ready' THEN 'worktree_ready_prepared'::decodex.repository_authority_transition_kind
					WHEN 'commit' THEN 'commit_prepared'::decodex.repository_authority_transition_kind
				END)
				AND preparation.evidence_id IS NULL
				AND preparation.active_operation_id = operation.operation_id
				AND preparation.head = operation.payload->>'expected_head'
				AND terminal_transition.active_operation_id IS NULL
				AND repository.active_operation_id IS NULL
				AND repository.project_id = operation.project_id
				AND repository.allocation_id = operation.allocation_id
				AND repository.worktree_id = operation.worktree_id
				AND repository.worktree_absolute_path = operation.worktree_absolute_path
				AND repository.phase = terminal_transition.phase
				AND repository.ambiguity IS NOT DISTINCT FROM terminal_transition.ambiguity
				AND repository.head = terminal_transition.head
				AND (CASE result.state
					WHEN 'ambiguous' THEN
						terminal_transition.transition_kind = 'operation_ambiguous'
						AND terminal_transition.phase = 'ambiguous'
						AND terminal_transition.ambiguity = result.ambiguity
						AND terminal_transition.head = operation.payload->>'expected_head'
					WHEN 'completed' THEN
						terminal_transition.transition_kind = (CASE operation.kind
							WHEN 'register' THEN 'register_completed'::decodex.repository_authority_transition_kind
							WHEN 'worktree_ready' THEN 'worktree_ready_completed'::decodex.repository_authority_transition_kind
							WHEN 'commit' THEN 'commit_completed'::decodex.repository_authority_transition_kind
						END)
						AND terminal_transition.ambiguity IS NULL
						AND (CASE operation.kind
							WHEN 'register' THEN result.result = pg_catalog.jsonb_build_object(
								'kind','registered','head',operation.payload->>'expected_head'
							) AND terminal_transition.phase = 'registered'
								AND terminal_transition.head = operation.payload->>'expected_head'
							WHEN 'worktree_ready' THEN result.result = pg_catalog.jsonb_build_object(
								'kind','worktree_ready','head',operation.payload->>'expected_head'
							) AND terminal_transition.phase = 'ready'
								AND terminal_transition.head = operation.payload->>'expected_head'
							WHEN 'commit' THEN result.result = pg_catalog.jsonb_build_object(
								'kind','committed','from',operation.payload->>'expected_head',
								'to',operation.payload->>'next_head'
							) AND terminal_transition.phase = 'ready'
								AND terminal_transition.head = operation.payload->>'next_head'
						END)
				END)
		)
		THEN
			RAISE EXCEPTION 'terminal repository history has no exact reconciliation cluster'
				USING ERRCODE = '23514', CONSTRAINT = 'repository_history_completeness';
		END IF;
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER repository_operation_evidence_complete
AFTER INSERT ON decodex.repository_operation_evidence
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_repository_history_completeness();
CREATE CONSTRAINT TRIGGER repository_operation_results_complete
AFTER INSERT ON decodex.repository_operation_results
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_repository_history_completeness();
CREATE CONSTRAINT TRIGGER repository_operation_events_complete
AFTER INSERT ON decodex.repository_operation_events
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_repository_history_completeness();
CREATE CONSTRAINT TRIGGER repository_authority_transitions_complete
AFTER INSERT ON decodex.repository_authority_transitions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_repository_history_completeness();

-- The runtime adapter receives only the DML needed by its explicit transactions. Immutable
-- triggers deny rewrite/truncate, and no repository function is SECURITY DEFINER.
REVOKE ALL ON TABLE decodex.repository_admissions, decodex.managed_repositories,
	decodex.repository_authority_transitions, decodex.repository_operations,
	decodex.repository_operation_events, decodex.repository_operation_evidence,
	decodex.repository_operation_results FROM PUBLIC;
REVOKE ALL ON TYPE decodex.managed_repository_phase,
	decodex.repository_operation_kind, decodex.repository_operation_state,
	decodex.repository_ambiguity, decodex.repository_authority_transition_kind,
	decodex.repository_evidence_kind FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;
