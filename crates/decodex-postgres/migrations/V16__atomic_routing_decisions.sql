-- XY-1359 atomic, inert multi-account routing decisions.
-- PostgreSQL owns the complete candidate universe and clock. This migration creates no
-- dispatch, credential, thread, continuation, wake, scheduler, or runtime composition path.

CREATE TYPE decodex.routing_decision_kind AS ENUM ('selected', 'waiting_usage', 'no_route');
CREATE TYPE decodex.routing_no_route_reason AS ENUM ('blocked_evidence');

ALTER TABLE decodex.routing_snapshot_members ADD CONSTRAINT routing_snapshot_members_exact_ref
	UNIQUE (snapshot_id, account_id, position);

CREATE TABLE decodex.routing_decisions (
	decision_id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
	operation_id uuid NOT NULL UNIQUE,
	snapshot_id uuid NOT NULL REFERENCES decodex.routing_snapshots(snapshot_id),
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL CHECK (routing_policy_revision > 0),
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 0),
	kind decodex.routing_decision_kind NOT NULL,
	selected_account_id uuid REFERENCES decodex.accounts(account_id),
	waiting_ready_at_micros bigint,
	no_route_reason decodex.routing_no_route_reason,
	decided_at timestamptz NOT NULL,
	CONSTRAINT routing_decisions_snapshot_identity UNIQUE (decision_id, snapshot_id),
	CONSTRAINT routing_decisions_run_fk FOREIGN KEY (managed_run_id, managed_run_revision)
		REFERENCES decodex.managed_runs(managed_run_id, revision),
	CONSTRAINT routing_decisions_shape CHECK (
		(kind = 'selected' AND selected_account_id IS NOT NULL
			AND waiting_ready_at_micros IS NULL AND no_route_reason IS NULL)
		OR (kind = 'waiting_usage' AND selected_account_id IS NULL
			AND waiting_ready_at_micros BETWEEN 0 AND 253402300799999999
			AND no_route_reason IS NULL)
		OR (kind = 'no_route' AND selected_account_id IS NULL
			AND waiting_ready_at_micros IS NULL AND no_route_reason = 'blocked_evidence')
	),
	CONSTRAINT routing_decisions_finite_time CHECK (pg_catalog.isfinite(decided_at))
);

CREATE TABLE decodex.routing_decision_member_refs (
	decision_id uuid NOT NULL,
	snapshot_id uuid NOT NULL,
	position integer NOT NULL,
	account_id uuid NOT NULL,
	PRIMARY KEY (decision_id, position),
	UNIQUE (decision_id, account_id),
	UNIQUE (decision_id, account_id, position),
	UNIQUE (decision_id, snapshot_id, account_id),
	CONSTRAINT routing_decision_member_snapshot_fk FOREIGN KEY (account_id)
		REFERENCES decodex.accounts(account_id),
	CONSTRAINT routing_decision_member_decision_fk FOREIGN KEY (decision_id, snapshot_id)
		REFERENCES decodex.routing_decisions(decision_id, snapshot_id)
		DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT routing_decision_member_exact_snapshot_fk FOREIGN KEY (
		snapshot_id, account_id, position
	) REFERENCES decodex.routing_snapshot_members(snapshot_id, account_id, position)
);

CREATE TABLE decodex.routing_decision_quota_refs (
	decision_id uuid NOT NULL,
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position smallint NOT NULL CHECK (position IN (1, 2)),
	window_class decodex.quota_window_class NOT NULL,
	duration_minutes smallint NOT NULL,
	observation_revision bigint,
	remaining_percent smallint,
	observed_at_micros bigint,
	resets_at_micros bigint,
	confidence decodex.observation_confidence,
	source_id text,
	timestamp_precision text,
	raw_observed_at text,
	raw_resets_at text,
	PRIMARY KEY (decision_id, account_id, position),
	UNIQUE (decision_id, account_id, window_class, duration_minutes, observation_revision),
	CONSTRAINT routing_decision_quota_member_fk FOREIGN KEY (decision_id, snapshot_id, account_id)
		REFERENCES decodex.routing_decision_member_refs(decision_id, snapshot_id, account_id),
	CONSTRAINT routing_decision_quota_snapshot_fk FOREIGN KEY (snapshot_id, account_id, position)
		REFERENCES decodex.routing_snapshot_quota_facts(snapshot_id, account_id, position),
	CONSTRAINT routing_decision_quota_identity CHECK (
		(position = 1 AND window_class = 'five_hour' AND duration_minutes = 300)
		OR (position = 2 AND window_class = 'seven_day' AND duration_minutes = 10080)
	),
	CONSTRAINT routing_decision_quota_provenance CHECK (
		(observation_revision IS NULL AND remaining_percent IS NULL
			AND observed_at_micros IS NULL AND resets_at_micros IS NULL
			AND confidence IS NULL AND source_id IS NULL AND timestamp_precision IS NULL
			AND raw_observed_at IS NULL AND raw_resets_at IS NULL)
		OR (observation_revision > 0 AND remaining_percent BETWEEN 0 AND 100
			AND observed_at_micros BETWEEN 0 AND 253402300799999999
			AND (resets_at_micros IS NULL OR resets_at_micros BETWEEN 0 AND 253402300799999999)
			AND confidence IS NOT NULL
			AND ((source_id IS NULL AND timestamp_precision IS NULL
				AND raw_observed_at IS NULL AND raw_resets_at IS NULL)
				OR (source_id <> '' AND pg_catalog.octet_length(source_id) <= 256
					AND NOT decodex.has_credential_material(source_id)
					AND timestamp_precision = 'unix_microsecond'
					AND raw_observed_at = observed_at_micros::text
					AND (resets_at_micros IS NULL) = (raw_resets_at IS NULL)
					AND (raw_resets_at IS NULL OR raw_resets_at = resets_at_micros::text))))
	)
);

