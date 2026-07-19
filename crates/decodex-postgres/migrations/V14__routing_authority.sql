-- XY-1356 complete routing-policy, compatibility-evidence, and candidate-snapshot authority.
-- This migration classifies immutable facts only. It creates no selection, waiting, wake,
-- continuation, dispatch, or live-routing authority.

CREATE TYPE decodex.routing_member_disposition AS ENUM ('included', 'excluded');
CREATE TYPE decodex.codex_capability AS ENUM (
	'initialize', 'account_read', 'thread_list', 'thread_read', 'thread_archive',
	'paginated_history', 'native_collaboration', 'thread_search'
);
CREATE TYPE decodex.capability_evidence_state AS ENUM (
	'supported', 'unsupported_schema_missing', 'unsupported_method_not_found',
	'unsupported_codex_rejected', 'unavailable_not_probed', 'unavailable_probe_failed',
	'degraded_legacy_history_only'
);
CREATE TYPE decodex.routing_blocker AS ENUM (
	'excluded_by_policy', 'account_from_future', 'account_stale', 'account_unavailable',
	'account_unknown', 'account_depleted', 'account_auth_failed', 'account_plugin_unready',
	'account_disabled',
	'evidence_missing', 'evidence_from_future', 'evidence_stale',
	'evidence_account_mismatch', 'evidence_profile_mismatch', 'evidence_build_mismatch',
	'quota_five_hour_missing', 'quota_five_hour_from_future', 'quota_five_hour_stale',
	'quota_five_hour_unknown', 'quota_five_hour_reset_elapsed', 'quota_five_hour_depleted',
	'quota_seven_day_missing',
	'quota_seven_day_from_future', 'quota_seven_day_stale', 'quota_seven_day_unknown',
	'quota_seven_day_reset_elapsed', 'quota_seven_day_depleted',
	'required_capability_unsatisfied'
);

CREATE TABLE decodex.routing_policy_heads (
	routing_policy_id uuid PRIMARY KEY,
	project_id uuid NOT NULL,
	current_revision bigint NOT NULL CHECK (current_revision > 0),
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT routing_policy_heads_project_fk FOREIGN KEY (project_id)
		REFERENCES decodex.projects(project_id) ON DELETE RESTRICT,
	CONSTRAINT routing_policy_heads_time CHECK (
		pg_catalog.isfinite(created_at) AND pg_catalog.isfinite(updated_at)
		AND updated_at >= created_at
	)
);

