-- Rebind account launch to the signed official Codex release image.

CREATE OR REPLACE FUNCTION decodex.read_account_registry_exact(
	p_account_id uuid,
	p_limit bigint
) RETURNS TABLE(
	account_id uuid,display_label text,enabled boolean,state decodex.account_state,revision bigint,
		provider_kind decodex.account_provider_kind,provider_account_id text,
		credential_store_schema_version integer,credential_version bigint,
		credential_fingerprint text,credential_writer_operation_id uuid,tombstoned boolean,
		lifecycle_readiness text,unsettled_operation_id uuid,
		unsettled_kind decodex.account_operation_kind,
		unsettled_phase decodex.account_operation_phase,unsettled_recovery_code text,
		five_hour_disposition text,five_hour_used_percent integer,
		five_hour_resets_at_micros bigint,five_hour_observed_at_micros bigint,
		five_hour_error_code decodex.account_quota_observation_error,
		seven_day_disposition text,seven_day_used_percent integer,
		seven_day_resets_at_micros bigint,seven_day_observed_at_micros bigint,
		seven_day_error_code decodex.account_quota_observation_error
) LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog,decodex AS $$
	BEGIN
	IF p_limit IS NULL OR p_limit < 1 OR p_limit > 512 THEN
		RAISE EXCEPTION 'account registry read limit is invalid' USING ERRCODE='22023';
	END IF;
	IF p_account_id IS NULL AND
		(SELECT pg_catalog.count(*) FROM decodex.accounts WHERE tombstoned_at IS NULL) > 512
	THEN
		RAISE EXCEPTION 'account registry cardinality exceeds the supported bound'
			USING ERRCODE='54000';
	END IF;
	RETURN QUERY
	SELECT account.account_id,account.display_label,account.enabled,account.state,account.revision,
		account.provider_kind,account.provider_account_id,
		account.credential_store_schema_version,account.credential_version,
		account.credential_fingerprint,account.credential_writer_operation_id,
		account.tombstoned_at IS NOT NULL,
		CASE
			WHEN account.tombstoned_at IS NOT NULL THEN 'tombstoned'
			WHEN operation.operation_id IS NOT NULL THEN 'operation_unsettled'
			WHEN account.credential_version IS NULL THEN 'credential_absent'
			WHEN NOT EXISTS (
				SELECT 1 FROM decodex.codex_account_capability AS capability
				WHERE capability.singleton
					AND capability.build_identity='codex-cli 0.146.0-alpha.3.1'
					AND capability.executable_sha256='fa0cb7c5f80e6a192563fcb1d9f98857f4a808a28cb29289400ed7110291bce4'
					AND capability.login_chatgpt_auth_tokens AND capability.refresh_callback
			) THEN 'callback_capability_unready'
			WHEN account.credential_store_observation='exact' THEN 'ready'
			WHEN account.credential_store_observation='unavailable' THEN 'store_unavailable'
			WHEN account.credential_store_observation='provider_mismatch' THEN 'provider_mismatch'
			ELSE 'store_mismatch'
		END,
		operation.operation_id,operation.kind,operation.phase,operation.recovery_code,
		CASE WHEN five_hour.account_id IS NULL THEN 'unknown'
			WHEN five_hour.error_code IS NOT NULL THEN 'error'
			WHEN five_hour.resets_at_micros<=
				(extract(epoch FROM pg_catalog.clock_timestamp())*1000000)::bigint
				OR five_hour.observed_at_micros+300000000<
				(extract(epoch FROM pg_catalog.clock_timestamp())*1000000)::bigint
			THEN 'stale' ELSE 'current' END,
		five_hour.used_percent,
		five_hour.resets_at_micros,
		five_hour.observed_at_micros,
		five_hour.error_code,
		CASE WHEN seven_day.account_id IS NULL THEN 'unknown'
			WHEN seven_day.error_code IS NOT NULL THEN 'error'
			WHEN seven_day.resets_at_micros<=
				(extract(epoch FROM pg_catalog.clock_timestamp())*1000000)::bigint
				OR seven_day.observed_at_micros+300000000<
				(extract(epoch FROM pg_catalog.clock_timestamp())*1000000)::bigint
			THEN 'stale' ELSE 'current' END,
		seven_day.used_percent,
		seven_day.resets_at_micros,
		seven_day.observed_at_micros,
		seven_day.error_code
	FROM decodex.accounts AS account
	LEFT JOIN decodex.account_operations AS operation
		ON operation.account_id=account.account_id
		AND operation.phase NOT IN ('committed','cancelled')
	LEFT JOIN decodex.account_quota_facts AS five_hour
		ON five_hour.account_id=account.account_id AND five_hour.duration_minutes=300
	LEFT JOIN decodex.account_quota_facts AS seven_day
		ON seven_day.account_id=account.account_id AND seven_day.duration_minutes=10080
	LEFT JOIN decodex.account_routing_order AS ordering ON ordering.account_id=account.account_id
	WHERE (p_account_id IS NULL AND account.tombstoned_at IS NULL)
		OR account.account_id=p_account_id
	ORDER BY ordering.position NULLS LAST,account.account_id
	LIMIT p_limit;
