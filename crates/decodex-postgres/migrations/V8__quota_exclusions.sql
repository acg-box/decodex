-- XY-1274 exact-microsecond quota persistence and locked zero-state boundary.
LOCK TABLE decodex.command_receipts IN ACCESS EXCLUSIVE MODE;
LOCK TABLE decodex.quota_windows IN ACCESS EXCLUSIVE MODE;
LOCK TABLE decodex.activity IN ACCESS EXCLUSIVE MODE;
LOCK TABLE decodex.outbox IN ACCESS EXCLUSIVE MODE;

DO $$
BEGIN
	IF EXISTS (SELECT 1 FROM decodex.quota_windows)
		OR EXISTS (
			SELECT 1 FROM decodex.command_receipts
			WHERE operation = 'mutate_quota_window' OR scope_id = 'quota_windows'
		)
		OR EXISTS (
			SELECT 1 FROM decodex.activity
			WHERE aggregate_kind = 'quota_window'
				OR event_kind IN (
					'quota_window_created', 'quota_window_updated', 'quota_window_excluded'
				)
				OR (
					pg_catalog.jsonb_typeof(payload) = 'object'
					AND (
						payload->>'kind' IN ('quota_window', 'quota_exclusion')
						OR payload OPERATOR(pg_catalog.?) 'window_class'
						OR payload OPERATOR(pg_catalog.?) 'duration_seconds'
						OR payload OPERATOR(pg_catalog.?) 'duration_minutes'
					)
				)
		)
		OR EXISTS (
			SELECT 1 FROM decodex.outbox
			WHERE aggregate_kind = 'quota_window'
				OR (
					pg_catalog.jsonb_typeof(payload) = 'object'
					AND (
						payload->>'aggregate_kind' = 'quota_window'
						OR payload->>'event_kind' IN (
							'quota_window_created', 'quota_window_updated', 'quota_window_excluded'
						)
						OR payload->'payload'->>'kind' IN ('quota_window', 'quota_exclusion')
						OR payload->'payload' OPERATOR(pg_catalog.?) 'window_class'
						OR payload->'payload' OPERATOR(pg_catalog.?) 'duration_seconds'
						OR payload->'payload' OPERATOR(pg_catalog.?) 'duration_minutes'
					)
				)
		)
		OR EXISTS (
			SELECT 1
			FROM decodex.outbox AS candidate
			JOIN decodex.activity AS linked
				ON candidate.payload @> pg_catalog.jsonb_build_object(
					'activity_sequence', linked.sequence
				)
			WHERE linked.aggregate_kind = 'quota_window'
				OR linked.event_kind IN (
					'quota_window_created', 'quota_window_updated', 'quota_window_excluded'
				)
				OR linked.payload->>'kind' IN ('quota_window', 'quota_exclusion')
				OR linked.payload OPERATOR(pg_catalog.?) 'window_class'
				OR linked.payload OPERATOR(pg_catalog.?) 'duration_seconds'
				OR linked.payload OPERATOR(pg_catalog.?) 'duration_minutes'
		)
	THEN
		RAISE EXCEPTION 'V8 requires empty pre-release quota state'
			USING ERRCODE = '55000', CONSTRAINT = 'quota_v8_zero_state';
	END IF;
END
$$;

CREATE TYPE decodex.quota_window_class AS ENUM ('five_hour', 'seven_day');
CREATE TYPE decodex.observation_confidence AS ENUM ('unknown', 'low', 'high');

CREATE FUNCTION decodex.enforce_quota_observation_monotonicity()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, decodex
AS $$
BEGIN
	IF NEW.observed_at < OLD.observed_at THEN
		RAISE EXCEPTION 'quota observations cannot move backward in time'
			USING ERRCODE = '23514', CONSTRAINT = 'quota_windows_observed_at_monotonic';
	END IF;
	RETURN NEW;
END;
$$;

REVOKE ALL ON FUNCTION decodex.enforce_quota_observation_monotonicity() FROM PUBLIC;

ALTER TABLE decodex.quota_windows
	DROP CONSTRAINT quota_windows_pkey,
	DROP CONSTRAINT quota_windows_window_class_check,
	DROP CONSTRAINT quota_windows_duration_seconds_check,
	DROP CONSTRAINT quota_windows_remaining_amount_check,
	DROP CONSTRAINT quota_windows_confidence_check,
	DROP CONSTRAINT quota_windows_finite_timestamps,
	DROP CONSTRAINT quota_windows_no_credentials,
	ALTER COLUMN window_class DROP DEFAULT,
	ALTER COLUMN window_class TYPE decodex.quota_window_class
		USING 'five_hour'::decodex.quota_window_class;

ALTER TABLE decodex.quota_windows
	RENAME COLUMN duration_seconds TO duration_minutes;