CREATE TABLE decodex.routing_policy_revisions (
	routing_policy_id uuid NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	project_id uuid NOT NULL,
	accepted_policy_id uuid NOT NULL,
	accepted_policy_revision bigint NOT NULL CHECK (accepted_policy_revision > 0),
	required_role decodex.role_profile_role NOT NULL,
	required_role_profile_revision bigint NOT NULL CHECK (required_role_profile_revision > 0),
	required_build_id text NOT NULL,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT routing_policy_revisions_pkey PRIMARY KEY (routing_policy_id, revision),
	CONSTRAINT routing_policy_revisions_head_fk FOREIGN KEY (routing_policy_id)
		REFERENCES decodex.routing_policy_heads(routing_policy_id) ON DELETE RESTRICT,
	CONSTRAINT routing_policy_revisions_policy_fk FOREIGN KEY (
		accepted_policy_id, project_id, accepted_policy_revision
	) REFERENCES decodex.policy_revisions(policy_id, project_id, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_policy_revisions_profile_fk FOREIGN KEY (
		required_role, required_role_profile_revision
	) REFERENCES decodex.role_profile_revisions(role, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_policy_revisions_build CHECK (
		pg_catalog.octet_length(required_build_id) BETWEEN 1 AND 256
		AND required_build_id COLLATE pg_catalog."C" ~ '^sha256:[0-9a-f]{64}$'
	),
	CONSTRAINT routing_policy_revisions_finite_time CHECK (pg_catalog.isfinite(created_at))
);

ALTER TABLE decodex.routing_policy_heads ADD CONSTRAINT routing_policy_heads_current_fk
	FOREIGN KEY (routing_policy_id, current_revision)
	REFERENCES decodex.routing_policy_revisions(routing_policy_id, revision)
	DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE decodex.routing_policy_members (
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL,
	position integer NOT NULL CHECK (position > 0),
	account_id uuid NOT NULL,
	account_revision bigint NOT NULL CHECK (account_revision > 0),
	disposition decodex.routing_member_disposition NOT NULL,
	CONSTRAINT routing_policy_members_pkey PRIMARY KEY (
		routing_policy_id, routing_policy_revision, position
	),
	CONSTRAINT routing_policy_members_account_unique UNIQUE (
		routing_policy_id, routing_policy_revision, account_id
	),
	CONSTRAINT routing_policy_members_revision_fk FOREIGN KEY (
		routing_policy_id, routing_policy_revision
	) REFERENCES decodex.routing_policy_revisions(routing_policy_id, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_policy_members_account_fk FOREIGN KEY (account_id)
		REFERENCES decodex.accounts(account_id) ON DELETE RESTRICT
);

CREATE TABLE decodex.routing_policy_required_capabilities (
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL,
	position integer NOT NULL CHECK (position > 0),
	capability decodex.codex_capability NOT NULL,
	CONSTRAINT routing_policy_required_capabilities_pkey PRIMARY KEY (
		routing_policy_id, routing_policy_revision, position
	),
	CONSTRAINT routing_policy_required_capabilities_unique UNIQUE (
		routing_policy_id, routing_policy_revision, capability
	),
	CONSTRAINT routing_policy_required_capabilities_revision_fk FOREIGN KEY (
		routing_policy_id, routing_policy_revision
	) REFERENCES decodex.routing_policy_revisions(routing_policy_id, revision) ON DELETE RESTRICT
);

CREATE TABLE decodex.routing_compatibility_evidence (
	evidence_id uuid PRIMARY KEY,
	account_id uuid NOT NULL,
	account_revision bigint NOT NULL CHECK (account_revision > 0),
	role decodex.role_profile_role NOT NULL,
	role_profile_revision bigint NOT NULL CHECK (role_profile_revision > 0),
	build_id text NOT NULL,
	process_id uuid NOT NULL,
	process_account_id uuid NOT NULL,
	schema_fingerprint text NOT NULL,
	evidence_revision bigint NOT NULL,
	ingested_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CONSTRAINT routing_compatibility_evidence_account_revision_unique UNIQUE (
		account_id, evidence_revision
	),
	CONSTRAINT routing_compatibility_evidence_process_unique UNIQUE (process_id),
	CONSTRAINT routing_compatibility_evidence_account_fk FOREIGN KEY (account_id)
		REFERENCES decodex.accounts(account_id) ON DELETE RESTRICT,
	CONSTRAINT routing_compatibility_evidence_profile_fk FOREIGN KEY (
		role, role_profile_revision
	) REFERENCES decodex.role_profile_revisions(role, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_compatibility_evidence_process_account CHECK (
		process_account_id = account_id
	),
	CONSTRAINT routing_compatibility_evidence_build CHECK (
		build_id COLLATE pg_catalog."C" ~ '^sha256:[0-9a-f]{64}$'
		AND schema_fingerprint COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
	),
	CONSTRAINT routing_compatibility_evidence_revision CHECK (evidence_revision > 0),
	CONSTRAINT routing_compatibility_evidence_finite_time CHECK (pg_catalog.isfinite(ingested_at))
);

CREATE TABLE decodex.routing_capability_evidence (
	evidence_id uuid NOT NULL,
	position integer NOT NULL CHECK (position BETWEEN 1 AND 8),
	capability decodex.codex_capability NOT NULL,
	state decodex.capability_evidence_state NOT NULL,
	CONSTRAINT routing_capability_evidence_pkey PRIMARY KEY (evidence_id, position),
	CONSTRAINT routing_capability_evidence_capability_unique UNIQUE (evidence_id, capability),
	CONSTRAINT routing_capability_evidence_identity CHECK (
		(position = 1 AND capability = 'initialize')
		OR (position = 2 AND capability = 'account_read')
		OR (position = 3 AND capability = 'thread_list')
		OR (position = 4 AND capability = 'thread_read')
		OR (position = 5 AND capability = 'thread_archive')
		OR (position = 6 AND capability = 'paginated_history')
		OR (position = 7 AND capability = 'native_collaboration')
		OR (position = 8 AND capability = 'thread_search')
	),
	CONSTRAINT routing_capability_evidence_parent_fk FOREIGN KEY (evidence_id)
		REFERENCES decodex.routing_compatibility_evidence(evidence_id) ON DELETE RESTRICT
);

CREATE TABLE decodex.routing_snapshots (
	snapshot_id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL,
	accepted_policy_id uuid NOT NULL,
	accepted_policy_revision bigint NOT NULL,
	required_role decodex.role_profile_role NOT NULL,
	required_role_profile_revision bigint NOT NULL CHECK (required_role_profile_revision > 0),
	required_build_id text NOT NULL,
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL,
	runtime_session_id uuid NOT NULL,
	runtime_session_revision bigint NOT NULL,
	account_snapshot_id uuid NOT NULL,
	account_snapshot_source_revision bigint NOT NULL CHECK (account_snapshot_source_revision > 0),
	profile_snapshot_id uuid NOT NULL,
	profile_snapshot_source_revision bigint NOT NULL CHECK (profile_snapshot_source_revision > 0),
	resolved_at timestamptz NOT NULL,
	CONSTRAINT routing_snapshots_policy_fk FOREIGN KEY (
		routing_policy_id, routing_policy_revision
	) REFERENCES decodex.routing_policy_revisions(routing_policy_id, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_accepted_policy_fk FOREIGN KEY (
		accepted_policy_id, accepted_policy_revision
	) REFERENCES decodex.policy_revisions(policy_id, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_profile_fk FOREIGN KEY (
		required_role, required_role_profile_revision
	) REFERENCES decodex.role_profile_revisions(role, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_build CHECK (
		required_build_id COLLATE pg_catalog."C" ~ '^sha256:[0-9a-f]{64}$'
	),
	CONSTRAINT routing_snapshots_managed_run_fk FOREIGN KEY (managed_run_id)
		REFERENCES decodex.managed_runs(managed_run_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_session_fk FOREIGN KEY (
		runtime_session_id, runtime_session_revision
	) REFERENCES decodex.runtime_sessions(runtime_session_id, revision) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_account_snapshot_fk FOREIGN KEY (account_snapshot_id)
		REFERENCES decodex.account_snapshots(account_snapshot_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_profile_snapshot_fk FOREIGN KEY (profile_snapshot_id)
		REFERENCES decodex.profile_snapshots(profile_snapshot_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshots_finite_time CHECK (pg_catalog.isfinite(resolved_at))
);

CREATE TABLE decodex.routing_snapshot_members (
	snapshot_id uuid NOT NULL,
	position integer NOT NULL CHECK (position > 0),
	account_id uuid NOT NULL,
	disposition decodex.routing_member_disposition NOT NULL,
	account_revision bigint NOT NULL CHECK (account_revision > 0),
	display_label text NOT NULL,
	account_state decodex.account_state NOT NULL,
	account_observed_at_utc text NOT NULL,
	evidence_id uuid,
	evidence_revision bigint CHECK (evidence_revision > 0),
	evidence_account_revision bigint CHECK (evidence_account_revision > 0),
	evidence_role decodex.role_profile_role,
	evidence_role_profile_revision bigint CHECK (evidence_role_profile_revision > 0),
	evidence_build_id text,
	process_id uuid,
	schema_fingerprint text,
	sticky boolean NOT NULL,
	blockers decodex.routing_blocker[] NOT NULL,
	CONSTRAINT routing_snapshot_members_pkey PRIMARY KEY (snapshot_id, position),
	CONSTRAINT routing_snapshot_members_account_unique UNIQUE (snapshot_id, account_id),
	CONSTRAINT routing_snapshot_members_snapshot_fk FOREIGN KEY (snapshot_id)
		REFERENCES decodex.routing_snapshots(snapshot_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshot_members_account_fk FOREIGN KEY (account_id)
		REFERENCES decodex.accounts(account_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshot_members_evidence_fk FOREIGN KEY (evidence_id)
		REFERENCES decodex.routing_compatibility_evidence(evidence_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshot_members_blocker_disposition CHECK (
		(disposition = 'excluded') = ('excluded_by_policy' = ANY(blockers))
	),
	CONSTRAINT routing_snapshot_members_evidence_pair CHECK (
		(evidence_id IS NULL) = (evidence_revision IS NULL)
		AND (evidence_id IS NULL) = (evidence_account_revision IS NULL)
		AND (evidence_id IS NULL) = (evidence_role IS NULL)
		AND (evidence_id IS NULL) = (evidence_role_profile_revision IS NULL)
		AND (evidence_id IS NULL) = (evidence_build_id IS NULL)
		AND (evidence_id IS NULL) = (process_id IS NULL)
		AND (evidence_id IS NULL) = (schema_fingerprint IS NULL)
	),
	CONSTRAINT routing_snapshot_members_facts CHECK (
		account_observed_at_utc COLLATE pg_catalog."C"
			~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[.][0-9]{6}Z$'
		AND pg_catalog.octet_length(display_label) BETWEEN 1 AND 128
		AND NOT decodex.has_credential_material(display_label)
		AND (evidence_build_id IS NULL
			OR evidence_build_id COLLATE pg_catalog."C" ~ '^sha256:[0-9a-f]{64}$')
		AND (schema_fingerprint IS NULL
			OR schema_fingerprint COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$')
	)
);

CREATE TABLE decodex.routing_snapshot_quota_facts (
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position smallint NOT NULL CHECK (position IN (1, 2)),
	window_class decodex.quota_window_class NOT NULL,
	duration_minutes smallint NOT NULL,
	observation_revision bigint,
	remaining_percent smallint,
	resets_at_micros bigint,
	observed_at_micros bigint,
	confidence decodex.observation_confidence,
	CONSTRAINT routing_snapshot_quota_facts_pkey PRIMARY KEY (snapshot_id, account_id, position),
	CONSTRAINT routing_snapshot_quota_facts_member_fk FOREIGN KEY (snapshot_id, account_id)
		REFERENCES decodex.routing_snapshot_members(snapshot_id, account_id) ON DELETE RESTRICT,
	CONSTRAINT routing_snapshot_quota_facts_identity CHECK (
		(position = 1 AND window_class = 'five_hour' AND duration_minutes = 300)
		OR (position = 2 AND window_class = 'seven_day' AND duration_minutes = 10080)
	),
	CONSTRAINT routing_snapshot_quota_facts_observation CHECK (
		(remaining_percent IS NULL OR remaining_percent BETWEEN 0 AND 100)
		AND (observation_revision IS NULL) = (observed_at_micros IS NULL)
		AND (observation_revision IS NULL) = (confidence IS NULL)
		AND (observation_revision IS NOT NULL
			OR remaining_percent IS NULL AND resets_at_micros IS NULL)
		AND (observation_revision IS NULL OR observation_revision > 0)
		AND (observed_at_micros IS NULL OR observed_at_micros >= 0)
		AND (resets_at_micros IS NULL OR resets_at_micros >= 0)
	)
);

CREATE TABLE decodex.routing_snapshot_capability_facts (
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position smallint NOT NULL CHECK (position BETWEEN 1 AND 8),
	capability decodex.codex_capability NOT NULL,
	applicable boolean NOT NULL,
	evidence_state decodex.capability_evidence_state,
	CONSTRAINT routing_snapshot_capability_facts_pkey PRIMARY KEY (
		snapshot_id, account_id, position
	),
	CONSTRAINT routing_snapshot_capability_facts_unique UNIQUE (
		snapshot_id, account_id, capability
	),
	CONSTRAINT routing_snapshot_capability_facts_identity CHECK (
		(position = 1 AND capability = 'initialize')
		OR (position = 2 AND capability = 'account_read')
		OR (position = 3 AND capability = 'thread_list')
		OR (position = 4 AND capability = 'thread_read')
		OR (position = 5 AND capability = 'thread_archive')
		OR (position = 6 AND capability = 'paginated_history')
		OR (position = 7 AND capability = 'native_collaboration')
		OR (position = 8 AND capability = 'thread_search')
	),
	CONSTRAINT routing_snapshot_capability_facts_member_fk FOREIGN KEY (snapshot_id, account_id)
		REFERENCES decodex.routing_snapshot_members(snapshot_id, account_id) ON DELETE RESTRICT
);

CREATE TABLE decodex.routing_snapshot_blockers (
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position integer NOT NULL CHECK (position > 0),
	blocker decodex.routing_blocker NOT NULL,
	CONSTRAINT routing_snapshot_blockers_pkey PRIMARY KEY (snapshot_id, account_id, position),
	CONSTRAINT routing_snapshot_blockers_unique UNIQUE (snapshot_id, account_id, blocker),
	CONSTRAINT routing_snapshot_blockers_member_fk FOREIGN KEY (snapshot_id, account_id)
		REFERENCES decodex.routing_snapshot_members(snapshot_id, account_id) ON DELETE RESTRICT
);

CREATE FUNCTION decodex.forbid_routing_history_mutation()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
BEGIN
	RAISE EXCEPTION 'V14 routing history is immutable'
		USING ERRCODE = '55000', CONSTRAINT = 'routing_history_immutable';
END
$$;

CREATE FUNCTION decodex.enforce_routing_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE member_count bigint; capability_count bigint; account_count bigint;
DECLARE quota_count bigint; matrix_count bigint; blocker_count bigint; blocker_array_count bigint;
BEGIN
	IF TG_TABLE_NAME = 'routing_policy_revisions' THEN
		SELECT pg_catalog.count(*) INTO member_count FROM decodex.routing_policy_members
		WHERE routing_policy_id = NEW.routing_policy_id AND routing_policy_revision = NEW.revision;
		SELECT pg_catalog.count(*) INTO account_count FROM decodex.accounts;
		IF member_count <> account_count OR EXISTS (
			SELECT 1 FROM decodex.routing_policy_members
			WHERE routing_policy_id = NEW.routing_policy_id AND routing_policy_revision = NEW.revision
			GROUP BY routing_policy_id, routing_policy_revision
			HAVING pg_catalog.min(position) <> 1 OR pg_catalog.max(position) <> pg_catalog.count(*)
		) OR EXISTS (
			SELECT account_id FROM decodex.accounts EXCEPT
			SELECT account_id FROM decodex.routing_policy_members
			WHERE routing_policy_id = NEW.routing_policy_id AND routing_policy_revision = NEW.revision
		) THEN
			RAISE EXCEPTION 'routing policy revision is not a complete account inventory'
				USING ERRCODE = '23514', CONSTRAINT = 'routing_policy_complete_inventory';
		END IF;
		IF EXISTS (
			SELECT 1 FROM decodex.routing_policy_required_capabilities
			WHERE routing_policy_id = NEW.routing_policy_id AND routing_policy_revision = NEW.revision
			GROUP BY routing_policy_id, routing_policy_revision
			HAVING pg_catalog.min(position) <> 1 OR pg_catalog.max(position) <> pg_catalog.count(*)
				OR pg_catalog.array_agg(capability ORDER BY position)
					<> pg_catalog.array_agg(capability ORDER BY capability)
		) THEN
			RAISE EXCEPTION 'required capability positions are not contiguous'
				USING ERRCODE = '23514', CONSTRAINT = 'routing_policy_capabilities_contiguous';
		END IF;
	ELSIF TG_TABLE_NAME = 'routing_compatibility_evidence' THEN
		SELECT pg_catalog.count(*) INTO capability_count FROM decodex.routing_capability_evidence
		WHERE evidence_id = NEW.evidence_id;
		IF capability_count <> 8 OR EXISTS (
			SELECT 1 FROM decodex.routing_capability_evidence WHERE evidence_id = NEW.evidence_id
			GROUP BY evidence_id HAVING pg_catalog.min(position) <> 1
				OR pg_catalog.max(position) <> 8 OR pg_catalog.count(*) <> 8
		) THEN
			RAISE EXCEPTION 'compatibility evidence must contain the closed capability projection'
				USING ERRCODE = '23514', CONSTRAINT = 'routing_evidence_complete_capabilities';
		END IF;
	ELSE
		SELECT pg_catalog.count(*) INTO member_count FROM decodex.routing_snapshot_members
		WHERE snapshot_id = NEW.snapshot_id;
		SELECT pg_catalog.count(*) INTO account_count FROM decodex.routing_policy_members
		WHERE routing_policy_id = NEW.routing_policy_id
			AND routing_policy_revision = NEW.routing_policy_revision;
		SELECT pg_catalog.count(*) INTO quota_count FROM decodex.routing_snapshot_quota_facts
		WHERE snapshot_id = NEW.snapshot_id;
		SELECT pg_catalog.count(*) INTO matrix_count FROM decodex.routing_snapshot_capability_facts
		WHERE snapshot_id = NEW.snapshot_id;
		SELECT pg_catalog.count(*) INTO blocker_count FROM decodex.routing_snapshot_blockers
		WHERE snapshot_id = NEW.snapshot_id;
		SELECT COALESCE(pg_catalog.sum(pg_catalog.cardinality(blockers)), 0) INTO blocker_array_count
		FROM decodex.routing_snapshot_members WHERE snapshot_id = NEW.snapshot_id;
		IF member_count <> account_count OR quota_count <> member_count * 2
			OR matrix_count <> member_count * 8 OR blocker_count <> blocker_array_count
			OR NOT EXISTS (
				SELECT 1 FROM decodex.managed_runs AS run
				JOIN decodex.runtime_sessions AS session
					ON session.runtime_session_id=NEW.runtime_session_id
					AND session.revision=NEW.runtime_session_revision
				JOIN decodex.account_snapshots AS account
					ON account.account_snapshot_id=NEW.account_snapshot_id
				JOIN decodex.profile_snapshots AS profile
					ON profile.profile_snapshot_id=NEW.profile_snapshot_id
				JOIN decodex.routing_policy_revisions AS policy
					ON policy.routing_policy_id=NEW.routing_policy_id
					AND policy.revision=NEW.routing_policy_revision
				JOIN decodex.routing_snapshot_members AS sticky
					ON sticky.snapshot_id=NEW.snapshot_id AND sticky.sticky
				WHERE run.managed_run_id=NEW.managed_run_id
					AND run.revision=NEW.managed_run_revision
					AND run.project_id=policy.project_id
					AND (run.runtime_session_id,run.runtime_session_revision)
						=(session.runtime_session_id,session.revision)
					AND (session.account_snapshot_id,session.profile_snapshot_id)
						=(account.account_snapshot_id,profile.profile_snapshot_id)
					AND (account.source_account_id,account.source_revision)
						=(sticky.account_id,NEW.account_snapshot_source_revision)
					AND (profile.role,profile.source_revision)
						=(NEW.required_role,NEW.profile_snapshot_source_revision)
					AND NEW.required_role_profile_revision=profile.source_revision
			)
			OR (SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_members
				WHERE snapshot_id=NEW.snapshot_id AND sticky) <> 1
			OR EXISTS (
				SELECT 1 FROM decodex.routing_snapshot_members WHERE snapshot_id = NEW.snapshot_id
				GROUP BY snapshot_id HAVING pg_catalog.min(position) <> 1
					OR pg_catalog.max(position) <> pg_catalog.count(*)
			) OR EXISTS (
				SELECT member.position,member.account_id,member.disposition
				FROM decodex.routing_snapshot_members AS member
				WHERE member.snapshot_id=NEW.snapshot_id
				EXCEPT SELECT position,account_id,disposition
				FROM decodex.routing_policy_members
				WHERE routing_policy_id=NEW.routing_policy_id
					AND routing_policy_revision=NEW.routing_policy_revision
			) OR EXISTS (
				SELECT member.account_id,ordinality::integer,blocker.blocker
				FROM decodex.routing_snapshot_members AS member
				CROSS JOIN LATERAL pg_catalog.unnest(member.blockers) WITH ORDINALITY
					AS blocker(blocker,ordinality)
				WHERE member.snapshot_id = NEW.snapshot_id
				EXCEPT SELECT account_id,position,blocker FROM decodex.routing_snapshot_blockers
				WHERE snapshot_id = NEW.snapshot_id
			) OR EXISTS (
				SELECT 1 FROM decodex.routing_snapshot_members AS member
				WHERE member.snapshot_id=NEW.snapshot_id AND (
					pg_catalog.array_position(member.blockers,NULL) IS NOT NULL
					OR member.blockers <> ARRAY(
						SELECT blocker FROM pg_catalog.unnest(member.blockers) AS item(blocker)
						ORDER BY blocker))
			) OR EXISTS (
				SELECT 1 FROM decodex.routing_policy_revisions AS policy
				WHERE policy.routing_policy_id=NEW.routing_policy_id
					AND policy.revision=NEW.routing_policy_revision
					AND (NEW.accepted_policy_id,NEW.accepted_policy_revision,
						NEW.required_role,NEW.required_role_profile_revision,NEW.required_build_id)
						IS DISTINCT FROM (policy.accepted_policy_id,policy.accepted_policy_revision,
							policy.required_role,policy.required_role_profile_revision,
							policy.required_build_id)
			) OR EXISTS (
				SELECT 1 FROM decodex.routing_snapshot_members AS member
				LEFT JOIN decodex.routing_compatibility_evidence AS evidence
					ON evidence.evidence_id=member.evidence_id
				WHERE member.snapshot_id=NEW.snapshot_id AND member.evidence_id IS NOT NULL
					AND (evidence.account_id,evidence.evidence_revision,evidence.account_revision,evidence.role,
						evidence.role_profile_revision,evidence.build_id,evidence.process_id,
						evidence.schema_fingerprint) IS DISTINCT FROM
						(member.account_id,member.evidence_revision,member.evidence_account_revision,
							member.evidence_role,
							member.evidence_role_profile_revision,member.evidence_build_id,
							member.process_id,member.schema_fingerprint)
			) OR EXISTS (
				SELECT 1 FROM decodex.routing_snapshot_capability_facts AS fact
				JOIN decodex.routing_snapshot_members AS member
					USING (snapshot_id,account_id)
				LEFT JOIN decodex.routing_policy_required_capabilities AS required
					ON required.routing_policy_id=NEW.routing_policy_id
					AND required.routing_policy_revision=NEW.routing_policy_revision
					AND required.capability=fact.capability
				WHERE fact.snapshot_id=NEW.snapshot_id
					AND (fact.applicable IS DISTINCT FROM (required.capability IS NOT NULL)
						OR (fact.evidence_state IS NULL) IS DISTINCT FROM
							(member.evidence_id IS NULL))
			) THEN
			RAISE EXCEPTION 'routing snapshot child sets are incomplete'
				USING ERRCODE = '23514', CONSTRAINT = 'routing_snapshot_complete';
		END IF;
	END IF;
	RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER routing_policy_revision_complete
AFTER INSERT ON decodex.routing_policy_revisions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_routing_completeness();
CREATE CONSTRAINT TRIGGER routing_evidence_complete
AFTER INSERT ON decodex.routing_compatibility_evidence DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_routing_completeness();
CREATE CONSTRAINT TRIGGER routing_snapshot_complete
AFTER INSERT ON decodex.routing_snapshots DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_routing_completeness();

DO $$
DECLARE relation_name text;
BEGIN
	FOREACH relation_name IN ARRAY ARRAY[
		'routing_policy_revisions','routing_policy_members',
		'routing_policy_required_capabilities','routing_compatibility_evidence',
		'routing_capability_evidence','routing_snapshots',
		'routing_snapshot_members',
		'routing_snapshot_quota_facts','routing_snapshot_capability_facts',
		'routing_snapshot_blockers'
	] LOOP
		EXECUTE pg_catalog.format(
			'CREATE TRIGGER %I_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.%I '
			|| 'FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_routing_history_mutation()',
			relation_name, relation_name
		);
	END LOOP;
END
$$;

CREATE FUNCTION decodex.enforce_routing_command_owner()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE owner_name name;
BEGIN
	SELECT role.rolname INTO owner_name
	FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner
	WHERE class.oid=TG_RELID;
	IF current_user::name <> owner_name THEN
		RAISE EXCEPTION 'V14 routing state is writable only by its command owner'
			USING ERRCODE='42501', CONSTRAINT='routing_command_owner';
	END IF;
	IF TG_OP='DELETE' OR TG_OP='TRUNCATE' THEN
		RAISE EXCEPTION 'V14 routing history is retained'
			USING ERRCODE='55000', CONSTRAINT='routing_history_retained';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER routing_policy_heads_command_owner
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.routing_policy_heads
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_routing_command_owner();
CREATE FUNCTION decodex.complete_exact_routing_rejection(
	p_protocol text, p_idempotency_key text, p_operation text, p_code text
) RETURNS bytea LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	core := pg_catalog.jsonb_build_object('operation', p_operation, 'rejection', p_code);
	effect := core || pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest', pg_catalog.encode(
		public.digest(pg_catalog.convert_to(core::text, 'UTF8'), 'sha256'), 'hex'));
	response := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','stable_domain_rejection','effect',effect)::text, 'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_rejected',
		outcome_class='stable_domain_rejection', effect_envelope=effect,
		response_bytes=response, completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.reserve_exact_routing_command(
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
		RAISE EXCEPTION 'idempotency key reused for another routing command'
			USING ERRCODE='DX001';
	END IF;
	IF stored.receipt_state <> 'executing' THEN RETURN stored.response_bytes; END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.replace_routing_policy_exact(
	p_protocol text, p_idempotency_key text, p_routing_policy_id uuid, p_project_id uuid,
	p_expected_revision bigint, p_accepted_policy_id uuid, p_accepted_policy_revision bigint,
	p_required_role decodex.role_profile_role, p_required_role_profile_revision bigint,
	p_required_build_id text, p_account_ids uuid[], p_account_revisions bigint[],
	p_dispositions decodex.routing_member_disposition[],
	p_required_capabilities decodex.codex_capability[]
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; actual_revision bigint; actual_project_id uuid;
DECLARE new_revision bigint;
DECLARE inventory_count bigint; core jsonb; effect jsonb; response bytea;
BEGIN
	request := pg_catalog.jsonb_build_object(
		'operation','replace_routing_policy','protocol',p_protocol,
		'routing_policy_id',p_routing_policy_id,'project_id',p_project_id,
		'expected_revision',p_expected_revision,'accepted_policy_id',p_accepted_policy_id,
		'accepted_policy_revision',p_accepted_policy_revision,'required_role',p_required_role,
		'required_role_profile_revision',p_required_role_profile_revision,
		'required_build_id',p_required_build_id,'account_ids',p_account_ids,
		'account_revisions',p_account_revisions,'dispositions',p_dispositions,
		'required_capabilities',p_required_capabilities);
	replay := decodex.reserve_exact_routing_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	LOCK TABLE decodex.accounts, decodex.policies, decodex.policy_revisions,
		decodex.role_profiles, decodex.role_profile_revisions,
		decodex.routing_policy_heads, decodex.routing_policy_revisions,
		decodex.routing_policy_members, decodex.routing_policy_required_capabilities
		IN SHARE MODE;
	IF p_routing_policy_id IS NULL OR p_project_id IS NULL OR p_accepted_policy_id IS NULL
		OR p_required_role IS NULL OR p_required_build_id IS NULL
		OR p_accepted_policy_revision<=0 OR p_required_role_profile_revision<=0
		OR p_expected_revision IS NOT NULL AND p_expected_revision<=0
		OR p_required_build_id COLLATE pg_catalog."C" !~ '^sha256:[0-9a-f]{64}$'
		OR p_account_ids IS NULL OR p_account_revisions IS NULL OR p_dispositions IS NULL
		OR pg_catalog.cardinality(p_account_ids) <> pg_catalog.cardinality(p_account_revisions)
		OR pg_catalog.cardinality(p_account_ids) <> pg_catalog.cardinality(p_dispositions)
		OR pg_catalog.array_position(p_account_ids,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_account_revisions,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_dispositions,NULL) IS NOT NULL
		OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_account_revisions) AS revision
			WHERE revision <= 0)
		OR pg_catalog.cardinality(p_account_ids) <> (
			SELECT pg_catalog.count(DISTINCT value) FROM pg_catalog.unnest(p_account_ids) AS item(value)
		) OR p_required_capabilities IS NULL
		OR pg_catalog.array_position(p_required_capabilities,NULL) IS NOT NULL
		OR pg_catalog.cardinality(p_required_capabilities) <> (
			SELECT pg_catalog.count(DISTINCT value)
			FROM pg_catalog.unnest(p_required_capabilities) AS item(value)
		) OR p_required_capabilities <> ARRAY(
			SELECT capability FROM pg_catalog.unnest(p_required_capabilities) AS item(capability)
			ORDER BY capability
		) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'replace_routing_policy','invalid_inventory');
	END IF;
	PERFORM 1 FROM decodex.accounts ORDER BY account_id FOR SHARE;
	SELECT pg_catalog.count(*) INTO inventory_count FROM decodex.accounts;
	IF inventory_count <> pg_catalog.cardinality(p_account_ids) OR EXISTS (
		SELECT account_id,revision FROM decodex.accounts
		EXCEPT SELECT account_id,revision FROM pg_catalog.unnest(p_account_ids,p_account_revisions)
			AS requested(account_id,revision)
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'replace_routing_policy','inventory_revision_mismatch');
	END IF;
	PERFORM 1 FROM decodex.policies WHERE policy_id=p_accepted_policy_id
		AND project_id=p_project_id AND current_revision=p_accepted_policy_revision FOR SHARE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'replace_routing_policy','accepted_policy_mismatch');
	END IF;
	PERFORM 1 FROM decodex.policy_revisions WHERE policy_id=p_accepted_policy_id
		AND project_id=p_project_id AND revision=p_accepted_policy_revision FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'replace_routing_policy','accepted_policy_mismatch'); END IF;
	PERFORM 1 FROM decodex.role_profiles WHERE role=p_required_role
		AND current_revision=p_required_role_profile_revision FOR SHARE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'replace_routing_policy','role_profile_mismatch'); END IF;
	PERFORM 1 FROM decodex.role_profile_revisions WHERE role=p_required_role
		AND revision=p_required_role_profile_revision FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'replace_routing_policy','role_profile_mismatch'); END IF;
	SELECT current_revision,project_id INTO actual_revision,actual_project_id
	FROM decodex.routing_policy_heads
	WHERE routing_policy_id=p_routing_policy_id FOR UPDATE;
	IF actual_revision IS NULL THEN
		IF p_expected_revision IS NOT NULL THEN RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'replace_routing_policy','stale_revision'); END IF;
		new_revision := 1;
		INSERT INTO decodex.routing_policy_heads(routing_policy_id,project_id,current_revision)
		VALUES (p_routing_policy_id,p_project_id,new_revision);
	ELSE
		IF actual_revision <> p_expected_revision OR actual_project_id<>p_project_id THEN
			RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'replace_routing_policy','stale_revision'); END IF;
		new_revision := actual_revision + 1;
		UPDATE decodex.routing_policy_heads SET current_revision=new_revision,
			updated_at=pg_catalog.clock_timestamp()
		WHERE routing_policy_id=p_routing_policy_id AND current_revision=actual_revision;
	END IF;
	INSERT INTO decodex.routing_policy_revisions VALUES (
		p_routing_policy_id,new_revision,p_project_id,p_accepted_policy_id,
		p_accepted_policy_revision,p_required_role,p_required_role_profile_revision,
		p_required_build_id,pg_catalog.clock_timestamp());
	INSERT INTO decodex.routing_policy_members
	SELECT p_routing_policy_id,new_revision,ordinality::integer,account_id,account_revision,disposition
	FROM pg_catalog.unnest(p_account_ids,p_account_revisions,p_dispositions) WITH ORDINALITY
		AS item(account_id,account_revision,disposition,ordinality);
	INSERT INTO decodex.routing_policy_required_capabilities
	SELECT p_routing_policy_id,new_revision,ordinality::integer,capability
	FROM pg_catalog.unnest(p_required_capabilities) WITH ORDINALITY AS item(capability,ordinality);
	core := pg_catalog.jsonb_build_object('operation','replace_routing_policy',
		'routing_policy_id',p_routing_policy_id,'routing_policy_revision',new_revision,
		'project_id',p_project_id,'accepted_policy_id',p_accepted_policy_id,
		'accepted_policy_revision',p_accepted_policy_revision,'required_role',p_required_role,
		'required_role_profile_revision',p_required_role_profile_revision,
		'required_build_id',p_required_build_id,
		'members',(SELECT COALESCE(
			pg_catalog.jsonb_agg(pg_catalog.to_jsonb(policy_member) ORDER BY position), '[]'::jsonb)
			FROM decodex.routing_policy_members AS policy_member
			WHERE routing_policy_id=p_routing_policy_id AND routing_policy_revision=new_revision),
		'required_capabilities',p_required_capabilities);
	effect := core || pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.publish_routing_evidence_exact(
	p_protocol text, p_idempotency_key text, p_evidence_id uuid, p_account_id uuid,
	p_expected_account_revision bigint, p_expected_evidence_revision bigint,
	p_role decodex.role_profile_role,
	p_role_profile_revision bigint, p_build_id text, p_process_id uuid,
	p_process_account_id uuid, p_schema_fingerprint text,
	p_capabilities decodex.codex_capability[], p_states decodex.capability_evidence_state[]
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; next_revision bigint; core jsonb; effect jsonb; response bytea;
BEGIN
	request := pg_catalog.jsonb_build_object('operation','publish_routing_evidence',
		'protocol',p_protocol,'evidence_id',p_evidence_id,'account_id',p_account_id,
		'expected_account_revision',p_expected_account_revision,
		'expected_evidence_revision',p_expected_evidence_revision,'role',p_role,
		'role_profile_revision',p_role_profile_revision,'build_id',p_build_id,
		'process_id',p_process_id,'process_account_id',p_process_account_id,
		'schema_fingerprint',p_schema_fingerprint,'capabilities',p_capabilities,'states',p_states);
	replay := decodex.reserve_exact_routing_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	LOCK TABLE decodex.accounts, decodex.role_profiles, decodex.role_profile_revisions,
		decodex.routing_compatibility_evidence, decodex.routing_capability_evidence IN SHARE MODE;
	IF p_evidence_id IS NULL OR p_account_id IS NULL OR p_role IS NULL
		OR p_build_id IS NULL OR p_process_id IS NULL OR p_process_account_id IS NULL
		OR p_schema_fingerprint IS NULL OR p_expected_account_revision<=0 OR p_role_profile_revision<=0
		OR p_expected_evidence_revision IS NOT NULL AND p_expected_evidence_revision<=0
		OR p_build_id COLLATE pg_catalog."C" !~ '^sha256:[0-9a-f]{64}$'
		OR p_schema_fingerprint COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$' THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','invalid_evidence');
	END IF;
	PERFORM 1 FROM decodex.accounts WHERE account_id=p_account_id
		AND revision=p_expected_account_revision FOR SHARE;
	IF NOT FOUND OR p_process_account_id <> p_account_id THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','account_provenance_mismatch');
	END IF;
	PERFORM 1 FROM decodex.role_profiles
		WHERE role=p_role AND current_revision=p_role_profile_revision FOR SHARE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','role_profile_mismatch');
	END IF;
	PERFORM 1 FROM decodex.role_profile_revisions WHERE role=p_role
		AND revision=p_role_profile_revision FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'publish_routing_evidence','role_profile_mismatch'); END IF;
	IF p_capabilities IS NULL OR p_states IS NULL
		OR pg_catalog.array_position(p_capabilities,NULL) IS NOT NULL
		OR pg_catalog.array_position(p_states,NULL) IS NOT NULL
		OR pg_catalog.cardinality(p_capabilities) <> 8 OR pg_catalog.cardinality(p_states) <> 8
		OR p_capabilities <> ARRAY['initialize','account_read','thread_list','thread_read',
			'thread_archive','paginated_history','native_collaboration','thread_search']::decodex.codex_capability[]
	THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'publish_routing_evidence','invalid_capability_projection'); END IF;
	IF EXISTS (SELECT 1 FROM decodex.routing_compatibility_evidence WHERE evidence_id=p_evidence_id) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','duplicate_evidence_id');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.routing_compatibility_evidence
		WHERE process_id=p_process_id) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','duplicate_process_id');
	END IF;
	PERFORM 1 FROM decodex.routing_compatibility_evidence WHERE account_id=p_account_id
	ORDER BY evidence_revision FOR UPDATE;
	SELECT COALESCE(pg_catalog.max(evidence_revision),0)+1 INTO next_revision
	FROM decodex.routing_compatibility_evidence WHERE account_id=p_account_id;
	IF (next_revision=1 AND p_expected_evidence_revision IS NOT NULL)
		OR (next_revision>1 AND p_expected_evidence_revision IS DISTINCT FROM next_revision-1) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'publish_routing_evidence','stale_evidence_revision');
	END IF;
	INSERT INTO decodex.routing_compatibility_evidence(
		evidence_id,account_id,account_revision,role,role_profile_revision,build_id,
		process_id,process_account_id,schema_fingerprint,evidence_revision
	) VALUES (p_evidence_id,p_account_id,p_expected_account_revision,p_role,p_role_profile_revision,
		p_build_id,p_process_id,p_process_account_id,p_schema_fingerprint,next_revision);
	INSERT INTO decodex.routing_capability_evidence
	SELECT p_evidence_id,ordinality::integer,capability,state
	FROM pg_catalog.unnest(p_capabilities,p_states) WITH ORDINALITY
		AS item(capability,state,ordinality);
	core := pg_catalog.jsonb_build_object('operation','publish_routing_evidence',
		'evidence_id',p_evidence_id,'account_id',p_account_id,'account_revision',p_expected_account_revision,
		'evidence_revision',next_revision,'role',p_role,'role_profile_revision',p_role_profile_revision,
		'build_id',p_build_id,'process_id',p_process_id,'schema_fingerprint',p_schema_fingerprint,
		'process_account_id',p_process_account_id,
		'capabilities',p_capabilities,'states',p_states,
		'ingested_at_micros',(SELECT
			(extract(epoch FROM ingested_at)*1000000)::bigint
			FROM decodex.routing_compatibility_evidence WHERE evidence_id=p_evidence_id));
	effect := core || pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.resolve_routing_snapshot_exact(
	p_protocol text, p_idempotency_key text, p_routing_policy_id uuid,
	p_expected_routing_policy_revision bigint, p_managed_run_id uuid,
	p_expected_managed_run_revision bigint
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; policy_row record; run_row record; session_row record;
DECLARE resolved timestamptz; new_snapshot_id uuid; member record; evidence record;
DECLARE quota record;
DECLARE blockers decodex.routing_blocker[]; sticky_account uuid; core jsonb; effect jsonb; response bytea;
BEGIN
	request := pg_catalog.jsonb_build_object('operation','resolve_routing_snapshot',
		'protocol',p_protocol,'routing_policy_id',p_routing_policy_id,
		'expected_routing_policy_revision',p_expected_routing_policy_revision,
		'managed_run_id',p_managed_run_id,
		'expected_managed_run_revision',p_expected_managed_run_revision);
	replay := decodex.reserve_exact_routing_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(p_managed_run_id::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	LOCK TABLE decodex.accounts, decodex.quota_windows, decodex.policies,
		decodex.policy_revisions, decodex.role_profiles, decodex.role_profile_revisions,
		decodex.profile_snapshots, decodex.account_snapshots, decodex.runtime_sessions,
		decodex.managed_runs, decodex.routing_policy_heads, decodex.routing_policy_revisions,
		decodex.routing_policy_members, decodex.routing_policy_required_capabilities,
		decodex.routing_compatibility_evidence, decodex.routing_capability_evidence IN SHARE MODE;
	SELECT revision.project_id,revision.accepted_policy_id,revision.accepted_policy_revision,
		revision.required_role,revision.required_role_profile_revision,revision.required_build_id
	INTO policy_row FROM decodex.routing_policy_heads AS head
	JOIN decodex.routing_policy_revisions AS revision
		ON revision.routing_policy_id=head.routing_policy_id AND revision.revision=head.current_revision
	WHERE head.routing_policy_id=p_routing_policy_id
		AND head.current_revision=p_expected_routing_policy_revision FOR SHARE OF head,revision;
	IF NOT FOUND OR NOT EXISTS (SELECT 1 FROM decodex.policies
		WHERE policy_id=policy_row.accepted_policy_id
		AND project_id=policy_row.project_id
		AND current_revision=policy_row.accepted_policy_revision)
		OR NOT EXISTS (SELECT 1 FROM decodex.role_profiles
			WHERE role=policy_row.required_role
			AND current_revision=policy_row.required_role_profile_revision) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot','routing_authority_mismatch');
	END IF;
	SELECT * INTO run_row FROM decodex.managed_runs WHERE managed_run_id=p_managed_run_id
		AND revision=p_expected_managed_run_revision FOR SHARE;
	IF NOT FOUND OR run_row.project_id <> policy_row.project_id THEN
		RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'resolve_routing_snapshot','managed_run_mismatch'); END IF;
	SELECT session.runtime_session_id,session.revision,session.account_snapshot_id,
		session.profile_snapshot_id,account.source_account_id,account.source_revision AS account_source_revision,
		account.display_label AS account_snapshot_display_label,
		account.observed_state AS account_snapshot_state,
		profile.role,profile.source_revision AS profile_source_revision
	INTO session_row FROM decodex.runtime_sessions AS session
	JOIN decodex.account_snapshots AS account USING (account_snapshot_id)
	JOIN decodex.profile_snapshots AS profile USING (profile_snapshot_id)
	WHERE session.runtime_session_id=run_row.runtime_session_id
		AND session.revision=run_row.runtime_session_revision FOR SHARE OF session,account,profile;
	IF NOT FOUND OR session_row.role <> policy_row.required_role
		OR session_row.profile_source_revision <> policy_row.required_role_profile_revision
		OR NOT EXISTS (SELECT 1 FROM decodex.accounts AS account
			WHERE account.account_id=session_row.source_account_id
				AND account.revision=session_row.account_source_revision
				AND account.display_label=session_row.account_snapshot_display_label
				AND account.state=session_row.account_snapshot_state)
		OR NOT EXISTS (SELECT 1 FROM decodex.routing_policy_members AS policy_member
			WHERE policy_member.routing_policy_id=p_routing_policy_id
				AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
				AND policy_member.account_id=session_row.source_account_id
				AND policy_member.account_revision=session_row.account_source_revision) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot','sticky_provenance_mismatch');
	END IF;
	sticky_account := session_row.source_account_id;
	IF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
		AND routing_policy_revision=p_expected_routing_policy_revision
		AND account_id=sticky_account) OR EXISTS (
		SELECT account_id,revision FROM decodex.accounts
		EXCEPT SELECT policy_member.account_id,policy_member.account_revision
		FROM decodex.routing_policy_members AS policy_member
		WHERE policy_member.routing_policy_id=p_routing_policy_id
			AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot','routing_authority_mismatch');
	END IF;
	PERFORM 1 FROM decodex.routing_policy_members AS policy_member
	JOIN decodex.accounts AS account USING (account_id)
	WHERE policy_member.routing_policy_id=p_routing_policy_id
		AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
	ORDER BY policy_member.position FOR SHARE OF policy_member,account;
	PERFORM 1 FROM decodex.routing_compatibility_evidence
	WHERE account_id IN (SELECT account_id FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
		AND routing_policy_revision=p_expected_routing_policy_revision)
	ORDER BY evidence_id FOR SHARE;
	PERFORM 1 FROM decodex.routing_capability_evidence
	WHERE evidence_id IN (SELECT evidence_id FROM decodex.routing_compatibility_evidence
		WHERE account_id IN (SELECT account_id FROM decodex.routing_policy_members
			WHERE routing_policy_id=p_routing_policy_id
				AND routing_policy_revision=p_expected_routing_policy_revision))
	ORDER BY evidence_id,position FOR SHARE;
	PERFORM 1 FROM decodex.routing_policy_required_capabilities
	WHERE routing_policy_id=p_routing_policy_id
		AND routing_policy_revision=p_expected_routing_policy_revision
	ORDER BY position FOR SHARE;
	PERFORM 1 FROM decodex.quota_windows WHERE account_id IN (
		SELECT account_id FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
		AND routing_policy_revision=p_expected_routing_policy_revision)
	ORDER BY account_id,window_class,duration_minutes FOR SHARE;
	resolved := pg_catalog.clock_timestamp();
	INSERT INTO decodex.routing_snapshots(
		routing_policy_id,routing_policy_revision,accepted_policy_id,accepted_policy_revision,
		required_role,required_role_profile_revision,required_build_id,
		managed_run_id,managed_run_revision,runtime_session_id,runtime_session_revision,
		account_snapshot_id,account_snapshot_source_revision,
		profile_snapshot_id,profile_snapshot_source_revision,resolved_at
	) VALUES (p_routing_policy_id,p_expected_routing_policy_revision,
		policy_row.accepted_policy_id,policy_row.accepted_policy_revision,
		policy_row.required_role,policy_row.required_role_profile_revision,policy_row.required_build_id,
		p_managed_run_id,p_expected_managed_run_revision,session_row.runtime_session_id,
		session_row.revision,session_row.account_snapshot_id,session_row.account_source_revision,
		session_row.profile_snapshot_id,session_row.profile_source_revision,resolved)
	RETURNING snapshot_id INTO new_snapshot_id;
	FOR member IN SELECT policy_member.position,policy_member.account_id,
		policy_member.account_revision,policy_member.disposition,account.display_label,
		account.state,account.observed_at,account.revision AS current_account_revision
		FROM decodex.routing_policy_members AS policy_member
		JOIN decodex.accounts AS account USING (account_id)
		WHERE policy_member.routing_policy_id=p_routing_policy_id
			AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
		ORDER BY policy_member.position
	LOOP
		blockers := ARRAY[]::decodex.routing_blocker[];
		IF member.disposition='excluded' THEN blockers:=blockers||'excluded_by_policy'; END IF;
		IF member.account_revision<>member.current_account_revision THEN blockers:=blockers||'account_stale'; END IF;
		IF member.observed_at>resolved THEN blockers:=blockers||'account_from_future';
		ELSIF resolved-member.observed_at>INTERVAL '300 seconds' THEN blockers:=blockers||'account_stale'; END IF;
		blockers:=blockers||CASE member.state
			WHEN 'unavailable' THEN 'account_unavailable'::decodex.routing_blocker
			WHEN 'unknown' THEN 'account_unknown'::decodex.routing_blocker
			WHEN 'depleted' THEN 'account_depleted'::decodex.routing_blocker
			WHEN 'auth_failed' THEN 'account_auth_failed'::decodex.routing_blocker
			WHEN 'plugin_unready' THEN 'account_plugin_unready'::decodex.routing_blocker
			WHEN 'disabled' THEN 'account_disabled'::decodex.routing_blocker ELSE NULL END;
		blockers:=pg_catalog.array_remove(blockers,NULL);
		SELECT candidate.* INTO evidence FROM decodex.routing_compatibility_evidence AS candidate
		WHERE candidate.account_id=member.account_id ORDER BY candidate.evidence_revision DESC LIMIT 1;
		IF NOT FOUND THEN blockers:=blockers||'evidence_missing';
		ELSE
			IF evidence.ingested_at>resolved THEN blockers:=blockers||'evidence_from_future';
			ELSIF resolved-evidence.ingested_at>INTERVAL '300 seconds' THEN blockers:=blockers||'evidence_stale'; END IF;
			IF evidence.account_revision<>member.current_account_revision THEN blockers:=blockers||'evidence_account_mismatch'; END IF;
			IF evidence.role<>policy_row.required_role OR evidence.role_profile_revision<>policy_row.required_role_profile_revision
				THEN blockers:=blockers||'evidence_profile_mismatch'; END IF;
			IF evidence.build_id<>policy_row.required_build_id THEN blockers:=blockers||'evidence_build_mismatch'; END IF;
		END IF;
		IF EXISTS (SELECT 1 FROM decodex.routing_policy_required_capabilities AS required
			LEFT JOIN decodex.routing_capability_evidence AS actual ON actual.evidence_id=evidence.evidence_id
				AND actual.capability=required.capability
			WHERE required.routing_policy_id=p_routing_policy_id
				AND required.routing_policy_revision=p_expected_routing_policy_revision
				AND actual.state IS DISTINCT FROM 'supported') THEN
			blockers:=blockers||'required_capability_unsatisfied';
		END IF;
		FOR quota IN SELECT definition.window_class,definition.duration_minutes,quota_window.revision,
			quota_window.remaining_percent,quota_window.resets_at,quota_window.observed_at,quota_window.confidence
			FROM (VALUES ('five_hour'::decodex.quota_window_class,300::smallint),
				('seven_day'::decodex.quota_window_class,10080::smallint))
				AS definition(window_class,duration_minutes)
			LEFT JOIN decodex.quota_windows AS quota_window ON quota_window.account_id=member.account_id
				AND quota_window.window_class=definition.window_class
				AND quota_window.duration_minutes=definition.duration_minutes
		LOOP
			IF quota.revision IS NULL THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_missing'::decodex.routing_blocker
					ELSE 'quota_seven_day_missing'::decodex.routing_blocker END;
			ELSIF quota.observed_at>resolved THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_from_future'::decodex.routing_blocker
					ELSE 'quota_seven_day_from_future'::decodex.routing_blocker END;
			ELSIF resolved-quota.observed_at>INTERVAL '300 seconds' THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_stale'::decodex.routing_blocker
					ELSE 'quota_seven_day_stale'::decodex.routing_blocker END;
			ELSIF quota.remaining_percent IS NULL OR quota.confidence<>'high' THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_unknown'::decodex.routing_blocker
					ELSE 'quota_seven_day_unknown'::decodex.routing_blocker END;
			ELSIF quota.resets_at IS NOT NULL AND quota.resets_at<=resolved THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_reset_elapsed'::decodex.routing_blocker
					ELSE 'quota_seven_day_reset_elapsed'::decodex.routing_blocker END;
			ELSIF quota.remaining_percent=0 THEN
				blockers:=blockers||CASE quota.window_class WHEN 'five_hour'
					THEN 'quota_five_hour_depleted'::decodex.routing_blocker
					ELSE 'quota_seven_day_depleted'::decodex.routing_blocker END;
			END IF;
		END LOOP;
		SELECT pg_catalog.array_agg(DISTINCT blocker ORDER BY blocker)
		INTO blockers FROM pg_catalog.unnest(blockers) AS item(blocker);
		blockers:=COALESCE(blockers,ARRAY[]::decodex.routing_blocker[]);
		INSERT INTO decodex.routing_snapshot_members VALUES (
			new_snapshot_id,member.position,member.account_id,member.disposition,
			member.current_account_revision,member.display_label,member.state,
			decodex.rfc3339_utc(member.observed_at),
			evidence.evidence_id,evidence.evidence_revision,evidence.account_revision,evidence.role,
			evidence.role_profile_revision,evidence.build_id,
			evidence.process_id,evidence.schema_fingerprint,member.account_id=sticky_account,blockers);
		INSERT INTO decodex.routing_snapshot_quota_facts
		SELECT new_snapshot_id,member.account_id,definition.position,definition.window_class,
			definition.duration_minutes,quota.revision,quota.remaining_percent,
			CASE WHEN quota.resets_at IS NULL THEN NULL ELSE
				(extract(epoch FROM quota.resets_at)*1000000)::bigint END,
			CASE WHEN quota.observed_at IS NULL THEN NULL ELSE
				(extract(epoch FROM quota.observed_at)*1000000)::bigint END,quota.confidence
		FROM (VALUES (1::smallint,'five_hour'::decodex.quota_window_class,300::smallint),
			(2::smallint,'seven_day'::decodex.quota_window_class,10080::smallint))
			AS definition(position,window_class,duration_minutes)
		LEFT JOIN decodex.quota_windows AS quota ON quota.account_id=member.account_id
			AND quota.window_class=definition.window_class
			AND quota.duration_minutes=definition.duration_minutes;
		INSERT INTO decodex.routing_snapshot_capability_facts
		SELECT new_snapshot_id,member.account_id,definition.position,definition.capability,
			required.capability IS NOT NULL,actual.state
		FROM (VALUES (1::smallint,'initialize'::decodex.codex_capability),
			(2::smallint,'account_read'),(3::smallint,'thread_list'),(4::smallint,'thread_read'),
			(5::smallint,'thread_archive'),(6::smallint,'paginated_history'),
			(7::smallint,'native_collaboration'),(8::smallint,'thread_search'))
			AS definition(position,capability)
		LEFT JOIN decodex.routing_policy_required_capabilities AS required
			ON required.routing_policy_id=p_routing_policy_id
			AND required.routing_policy_revision=p_expected_routing_policy_revision
			AND required.capability=definition.capability
		LEFT JOIN decodex.routing_capability_evidence AS actual
			ON actual.evidence_id=evidence.evidence_id AND actual.capability=definition.capability;
		INSERT INTO decodex.routing_snapshot_blockers
		SELECT new_snapshot_id,member.account_id,ordinality::integer,blocker
		FROM pg_catalog.unnest(blockers) WITH ORDINALITY AS item(blocker,ordinality);
	END LOOP;
	core := pg_catalog.jsonb_build_object('operation','resolve_routing_snapshot',
		'snapshot_id',new_snapshot_id,'routing_policy_id',p_routing_policy_id,
		'routing_policy_revision',p_expected_routing_policy_revision,
		'accepted_policy_id',policy_row.accepted_policy_id,
		'accepted_policy_revision',policy_row.accepted_policy_revision,
		'required_role',policy_row.required_role,
		'required_role_profile_revision',policy_row.required_role_profile_revision,
		'required_build_id',policy_row.required_build_id,
		'managed_run_id',p_managed_run_id,'managed_run_revision',p_expected_managed_run_revision,
		'runtime_session_id',session_row.runtime_session_id,
		'runtime_session_revision',session_row.revision,
		'account_snapshot_id',session_row.account_snapshot_id,
		'account_snapshot_source_revision',session_row.account_source_revision,
		'profile_snapshot_id',session_row.profile_snapshot_id,
		'profile_snapshot_source_revision',session_row.profile_source_revision,
		'resolved_at_micros',(extract(epoch FROM resolved)*1000000)::bigint,
		'members',(SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(member_row) ORDER BY position)
			FROM decodex.routing_snapshot_members AS member_row WHERE snapshot_id=new_snapshot_id),
		'quota_facts',(SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(quota_row) ORDER BY snapshot_member.position,quota_row.position)
			FROM decodex.routing_snapshot_quota_facts AS quota_row
			JOIN decodex.routing_snapshot_members AS snapshot_member USING (snapshot_id,account_id)
			WHERE quota_row.snapshot_id=new_snapshot_id),
		'capability_facts',(SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(capability_row) ORDER BY snapshot_member.position,capability_row.position)
			FROM decodex.routing_snapshot_capability_facts AS capability_row
			JOIN decodex.routing_snapshot_members AS snapshot_member USING (snapshot_id,account_id)
			WHERE capability_row.snapshot_id=new_snapshot_id),
		'blockers',(SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(blocker_row)
			ORDER BY snapshot_member.position,blocker_row.position), '[]'::jsonb)
			FROM decodex.routing_snapshot_blockers AS blocker_row
			JOIN decodex.routing_snapshot_members AS snapshot_member USING (snapshot_id,account_id)
			WHERE blocker_row.snapshot_id=new_snapshot_id));
	effect := core || pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response := pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON TABLE decodex.routing_policy_heads, decodex.routing_policy_revisions,
	decodex.routing_policy_members, decodex.routing_policy_required_capabilities,
	decodex.routing_compatibility_evidence, decodex.routing_capability_evidence,
	decodex.routing_snapshots, decodex.routing_snapshot_members,
	decodex.routing_snapshot_quota_facts, decodex.routing_snapshot_capability_facts,
	decodex.routing_snapshot_blockers FROM PUBLIC;
REVOKE ALL ON TYPE decodex.routing_member_disposition, decodex.codex_capability,
	decodex.capability_evidence_state, decodex.routing_blocker FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE USAGE ON TYPES FROM PUBLIC;

-- Extend exactly the already accepted V12 runtime command principals. This neither discovers
-- nor creates a new role, and grants no helper or relation authority.
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
			'GRANT USAGE ON TYPE decodex.routing_member_disposition, decodex.codex_capability, '
			|| 'decodex.capability_evidence_state, decodex.routing_blocker TO %I', runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.replace_routing_policy_exact(text,text,uuid,uuid,bigint,uuid,bigint,decodex.role_profile_role,bigint,text,uuid[],bigint[],decodex.routing_member_disposition[],decodex.codex_capability[]) TO %I',
			runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.publish_routing_evidence_exact(text,text,uuid,uuid,bigint,bigint,decodex.role_profile_role,bigint,text,uuid,uuid,text,decodex.codex_capability[],decodex.capability_evidence_state[]) TO %I',
			runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.resolve_routing_snapshot_exact(text,text,uuid,bigint,uuid,bigint) TO %I',
			runtime_role);
	END LOOP;
END
$$;