END
$$;

CREATE OR REPLACE FUNCTION decodex.read_reset_card_account_admission_exact(
	p_account_id uuid,p_callback_profile_sha256 text
) RETURNS TABLE(
	state decodex.account_state,revision bigint,enabled boolean,tombstoned boolean,
	credential_store_schema_version integer,credential_version bigint,
	credential_fingerprint text,credential_writer_operation_id uuid,
	provider_kind decodex.account_provider_kind,provider_account_id text,
	credential_store_observation decodex.account_store_observation,
	operation_unsettled boolean,callback_profile_ready boolean
) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	RETURN QUERY SELECT account.state,account.revision,account.enabled,
		account.tombstoned_at IS NOT NULL,account.credential_store_schema_version,
		account.credential_version,account.credential_fingerprint,
		account.credential_writer_operation_id,account.provider_kind,
		account.provider_account_id,account.credential_store_observation,
		EXISTS (
			SELECT 1 FROM decodex.account_operations AS operation
			WHERE operation.account_id=account.account_id
				AND operation.phase NOT IN ('committed','cancelled')
		),
		EXISTS (
			SELECT 1 FROM decodex.codex_account_capability AS capability
			WHERE capability.singleton
				AND capability.build_identity='codex-cli 0.146.0-alpha.3.1'
				AND capability.executable_sha256='fa0cb7c5f80e6a192563fcb1d9f98857f4a808a28cb29289400ed7110291bce4'
				AND capability.schema_sha256 ~ '^[0-9a-f]{64}$'
				AND capability.callback_profile_sha256 ~ '^[0-9a-f]{64}$'
				AND capability.login_chatgpt_auth_tokens
				AND capability.refresh_callback
				AND capability.callback_profile_sha256=p_callback_profile_sha256
		)
	FROM decodex.accounts AS account
	WHERE account.account_id=p_account_id
	FOR SHARE OF account;
END
$$;

