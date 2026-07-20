-- XY-1360 inert continuation authority after one exact persisted V16 decision.
-- This migration creates no routing resolution, account selection, credential switching,
-- scheduling, wake, dispatch, turn submission, or side-effect replay authority.

CREATE TYPE decodex.continuation_plan_kind AS ENUM ('same_thread', 'context_pack_fallback');

CREATE TABLE decodex.continuation_plans (
	plan_id uuid PRIMARY KEY,
	operation_id uuid NOT NULL UNIQUE,
	routing_decision_id uuid NOT NULL UNIQUE,
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 0),
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id),
	source_runtime_session_id uuid NOT NULL,
	source_runtime_session_revision bigint NOT NULL CHECK (source_runtime_session_revision > 0),
	selected_account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	kind decodex.continuation_plan_kind NOT NULL,
	codex_thread_id uuid,
	fallback_context_pack_id uuid UNIQUE,
	fallback_runtime_session_id uuid UNIQUE,
	routing_evidence_id uuid REFERENCES decodex.routing_compatibility_evidence(evidence_id),
	routing_evidence_revision bigint,
	schema_fingerprint text,
	codex_experiment_id uuid REFERENCES decodex.codex_experiments(experiment_id),
	codex_experiment_revision bigint,
	codex_observation_id uuid REFERENCES decodex.codex_experiment_observations(observation_id),
	effect_barrier_state decodex.effect_barrier_state NOT NULL,
	effect_barrier_revision bigint NOT NULL CHECK (effect_barrier_revision > 0),
	submitted_turn_receipt_count bigint NOT NULL CHECK (submitted_turn_receipt_count >= 0),
	replay_permitted boolean NOT NULL DEFAULT false CHECK (NOT replay_permitted),
	dispatch_enabled boolean NOT NULL DEFAULT false CHECK (NOT dispatch_enabled),
	revision bigint NOT NULL DEFAULT 1 CHECK (revision = 1),
	request_envelope jsonb NOT NULL,
	effect_envelope jsonb NOT NULL,
	response_bytes bytea NOT NULL,
	planned_at timestamptz NOT NULL,
	CONSTRAINT continuation_plans_run_revision_authority_fk FOREIGN KEY (
		routing_decision_id, managed_run_id, managed_run_revision
	) REFERENCES decodex.routing_decisions(
		decision_id, managed_run_id, managed_run_revision
	),
	CONSTRAINT continuation_plans_source_conversation_fk FOREIGN KEY (
		source_runtime_session_id, conversation_id
	) REFERENCES decodex.runtime_sessions(runtime_session_id, conversation_id),
	CONSTRAINT continuation_plans_fallback_pack_fk FOREIGN KEY (
		fallback_context_pack_id, conversation_id
	) REFERENCES decodex.context_packs(context_pack_id, conversation_id),
	CONSTRAINT continuation_plans_fallback_session_fk FOREIGN KEY (
		fallback_runtime_session_id, conversation_id
	) REFERENCES decodex.runtime_sessions(runtime_session_id, conversation_id),
	CONSTRAINT continuation_plans_evidence_revision CHECK (
		(routing_evidence_id IS NULL) = (routing_evidence_revision IS NULL)
		AND (routing_evidence_id IS NULL) = (schema_fingerprint IS NULL)
	),
	CONSTRAINT continuation_plans_experiment_revision CHECK (
		(codex_experiment_id IS NULL) = (codex_experiment_revision IS NULL)
		AND (codex_experiment_id IS NULL) = (codex_observation_id IS NULL)
	),
	CONSTRAINT continuation_plans_shape CHECK (
		(kind = 'same_thread' AND codex_thread_id IS NOT NULL
			AND fallback_context_pack_id IS NULL AND fallback_runtime_session_id IS NULL
			AND routing_evidence_id IS NOT NULL AND routing_evidence_revision > 0
			AND schema_fingerprint COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
			AND codex_experiment_id IS NOT NULL AND codex_experiment_revision = 3
			AND codex_observation_id IS NOT NULL)
		OR (kind = 'context_pack_fallback' AND codex_thread_id IS NULL
			AND fallback_context_pack_id IS NOT NULL AND fallback_runtime_session_id IS NOT NULL
			AND routing_evidence_id IS NULL AND routing_evidence_revision IS NULL
			AND schema_fingerprint IS NULL AND codex_experiment_id IS NULL
			AND codex_experiment_revision IS NULL AND codex_observation_id IS NULL)
	),
	CONSTRAINT continuation_plans_finite_time CHECK (pg_catalog.isfinite(planned_at))
);

CREATE FUNCTION decodex.forbid_continuation_plan_mutation()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
BEGIN
	SELECT role.rolname INTO owner_name
	FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner
	WHERE class.oid=TG_RELID;
	IF current_user::name<>owner_name THEN
		RAISE EXCEPTION 'continuation plans are writable only by their command owner'
			USING ERRCODE='42501', CONSTRAINT='continuation_plan_command_owner';
	END IF;
	IF TG_OP<>'INSERT' THEN
		RAISE EXCEPTION 'continuation plans are immutable'
			USING ERRCODE='55000', CONSTRAINT='continuation_plan_immutable';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER continuation_plans_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.continuation_plans
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_continuation_plan_mutation();

