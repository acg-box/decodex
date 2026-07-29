-- Repair the PostgreSQL 18 array zip used by account profile observation.

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
			SELECT 1 FROM pg_catalog.unnest(p_daily_start_dates) AS date_value
			GROUP BY date_value HAVING pg_catalog.count(*)>1
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
