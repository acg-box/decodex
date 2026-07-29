-- Bounded per-account provider profile snapshots for the vNext macOS account panel.
-- Credentials remain in HostCredentialStore. PostgreSQL stores only non-secret provider facts.

CREATE TABLE decodex.account_profile_snapshots (
	account_id uuid PRIMARY KEY REFERENCES decodex.accounts(account_id),
	account_revision bigint NOT NULL CHECK(account_revision>0),
	provider_kind decodex.account_provider_kind NOT NULL,
	provider_account_id text NOT NULL
		CHECK(pg_catalog.octet_length(provider_account_id)>=1
			AND pg_catalog.octet_length(provider_account_id)<=512),
	observed_at_micros bigint NOT NULL
		CHECK(observed_at_micros>=1 AND observed_at_micros<=253402300799999999),
	display_name text
		CHECK(display_name IS NULL OR (pg_catalog.octet_length(display_name)>=1
			AND pg_catalog.octet_length(display_name)<=256
			AND display_name !~ '[[:cntrl:]]')),
	username text
		CHECK(username IS NULL OR (pg_catalog.octet_length(username)>=1
			AND pg_catalog.octet_length(username)<=256
			AND username !~ '[[:cntrl:]]')),
	lifetime_tokens bigint CHECK(lifetime_tokens IS NULL OR lifetime_tokens>=0),
	peak_daily_tokens bigint CHECK(peak_daily_tokens IS NULL OR peak_daily_tokens>=0),
	longest_task_seconds bigint CHECK(longest_task_seconds IS NULL OR longest_task_seconds>=0),
	current_streak_days integer CHECK(current_streak_days IS NULL OR current_streak_days>=0),
	longest_streak_days integer CHECK(longest_streak_days IS NULL OR longest_streak_days>=0),
	CHECK(display_name IS NOT NULL OR username IS NOT NULL OR lifetime_tokens IS NOT NULL
		OR peak_daily_tokens IS NOT NULL OR longest_task_seconds IS NOT NULL
		OR current_streak_days IS NOT NULL OR longest_streak_days IS NOT NULL)
);

CREATE TABLE decodex.account_profile_daily_usage (
	account_id uuid NOT NULL REFERENCES decodex.account_profile_snapshots(account_id)
		ON DELETE CASCADE,
	start_date date NOT NULL,
	tokens bigint NOT NULL CHECK(tokens>=0),
	observed_at_micros bigint NOT NULL
		CHECK(observed_at_micros>=1 AND observed_at_micros<=253402300799999999),
	PRIMARY KEY(account_id,start_date)
);

CREATE FUNCTION decodex.observe_account_profile_exact(
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
			SELECT 1 FROM pg_catalog.unnest(p_daily_start_dates,p_daily_tokens)
				AS daily(start_date,tokens)
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
	FROM pg_catalog.unnest(p_daily_start_dates,p_daily_tokens) AS daily(start_date,tokens);
	RETURN 'observed';
END
$$;

CREATE FUNCTION decodex.read_account_profile_exact(
	p_account_id uuid
) RETURNS TABLE(
	account_id uuid,account_revision bigint,provider_kind decodex.account_provider_kind,
	provider_account_id text,observed_at_micros bigint,display_name text,username text,
	lifetime_tokens bigint,peak_daily_tokens bigint,longest_task_seconds bigint,
	current_streak_days integer,longest_streak_days integer,
	daily_start_dates text[],daily_tokens bigint[]
) LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
	SELECT snapshot.account_id,snapshot.account_revision,snapshot.provider_kind,
		snapshot.provider_account_id,snapshot.observed_at_micros,snapshot.display_name,
		snapshot.username,snapshot.lifetime_tokens,snapshot.peak_daily_tokens,
		snapshot.longest_task_seconds,snapshot.current_streak_days,snapshot.longest_streak_days,
		COALESCE(
			pg_catalog.array_agg(
				pg_catalog.to_char(daily.start_date,'YYYY-MM-DD') ORDER BY daily.start_date
			)
				FILTER (WHERE daily.start_date IS NOT NULL),
			ARRAY[]::text[]
		),
		COALESCE(
			pg_catalog.array_agg(daily.tokens ORDER BY daily.start_date)
				FILTER (WHERE daily.start_date IS NOT NULL),
			ARRAY[]::bigint[]
		)
	FROM decodex.account_profile_snapshots AS snapshot
	JOIN decodex.accounts AS account ON account.account_id=snapshot.account_id
		AND account.tombstoned_at IS NULL
		AND account.revision=snapshot.account_revision
		AND account.provider_kind=snapshot.provider_kind
		AND account.provider_account_id=snapshot.provider_account_id
	LEFT JOIN decodex.account_profile_daily_usage AS daily
		ON daily.account_id=snapshot.account_id
	WHERE snapshot.account_id=p_account_id
	GROUP BY snapshot.account_id;
$$;

REVOKE ALL ON TABLE decodex.account_profile_snapshots,
	decodex.account_profile_daily_usage FROM PUBLIC;
REVOKE ALL ON FUNCTION
	decodex.observe_account_profile_exact(uuid,bigint,decodex.account_provider_kind,text,bigint,text,text,bigint,bigint,bigint,integer,integer,text[],bigint[]),
	decodex.read_account_profile_exact(uuid)
	FROM PUBLIC;