CREATE TABLE decodex.routing_decision_capability_refs (
	decision_id uuid NOT NULL,
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position smallint NOT NULL CHECK (position BETWEEN 1 AND 8),
	capability decodex.codex_capability NOT NULL,
	applicable boolean NOT NULL,
	evidence_state decodex.capability_evidence_state,
	PRIMARY KEY (decision_id, account_id, position),
	CONSTRAINT routing_decision_capability_member_fk FOREIGN KEY (decision_id, snapshot_id, account_id)
		REFERENCES decodex.routing_decision_member_refs(decision_id, snapshot_id, account_id),
	CONSTRAINT routing_decision_capability_snapshot_fk FOREIGN KEY (snapshot_id, account_id, position)
		REFERENCES decodex.routing_snapshot_capability_facts(snapshot_id, account_id, position)
);

CREATE TABLE decodex.routing_decision_blocker_refs (
	decision_id uuid NOT NULL,
	snapshot_id uuid NOT NULL,
	account_id uuid NOT NULL,
	position integer NOT NULL CHECK (position > 0),
	blocker decodex.routing_blocker NOT NULL,
	PRIMARY KEY (decision_id, account_id, position),
	UNIQUE (decision_id, account_id, blocker),
	CONSTRAINT routing_decision_blocker_member_fk FOREIGN KEY (decision_id, snapshot_id, account_id)
		REFERENCES decodex.routing_decision_member_refs(decision_id, snapshot_id, account_id),
	CONSTRAINT routing_decision_blocker_snapshot_fk FOREIGN KEY (snapshot_id, account_id, position)
		REFERENCES decodex.routing_snapshot_blockers(snapshot_id, account_id, position)
);

CREATE TABLE decodex.routing_decision_exclusions (
	decision_id uuid NOT NULL,
	account_id uuid NOT NULL,
	member_position integer NOT NULL CHECK (member_position > 0),
	window_class decodex.quota_window_class NOT NULL,
	duration_minutes smallint NOT NULL,
	observation_revision bigint NOT NULL CHECK (observation_revision > 0),
	remaining_percent smallint NOT NULL CHECK (remaining_percent = 0),
	observed_at_micros bigint NOT NULL,
	resets_at_micros bigint NOT NULL,
	confidence decodex.observation_confidence NOT NULL CHECK (confidence = 'high'),
	source_id text NOT NULL CHECK (source_id <> '' AND pg_catalog.octet_length(source_id) <= 256
		AND NOT decodex.has_credential_material(source_id)),
	timestamp_precision text NOT NULL CHECK (timestamp_precision = 'unix_microsecond'),
	raw_observed_at text NOT NULL,
	raw_resets_at text NOT NULL,
	reason text NOT NULL CHECK (reason = 'usage_depleted'),
	PRIMARY KEY (decision_id, account_id, window_class, duration_minutes),
	CONSTRAINT routing_decision_exclusion_quota_fk FOREIGN KEY (
		decision_id, account_id, member_position
	) REFERENCES decodex.routing_decision_member_refs(decision_id, account_id, position),
	CONSTRAINT routing_decision_exclusion_exact_quota_fk FOREIGN KEY (
		decision_id, account_id, window_class, duration_minutes, observation_revision
	) REFERENCES decodex.routing_decision_quota_refs(
		decision_id, account_id, window_class, duration_minutes, observation_revision
	),
	CONSTRAINT routing_decision_exclusion_range CHECK (
		observed_at_micros BETWEEN 0 AND 253402300799999999
		AND resets_at_micros BETWEEN observed_at_micros + 1 AND 253402300799999999
		AND raw_observed_at = observed_at_micros::text
		AND raw_resets_at = resets_at_micros::text
	),
	CONSTRAINT routing_decision_exclusion_duration CHECK (
		(window_class = 'five_hour' AND duration_minutes = 300)
		OR (window_class = 'seven_day' AND duration_minutes = 10080)
	)
);

CREATE FUNCTION decodex.forbid_routing_decision_mutation()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF TG_OP='INSERT' THEN
		IF EXISTS (SELECT 1 FROM decodex.routing_decisions WHERE decision_id=NEW.decision_id) THEN
			RAISE EXCEPTION 'V16 routing decision evidence is sealed'
				USING ERRCODE='55000', CONSTRAINT='routing_decision_evidence_sealed';
		END IF;
		RETURN NEW;
	END IF;
	RAISE EXCEPTION 'V16 routing decisions are immutable'
		USING ERRCODE='55000', CONSTRAINT='routing_decision_immutable';
END
$$;