CREATE FUNCTION decodex.enforce_continuation_plan_completeness()
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
		JOIN decodex.codex_experiment_thread_bindings AS binding
			ON binding.experiment_id=experiment.experiment_id
		JOIN decodex.codex_experiment_observations AS observation
			ON observation.observation_id=NEW.codex_observation_id
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
			AND observation.experiment_id=experiment.experiment_id
			AND observation.experiment_revision=3 AND observation.thread_id=binding.thread_id
			AND observation.kind='thread_read_item'
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
			AND observation.observed_at<=NEW.planned_at
			AND NEW.planned_at-observation.observed_at<=INTERVAL '300 seconds'
			AND 4=(SELECT pg_catalog.count(DISTINCT capability.capability)
				FROM decodex.routing_capability_evidence AS capability
				WHERE capability.evidence_id=evidence.evidence_id
					AND capability.capability IN
						('initialize','account_read','thread_read','paginated_history')
					AND capability.state='supported')
	) THEN
		RAISE EXCEPTION 'same-thread plan has forged positive evidence'
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
CREATE CONSTRAINT TRIGGER continuation_plan_complete
AFTER INSERT ON decodex.continuation_plans DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_continuation_plan_completeness();

CREATE FUNCTION decodex.enforce_continuation_event_namespace()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name; linked boolean;
BEGIN
	SELECT role.rolname INTO owner_name FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner WHERE class.oid=TG_RELID;
	IF TG_TABLE_NAME='activity' THEN
		linked:=NEW.aggregate_kind='continuation_plan'
			OR NEW.event_kind='continuation_plan_created'
			OR pg_catalog.jsonb_path_exists(NEW.payload,'$.** ? (
				exists(@.continuation_plan_id) || exists(@.routing_decision_id) ||
				exists(@.fallback_context_pack_id) || exists(@.fallback_runtime_session_id)
			)');
		IF TG_OP='UPDATE' THEN
			linked:=linked OR OLD.aggregate_kind='continuation_plan'
				OR OLD.event_kind='continuation_plan_created'
				OR pg_catalog.jsonb_path_exists(OLD.payload,'$.** ? (
					exists(@.continuation_plan_id) || exists(@.routing_decision_id) ||
					exists(@.fallback_context_pack_id) || exists(@.fallback_runtime_session_id)
				)');
		END IF;
		IF linked AND current_user::name<>owner_name THEN
			RAISE EXCEPTION 'continuation activity/outbox namespace is command-owned'
				USING ERRCODE='42501', CONSTRAINT='continuation_event_namespace';
		END IF;
	ELSIF TG_TABLE_NAME='outbox' THEN
		linked:=NEW.aggregate_kind='continuation_plan'
			OR pg_catalog.jsonb_path_exists(NEW.payload,'$.** ? (
				exists(@.continuation_plan_id) || exists(@.routing_decision_id) ||
				exists(@.fallback_context_pack_id) || exists(@.fallback_runtime_session_id)
			)');
		IF TG_OP='UPDATE' THEN
			linked:=linked OR OLD.aggregate_kind='continuation_plan'
				OR pg_catalog.jsonb_path_exists(OLD.payload,'$.** ? (
					exists(@.continuation_plan_id) || exists(@.routing_decision_id) ||
					exists(@.fallback_context_pack_id) || exists(@.fallback_runtime_session_id)
				)');
		END IF;
		IF linked AND current_user::name<>owner_name THEN
			IF TG_OP='INSERT' THEN
				RAISE EXCEPTION 'continuation activity/outbox namespace is command-owned'
					USING ERRCODE='42501', CONSTRAINT='continuation_event_namespace';
			ELSIF NEW.id IS DISTINCT FROM OLD.id OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload OR NEW.created_at IS DISTINCT FROM OLD.created_at
			THEN
				RAISE EXCEPTION 'continuation outbox authority fields are command-owned'
					USING ERRCODE='42501', CONSTRAINT='continuation_event_namespace';
			END IF;
		END IF;
	ELSE
		RAISE EXCEPTION 'continuation event namespace has unexpected trigger relation'
			USING ERRCODE='42501', CONSTRAINT='continuation_event_namespace';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER activity_continuation_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_continuation_event_namespace();
CREATE TRIGGER outbox_continuation_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_continuation_event_namespace();

