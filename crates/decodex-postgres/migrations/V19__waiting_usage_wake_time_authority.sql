-- V19 acceptance-enabling forward repair for deterministic waiting-usage wake timing.
-- Public runtime commands retain their exact V18 signatures and always select PostgreSQL's
-- post-lock clock through typed NULL. Only these migration-owner SECURITY INVOKER internals
-- accept an explicit finite, monotonic authority instant for deferred deterministic acceptance.

CREATE FUNCTION decodex.register_waiting_usage_wake_exact_internal(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_decision_id uuid, p_expected_managed_run_revision bigint,
	p_authority_now timestamptz
) RETURNS bytea LANGUAGE plpgsql SECURITY INVOKER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; decision_row record; run_row record; existing_head record;
DECLARE run_uuid uuid; wake_uuid uuid; transition_uuid uuid; now_value timestamptz;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb;
BEGIN
	IF p_authority_now IS NOT NULL THEN
		IF NOT pg_catalog.isfinite(p_authority_now)
			OR extract(epoch FROM p_authority_now)*1000000 < 0
			OR extract(epoch FROM p_authority_now)*1000000
				> 253402300739999999
			OR extract(epoch FROM p_authority_now)*1000000
				<> pg_catalog.trunc(extract(epoch FROM p_authority_now)*1000000)
		THEN
			RAISE EXCEPTION 'explicit waiting-usage wake authority time is outside the canonical range'
				USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time';
		END IF;
	END IF;
	request:=pg_catalog.jsonb_build_object('operation','register_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'decision_id',p_decision_id,
		'expected_managed_run_revision',p_expected_managed_run_revision);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_decision_id IS NULL
		OR p_expected_managed_run_revision IS NULL OR p_expected_managed_run_revision<=0 THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'register_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'register_waiting_usage_wake','missing_decision'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1362,pg_catalog.hashtext(p_decision_id::text));
	SELECT * INTO decision_row FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id FOR UPDATE;
	IF decision_row.kind<>'waiting_usage' THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','decision_not_waiting_usage');
	END IF;
	SELECT * INTO existing_head FROM decodex.waiting_usage_wake_heads
	WHERE routing_decision_id=p_decision_id FOR UPDATE;
	IF FOUND THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','decision_already_registered');
	END IF;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=decision_row.managed_run_id FOR UPDATE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>p_expected_managed_run_revision
		OR decision_row.managed_run_revision<>p_expected_managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','stale_managed_run');
	END IF;
	IF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=decision_row.routing_policy_id
			AND current_revision=decision_row.routing_policy_revision FOR SHARE) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','stale_policy');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
		WHERE other.managed_run_id=decision_row.managed_run_id
			AND other.managed_run_revision>=decision_row.managed_run_revision
			AND other.decision_id<>decision_row.decision_id) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake',
			'ambiguous_decision_lineage');
	END IF;
	now_value:=CASE WHEN p_authority_now IS NULL THEN pg_catalog.clock_timestamp()
		ELSE p_authority_now END;
	IF p_authority_now IS NOT NULL AND (
		now_value < decision_row.decided_at OR now_value < run_row.updated_at) THEN
		RAISE EXCEPTION 'explicit wake registration time precedes locked decision or run authority'
			USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time_monotonic';
	END IF;
	wake_uuid:=pg_catalog.gen_random_uuid();
	transition_uuid:=pg_catalog.gen_random_uuid();
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',wake_uuid,
		'waiting_usage_wake_transition_id',transition_uuid,'state','pending',
		'routing_decision_id',p_decision_id,'managed_run_id',decision_row.managed_run_id,
		'managed_run_revision',decision_row.managed_run_revision,'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','register_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind','registered','wake_id',wake_uuid,'revision',1,
		'predecessor_revision',NULL,'predecessor_transition_id',NULL,
		'registration_operation_id',p_operation_id,'routing_decision_id',p_decision_id,
		'routing_decision_revision',1,'routing_policy_id',decision_row.routing_policy_id,
		'routing_policy_revision',decision_row.routing_policy_revision,
		'managed_run_id',decision_row.managed_run_id,
		'managed_run_revision',decision_row.managed_run_revision,
		'earliest_ready_at_micros',decision_row.waiting_ready_at_micros,'state','pending',
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason',NULL,'routing_resolution_request_id',NULL,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind','waiting_usage_wake_registered')),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,operation_id,transition_kind,request_envelope,
		request_digest,registration_operation_id,routing_decision_id,routing_policy_id,
		routing_policy_revision,managed_run_id,managed_run_revision,earliest_ready_at_micros,
		state,registered_at,transitioned_at,activity_sequence,outbox_id,effect_envelope,
		effect_digest,response_bytes
	) VALUES(transition_uuid,wake_uuid,1,p_operation_id,'registered',request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),p_operation_id,
		p_decision_id,decision_row.routing_policy_id,decision_row.routing_policy_revision,
		decision_row.managed_run_id,decision_row.managed_run_revision,
		decision_row.waiting_ready_at_micros,'pending',now_value,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	INSERT INTO decodex.waiting_usage_wake_heads(
		wake_id,revision,transition_id,registration_operation_id,routing_decision_id,
		routing_decision_revision,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,registered_at,updated_at
	) VALUES(wake_uuid,1,transition_uuid,p_operation_id,p_decision_id,1,
		decision_row.routing_policy_id,decision_row.routing_policy_revision,
		decision_row.managed_run_id,decision_row.managed_run_revision,
		decision_row.waiting_ready_at_micros,'pending',now_value,now_value);
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',wake_uuid::text,1,
		'waiting_usage_wake_registered',p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',wake_uuid::text,1,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.claim_due_waiting_usage_wake_exact_internal(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_claim_id uuid, p_holder_id uuid,
	p_authority_now timestamptz
) RETURNS bytea LANGUAGE plpgsql SECURITY INVOKER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; decision_row record; run_row record;
DECLARE now_value timestamptz; now_micros bigint; reason text; kind text; state_value text;
DECLARE transition_uuid uuid; fence_uuid uuid; revision_value bigint;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb; event_kind text; claimed_value boolean;
BEGIN
	IF p_authority_now IS NOT NULL THEN
		IF NOT pg_catalog.isfinite(p_authority_now)
			OR extract(epoch FROM p_authority_now)*1000000 < 0
			OR extract(epoch FROM p_authority_now)*1000000
				> 253402300739999999
			OR extract(epoch FROM p_authority_now)*1000000
				<> pg_catalog.trunc(extract(epoch FROM p_authority_now)*1000000)
		THEN
			RAISE EXCEPTION 'explicit waiting-usage wake authority time is outside the canonical range'
				USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time';
		END IF;
	END IF;
	request:=pg_catalog.jsonb_build_object('operation','claim_due_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,
		'claim_id',p_claim_id,'holder_id',p_holder_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_claim_id IS NULL OR p_holder_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1362,0);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF EXISTS (SELECT 1 FROM decodex.waiting_usage_wake_transitions
		WHERE claim_id=p_claim_id) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','claim_identity_conflict');
	END IF;
	now_value:=CASE WHEN p_authority_now IS NULL THEN pg_catalog.clock_timestamp()
		ELSE p_authority_now END;
	now_micros:=(extract(epoch FROM now_value)*1000000)::bigint;
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads
	WHERE earliest_ready_at_micros<=now_micros
		AND (state='pending' OR (state='leased' AND lease_expires_at<=now_value))
	ORDER BY earliest_ready_at_micros,registered_at,wake_id FOR UPDATE LIMIT 1;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','no_due_wake'); END IF;
	IF p_authority_now IS NOT NULL AND now_value<head.updated_at THEN
		RAISE EXCEPTION 'explicit wake claim time precedes the locked wake tip'
			USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time_monotonic';
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(head.managed_run_id::text));
	SELECT * INTO decision_row FROM decodex.routing_decisions
	WHERE decision_id=head.routing_decision_id FOR SHARE;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=head.managed_run_id FOR SHARE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>head.managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN reason:='managed_run_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=head.routing_policy_id
			AND current_revision=head.routing_policy_revision FOR SHARE) THEN
		reason:='policy_revision_stale';
	ELSIF decision_row.decision_id IS NULL OR decision_row.kind<>'waiting_usage'
		OR decision_row.managed_run_revision<>head.managed_run_revision
		OR EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
			WHERE other.managed_run_id=head.managed_run_id
				AND other.managed_run_revision>=head.managed_run_revision
				AND other.decision_id<>head.routing_decision_id) THEN
		reason:='ambiguous_decision_lineage';
	END IF;
	transition_uuid:=pg_catalog.gen_random_uuid();
	revision_value:=head.revision+1;
	IF reason IS NULL THEN
		kind:=CASE WHEN head.state='pending' THEN 'claimed' ELSE 'reclaimed' END;
		state_value:='leased'; fence_uuid:=pg_catalog.gen_random_uuid();
		event_kind:=CASE WHEN kind='claimed' THEN 'waiting_usage_wake_claimed'
			ELSE 'waiting_usage_wake_reclaimed' END; claimed_value:=true;
	ELSE
		kind:='superseded'; state_value:='superseded'; event_kind:='waiting_usage_wake_superseded';
		claimed_value:=false;
	END IF;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state',state_value,
		'terminal_reason',reason,'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','claim_due_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,'transition_kind',kind,
		'wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state',state_value,
		'claim_id',CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		'lease_holder',CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,
		'lease_fence_id',fence_uuid,
		'lease_acquired_at_micros',CASE WHEN reason IS NULL THEN now_micros ELSE NULL END,
		'lease_expires_at_micros',CASE WHEN reason IS NULL THEN now_micros+60000000 ELSE NULL END,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',now_micros,'terminal_reason',reason,
		'routing_resolution_request_id',NULL,'fresh_routing_resolution_only',true,
		'prior_decision_reusable',false,'production_enabled',false,'claimed',claimed_value,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind',event_kind)),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,claim_id,lease_holder,lease_fence_id,
		lease_acquired_at,lease_expires_at,terminal_reason,registered_at,transitioned_at,
		activity_sequence,outbox_id,effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,kind::decodex.waiting_usage_wake_transition_kind,request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,state_value::decodex.waiting_usage_wake_state,
		CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,fence_uuid,
		CASE WHEN reason IS NULL THEN now_value ELSE NULL END,
		CASE WHEN reason IS NULL THEN now_value+INTERVAL '60 seconds' ELSE NULL END,
		reason::decodex.waiting_usage_wake_terminal_reason,head.registered_at,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state=state_value::decodex.waiting_usage_wake_state,
		claim_id=CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		lease_holder=CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,
		lease_fence_id=fence_uuid,
		lease_acquired_at=CASE WHEN reason IS NULL THEN now_value ELSE NULL END,
		lease_expires_at=CASE WHEN reason IS NULL THEN now_value+INTERVAL '60 seconds' ELSE NULL END,
		terminal_reason=reason::decodex.waiting_usage_wake_terminal_reason,updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during claim'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		event_kind,p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.fire_waiting_usage_wake_exact_internal(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid,
	p_holder_id uuid, p_lease_fence_id uuid,
	p_authority_now timestamptz
) RETURNS bytea LANGUAGE plpgsql SECURITY INVOKER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; run_row record; now_value timestamptz;
DECLARE transition_uuid uuid; request_uuid uuid; revision_value bigint; reason text;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb; state_value text; event_kind text; run_uuid uuid;
BEGIN
	IF p_authority_now IS NOT NULL THEN
		IF NOT pg_catalog.isfinite(p_authority_now)
			OR extract(epoch FROM p_authority_now)*1000000 < 0
			OR extract(epoch FROM p_authority_now)*1000000
				> 253402300739999999
			OR extract(epoch FROM p_authority_now)*1000000
				<> pg_catalog.trunc(extract(epoch FROM p_authority_now)*1000000)
		THEN
			RAISE EXCEPTION 'explicit waiting-usage wake authority time is outside the canonical range'
				USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time';
		END IF;
	END IF;
	request:=pg_catalog.jsonb_build_object('operation','fire_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'wake_id',p_wake_id,
		'expected_revision',p_expected_revision,'expected_transition_id',p_expected_transition_id,
		'holder_id',p_holder_id,'lease_fence_id',p_lease_fence_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_wake_id IS NULL OR p_expected_revision IS NULL
		OR p_expected_revision<=0 OR p_expected_transition_id IS NULL
		OR p_holder_id IS NULL OR p_lease_fence_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'fire_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.waiting_usage_wake_heads
	WHERE wake_id=p_wake_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'fire_waiting_usage_wake','wake_not_found'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id FOR UPDATE;
	IF head.state IN ('fired','cancelled','superseded') THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','wake_terminal');
	END IF;
	IF head.revision<>p_expected_revision OR head.transition_id<>p_expected_transition_id THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','stale_wake_tip');
	END IF;
	now_value:=CASE WHEN p_authority_now IS NULL THEN pg_catalog.clock_timestamp()
		ELSE p_authority_now END;
	IF p_authority_now IS NOT NULL AND now_value<head.updated_at THEN
		RAISE EXCEPTION 'explicit wake fire time precedes the locked wake tip'
			USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time_monotonic';
	END IF;
	IF head.state<>'leased' OR head.lease_holder<>p_holder_id
		OR head.lease_fence_id<>p_lease_fence_id OR head.lease_expires_at<=now_value THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','lease_lost');
	END IF;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=head.managed_run_id FOR SHARE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>head.managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN reason:='managed_run_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=head.routing_policy_id
			AND current_revision=head.routing_policy_revision FOR SHARE) THEN
		reason:='policy_revision_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_decisions
		WHERE decision_id=head.routing_decision_id AND kind='waiting_usage'
			AND managed_run_id=head.managed_run_id
			AND managed_run_revision=head.managed_run_revision)
		OR EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
			WHERE other.managed_run_id=head.managed_run_id
				AND other.managed_run_revision>=head.managed_run_revision
				AND other.decision_id<>head.routing_decision_id) THEN
		reason:='ambiguous_decision_lineage';
	END IF;
	transition_uuid:=pg_catalog.gen_random_uuid(); revision_value:=head.revision+1;
	IF reason IS NULL THEN
		request_uuid:=pg_catalog.gen_random_uuid(); state_value:='fired';
		event_kind:='waiting_usage_wake_fired';
	ELSE state_value:='superseded'; event_kind:='waiting_usage_wake_superseded'; END IF;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state',state_value,
		'terminal_reason',reason,'routing_resolution_request_id',request_uuid,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','fire_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind',CASE WHEN reason IS NULL THEN 'fired' ELSE 'superseded' END,
		'wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state',state_value,
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason',reason,'routing_resolution_request_id',request_uuid,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind',event_kind)),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,terminal_reason,
		routing_resolution_request_id,registered_at,transitioned_at,activity_sequence,outbox_id,
		effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,CASE WHEN reason IS NULL THEN 'fired' ELSE 'superseded' END::
			decodex.waiting_usage_wake_transition_kind,request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,state_value::decodex.waiting_usage_wake_state,
		reason::decodex.waiting_usage_wake_terminal_reason,request_uuid,head.registered_at,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state=state_value::decodex.waiting_usage_wake_state,
		claim_id=NULL,lease_holder=NULL,lease_fence_id=NULL,lease_acquired_at=NULL,
		lease_expires_at=NULL,terminal_reason=reason::decodex.waiting_usage_wake_terminal_reason,
		routing_resolution_request_id=request_uuid,updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during fire'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		event_kind,p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.cancel_waiting_usage_wake_exact_internal(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid,
	p_authority_now timestamptz
) RETURNS bytea LANGUAGE plpgsql SECURITY INVOKER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; now_value timestamptz; run_uuid uuid;
DECLARE transition_uuid uuid; revision_value bigint; activity_sequence bigint; outbox_id bigint;
DECLARE core jsonb; effect jsonb; response bytea; payload jsonb;
BEGIN
	IF p_authority_now IS NOT NULL THEN
		IF NOT pg_catalog.isfinite(p_authority_now)
			OR extract(epoch FROM p_authority_now)*1000000 < 0
			OR extract(epoch FROM p_authority_now)*1000000
				> 253402300739999999
			OR extract(epoch FROM p_authority_now)*1000000
				<> pg_catalog.trunc(extract(epoch FROM p_authority_now)*1000000)
		THEN
			RAISE EXCEPTION 'explicit waiting-usage wake authority time is outside the canonical range'
				USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time';
		END IF;
	END IF;
	request:=pg_catalog.jsonb_build_object('operation','cancel_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'wake_id',p_wake_id,
		'expected_revision',p_expected_revision,'expected_transition_id',p_expected_transition_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_wake_id IS NULL OR p_expected_revision IS NULL
		OR p_expected_revision<=0 OR p_expected_transition_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'cancel_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','wake_not_found'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id FOR UPDATE;
	IF head.state IN ('fired','cancelled','superseded') THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','wake_terminal');
	END IF;
	IF head.revision<>p_expected_revision OR head.transition_id<>p_expected_transition_id THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','stale_wake_tip');
	END IF;
	now_value:=CASE WHEN p_authority_now IS NULL THEN pg_catalog.clock_timestamp()
		ELSE p_authority_now END;
	IF p_authority_now IS NOT NULL AND now_value<head.updated_at THEN
		RAISE EXCEPTION 'explicit wake cancellation time precedes the locked wake tip'
			USING ERRCODE='22023', CONSTRAINT='waiting_usage_wake_authority_time_monotonic';
	END IF;
	transition_uuid:=pg_catalog.gen_random_uuid();
	revision_value:=head.revision+1;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state','cancelled',
		'terminal_reason','explicit_cancellation','production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','cancel_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind','cancelled','wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state','cancelled',
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason','explicit_cancellation','routing_resolution_request_id',NULL,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind','waiting_usage_wake_cancelled')),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,terminal_reason,registered_at,
		transitioned_at,activity_sequence,outbox_id,effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,'cancelled',request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,'cancelled','explicit_cancellation',head.registered_at,
		now_value,activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state='cancelled',claim_id=NULL,lease_holder=NULL,
		lease_fence_id=NULL,lease_acquired_at=NULL,lease_expires_at=NULL,
		terminal_reason='explicit_cancellation',updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during cancellation'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		'waiting_usage_wake_cancelled',p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE OR REPLACE FUNCTION decodex.register_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_decision_id uuid, p_expected_managed_run_revision bigint
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RETURN decodex.register_waiting_usage_wake_exact_internal(
		p_protocol,p_idempotency_key,p_operation_id,p_decision_id,
		p_expected_managed_run_revision,NULL::pg_catalog.timestamptz);