ALTER TABLE decodex.quota_windows
	ALTER COLUMN duration_minutes TYPE smallint USING 300::smallint;

ALTER TABLE decodex.quota_windows
	RENAME COLUMN remaining_amount TO remaining_percent;

ALTER TABLE decodex.quota_windows
	ALTER COLUMN remaining_percent TYPE smallint USING NULL::smallint;

ALTER TABLE decodex.quota_windows
	ALTER COLUMN confidence TYPE decodex.observation_confidence
		USING 'unknown'::decodex.observation_confidence,
	ADD CONSTRAINT quota_windows_pkey
		PRIMARY KEY (account_id, window_class, duration_minutes),
	ADD CONSTRAINT quota_windows_duration_identity CHECK (
		(window_class = 'five_hour' AND duration_minutes = 300)
		OR (window_class = 'seven_day' AND duration_minutes = 10080)
	),
	ADD CONSTRAINT quota_windows_remaining_percent CHECK (
		remaining_percent IS NULL OR remaining_percent BETWEEN 0 AND 100
	),
	ADD CONSTRAINT quota_windows_timestamp_range CHECK (
		observed_at >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
		AND observed_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		AND (
			resets_at IS NULL
			OR resets_at >= observed_at
				AND resets_at <= TIMESTAMPTZ '9999-12-31 23:59:59.999999+00'
		)
	),
	ADD CONSTRAINT quota_windows_finite_timestamps CHECK (
		isfinite(observed_at)
		AND (resets_at IS NULL OR isfinite(resets_at))
		AND isfinite(updated_at)
	),
	ADD CONSTRAINT quota_windows_no_credentials CHECK (
		NOT decodex.has_credential_material(metadata)
	);

CREATE TRIGGER quota_windows_observed_at_monotonic
BEFORE UPDATE ON decodex.quota_windows
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_quota_observation_monotonicity();

CREATE TABLE decodex.quota_exclusions (
	account_id uuid NOT NULL,
	window_class decodex.quota_window_class NOT NULL,
	duration_minutes smallint NOT NULL,
	observation_revision bigint NOT NULL CHECK (observation_revision > 0),
	remaining_percent smallint NOT NULL,
	confidence decodex.observation_confidence NOT NULL,
	observation_metadata jsonb NOT NULL,
	observed_at_micros bigint NOT NULL,
	resets_at_micros bigint NOT NULL,
	excluded_at_micros bigint NOT NULL,
	maximum_age_micros bigint NOT NULL,
	mutation_sha256 text NOT NULL,
	mutation_length bigint NOT NULL,
	dispatch_enabled boolean NOT NULL DEFAULT false,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT quota_exclusions_pkey PRIMARY KEY (
		account_id, window_class, duration_minutes, observation_revision
	),
	CONSTRAINT quota_exclusions_window_fk FOREIGN KEY (
		account_id, window_class, duration_minutes
	) REFERENCES decodex.quota_windows (
		account_id, window_class, duration_minutes
	) ON DELETE CASCADE,
	CONSTRAINT quota_exclusions_duration_identity CHECK (
		(window_class = 'five_hour' AND duration_minutes = 300)
		OR (window_class = 'seven_day' AND duration_minutes = 10080)
	),
	CONSTRAINT quota_exclusions_depleted_high_confidence CHECK (
		remaining_percent = 0 AND confidence = 'high'
	),
	CONSTRAINT quota_exclusions_timestamp_range CHECK (
		observed_at_micros >= 0
		AND observed_at_micros <= 253402300799999999
		AND excluded_at_micros >= observed_at_micros
		AND excluded_at_micros <= 253402300799999999
		AND resets_at_micros >= excluded_at_micros + 1
		AND resets_at_micros <= 253402300799999999
	),
	CONSTRAINT quota_exclusions_freshness CHECK (
		maximum_age_micros = 300000000
		AND excluded_at_micros - observed_at_micros <= maximum_age_micros
	),
	CONSTRAINT quota_exclusions_mutation_identity CHECK (
		mutation_sha256 COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
		AND mutation_length BETWEEN 1 AND 67108864
	),
	CONSTRAINT quota_exclusions_inert CHECK (NOT dispatch_enabled),
	CONSTRAINT quota_exclusions_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT quota_exclusions_no_credentials CHECK (
		NOT decodex.has_credential_material(observation_metadata)
	)
);

CREATE INDEX quota_exclusions_reset_idx
	ON decodex.quota_exclusions (account_id, resets_at_micros);

REVOKE ALL ON TABLE decodex.quota_exclusions FROM PUBLIC;
REVOKE ALL ON TYPE decodex.quota_window_class, decodex.observation_confidence FROM PUBLIC;