CREATE FUNCTION decodex.is_canonical_continuation_pack(
	p_conversation_id uuid, p_compiled_bytes bytea, p_compiled_digest text,
	p_manifest_digest text, p_max_bytes integer, p_recent_item_limit integer,
	p_possible_side_effects text, p_truncated boolean, p_omitted_source_count integer,
	p_source_kinds text[], p_source_ids text[], p_source_revisions bigint[],
	p_content_digests text[], p_original_lengths bigint[], p_included_lengths bigint[],
	p_included_digests text[], p_dispositions text[], p_artifact_ids text[],
	p_artifact_revisions bigint[]
) RETURNS boolean LANGUAGE plpgsql IMMUTABLE
SET search_path = pg_catalog, decodex
AS $$
DECLARE source_count integer; position integer; kind_tag integer; prior_kind integer:=-1;
DECLARE disposition_tag integer; manifest_bytes bytea; computed_manifest text;
DECLARE header bytea; cursor integer; encoded_position integer; encoded_length bigint;
DECLARE represented bytea; computed_truncated boolean:=false; computed_omitted integer:=0;
BEGIN
	source_count:=pg_catalog.cardinality(p_source_kinds);
	IF p_conversation_id IS NULL OR p_compiled_bytes IS NULL
		OR p_compiled_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
		OR p_manifest_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
		OR pg_catalog.encode(public.digest(p_compiled_bytes,'sha256'),'hex')<>p_compiled_digest
		OR pg_catalog.octet_length(p_compiled_bytes) NOT BETWEEN 1 AND 262144
		OR p_max_bytes NOT BETWEEN 1024 AND 262144
		OR pg_catalog.octet_length(p_compiled_bytes)>p_max_bytes
		OR p_recent_item_limit NOT BETWEEN 1 AND 256
		OR p_possible_side_effects NOT IN ('none','possible','unknown')
		OR source_count NOT BETWEEN 1 AND 512
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_source_ids)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_source_revisions)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_content_digests)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_original_lengths)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_included_lengths)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_included_digests)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_dispositions)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_artifact_ids)
		OR source_count IS DISTINCT FROM pg_catalog.cardinality(p_artifact_revisions)
	THEN RETURN false; END IF;
	manifest_bytes:=pg_catalog.int2send(source_count::smallint);
	FOR position IN 1..source_count LOOP
		kind_tag:=CASE p_source_kinds[position]
			WHEN 'pinned_revision' THEN 0 WHEN 'repository_instructions' THEN 1
			WHEN 'openwiki' THEN 2 WHEN 'decision' THEN 3 WHEN 'fact' THEN 4
			WHEN 'artifact' THEN 5 WHEN 'recent_raw' THEN 6 ELSE -1 END;
		disposition_tag:=CASE p_dispositions[position]
			WHEN 'complete' THEN 0 WHEN 'truncated' THEN 1 WHEN 'omitted' THEN 2 ELSE -1 END;
		IF kind_tag<0 OR disposition_tag<0 OR kind_tag<prior_kind
			OR (position=1) IS DISTINCT FROM (kind_tag=0) OR (position>1 AND kind_tag=0)
			OR p_source_ids[position] IS NULL
			OR pg_catalog.octet_length(p_source_ids[position]) NOT BETWEEN 1 AND 256
			OR decodex.has_credential_material(p_source_ids[position])
			OR p_source_revisions[position] IS NULL OR p_source_revisions[position]<=0
			OR p_content_digests[position] COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
			OR p_included_digests[position] COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
			OR p_original_lengths[position] NOT BETWEEN 0 AND 2097152
			OR p_included_lengths[position] NOT BETWEEN 0 AND p_original_lengths[position]
			OR (disposition_tag=0 AND (p_original_lengths[position]<=0
				OR p_included_lengths[position]<>p_original_lengths[position]
				OR p_included_digests[position]<>p_content_digests[position]))
			OR (disposition_tag=1 AND (p_included_lengths[position]<=0
				OR p_included_lengths[position]>=p_original_lengths[position]))
			OR (disposition_tag=2 AND (p_included_lengths[position]<>0
				OR p_included_digests[position]<>'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'))
			OR (position=1 AND (disposition_tag<>0 OR p_included_lengths[position]<=0))
			OR (kind_tag=5) IS DISTINCT FROM (p_artifact_ids[position]<>'')
			OR (kind_tag=5 AND (p_artifact_ids[position] COLLATE pg_catalog."C" !~
				'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
				OR p_artifact_ids[position]<>p_source_ids[position]
				OR p_artifact_revisions[position]<>p_source_revisions[position]))
			OR (kind_tag<>5 AND (p_artifact_ids[position]<>'' OR p_artifact_revisions[position]<>0))
		THEN RETURN false; END IF;
		manifest_bytes:=manifest_bytes
			||pg_catalog.decode(pg_catalog.lpad(pg_catalog.to_hex(kind_tag),2,'0'),'hex')
			||pg_catalog.int2send(pg_catalog.octet_length(p_source_ids[position])::smallint)
			||pg_catalog.convert_to(p_source_ids[position],'UTF8')
			||pg_catalog.int8send(p_source_revisions[position])
			||pg_catalog.convert_to(p_content_digests[position],'UTF8')
			||pg_catalog.int8send(p_original_lengths[position])
			||pg_catalog.int8send(p_included_lengths[position])
			||pg_catalog.convert_to(p_included_digests[position],'UTF8')
			||pg_catalog.decode(pg_catalog.lpad(pg_catalog.to_hex(disposition_tag),2,'0'),'hex')
			||CASE WHEN kind_tag=5 THEN pg_catalog.decode('01','hex')
				||pg_catalog.convert_to(p_artifact_ids[position],'UTF8')
				||pg_catalog.int8send(p_artifact_revisions[position])
				ELSE pg_catalog.decode('00','hex') END;
		prior_kind:=kind_tag;
		computed_truncated:=computed_truncated OR disposition_tag<>0;
		computed_omitted:=computed_omitted+CASE WHEN disposition_tag=2 THEN 1 ELSE 0 END;
	END LOOP;
	computed_manifest:=pg_catalog.encode(public.digest(manifest_bytes,'sha256'),'hex');
	IF computed_manifest<>p_manifest_digest OR computed_truncated IS DISTINCT FROM p_truncated
		OR computed_omitted<>p_omitted_source_count THEN RETURN false; END IF;
	header:=pg_catalog.convert_to('decodex/context-pack/2','UTF8')||pg_catalog.decode('00','hex')
		||pg_catalog.convert_to(p_conversation_id::text,'UTF8')
		||pg_catalog.decode(CASE p_possible_side_effects WHEN 'none' THEN '00'
			WHEN 'possible' THEN '01' ELSE '02' END,'hex')
		||pg_catalog.int4send(p_max_bytes)||pg_catalog.int2send(p_recent_item_limit::smallint)
		||pg_catalog.int2send(source_count::smallint)||pg_catalog.convert_to(p_manifest_digest,'UTF8')
		||pg_catalog.decode(CASE WHEN p_truncated THEN '01' ELSE '00' END,'hex');
	IF pg_catalog.substr(p_compiled_bytes, 1, pg_catalog.octet_length(header))<>header
	THEN RETURN false; END IF;
	cursor:=pg_catalog.octet_length(header)+1;
	FOR position IN 1..source_count LOOP
		IF p_included_lengths[position]=0 THEN CONTINUE; END IF;
		IF cursor+5>pg_catalog.octet_length(p_compiled_bytes) THEN RETURN false; END IF;
		encoded_position:=pg_catalog.get_byte(p_compiled_bytes,cursor-1)*256
			+pg_catalog.get_byte(p_compiled_bytes,cursor);
		encoded_length:=pg_catalog.get_byte(p_compiled_bytes,cursor+1)::bigint*16777216
			+pg_catalog.get_byte(p_compiled_bytes,cursor+2)::bigint*65536
			+pg_catalog.get_byte(p_compiled_bytes,cursor+3)::bigint*256
			+pg_catalog.get_byte(p_compiled_bytes,cursor+4)::bigint;
		cursor:=cursor+6;
		IF encoded_position<>position-1 OR encoded_length<>p_included_lengths[position]
			OR cursor+encoded_length-1>pg_catalog.octet_length(p_compiled_bytes)
		THEN RETURN false; END IF;
		represented:=pg_catalog.substr(p_compiled_bytes, cursor, encoded_length::integer);
		IF pg_catalog.encode(public.digest(represented,'sha256'),'hex')<>p_included_digests[position]
		THEN RETURN false; END IF;
		cursor:=cursor+encoded_length::integer;
	END LOOP;
	RETURN cursor=pg_catalog.octet_length(p_compiled_bytes)+1;