END
$$;

CREATE OR REPLACE FUNCTION decodex.claim_due_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_claim_id uuid, p_holder_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RETURN decodex.claim_due_waiting_usage_wake_exact_internal(
		p_protocol,p_idempotency_key,p_operation_id,p_claim_id,p_holder_id,
		NULL::pg_catalog.timestamptz);
END
$$;

CREATE OR REPLACE FUNCTION decodex.fire_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid,
	p_holder_id uuid, p_lease_fence_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RETURN decodex.fire_waiting_usage_wake_exact_internal(
		p_protocol,p_idempotency_key,p_operation_id,p_wake_id,p_expected_revision,
		p_expected_transition_id,p_holder_id,p_lease_fence_id,
		NULL::pg_catalog.timestamptz);
END
$$;

CREATE OR REPLACE FUNCTION decodex.cancel_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER VOLATILE PARALLEL UNSAFE
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RETURN decodex.cancel_waiting_usage_wake_exact_internal(
		p_protocol,p_idempotency_key,p_operation_id,p_wake_id,p_expected_revision,
		p_expected_transition_id,NULL::pg_catalog.timestamptz);
END
$$;

REVOKE ALL ON FUNCTION
	decodex.register_waiting_usage_wake_exact_internal(
		text,text,uuid,uuid,bigint,timestamptz),
	decodex.claim_due_waiting_usage_wake_exact_internal(
		text,text,uuid,uuid,uuid,timestamptz),
	decodex.fire_waiting_usage_wake_exact_internal(
		text,text,uuid,uuid,bigint,uuid,uuid,uuid,timestamptz),
	decodex.cancel_waiting_usage_wake_exact_internal(
		text,text,uuid,uuid,bigint,uuid,timestamptz)
FROM PUBLIC;

-- Seal the V19 internals from the exact unambiguous migration-owned V12 anchor grantee.
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
		RAISE EXCEPTION 'V19 runtime principal anchor is missing or not migration-owned'
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
		RAISE EXCEPTION 'V19 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count=1 THEN
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON FUNCTION decodex.register_waiting_usage_wake_exact_internal(text,text,uuid,uuid,bigint,timestamptz), decodex.claim_due_waiting_usage_wake_exact_internal(text,text,uuid,uuid,uuid,timestamptz), decodex.fire_waiting_usage_wake_exact_internal(text,text,uuid,uuid,bigint,uuid,uuid,uuid,timestamptz), decodex.cancel_waiting_usage_wake_exact_internal(text,text,uuid,uuid,bigint,uuid,timestamptz) FROM %I',
			runtime_role);
	END IF;
END
$$;