CREATE OR REPLACE FUNCTION decodex.attest_codex_account_capability_exact(
	p_build_identity text,p_executable_sha256 text,p_schema_sha256 text,
	p_callback_profile_sha256 text,p_login_chatgpt_auth_tokens boolean,p_refresh_callback boolean
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	INSERT INTO decodex.codex_account_capability(
		singleton,build_identity,executable_sha256,schema_sha256,callback_profile_sha256,
		login_chatgpt_auth_tokens,refresh_callback
	) VALUES (true,p_build_identity,p_executable_sha256,p_schema_sha256,p_callback_profile_sha256,
		p_login_chatgpt_auth_tokens,p_refresh_callback)
	ON CONFLICT(singleton) DO UPDATE SET build_identity=EXCLUDED.build_identity,
		executable_sha256=EXCLUDED.executable_sha256,schema_sha256=EXCLUDED.schema_sha256,
		callback_profile_sha256=EXCLUDED.callback_profile_sha256,
		login_chatgpt_auth_tokens=EXCLUDED.login_chatgpt_auth_tokens,
		refresh_callback=EXCLUDED.refresh_callback,observed_at=pg_catalog.clock_timestamp();
	RETURN CASE WHEN p_build_identity='codex-cli 0.146.0-alpha.3.1'
		AND p_executable_sha256='fa0cb7c5f80e6a192563fcb1d9f98857f4a808a28cb29289400ed7110291bce4'
		AND p_schema_sha256 ~ '^[0-9a-f]{64}$'
		AND p_callback_profile_sha256 ~ '^[0-9a-f]{64}$'
		AND p_login_chatgpt_auth_tokens AND p_refresh_callback
		THEN 'ready' ELSE 'unready' END;
END
$$;

CREATE OR REPLACE FUNCTION decodex.prepare_process_generation_exact(
	p_generation_id uuid,p_account_id uuid,p_execution_epoch_id uuid,
	p_authorization_digest text,p_runner_identity text,p_intended_boot_id text,
	p_control_kind decodex.process_generation_control_kind,
	p_isolation_kind decodex.process_generation_isolation_kind,
	p_initial_account_revision bigint,p_credential_store_schema_version integer,
	p_credential_version bigint,p_credential_fingerprint text,
	p_credential_writer_operation_id uuid,
	p_provider_kind decodex.account_provider_kind,p_provider_account_id text,
	p_refresh_callback_profile_sha256 text,
	p_reset_card_outbox_id bigint,p_reset_card_worker_id uuid,p_reset_card_claim_token uuid
) RETURNS TABLE(result_code text,revision bigint,state decodex.process_generation_state,
	created_at_micros bigint,updated_at_micros bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE existing decodex.process_generations%ROWTYPE;
DECLARE account decodex.accounts%ROWTYPE;
DECLARE now_value timestamptz;
DECLARE reconciliation_authorized boolean:=false;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
	PERFORM pg_catalog.pg_advisory_xact_lock(1400,pg_catalog.hashtext(p_generation_id::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1401,pg_catalog.hashtext(p_account_id::text));
	SELECT * INTO existing FROM decodex.process_generations
	WHERE generation_id=p_generation_id FOR UPDATE;
	IF FOUND THEN
		IF (existing.account_id,existing.execution_epoch_id,existing.runner_identity,
			existing.intended_boot_id,existing.control_kind,existing.isolation_kind,
			existing.initial_account_revision,existing.credential_store_schema_version,
			existing.credential_version,existing.credential_fingerprint,
			existing.credential_writer_operation_id,
			existing.provider_kind,existing.provider_account_id,
			existing.refresh_callback_profile_sha256) IS DISTINCT FROM
			(p_account_id,p_execution_epoch_id,p_runner_identity,p_intended_boot_id,
			p_control_kind,p_isolation_kind,p_initial_account_revision,
			p_credential_store_schema_version,p_credential_version,p_credential_fingerprint,
			p_credential_writer_operation_id,
			p_provider_kind,p_provider_account_id,p_refresh_callback_profile_sha256)
		THEN RETURN QUERY SELECT 'identity_conflict',existing.revision,existing.state,
			(extract(epoch FROM existing.created_at)*1000000)::bigint,
			(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		ELSE RETURN QUERY SELECT 'replayed',existing.revision,existing.state,
			(extract(epoch FROM existing.created_at)*1000000)::bigint,
			(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;
	IF p_authorization_digest IS NULL OR p_authorization_digest !~ '^[0-9a-f]{64}$' THEN
		RETURN QUERY SELECT 'restore_authority_unavailable',0::bigint,
			'starting'::decodex.process_generation_state,0::bigint,0::bigint;
		RETURN;
	END IF;
	PERFORM 1 FROM decodex.process_generation_execution_epochs AS epoch
	WHERE epoch.execution_epoch_id=p_execution_epoch_id
		AND epoch.authorization_digest=p_authorization_digest AND epoch.retired_at IS NULL FOR SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'restore_authority_unavailable',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	SELECT * INTO account FROM decodex.accounts WHERE account_id=p_account_id FOR KEY SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'account_missing',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	IF (p_reset_card_outbox_id IS NULL,p_reset_card_worker_id IS NULL,
		p_reset_card_claim_token IS NULL) NOT IN ((true,true,true),(false,false,false))
	THEN RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	IF p_reset_card_outbox_id IS NOT NULL THEN
		SELECT EXISTS (
			SELECT 1 FROM decodex.outbox AS work
			WHERE work.id=p_reset_card_outbox_id
				AND work.aggregate_kind='reset_card_operation'
				AND work.state='in_flight'
				AND work.lease_holder=p_reset_card_worker_id
				AND work.claim_token=p_reset_card_claim_token
				AND work.lease_expires_at>pg_catalog.clock_timestamp()
				AND work.effect_state IN ('ambiguous','receipt_recorded')
				AND work.payload #>> '{payload,account_id}'=p_account_id::text
				AND work.payload #>> '{payload,provider_kind}'=p_provider_kind::text
				AND work.payload #>> '{payload,provider_account_id}'=p_provider_account_id
				AND work.payload #>> '{payload,refresh_callback_profile_sha256}'=
					p_refresh_callback_profile_sha256
		) INTO reconciliation_authorized;
		IF NOT reconciliation_authorized THEN
			RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,
				'starting'::decodex.process_generation_state,0::bigint,0::bigint;
			RETURN;
		END IF;
	END IF;
	IF account.tombstoned_at IS NOT NULL OR account.revision<>p_initial_account_revision
		OR account.credential_store_observation<>'exact'
		OR account.credential_store_schema_version<>p_credential_store_schema_version
		OR account.credential_version<>p_credential_version
		OR account.credential_fingerprint<>p_credential_fingerprint
		OR account.credential_writer_operation_id<>p_credential_writer_operation_id
		OR account.provider_kind<>p_provider_kind
		OR account.provider_account_id<>p_provider_account_id
		OR p_refresh_callback_profile_sha256 !~ '^[0-9a-f]{64}$'
		OR (NOT reconciliation_authorized AND (NOT account.enabled OR EXISTS (
			SELECT 1 FROM decodex.account_operations AS operation
			WHERE operation.account_id=p_account_id
				AND operation.phase NOT IN ('committed','cancelled'))))
	THEN RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	IF NOT reconciliation_authorized AND NOT EXISTS (
		SELECT 1 FROM decodex.codex_account_capability AS capability
		WHERE capability.singleton AND capability.login_chatgpt_auth_tokens
			AND capability.refresh_callback
			AND capability.build_identity='codex-cli 0.146.0-alpha.3.1'
			AND capability.executable_sha256='fa0cb7c5f80e6a192563fcb1d9f98857f4a808a28cb29289400ed7110291bce4'
			AND capability.callback_profile_sha256=p_refresh_callback_profile_sha256)
	THEN RETURN QUERY SELECT 'callback_capability_unready',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	IF EXISTS (SELECT 1 FROM decodex.process_generations AS generation
		WHERE generation.account_id=p_account_id AND generation.state<>'dead')
	THEN RETURN QUERY SELECT 'account_quarantined',0::bigint,
		'starting'::decodex.process_generation_state,0::bigint,0::bigint; RETURN; END IF;
	now_value:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.process_generations(
		generation_id,account_id,execution_epoch_id,runner_identity,intended_boot_id,
		control_kind,isolation_kind,state,revision,created_at,updated_at,
		initial_account_revision,credential_store_schema_version,credential_version,
		credential_fingerprint,credential_writer_operation_id,provider_kind,
		provider_account_id,refresh_callback_profile_sha256
	) VALUES (
		p_generation_id,p_account_id,p_execution_epoch_id,p_runner_identity,p_intended_boot_id,
		p_control_kind,p_isolation_kind,'starting',1,now_value,now_value,
		p_initial_account_revision,p_credential_store_schema_version,p_credential_version,
		p_credential_fingerprint,p_credential_writer_operation_id,
		p_provider_kind,p_provider_account_id,
		p_refresh_callback_profile_sha256
	);
	RETURN QUERY SELECT 'prepared',1::bigint,'starting'::decodex.process_generation_state,
		(extract(epoch FROM now_value)*1000000)::bigint,
		(extract(epoch FROM now_value)*1000000)::bigint;
END
$$;