EXCEPTION WHEN OTHERS THEN RETURN false;
END
$$;

CREATE FUNCTION decodex.complete_exact_continuation_rejection(
	p_protocol text, p_idempotency_key text, p_code text
) RETURNS bytea LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	core:=pg_catalog.jsonb_build_object('operation','plan_continuation','rejection',p_code);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','stable_domain_rejection','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_rejected',
		outcome_class='stable_domain_rejection',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.reserve_exact_continuation_command(
	p_protocol text, p_idempotency_key text, p_request jsonb
) RETURNS bytea LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE stored record;
BEGIN
	IF pg_catalog.current_setting('transaction_isolation')<>'read committed' THEN
		RAISE EXCEPTION 'exact commands require READ COMMITTED' USING ERRCODE='40001';
	END IF;
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_request IS NULL
		OR pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key) THEN
		RAISE EXCEPTION 'exact continuation command identity is invalid' USING ERRCODE='22023';
	END IF;
	INSERT INTO decodex.exact_command_receipts(
		protocol_version,idempotency_key,request_envelope,request_digest,receipt_state
	) VALUES(p_protocol,p_idempotency_key,p_request,
		public.digest(pg_catalog.convert_to(p_request::text,'UTF8'),'sha256'),'executing')
	ON CONFLICT DO NOTHING;
	SELECT request_envelope,response_bytes,receipt_state INTO STRICT stored
	FROM decodex.exact_command_receipts
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key FOR UPDATE;
	IF stored.request_envelope<>p_request THEN
		RAISE EXCEPTION 'idempotency key reused for another continuation command' USING ERRCODE='DX001';
	END IF;
	IF stored.receipt_state<>'executing' THEN RETURN stored.response_bytes; END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.read_continuation_plan_exact(
	p_plan_id uuid, p_expected_revision bigint
) RETURNS TABLE(
	response_bytes bytea, effect_envelope jsonb, kind text, codex_thread_id text,
	fallback_context_pack_id text, fallback_runtime_session_id text,
	replay_permitted boolean, dispatch_enabled boolean
) LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
	SELECT plan.response_bytes,plan.effect_envelope,plan.kind::text,plan.codex_thread_id::text,
		plan.fallback_context_pack_id::text,plan.fallback_runtime_session_id::text,
		plan.replay_permitted,plan.dispatch_enabled
	FROM decodex.continuation_plans AS plan
	WHERE plan.plan_id=p_plan_id AND plan.revision=p_expected_revision
$$;