CREATE FUNCTION decodex.enforce_routing_decision_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE decision_row decodex.routing_decisions%ROWTYPE;
DECLARE member_count bigint; quota_count bigint; capability_count bigint; blocker_count bigint;
DECLARE required_exclusions bigint; stored_exclusions bigint;
BEGIN
	SELECT * INTO STRICT decision_row FROM decodex.routing_decisions
	WHERE decision_id=NEW.decision_id;
	IF NOT EXISTS (
		SELECT 1 FROM decodex.routing_snapshots AS snapshot
		WHERE snapshot.snapshot_id=decision_row.snapshot_id
			AND snapshot.routing_policy_id=decision_row.routing_policy_id
			AND snapshot.routing_policy_revision=decision_row.routing_policy_revision
			AND snapshot.managed_run_id=decision_row.managed_run_id
			AND snapshot.managed_run_revision=decision_row.managed_run_revision
	) OR (decision_row.kind='selected' AND NOT EXISTS (
		SELECT 1 FROM decodex.routing_decision_member_refs AS member
		JOIN decodex.routing_snapshot_members AS snapshot_member
			ON snapshot_member.snapshot_id=member.snapshot_id
				AND snapshot_member.account_id=member.account_id
		WHERE member.decision_id=NEW.decision_id
			AND member.account_id=decision_row.selected_account_id
			AND snapshot_member.disposition='included'
			AND pg_catalog.cardinality(snapshot_member.blockers)=0
			AND NOT EXISTS (
				SELECT 1 FROM decodex.routing_decision_quota_refs AS quota
				WHERE quota.decision_id=NEW.decision_id AND quota.account_id=member.account_id
					AND (quota.remaining_percent IS NULL OR quota.remaining_percent=0
						OR quota.confidence<>'high' OR quota.source_id IS NULL
						OR quota.observed_at_micros IS NULL OR quota.resets_at_micros IS NULL
						OR quota.observed_at_micros>(pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint
						OR (pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint
							-quota.observed_at_micros>300000000
						OR quota.resets_at_micros<=(pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint)
			)
	)) THEN
		RAISE EXCEPTION 'V16 routing decision lineage is incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	SELECT pg_catalog.count(*) INTO member_count FROM decodex.routing_snapshot_members
	WHERE snapshot_id=decision_row.snapshot_id;
	SELECT pg_catalog.count(*) INTO quota_count FROM decodex.routing_snapshot_quota_facts
	WHERE snapshot_id=decision_row.snapshot_id;
	SELECT pg_catalog.count(*) INTO capability_count FROM decodex.routing_snapshot_capability_facts
	WHERE snapshot_id=decision_row.snapshot_id;
	SELECT pg_catalog.count(*) INTO blocker_count FROM decodex.routing_snapshot_blockers
	WHERE snapshot_id=decision_row.snapshot_id;
	IF member_count<>(SELECT pg_catalog.count(*) FROM decodex.routing_decision_member_refs
		WHERE decision_id=NEW.decision_id)
		OR quota_count<>(SELECT pg_catalog.count(*) FROM decodex.routing_decision_quota_refs
			WHERE decision_id=NEW.decision_id)
		OR capability_count<>(SELECT pg_catalog.count(*) FROM decodex.routing_decision_capability_refs
			WHERE decision_id=NEW.decision_id)
		OR blocker_count<>(SELECT pg_catalog.count(*) FROM decodex.routing_decision_blocker_refs
			WHERE decision_id=NEW.decision_id) THEN
		RAISE EXCEPTION 'V16 routing decision evidence is incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	IF EXISTS (
		SELECT 1 FROM decodex.routing_decision_quota_refs AS reference
		LEFT JOIN decodex.routing_snapshot_quota_facts AS fact
			ON fact.snapshot_id=decision_row.snapshot_id
				AND fact.account_id=reference.account_id AND fact.position=reference.position
		LEFT JOIN decodex.quota_windows AS current_fact
			ON current_fact.account_id=fact.account_id AND current_fact.window_class=fact.window_class
				AND current_fact.duration_minutes=fact.duration_minutes
		LEFT JOIN LATERAL (SELECT current_fact.account_id IS NOT NULL
			AND current_fact.metadata->>'timestamp_precision'='unix_microsecond'
			AND current_fact.metadata->>'evidence_revision'=fact.observation_revision::text
			AND current_fact.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(current_fact.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(current_fact.metadata->>'source_id')
			AND current_fact.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND current_fact.metadata->>'raw_resets_at'=fact.resets_at_micros::text AS exact) provenance ON true
		WHERE reference.decision_id=NEW.decision_id AND (
			reference.snapshot_id<>decision_row.snapshot_id OR fact.snapshot_id IS NULL
			OR reference.window_class IS DISTINCT FROM fact.window_class
			OR reference.duration_minutes IS DISTINCT FROM fact.duration_minutes
			OR reference.observation_revision IS DISTINCT FROM fact.observation_revision
			OR reference.remaining_percent IS DISTINCT FROM fact.remaining_percent
			OR reference.observed_at_micros IS DISTINCT FROM fact.observed_at_micros
			OR reference.resets_at_micros IS DISTINCT FROM fact.resets_at_micros
			OR reference.confidence IS DISTINCT FROM fact.confidence
			OR reference.source_id IS DISTINCT FROM
				CASE WHEN provenance.exact THEN current_fact.metadata->>'source_id' END
			OR reference.timestamp_precision IS DISTINCT FROM
				CASE WHEN provenance.exact THEN 'unix_microsecond' END
			OR reference.raw_observed_at IS DISTINCT FROM
				CASE WHEN provenance.exact THEN current_fact.metadata->>'raw_observed_at' END
			OR reference.raw_resets_at IS DISTINCT FROM
				CASE WHEN provenance.exact THEN current_fact.metadata->>'raw_resets_at' END)
	) OR EXISTS (
		SELECT 1 FROM decodex.routing_decision_capability_refs AS reference
		LEFT JOIN decodex.routing_snapshot_capability_facts AS fact
			ON fact.snapshot_id=decision_row.snapshot_id
				AND fact.account_id=reference.account_id AND fact.position=reference.position
		WHERE reference.decision_id=NEW.decision_id AND (
			reference.snapshot_id<>decision_row.snapshot_id OR fact.snapshot_id IS NULL
			OR reference.capability IS DISTINCT FROM fact.capability
			OR reference.applicable IS DISTINCT FROM fact.applicable
			OR reference.evidence_state IS DISTINCT FROM fact.evidence_state)
	) OR EXISTS (
		SELECT 1 FROM decodex.routing_decision_blocker_refs AS reference
		LEFT JOIN decodex.routing_snapshot_blockers AS fact
			ON fact.snapshot_id=decision_row.snapshot_id
				AND fact.account_id=reference.account_id AND fact.position=reference.position
		WHERE reference.decision_id=NEW.decision_id AND (
			reference.snapshot_id<>decision_row.snapshot_id OR fact.snapshot_id IS NULL
			OR reference.blocker IS DISTINCT FROM fact.blocker)
	) THEN
		RAISE EXCEPTION 'V16 routing decision evidence identity is incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	SELECT pg_catalog.count(*) INTO stored_exclusions FROM decodex.routing_decision_exclusions
	WHERE decision_id=NEW.decision_id;
	IF decision_row.kind='selected' THEN
		SELECT pg_catalog.count(*) INTO required_exclusions
		FROM decodex.routing_decision_blocker_refs AS blocker
		JOIN decodex.routing_decision_member_refs AS member
			ON member.decision_id=blocker.decision_id AND member.account_id=blocker.account_id
		JOIN decodex.routing_snapshot_members AS snapshot_member
			ON snapshot_member.snapshot_id=member.snapshot_id
				AND snapshot_member.account_id=member.account_id
		WHERE blocker.decision_id=NEW.decision_id
			AND snapshot_member.disposition='included'
			AND member.position<(SELECT position FROM decodex.routing_decision_member_refs
				WHERE decision_id=NEW.decision_id AND account_id=decision_row.selected_account_id)
			AND blocker.blocker IN ('quota_five_hour_depleted','quota_seven_day_depleted');
	ELSIF decision_row.kind='waiting_usage' THEN
		SELECT pg_catalog.count(*) INTO required_exclusions
		FROM decodex.routing_decision_blocker_refs AS blocker
		JOIN decodex.routing_decision_member_refs AS member
			ON member.decision_id=blocker.decision_id AND member.account_id=blocker.account_id
		JOIN decodex.routing_snapshot_members AS snapshot_member
			ON snapshot_member.snapshot_id=member.snapshot_id
				AND snapshot_member.account_id=member.account_id
		WHERE blocker.decision_id=NEW.decision_id AND snapshot_member.disposition='included'
			AND blocker.blocker IN ('quota_five_hour_depleted','quota_seven_day_depleted');
	ELSE required_exclusions:=0;
	END IF;
	IF stored_exclusions<>required_exclusions THEN
		RAISE EXCEPTION 'V16 routing decision exclusions are incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	IF EXISTS (
		SELECT 1 FROM decodex.routing_decision_exclusions AS exclusion
		JOIN decodex.routing_decision_quota_refs AS quota
			ON quota.decision_id=exclusion.decision_id AND quota.account_id=exclusion.account_id
				AND quota.window_class=exclusion.window_class
				AND quota.duration_minutes=exclusion.duration_minutes
				AND quota.observation_revision=exclusion.observation_revision
		JOIN decodex.routing_decision_member_refs AS member
			ON member.decision_id=exclusion.decision_id AND member.account_id=exclusion.account_id
		JOIN decodex.routing_snapshot_members AS snapshot_member
			ON snapshot_member.snapshot_id=member.snapshot_id
				AND snapshot_member.account_id=member.account_id
		WHERE exclusion.decision_id=NEW.decision_id AND (
			exclusion.member_position IS DISTINCT FROM member.position
			OR exclusion.remaining_percent IS DISTINCT FROM quota.remaining_percent
			OR exclusion.observed_at_micros IS DISTINCT FROM quota.observed_at_micros
			OR exclusion.resets_at_micros IS DISTINCT FROM quota.resets_at_micros
			OR exclusion.confidence IS DISTINCT FROM quota.confidence
			OR exclusion.source_id IS DISTINCT FROM quota.source_id
			OR exclusion.timestamp_precision IS DISTINCT FROM quota.timestamp_precision
			OR exclusion.raw_observed_at IS DISTINCT FROM quota.raw_observed_at
			OR exclusion.raw_resets_at IS DISTINCT FROM quota.raw_resets_at
			OR snapshot_member.disposition<>'included'
			OR (decision_row.kind='selected' AND member.position>=(SELECT position
				FROM decodex.routing_decision_member_refs WHERE decision_id=NEW.decision_id
					AND account_id=decision_row.selected_account_id))
			OR decision_row.kind='no_route'
			OR NOT EXISTS (
				SELECT 1 FROM decodex.routing_decision_blocker_refs AS blocker
				WHERE blocker.decision_id=exclusion.decision_id
					AND blocker.account_id=exclusion.account_id
					AND blocker.blocker=CASE exclusion.window_class
						WHEN 'five_hour' THEN 'quota_five_hour_depleted'::decodex.routing_blocker
						ELSE 'quota_seven_day_depleted'::decodex.routing_blocker END))
	) THEN
		RAISE EXCEPTION 'V16 routing decision exclusion identity is incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	IF decision_row.kind='waiting_usage' AND (
		EXISTS (
			SELECT 1 FROM decodex.routing_snapshot_members AS member
			LEFT JOIN decodex.routing_decision_blocker_refs AS blocker
				ON blocker.decision_id=NEW.decision_id AND blocker.account_id=member.account_id
			WHERE member.snapshot_id=decision_row.snapshot_id AND member.disposition='included'
			GROUP BY member.account_id
			HAVING pg_catalog.count(blocker.blocker)=0 OR pg_catalog.bool_or(
				blocker.blocker NOT IN ('quota_five_hour_depleted','quota_seven_day_depleted'))
		) OR EXISTS (
			SELECT 1 FROM decodex.routing_decision_quota_refs AS quota
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=decision_row.snapshot_id AND member.account_id=quota.account_id
			WHERE quota.decision_id=NEW.decision_id AND member.disposition='included'
				AND (quota.remaining_percent IS NULL OR quota.confidence<>'high'
					OR quota.source_id IS NULL OR quota.observed_at_micros IS NULL
					OR quota.resets_at_micros IS NULL
					OR quota.observed_at_micros>(pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint
					OR (pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint
						-quota.observed_at_micros>300000000
					OR quota.resets_at_micros<=(pg_catalog.extract(epoch FROM decision_row.decided_at)*1000000)::bigint)
		) OR decision_row.waiting_ready_at_micros IS DISTINCT FROM (
			SELECT pg_catalog.min(account_ready) FROM (
				SELECT pg_catalog.max(resets_at_micros) AS account_ready
				FROM decodex.routing_decision_exclusions WHERE decision_id=NEW.decision_id
				GROUP BY account_id
			) AS readiness
		)
	) THEN
		RAISE EXCEPTION 'V16 waiting decision is incomplete'
			USING ERRCODE='23514', CONSTRAINT='routing_decision_complete';
	END IF;
	RETURN NULL;
END
$$;

DO $$
DECLARE relation_name text;
BEGIN
	FOREACH relation_name IN ARRAY ARRAY[
		'routing_decisions', 'routing_decision_member_refs', 'routing_decision_quota_refs',
		'routing_decision_capability_refs', 'routing_decision_blocker_refs',
		'routing_decision_exclusions'
	] LOOP
		EXECUTE pg_catalog.format(
			'CREATE TRIGGER %I_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.%I '
			|| 'FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_routing_decision_mutation()',
			relation_name, relation_name);
	END LOOP;
	FOREACH relation_name IN ARRAY ARRAY[
		'routing_decision_member_refs', 'routing_decision_quota_refs',
		'routing_decision_capability_refs', 'routing_decision_blocker_refs',
		'routing_decision_exclusions'
	] LOOP
		EXECUTE pg_catalog.format(
			'CREATE TRIGGER %I_open_insert BEFORE INSERT ON decodex.%I '
			|| 'FOR EACH ROW EXECUTE FUNCTION decodex.forbid_routing_decision_mutation()',
			relation_name, relation_name);
	END LOOP;
END
$$;

CREATE CONSTRAINT TRIGGER routing_decision_complete
AFTER INSERT ON decodex.routing_decisions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_routing_decision_completeness();

CREATE FUNCTION decodex.route_account_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_routing_policy_id uuid, p_expected_routing_policy_revision bigint,
	p_managed_run_id uuid, p_expected_managed_run_revision bigint
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; snapshot_row decodex.routing_snapshots%ROWTYPE;
DECLARE selected_account uuid; selected_position integer; decided timestamptz;
DECLARE decided_micros bigint; decision_kind text; no_route_value text;
DECLARE ready_micros bigint; decision_uuid uuid; core jsonb; effect jsonb; response bytea;
DECLARE included_count bigint; non_depletion_count bigint; depleted_count bigint;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','route_account','protocol',p_protocol,
		'operation_id',p_operation_id,'routing_policy_id',p_routing_policy_id,
		'expected_routing_policy_revision',p_expected_routing_policy_revision,
		'managed_run_id',p_managed_run_id,
		'expected_managed_run_revision',p_expected_managed_run_revision);
	replay:=decodex.reserve_exact_routing_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(p_managed_run_id::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	PERFORM pg_catalog.pg_advisory_xact_lock(1359,pg_catalog.hashtext(p_operation_id::text));
	LOCK TABLE decodex.accounts, decodex.quota_windows, decodex.routing_policy_heads,
		decodex.policies, decodex.policy_revisions, decodex.role_profiles,
		decodex.role_profile_revisions, decodex.profile_snapshots, decodex.account_snapshots,
		decodex.runtime_sessions,
		decodex.routing_policy_revisions, decodex.routing_policy_members,
		decodex.routing_policy_required_capabilities, decodex.routing_compatibility_evidence,
		decodex.routing_capability_evidence, decodex.routing_snapshots,
		decodex.routing_snapshot_members, decodex.routing_snapshot_quota_facts,
		decodex.routing_snapshot_capability_facts, decodex.routing_snapshot_blockers,
		decodex.managed_runs IN SHARE MODE;
	IF p_operation_id IS NULL OR p_routing_policy_id IS NULL OR p_managed_run_id IS NULL
		OR p_expected_routing_policy_revision <= 0 OR p_expected_managed_run_revision <= 0 THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account','malformed_input');
	END IF;
	PERFORM 1 FROM decodex.routing_policy_heads
	WHERE routing_policy_id=p_routing_policy_id
		AND current_revision=p_expected_routing_policy_revision FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'route_account','stale_routing_policy'); END IF;
	PERFORM 1 FROM decodex.managed_runs WHERE managed_run_id=p_managed_run_id
		AND revision=p_expected_managed_run_revision FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'route_account','stale_managed_run'); END IF;
	SELECT snapshot.* INTO snapshot_row FROM decodex.routing_snapshots AS snapshot
	WHERE snapshot.routing_policy_id=p_routing_policy_id
		AND snapshot.routing_policy_revision=p_expected_routing_policy_revision
		AND snapshot.managed_run_id=p_managed_run_id
		AND snapshot.managed_run_revision=p_expected_managed_run_revision
	ORDER BY snapshot.resolved_at DESC, snapshot.snapshot_id LIMIT 1 FOR SHARE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'route_account','snapshot_missing'); END IF;
	IF NOT EXISTS (SELECT 1 FROM decodex.policies WHERE policy_id=snapshot_row.accepted_policy_id
		AND current_revision=snapshot_row.accepted_policy_revision FOR SHARE)
		OR NOT EXISTS (SELECT 1 FROM decodex.role_profiles WHERE role=snapshot_row.required_role
			AND current_revision=snapshot_row.required_role_profile_revision FOR SHARE)
		OR NOT EXISTS (SELECT 1 FROM decodex.runtime_sessions
			WHERE runtime_session_id=snapshot_row.runtime_session_id
				AND revision=snapshot_row.runtime_session_revision FOR SHARE) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account','concurrent_authority_change');
	END IF;
	decided:=pg_catalog.clock_timestamp();
	decided_micros:=(pg_catalog.extract(epoch FROM decided)*1000000)::bigint;

	-- A decision may consume only a snapshot still equal to every mutable source fact.
	IF EXISTS (
		SELECT account_id,revision FROM decodex.accounts
		EXCEPT SELECT account_id,account_revision FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
	) OR EXISTS (
		SELECT account_id,account_revision FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
		EXCEPT SELECT account_id,revision FROM decodex.accounts
	) OR EXISTS (
		SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
		EXCEPT SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_snapshot_members WHERE snapshot_id=snapshot_row.snapshot_id
	) OR EXISTS (
		SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_snapshot_members WHERE snapshot_id=snapshot_row.snapshot_id
		EXCEPT SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
	) OR EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members AS member
		LEFT JOIN decodex.accounts AS account ON account.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND (
			account.account_id IS NULL OR account.revision<>member.account_revision
			OR account.state<>member.account_state
			OR decodex.rfc3339_utc(account.observed_at)<>member.account_observed_at_utc
			OR account.observed_at>decided OR decided-account.observed_at>INTERVAL '300 seconds')
	) OR EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members AS member
		LEFT JOIN LATERAL (
			SELECT evidence_id,evidence_revision,ingested_at FROM decodex.routing_compatibility_evidence
			WHERE account_id=member.account_id ORDER BY evidence_revision DESC LIMIT 1
		) AS current_evidence ON true
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND (
			member.evidence_id IS DISTINCT FROM current_evidence.evidence_id
			OR member.evidence_revision IS DISTINCT FROM current_evidence.evidence_revision
			OR (member.evidence_id IS NOT NULL AND (current_evidence.ingested_at>decided
				OR decided-current_evidence.ingested_at>INTERVAL '300 seconds')))
	) OR EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_quota_facts AS fact
		LEFT JOIN decodex.quota_windows AS quota ON quota.account_id=fact.account_id
			AND quota.window_class=fact.window_class AND quota.duration_minutes=fact.duration_minutes
		WHERE fact.snapshot_id=snapshot_row.snapshot_id AND (
			(fact.observation_revision IS NULL)<>(quota.account_id IS NULL)
			OR fact.observation_revision IS DISTINCT FROM quota.revision
			OR fact.remaining_percent IS DISTINCT FROM quota.remaining_percent
			OR fact.observed_at_micros IS DISTINCT FROM
				(pg_catalog.extract(epoch FROM quota.observed_at)*1000000)::bigint
			OR fact.resets_at_micros IS DISTINCT FROM
				(pg_catalog.extract(epoch FROM quota.resets_at)*1000000)::bigint
			OR fact.confidence IS DISTINCT FROM quota.confidence)
	) THEN RETURN decodex.complete_exact_routing_rejection(
		p_protocol,p_idempotency_key,'route_account','concurrent_authority_change'); END IF;

	SELECT member.position,member.account_id INTO selected_position,selected_account
	FROM decodex.routing_snapshot_members AS member
	WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
		AND pg_catalog.cardinality(member.blockers)=0
		AND NOT EXISTS (
			SELECT 1 FROM decodex.routing_snapshot_quota_facts AS fact
			LEFT JOIN decodex.quota_windows AS quota ON quota.account_id=fact.account_id
				AND quota.window_class=fact.window_class
				AND quota.duration_minutes=fact.duration_minutes
			WHERE fact.snapshot_id=member.snapshot_id AND fact.account_id=member.account_id
				AND (fact.observation_revision IS NULL OR fact.remaining_percent IS NULL
					OR fact.remaining_percent=0 OR fact.confidence<>'high'
					OR fact.observed_at_micros>decided_micros
					OR decided_micros-fact.observed_at_micros>300000000
					OR fact.resets_at_micros IS NULL OR fact.resets_at_micros<=decided_micros
					OR quota.account_id IS NULL
					OR quota.metadata->>'timestamp_precision' IS DISTINCT FROM 'unix_microsecond'
					OR quota.metadata->>'source_id' IS NULL OR quota.metadata->>'source_id'=''
					OR pg_catalog.octet_length(quota.metadata->>'source_id')>256
					OR decodex.has_credential_material(quota.metadata->>'source_id')
					OR quota.metadata->>'evidence_revision'
						IS DISTINCT FROM fact.observation_revision::text
					OR quota.metadata->>'raw_observed_at' IS DISTINCT FROM fact.observed_at_micros::text
					OR quota.metadata->>'raw_resets_at' IS DISTINCT FROM fact.resets_at_micros::text)
		)
		AND (SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_blockers AS blocker
			JOIN decodex.routing_snapshot_members AS predecessor
				ON predecessor.snapshot_id=blocker.snapshot_id AND predecessor.account_id=blocker.account_id
			WHERE blocker.snapshot_id=member.snapshot_id AND predecessor.disposition='included'
				AND predecessor.position<member.position
				AND blocker.blocker IN ('quota_five_hour_depleted','quota_seven_day_depleted'))
		= (SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_blockers AS blocker
			JOIN decodex.routing_snapshot_members AS predecessor
				ON predecessor.snapshot_id=blocker.snapshot_id AND predecessor.account_id=blocker.account_id
			JOIN decodex.routing_snapshot_quota_facts AS fact
				ON fact.snapshot_id=blocker.snapshot_id AND fact.account_id=blocker.account_id
				AND fact.window_class=CASE blocker.blocker
					WHEN 'quota_five_hour_depleted' THEN 'five_hour'::decodex.quota_window_class
					ELSE 'seven_day'::decodex.quota_window_class END
			JOIN decodex.quota_windows AS quota ON quota.account_id=fact.account_id
				AND quota.window_class=fact.window_class AND quota.duration_minutes=fact.duration_minutes
			WHERE blocker.snapshot_id=member.snapshot_id AND predecessor.disposition='included'
				AND predecessor.position<member.position AND fact.remaining_percent=0
				AND fact.confidence='high' AND fact.observed_at_micros<=decided_micros
				AND decided_micros-fact.observed_at_micros<=300000000
				AND fact.resets_at_micros>decided_micros
				AND quota.metadata->>'timestamp_precision'='unix_microsecond'
				AND quota.metadata->>'source_id'<>''
				AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
				AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
				AND quota.metadata->>'evidence_revision'=fact.observation_revision::text
				AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
				AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text)
	ORDER BY member.sticky DESC, member.position, member.account_id LIMIT 1;
	IF FOUND THEN
		decision_kind:='selected';
	ELSIF EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members AS member
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
	) AND NOT EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members AS member
		LEFT JOIN decodex.routing_snapshot_blockers AS blocker
			ON blocker.snapshot_id=member.snapshot_id AND blocker.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
		GROUP BY member.account_id
		HAVING pg_catalog.count(blocker.blocker)=0 OR pg_catalog.bool_or(
			blocker.blocker NOT IN ('quota_five_hour_depleted','quota_seven_day_depleted'))
	) AND NOT EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members AS member
		JOIN decodex.routing_snapshot_quota_facts AS fact
			ON fact.snapshot_id=member.snapshot_id AND fact.account_id=member.account_id
		LEFT JOIN decodex.quota_windows AS quota ON quota.account_id=fact.account_id
			AND quota.window_class=fact.window_class AND quota.duration_minutes=fact.duration_minutes
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
			AND (fact.observation_revision IS NULL OR fact.remaining_percent IS NULL
				OR fact.confidence<>'high' OR fact.observed_at_micros>decided_micros
				OR decided_micros-fact.observed_at_micros>300000000
				OR fact.resets_at_micros IS NULL OR fact.resets_at_micros<=decided_micros
				OR quota.account_id IS NULL
				OR pg_catalog.jsonb_typeof(quota.metadata)<>'object'
				OR quota.metadata->>'timestamp_precision' IS DISTINCT FROM 'unix_microsecond'
				OR quota.metadata->>'source_id' IS NULL OR quota.metadata->>'source_id'=''
				OR pg_catalog.octet_length(quota.metadata->>'source_id')>256
				OR decodex.has_credential_material(quota.metadata->>'source_id')
				OR quota.metadata->>'evidence_revision'
					IS DISTINCT FROM fact.observation_revision::text
				OR quota.metadata->>'raw_observed_at' IS DISTINCT FROM fact.observed_at_micros::text
				OR quota.metadata->>'raw_resets_at' IS DISTINCT FROM fact.resets_at_micros::text)
	) AND (SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_blockers AS blocker
		JOIN decodex.routing_snapshot_members AS member
			ON member.snapshot_id=blocker.snapshot_id AND member.account_id=blocker.account_id
		WHERE blocker.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
			AND blocker.blocker IN ('quota_five_hour_depleted','quota_seven_day_depleted'))
	= (SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_blockers AS blocker
		JOIN decodex.routing_snapshot_members AS member
			ON member.snapshot_id=blocker.snapshot_id AND member.account_id=blocker.account_id
		JOIN decodex.routing_snapshot_quota_facts AS fact
			ON fact.snapshot_id=blocker.snapshot_id AND fact.account_id=blocker.account_id
			AND fact.window_class=CASE blocker.blocker
				WHEN 'quota_five_hour_depleted' THEN 'five_hour'::decodex.quota_window_class
				ELSE 'seven_day'::decodex.quota_window_class END
		WHERE blocker.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
			AND fact.remaining_percent=0
	) THEN
		decision_kind:='waiting_usage';
		SELECT pg_catalog.min(account_ready) INTO ready_micros FROM (
			SELECT pg_catalog.max(fact.resets_at_micros) AS account_ready
			FROM decodex.routing_snapshot_members AS member
			JOIN decodex.routing_snapshot_quota_facts AS fact
				ON fact.snapshot_id=member.snapshot_id AND fact.account_id=member.account_id
			WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
				AND fact.remaining_percent=0 GROUP BY member.account_id
		) AS readiness;
	ELSE
		decision_kind:='no_route';
		no_route_value:='blocked_evidence';
	END IF;
	decision_uuid:=pg_catalog.gen_random_uuid();
	INSERT INTO decodex.routing_decision_member_refs(decision_id,snapshot_id,position,account_id)
	SELECT decision_uuid,snapshot_id,position,account_id FROM decodex.routing_snapshot_members
	WHERE snapshot_id=snapshot_row.snapshot_id ORDER BY position;
	INSERT INTO decodex.routing_decision_quota_refs(decision_id,snapshot_id,account_id,position,window_class,
		duration_minutes,observation_revision,remaining_percent,observed_at_micros,resets_at_micros,
		confidence,source_id,timestamp_precision,raw_observed_at,raw_resets_at)
	SELECT decision_uuid,fact.snapshot_id,fact.account_id,fact.position,fact.window_class,fact.duration_minutes,
		fact.observation_revision,fact.remaining_percent,fact.observed_at_micros,fact.resets_at_micros,
		fact.confidence,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'source_id' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN 'unix_microsecond' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'raw_observed_at' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'raw_resets_at' END
	FROM decodex.routing_snapshot_quota_facts AS fact
	LEFT JOIN decodex.quota_windows AS quota ON quota.account_id=fact.account_id
		AND quota.window_class=fact.window_class AND quota.duration_minutes=fact.duration_minutes
	WHERE fact.snapshot_id=snapshot_row.snapshot_id ORDER BY fact.account_id,fact.position;
	INSERT INTO decodex.routing_decision_capability_refs
	SELECT decision_uuid,snapshot_id,account_id,position,capability,applicable,evidence_state
	FROM decodex.routing_snapshot_capability_facts WHERE snapshot_id=snapshot_row.snapshot_id
	ORDER BY account_id,position;
	INSERT INTO decodex.routing_decision_blocker_refs
	SELECT decision_uuid,snapshot_id,account_id,position,blocker FROM decodex.routing_snapshot_blockers
	WHERE snapshot_id=snapshot_row.snapshot_id ORDER BY account_id,position;

	IF decision_kind='selected' THEN
		INSERT INTO decodex.routing_decision_exclusions
		SELECT decision_uuid,member.account_id,member.position,quota.window_class,
			quota.duration_minutes,quota.observation_revision,0,quota.observed_at_micros,
			quota.resets_at_micros,quota.confidence,quota.source_id,quota.timestamp_precision,
			quota.raw_observed_at,quota.raw_resets_at,'usage_depleted'
		FROM decodex.routing_snapshot_members AS member
		JOIN decodex.routing_decision_quota_refs AS quota
			ON quota.decision_id=decision_uuid AND quota.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
			AND member.position<selected_position AND quota.remaining_percent=0
			AND quota.confidence='high' AND quota.observed_at_micros<=decided_micros
			AND decided_micros-quota.observed_at_micros<=300000000
			AND quota.resets_at_micros>decided_micros
			AND quota.source_id IS NOT NULL AND quota.raw_observed_at IS NOT NULL
			AND quota.raw_resets_at IS NOT NULL
			AND EXISTS (SELECT 1 FROM decodex.routing_snapshot_blockers AS blocker
				WHERE blocker.snapshot_id=member.snapshot_id AND blocker.account_id=member.account_id
					AND blocker.blocker=CASE quota.window_class
						WHEN 'five_hour' THEN 'quota_five_hour_depleted'::decodex.routing_blocker
						ELSE 'quota_seven_day_depleted'::decodex.routing_blocker END)
		ORDER BY member.position,quota.position;
	ELSIF decision_kind='waiting_usage' THEN
			INSERT INTO decodex.routing_decision_exclusions
			SELECT decision_uuid,member.account_id,member.position,quota.window_class,
				quota.duration_minutes,quota.observation_revision,0,quota.observed_at_micros,
				quota.resets_at_micros,quota.confidence,quota.source_id,quota.timestamp_precision,
				quota.raw_observed_at,quota.raw_resets_at,'usage_depleted'
			FROM decodex.routing_snapshot_members AS member
			JOIN decodex.routing_decision_quota_refs AS quota
				ON quota.decision_id=decision_uuid AND quota.account_id=member.account_id
			WHERE member.snapshot_id=snapshot_row.snapshot_id AND member.disposition='included'
				AND quota.remaining_percent=0 ORDER BY member.position,quota.position;
	END IF;
	INSERT INTO decodex.routing_decisions(decision_id,operation_id,snapshot_id,routing_policy_id,
		routing_policy_revision,managed_run_id,managed_run_revision,kind,selected_account_id,
		waiting_ready_at_micros,no_route_reason,decided_at)
	VALUES(decision_uuid,p_operation_id,snapshot_row.snapshot_id,p_routing_policy_id,
		p_expected_routing_policy_revision,p_managed_run_id,p_expected_managed_run_revision,
		decision_kind::decodex.routing_decision_kind,selected_account,ready_micros,
		no_route_value::decodex.routing_no_route_reason,decided);

	core:=pg_catalog.jsonb_build_object('operation','route_account','decision_id',decision_uuid,
		'operation_id',p_operation_id,'snapshot_id',snapshot_row.snapshot_id,'kind',decision_kind,
		'selected_account_id',selected_account,'waiting_ready_at_micros',ready_micros,
		'no_route_reason',CASE WHEN decision_kind='no_route' THEN 'blocked_evidence' END,
		'decided_at_micros',decided_micros,
		'members',(SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'position',member.position,'account_id',member.account_id,'disposition',member.disposition,
			'sticky',member.sticky,'blockers',member.blockers) ORDER BY member.position)
			FROM decodex.routing_snapshot_members AS member WHERE member.snapshot_id=snapshot_row.snapshot_id),
		'quota_facts',(SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(quota)-'decision_id'-'snapshot_id'
			ORDER BY member.position,quota.position)
			FROM decodex.routing_decision_quota_refs AS quota
			JOIN decodex.routing_decision_member_refs AS member USING(decision_id,account_id)
			WHERE quota.decision_id=decision_uuid),
		'capability_facts',(SELECT pg_catalog.jsonb_agg(
			pg_catalog.to_jsonb(capability)-'decision_id'-'snapshot_id' ORDER BY member.position,capability.position)
			FROM decodex.routing_decision_capability_refs AS capability
			JOIN decodex.routing_decision_member_refs AS member USING(decision_id,account_id)
			WHERE capability.decision_id=decision_uuid),
		'exclusions',(SELECT pg_catalog.coalesce(pg_catalog.jsonb_agg(
			pg_catalog.to_jsonb(exclusion)-'decision_id' ORDER BY member_position,window_class),'[]'::jsonb)
			FROM decodex.routing_decision_exclusions AS exclusion WHERE decision_id=decision_uuid));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,'effect_digest',
		pg_catalog.encode(public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','completed_success','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='completed_success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON TABLE decodex.routing_decisions, decodex.routing_decision_member_refs,
	decodex.routing_decision_quota_refs, decodex.routing_decision_capability_refs,
	decodex.routing_decision_blocker_refs, decodex.routing_decision_exclusions FROM PUBLIC;
REVOKE ALL ON TYPE decodex.routing_decision_kind, decodex.routing_no_route_reason FROM PUBLIC;
REVOKE ALL ON FUNCTION decodex.forbid_routing_decision_mutation(),
	decodex.enforce_routing_decision_completeness(),
	decodex.route_account_exact(text,text,uuid,uuid,bigint,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION decodex.route_account_exact(text,text,uuid,uuid,bigint,uuid,bigint)
	TO decodex_runtime;
