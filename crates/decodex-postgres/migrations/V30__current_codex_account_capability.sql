-- Bind account launch to the current exact Codex image and repair profile duplicate-date input.

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
					AND capability.executable_sha256='fb2b6b35789e59c885cf4d2aee12475809dd67b2c10df580e638122fd6b3438e'
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
				AND capability.executable_sha256='fb2b6b35789e59c885cf4d2aee12475809dd67b2c10df580e638122fd6b3438e'
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
		AND p_executable_sha256='fb2b6b35789e59c885cf4d2aee12475809dd67b2c10df580e638122fd6b3438e'
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
			AND capability.executable_sha256='fb2b6b35789e59c885cf4d2aee12475809dd67b2c10df580e638122fd6b3438e'
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

CREATE OR REPLACE FUNCTION decodex.observe_account_profile_exact(
	p_account_id uuid,p_expected_revision bigint,
	p_expected_provider decodex.account_provider_kind,p_expected_provider_account_id text,
	p_observed_at_micros bigint,p_display_name text,p_username text,
	p_lifetime_tokens bigint,p_peak_daily_tokens bigint,p_longest_task_seconds bigint,
	p_current_streak_days integer,p_longest_streak_days integer,
	p_daily_start_dates text[],p_daily_tokens bigint[]
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE account decodex.accounts%ROWTYPE;
DECLARE existing_observed_at bigint;
DECLARE daily_count integer;
DECLARE date_value text;
DECLARE date_year integer;
DECLARE date_month integer;
DECLARE date_day integer;
DECLARE maximum_day integer;
DECLARE effective_peak_daily_tokens bigint;
BEGIN
	daily_count:=COALESCE(pg_catalog.cardinality(p_daily_start_dates),0);
	IF p_expected_revision IS NULL OR p_expected_revision<1
		OR p_expected_provider IS NULL OR p_expected_provider_account_id IS NULL
		OR pg_catalog.octet_length(p_expected_provider_account_id)<1
		OR pg_catalog.octet_length(p_expected_provider_account_id)>512
		OR p_observed_at_micros IS NULL OR p_observed_at_micros<1
		OR p_observed_at_micros>253402300799999999
		OR (p_display_name IS NOT NULL AND (
			pg_catalog.octet_length(p_display_name)<1
			OR pg_catalog.octet_length(p_display_name)>256
			OR p_display_name~'[[:cntrl:]]'))
		OR (p_username IS NOT NULL AND (
			pg_catalog.octet_length(p_username)<1 OR pg_catalog.octet_length(p_username)>256
			OR p_username~'[[:cntrl:]]'))
		OR p_lifetime_tokens<0 OR p_peak_daily_tokens<0 OR p_longest_task_seconds<0
		OR p_current_streak_days<0 OR p_longest_streak_days<0
		OR daily_count<>COALESCE(pg_catalog.cardinality(p_daily_tokens),0)
		OR daily_count>36
		OR EXISTS (
			SELECT 1
			FROM ROWS FROM (
				pg_catalog.unnest(p_daily_start_dates),
				pg_catalog.unnest(p_daily_tokens)
			) AS daily(start_date,tokens)
			WHERE daily.start_date IS NULL
				OR daily.tokens IS NULL OR daily.tokens<0
		)
		OR EXISTS (
			SELECT 1
			FROM pg_catalog.unnest(p_daily_start_dates) AS duplicate_date(value)
			GROUP BY duplicate_date.value HAVING pg_catalog.count(*)>1
		)
		OR (p_display_name IS NULL AND p_username IS NULL AND p_lifetime_tokens IS NULL
			AND p_peak_daily_tokens IS NULL AND p_longest_task_seconds IS NULL
			AND p_current_streak_days IS NULL AND p_longest_streak_days IS NULL
			AND daily_count=0)
	THEN RETURN 'invalid_fact'; END IF;

	FOREACH date_value IN ARRAY COALESCE(p_daily_start_dates,ARRAY[]::text[]) LOOP
		IF date_value!~'^[0-9]{4}-[0-9]{2}-[0-9]{2}$' THEN
			RETURN 'invalid_fact';
		END IF;
		date_year:=substring(date_value FROM 1 FOR 4)::integer;
		date_month:=substring(date_value FROM 6 FOR 2)::integer;
		date_day:=substring(date_value FROM 9 FOR 2)::integer;
		maximum_day:=CASE date_month
			WHEN 1 THEN 31 WHEN 3 THEN 31 WHEN 5 THEN 31 WHEN 7 THEN 31
			WHEN 8 THEN 31 WHEN 10 THEN 31 WHEN 12 THEN 31
			WHEN 4 THEN 30 WHEN 6 THEN 30 WHEN 9 THEN 30 WHEN 11 THEN 30
			WHEN 2 THEN CASE
				WHEN date_year%4=0 AND (date_year%100<>0 OR date_year%400=0) THEN 29
				ELSE 28
			END
			ELSE 0
		END;
		IF date_year<1 OR date_day<1 OR date_day>maximum_day THEN
			RETURN 'invalid_fact';
		END IF;
	END LOOP;
	effective_peak_daily_tokens:=COALESCE(
		p_peak_daily_tokens,
		(SELECT pg_catalog.max(daily.tokens)
			FROM pg_catalog.unnest(p_daily_tokens) AS daily(tokens))
	);

	SELECT * INTO account FROM decodex.accounts
	WHERE account_id=p_account_id FOR UPDATE;
	IF NOT FOUND OR account.tombstoned_at IS NOT NULL THEN
		RETURN 'account_unavailable';
	END IF;
	IF account.revision<>p_expected_revision
		OR account.provider_kind IS DISTINCT FROM p_expected_provider
		OR account.provider_account_id IS DISTINCT FROM p_expected_provider_account_id
	THEN RETURN 'stale_account'; END IF;

	SELECT snapshot.observed_at_micros INTO existing_observed_at
	FROM decodex.account_profile_snapshots AS snapshot
	WHERE snapshot.account_id=p_account_id FOR UPDATE;
	IF FOUND AND existing_observed_at>=p_observed_at_micros THEN
		RETURN 'stale_observation';
	END IF;

	INSERT INTO decodex.account_profile_snapshots(
		account_id,account_revision,provider_kind,provider_account_id,observed_at_micros,
		display_name,username,lifetime_tokens,peak_daily_tokens,longest_task_seconds,
		current_streak_days,longest_streak_days
	) VALUES (
		p_account_id,p_expected_revision,p_expected_provider,p_expected_provider_account_id,
		p_observed_at_micros,p_display_name,p_username,p_lifetime_tokens,effective_peak_daily_tokens,
		p_longest_task_seconds,p_current_streak_days,p_longest_streak_days
	) ON CONFLICT(account_id) DO UPDATE SET
		account_revision=EXCLUDED.account_revision,provider_kind=EXCLUDED.provider_kind,
		provider_account_id=EXCLUDED.provider_account_id,
		observed_at_micros=EXCLUDED.observed_at_micros,
		display_name=EXCLUDED.display_name,username=EXCLUDED.username,
		lifetime_tokens=EXCLUDED.lifetime_tokens,peak_daily_tokens=EXCLUDED.peak_daily_tokens,
		longest_task_seconds=EXCLUDED.longest_task_seconds,
		current_streak_days=EXCLUDED.current_streak_days,
		longest_streak_days=EXCLUDED.longest_streak_days;
	DELETE FROM decodex.account_profile_daily_usage WHERE account_id=p_account_id;
	INSERT INTO decodex.account_profile_daily_usage(
		account_id,start_date,tokens,observed_at_micros
	) SELECT p_account_id,daily.start_date::date,daily.tokens,p_observed_at_micros
	FROM ROWS FROM (
		pg_catalog.unnest(p_daily_start_dates),
		pg_catalog.unnest(p_daily_tokens)
	) AS daily(start_date,tokens);
	RETURN 'observed';
END
$$;