CREATE FUNCTION decodex.plan_continuation_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_decision_id uuid,
	p_expected_managed_run_revision bigint, p_plan_id uuid, p_fallback_session_id uuid,
	p_account_snapshot_id uuid, p_context_pack_id uuid,
	p_compiled_bytes bytea, p_compiled_digest text, p_manifest_digest text,
	p_max_bytes integer, p_recent_item_limit integer, p_possible_side_effects text,
	p_truncated boolean, p_omitted_source_count integer,
	p_source_kinds text[], p_source_ids text[], p_source_revisions bigint[],
	p_content_digests text[], p_original_lengths bigint[], p_included_lengths bigint[],
	p_included_digests text[], p_dispositions text[], p_artifact_ids text[],
	p_artifact_revisions bigint[]
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; existing_plan record; decision_row record;
DECLARE snapshot_row record; run_row record; session_row record; profile_row record;
DECLARE conversation_row record; member_row record; barrier_row record;
DECLARE evidence_count bigint; evidence_row record;
DECLARE planned timestamptz; pack_revision bigint; source_count integer;
DECLARE position integer; inline_value bytea; blob_value text; submitted_count bigint;
DECLARE plan_kind decodex.continuation_plan_kind; thread_value uuid;
DECLARE core jsonb; effect jsonb; response bytea; activity_sequence bigint; outbox_id bigint;
DECLARE activity_rows jsonb:='[]'::jsonb; outbox_rows jsonb:='[]'::jsonb; payload jsonb;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','plan_continuation','protocol',p_protocol,'operation_id',p_operation_id,
		'decision_id',p_decision_id,'expected_managed_run_revision',p_expected_managed_run_revision,
		'plan_id',p_plan_id,'fallback_session_id',p_fallback_session_id,
		'account_snapshot_id',p_account_snapshot_id,'context_pack_id',p_context_pack_id,
		'compiled_digest',p_compiled_digest,'manifest_digest',p_manifest_digest,
		'byte_length',pg_catalog.octet_length(p_compiled_bytes),'max_bytes',p_max_bytes,
		'recent_item_limit',p_recent_item_limit,'possible_side_effects',p_possible_side_effects,
		'truncated',p_truncated,'omitted_source_count',p_omitted_source_count,
		'source_kinds',p_source_kinds,'source_ids',p_source_ids,
		'source_revisions',p_source_revisions,'content_digests',p_content_digests,
		'original_lengths',p_original_lengths,'included_lengths',p_included_lengths,
		'included_digests',p_included_digests,'dispositions',p_dispositions,
		'artifact_ids',p_artifact_ids,'artifact_revisions',p_artifact_revisions);
	replay:=decodex.reserve_exact_continuation_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_decision_id IS NULL OR p_plan_id IS NULL
		OR p_fallback_session_id IS NULL OR p_account_snapshot_id IS NULL OR p_context_pack_id IS NULL
		OR p_expected_managed_run_revision IS NULL OR p_expected_managed_run_revision<=0 THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1360,pg_catalog.hashtext(p_decision_id::text));
	SELECT * INTO decision_row FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id FOR UPDATE;
	IF NOT FOUND THEN RETURN decodex.complete_exact_continuation_rejection(
		p_protocol,p_idempotency_key,'missing_decision'); END IF;
	IF decision_row.kind<>'selected' THEN RETURN decodex.complete_exact_continuation_rejection(
		p_protocol,p_idempotency_key,'decision_not_selected'); END IF;
	SELECT * INTO existing_plan FROM decodex.continuation_plans
	WHERE routing_decision_id=p_decision_id OR operation_id=p_operation_id OR plan_id=p_plan_id
	FOR SHARE;
	IF FOUND THEN
		IF existing_plan.routing_decision_id<>p_decision_id
			OR existing_plan.operation_id<>p_operation_id OR existing_plan.plan_id<>p_plan_id
			OR existing_plan.request_envelope<>request THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'decision_already_consumed');
		END IF;
		UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
			outcome_class='success',effect_envelope=existing_plan.effect_envelope,
			response_bytes=existing_plan.response_bytes,completed_at=pg_catalog.clock_timestamp()
		WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
		RETURN existing_plan.response_bytes;
	END IF;
	SELECT * INTO snapshot_row FROM decodex.routing_snapshots
	WHERE snapshot_id=decision_row.snapshot_id FOR SHARE;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=decision_row.managed_run_id FOR UPDATE;
	IF snapshot_row.snapshot_id IS NULL OR run_row.managed_run_id IS NULL
		OR decision_row.managed_run_revision<>p_expected_managed_run_revision
		OR run_row.revision<>p_expected_managed_run_revision
		OR snapshot_row.managed_run_revision<>p_expected_managed_run_revision THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'stale_managed_run_revision');
	END IF;
	SELECT * INTO session_row FROM decodex.runtime_sessions
	WHERE (runtime_session_id,revision)=
		(snapshot_row.runtime_session_id,snapshot_row.runtime_session_revision) FOR SHARE;
	SELECT * INTO profile_row FROM decodex.profile_snapshots
	WHERE profile_snapshot_id=snapshot_row.profile_snapshot_id FOR SHARE;
	SELECT * INTO conversation_row FROM decodex.conversations
	WHERE conversation_id=session_row.conversation_id FOR SHARE;
	SELECT * INTO member_row FROM decodex.routing_snapshot_members
	WHERE snapshot_id=snapshot_row.snapshot_id AND account_id=decision_row.selected_account_id
	FOR SHARE;
	SELECT * INTO barrier_row FROM decodex.managed_run_effect_barriers
	WHERE managed_run_id=run_row.managed_run_id FOR SHARE;
	SELECT pg_catalog.count(*) INTO submitted_count
	FROM decodex.managed_run_submitted_turn_receipts
	WHERE managed_run_id=run_row.managed_run_id;
	IF session_row.runtime_session_id IS NULL OR profile_row.profile_snapshot_id IS NULL
		OR conversation_row.conversation_id IS NULL OR conversation_row.status<>'open'
		OR member_row.account_id IS NULL OR barrier_row.managed_run_id IS NULL
		OR session_row.conversation_id IS NULL OR member_row.disposition<>'included'
		OR pg_catalog.cardinality(member_row.blockers)<>0
		OR (session_row.profile_snapshot_id,profile_row.role,profile_row.source_revision)
			IS DISTINCT FROM (snapshot_row.profile_snapshot_id,snapshot_row.required_role,
				snapshot_row.required_role_profile_revision) THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'stale_managed_run_revision');
	END IF;
	planned:=pg_catalog.clock_timestamp();
	SELECT pg_catalog.count(*) INTO evidence_count FROM (
	SELECT experiment.experiment_id
	FROM decodex.routing_compatibility_evidence AS evidence
	JOIN decodex.routing_capability_evidence AS capability ON capability.evidence_id=evidence.evidence_id
	JOIN decodex.codex_experiments AS experiment
		ON (experiment.managed_run_id,experiment.managed_run_revision,
			experiment.routing_snapshot_id,experiment.account_id,experiment.account_revision,
			experiment.role_profile_revision,experiment.build_id,experiment.revision,experiment.state)=
			(decision_row.managed_run_id,decision_row.managed_run_revision,snapshot_row.snapshot_id,
			 decision_row.selected_account_id,member_row.account_revision,
			 snapshot_row.required_role_profile_revision,snapshot_row.required_build_id,3,'thread_bound')
	JOIN decodex.codex_experiment_thread_bindings AS binding
		ON binding.experiment_id=experiment.experiment_id
	JOIN decodex.codex_experiment_observations AS observation
		ON observation.experiment_id=experiment.experiment_id
		AND observation.experiment_revision=3 AND observation.thread_id=binding.thread_id
		AND observation.kind='thread_read_item'
	WHERE evidence.evidence_id=member_row.evidence_id
		AND evidence.evidence_revision=member_row.evidence_revision
		AND evidence.account_id=decision_row.selected_account_id
		AND evidence.account_revision=member_row.account_revision
		AND evidence.role=snapshot_row.required_role
		AND evidence.role_profile_revision=snapshot_row.required_role_profile_revision
		AND evidence.build_id=snapshot_row.required_build_id
		AND evidence.process_account_id=decision_row.selected_account_id
		AND session_row.codex_thread_id IS NOT NULL
		AND binding.thread_id=session_row.codex_thread_id::text
		AND session_row.state='active' AND NOT run_row.diverged
		AND evidence.ingested_at<=planned AND planned-evidence.ingested_at<=INTERVAL '300 seconds'
		AND experiment.updated_at<=planned AND planned-experiment.updated_at<=INTERVAL '300 seconds'
		AND observation.observed_at<=planned AND planned-observation.observed_at<=INTERVAL '300 seconds'
		AND capability.capability IN ('initialize','account_read','thread_read','paginated_history')
		AND capability.state='supported'
	GROUP BY experiment.experiment_id
	HAVING pg_catalog.count(DISTINCT capability.capability)=4
	) AS canonical_experiment;
	IF evidence_count=1 THEN
		SELECT evidence.evidence_id,evidence.evidence_revision,evidence.schema_fingerprint,
			experiment.experiment_id,experiment.revision,
			observation.observation_id,observation.observed_at
		INTO evidence_row
		FROM decodex.routing_compatibility_evidence AS evidence
		JOIN decodex.codex_experiments AS experiment
			ON experiment.managed_run_id=decision_row.managed_run_id
			AND experiment.managed_run_revision=decision_row.managed_run_revision
			AND experiment.routing_snapshot_id=snapshot_row.snapshot_id
			AND experiment.account_id=decision_row.selected_account_id
			AND experiment.account_revision=member_row.account_revision
			AND experiment.role_profile_revision=snapshot_row.required_role_profile_revision
			AND experiment.build_id=snapshot_row.required_build_id
			AND experiment.revision=3 AND experiment.state='thread_bound'
		JOIN decodex.codex_experiment_thread_bindings AS binding
			ON binding.experiment_id=experiment.experiment_id
		JOIN decodex.codex_experiment_observations AS observation
			ON observation.experiment_id=experiment.experiment_id
			AND observation.experiment_revision=3 AND observation.thread_id=binding.thread_id
			AND observation.kind='thread_read_item'
		WHERE evidence.evidence_id=member_row.evidence_id
			AND evidence.evidence_revision=member_row.evidence_revision
			AND binding.thread_id=session_row.codex_thread_id::text
			AND evidence.ingested_at<=planned AND planned-evidence.ingested_at<=INTERVAL '300 seconds'
			AND experiment.updated_at<=planned AND planned-experiment.updated_at<=INTERVAL '300 seconds'
			AND observation.observed_at<=planned AND planned-observation.observed_at<=INTERVAL '300 seconds'
		ORDER BY observation.observed_at DESC,observation.observation_id LIMIT 1;
		plan_kind:='same_thread'; thread_value:=session_row.codex_thread_id;
	ELSE
		plan_kind:='context_pack_fallback'; thread_value:=NULL;
		IF NOT decodex.is_canonical_continuation_pack(
			session_row.conversation_id,p_compiled_bytes,p_compiled_digest,p_manifest_digest,
			p_max_bytes,p_recent_item_limit,p_possible_side_effects,p_truncated,
			p_omitted_source_count,p_source_kinds,p_source_ids,p_source_revisions,
			p_content_digests,p_original_lengths,p_included_lengths,p_included_digests,
			p_dispositions,p_artifact_ids,p_artifact_revisions) THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'invalid_context_pack');
		END IF;
		IF EXISTS (SELECT 1 FROM decodex.runtime_sessions
			WHERE runtime_session_id=p_fallback_session_id)
			OR EXISTS (SELECT 1 FROM decodex.account_snapshots
				WHERE account_snapshot_id=p_account_snapshot_id)
			OR EXISTS (SELECT 1 FROM decodex.context_packs
				WHERE context_pack_id=p_context_pack_id) THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'fallback_identity_conflict');
		END IF;
		SELECT COALESCE(pg_catalog.max(stored_pack.pack_revision),0)+1 INTO pack_revision
		FROM decodex.context_packs AS stored_pack WHERE stored_pack.conversation_id=session_row.conversation_id;
		source_count:=pg_catalog.cardinality(p_source_kinds);
		FOR position IN 1..source_count LOOP
			IF p_artifact_ids[position]<>'' AND NOT EXISTS (
				SELECT 1 FROM decodex.artifact_revisions AS artifact
				JOIN decodex.blob_objects AS blob ON blob.blob_hash=artifact.blob_hash
				WHERE artifact.artifact_id=p_artifact_ids[position]::uuid
					AND artifact.conversation_id=session_row.conversation_id
					AND artifact.revision=p_artifact_revisions[position]
					AND artifact.blob_hash=p_content_digests[position]
					AND blob.byte_length=p_original_lengths[position]) THEN
				RETURN decodex.complete_exact_continuation_rejection(
					p_protocol,p_idempotency_key,'invalid_context_pack');
			END IF;
		END LOOP;
		IF pg_catalog.octet_length(p_compiled_bytes)>16384 THEN
			inline_value:=NULL; blob_value:=p_compiled_digest;
			INSERT INTO decodex.blob_objects(blob_hash,byte_length,verified_at,created_at)
			VALUES(blob_value,pg_catalog.octet_length(p_compiled_bytes),planned,planned)
			ON CONFLICT (blob_hash) DO NOTHING;
			IF NOT EXISTS (SELECT 1 FROM decodex.blob_objects
				WHERE blob_hash=blob_value AND byte_length=pg_catalog.octet_length(p_compiled_bytes)) THEN
				RETURN decodex.complete_exact_continuation_rejection(
					p_protocol,p_idempotency_key,'invalid_context_pack');
			END IF;
		ELSE inline_value:=p_compiled_bytes; blob_value:=NULL; END IF;
		FOR position IN 1..source_count LOOP
			INSERT INTO decodex.context_pack_sources(context_pack_id,conversation_id,position,kind,
				source_id,source_revision,content_digest,original_byte_length,included_byte_length,
				included_digest,disposition,artifact_id,artifact_revision)
			VALUES(p_context_pack_id,session_row.conversation_id,position-1,
				p_source_kinds[position]::decodex.context_source_kind,p_source_ids[position],
				p_source_revisions[position],p_content_digests[position],p_original_lengths[position],
				p_included_lengths[position],p_included_digests[position],
				p_dispositions[position]::decodex.context_source_disposition,
				NULLIF(p_artifact_ids[position],'')::uuid,
				NULLIF(p_artifact_revisions[position],0));
		END LOOP;
		INSERT INTO decodex.context_packs(context_pack_id,conversation_id,pack_revision,
			compiled_digest,manifest_digest,inline_bytes,blob_hash,byte_length,max_bytes,
			recent_item_limit,possible_side_effects,truncated,omitted_source_count,source_count)
		VALUES(p_context_pack_id,session_row.conversation_id,pack_revision,p_compiled_digest,
			p_manifest_digest,inline_value,blob_value,pg_catalog.octet_length(p_compiled_bytes),
			p_max_bytes,p_recent_item_limit,p_possible_side_effects::decodex.side_effect_state,
			p_truncated,p_omitted_source_count,source_count);
		INSERT INTO decodex.account_snapshots(account_snapshot_id,source_account_id,display_label,
			observed_state,source_revision)
		VALUES(p_account_snapshot_id,member_row.account_id,member_row.display_label,
			member_row.account_state,member_row.account_revision);
		INSERT INTO decodex.runtime_sessions(runtime_session_id,conversation_id,profile_snapshot_id,
			account_snapshot_id,codex_thread_id,state,last_known_turn_id)
		VALUES(p_fallback_session_id,session_row.conversation_id,profile_row.profile_snapshot_id,
			p_account_snapshot_id,NULL,'starting',NULL);
	END IF;
	core:=pg_catalog.jsonb_build_object('operation','plan_continuation','plan_id',p_plan_id,
		'operation_id',p_operation_id,'routing_decision_id',p_decision_id,
		'managed_run_id',run_row.managed_run_id,'managed_run_revision',run_row.revision,
		'conversation_id',session_row.conversation_id,
		'source_runtime_session_id',session_row.runtime_session_id,
		'source_runtime_session_revision',session_row.revision,
		'selected_account_id',decision_row.selected_account_id,'kind',plan_kind,
		'codex_thread_id',thread_value,
		'fallback_context_pack_id',CASE WHEN plan_kind='context_pack_fallback' THEN p_context_pack_id END,
		'fallback_context_pack_revision',CASE WHEN plan_kind='context_pack_fallback' THEN pack_revision END,
		'fallback_runtime_session_id',CASE WHEN plan_kind='context_pack_fallback' THEN p_fallback_session_id END,
		'routing_evidence_id',CASE WHEN plan_kind='same_thread' THEN evidence_row.evidence_id END,
		'routing_evidence_revision',CASE WHEN plan_kind='same_thread' THEN evidence_row.evidence_revision END,
		'schema_fingerprint',CASE WHEN plan_kind='same_thread' THEN evidence_row.schema_fingerprint END,
		'codex_experiment_id',CASE WHEN plan_kind='same_thread' THEN evidence_row.experiment_id END,
		'codex_experiment_revision',CASE WHEN plan_kind='same_thread' THEN evidence_row.revision END,
		'codex_observation_id',CASE WHEN plan_kind='same_thread' THEN evidence_row.observation_id END,
		'effect_barrier_state',barrier_row.state,'effect_barrier_revision',barrier_row.revision,
		'submitted_turn_receipt_count',submitted_count,'replay_permitted',false,
		'dispatch_enabled',false,'planned_at_micros',
		(extract(epoch FROM planned)*1000000)::bigint);
	payload:=core||pg_catalog.jsonb_build_object('continuation_plan_id',p_plan_id);
	INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload)
	VALUES('continuation_plan',p_plan_id::text,1,'continuation_plan_created',p_idempotency_key,payload)
	RETURNING sequence INTO activity_sequence;
	activity_rows:=activity_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
		'sequence',activity_sequence,'aggregate_kind','continuation_plan',
		'aggregate_id',p_plan_id,'revision',1,'event_kind','continuation_plan_created','payload',payload));
	INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	VALUES('activity/'||activity_sequence::text,'continuation_plan',p_plan_id::text,1,
		pg_catalog.jsonb_build_object('activity_sequence',activity_sequence,
			'event_kind','continuation_plan_created','aggregate_kind','continuation_plan',
			'aggregate_id',p_plan_id,'revision',1,'payload',payload)) RETURNING id INTO outbox_id;
	outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
		'id',outbox_id,'effect_key','activity/'||activity_sequence::text,
		'aggregate_kind','continuation_plan','aggregate_id',p_plan_id,'aggregate_revision',1));
	IF plan_kind='context_pack_fallback' THEN
		payload:=pg_catalog.jsonb_build_object('kind','context_pack','continuation_plan_id',p_plan_id,
			'routing_decision_id',p_decision_id,'fallback_context_pack_id',p_context_pack_id,
			'conversation_id',session_row.conversation_id,'pack_revision',pack_revision,
			'compiled_digest',p_compiled_digest,'dispatch_enabled',false);
		INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload)
		VALUES('context_pack',p_context_pack_id::text,pack_revision,'context_pack_persisted',
			p_idempotency_key,payload) RETURNING sequence INTO activity_sequence;
		activity_rows:=activity_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'aggregate_kind','context_pack','aggregate_id',p_context_pack_id,
			'revision',pack_revision,'event_kind','context_pack_persisted','payload',payload));
		INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
		VALUES('activity/'||activity_sequence::text,'context_pack',p_context_pack_id::text,pack_revision,
			pg_catalog.jsonb_build_object('activity_sequence',activity_sequence,
				'event_kind','context_pack_persisted','aggregate_kind','context_pack',
				'aggregate_id',p_context_pack_id,'revision',pack_revision,'payload',payload))
		RETURNING id INTO outbox_id;
		outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text,
			'aggregate_kind','context_pack','aggregate_id',p_context_pack_id,
			'aggregate_revision',pack_revision));
		payload:=pg_catalog.jsonb_build_object('kind','runtime_session','continuation_plan_id',p_plan_id,
			'routing_decision_id',p_decision_id,'fallback_runtime_session_id',p_fallback_session_id,
			'conversation_id',session_row.conversation_id,'selected_account_id',decision_row.selected_account_id,
			'state','starting','revision',1,'dispatch_enabled',false);
		INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload)
		VALUES('runtime_session',p_fallback_session_id::text,1,'runtime_session_created',
			p_idempotency_key,payload) RETURNING sequence INTO activity_sequence;
		activity_rows:=activity_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'aggregate_kind','runtime_session',
			'aggregate_id',p_fallback_session_id,'revision',1,
			'event_kind','runtime_session_created','payload',payload));
		INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
		VALUES('activity/'||activity_sequence::text,'runtime_session',p_fallback_session_id::text,1,
			pg_catalog.jsonb_build_object('activity_sequence',activity_sequence,
				'event_kind','runtime_session_created','aggregate_kind','runtime_session',
				'aggregate_id',p_fallback_session_id,'revision',1,'payload',payload))
		RETURNING id INTO outbox_id;
		outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text,
			'aggregate_kind','runtime_session','aggregate_id',p_fallback_session_id,
			'aggregate_revision',1));
	END IF;
	effect:=core||pg_catalog.jsonb_build_object('activity_effects',activity_rows,
		'outbox_effects',outbox_rows,'effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.continuation_plans(plan_id,operation_id,routing_decision_id,
		managed_run_id,managed_run_revision,conversation_id,source_runtime_session_id,
		source_runtime_session_revision,selected_account_id,kind,codex_thread_id,
		fallback_context_pack_id,fallback_runtime_session_id,routing_evidence_id,
		routing_evidence_revision,schema_fingerprint,codex_experiment_id,
		codex_experiment_revision,codex_observation_id,effect_barrier_state,
		effect_barrier_revision,submitted_turn_receipt_count,replay_permitted,dispatch_enabled,
		revision,request_envelope,effect_envelope,response_bytes,planned_at)
	VALUES(p_plan_id,p_operation_id,p_decision_id,run_row.managed_run_id,run_row.revision,
		session_row.conversation_id,session_row.runtime_session_id,session_row.revision,
		decision_row.selected_account_id,plan_kind,thread_value,
		CASE WHEN plan_kind='context_pack_fallback' THEN p_context_pack_id END,
		CASE WHEN plan_kind='context_pack_fallback' THEN p_fallback_session_id END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.evidence_id END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.evidence_revision END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.schema_fingerprint END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.experiment_id END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.revision END,
		CASE WHEN plan_kind='same_thread' THEN evidence_row.observation_id END,
		barrier_row.state,barrier_row.revision,submitted_count,false,false,1,request,effect,response,planned);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON TABLE decodex.continuation_plans FROM PUBLIC;
REVOKE ALL ON TYPE decodex.continuation_plan_kind FROM PUBLIC;
REVOKE ALL ON FUNCTION decodex.forbid_continuation_plan_mutation(),
	decodex.enforce_continuation_plan_completeness(),
	decodex.enforce_continuation_event_namespace(),
	decodex.is_canonical_continuation_pack(uuid,bytea,text,text,integer,integer,text,boolean,integer,
		text[],text[],bigint[],text[],bigint[],bigint[],text[],text[],text[],bigint[]),
	decodex.complete_exact_continuation_rejection(text,text,text),
	decodex.reserve_exact_continuation_command(text,text,jsonb),
	decodex.read_continuation_plan_exact(uuid,bigint),
	decodex.plan_continuation_exact(text,text,uuid,uuid,bigint,uuid,uuid,uuid,uuid,bytea,text,text,
		integer,integer,text,boolean,integer,text[],text[],bigint[],text[],bigint[],bigint[],text[],
		text[],text[],bigint[]) FROM PUBLIC;

-- Bind the V17 entrypoints only to the exact unambiguous migration-owned V12 anchor grantee.
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
		RAISE EXCEPTION 'V17 runtime principal anchor is missing or not migration-owned'
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
		RAISE EXCEPTION 'V17 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count=1 THEN
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_continuation_plan_exact(uuid,bigint) TO %I',
			runtime_role);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.plan_continuation_exact(text,text,uuid,uuid,bigint,uuid,uuid,uuid,uuid,bytea,text,text,integer,integer,text,boolean,integer,text[],text[],bigint[],text[],bigint[],bigint[],text[],text[],text[],bigint[]) TO %I',
			runtime_role);
	END IF;
END
$$;
