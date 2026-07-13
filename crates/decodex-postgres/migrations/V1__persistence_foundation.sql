DO $$
BEGIN
	IF current_setting('server_version_num')::integer < 180000
		OR current_setting('server_version_num')::integer >= 190000
	THEN
		RAISE EXCEPTION 'Decodex vNext requires PostgreSQL 18.x, found %', version();
	END IF;
	IF current_setting('data_checksums') <> 'on' THEN
		RAISE EXCEPTION 'Decodex vNext requires PostgreSQL data checksums';
	END IF;
END
$$;

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE SCHEMA decodex;

CREATE TYPE decodex.account_state AS ENUM (
	'unknown', 'available', 'depleted', 'auth_failed', 'plugin_unready', 'disabled'
);
CREATE TYPE decodex.outbox_state AS ENUM ('pending', 'in_flight', 'delivered', 'dead_letter');
CREATE TYPE decodex.effect_state AS ENUM ('not_started', 'ambiguous', 'receipt_recorded');

CREATE FUNCTION decodex.normalize_unicode_whitespace(value text)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT translate(
		value,
		U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000',
		repeat(' ', 25)
	)
$$;

CREATE FUNCTION decodex.ascii_lower(value text)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT translate(value, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')
$$;

CREATE FUNCTION decodex.has_credential_material(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT (
		decodex.ascii_lower(decodex.normalize_unicode_whitespace(value)) COLLATE "C"
	) ~ '(^|[[:space:][:punct:]])(bearer[[:space:]]+[[:alnum:]_.~+/-]{8,}|basic[[:space:]]+[[:alnum:]+/]{8,}={0,2})|(^|[^[:alnum:]])(sk-[[:alnum:]_-]{8,}|(sk|pk|rk)_(live|test|proj)?[[:alnum:]_-]{8,}|xox[baprs]-[[:alnum:]-]{8,}|glpat-[[:alnum:]_-]{8,}|npm_[[:alnum:]]{8,})|gh[pousr]_[[:alnum:]]{20,}|eyj[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}|-----begin[^-]*private[[:space:]]+key-----|(password|passphrase|secret|token|authorization)[[:space:]]*[:=][[:space:]]*[^[:space:]]{4,}|[a-z][a-z0-9+.-]*://[^/:[:space:]]+:[^@[:space:]]+@|akia[0-9a-z]{16}'
$$;

CREATE FUNCTION decodex.has_credential_material(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	entry record;
	normalized_key text;
BEGIN
	CASE jsonb_typeof(document)
		WHEN 'object' THEN
			FOR entry IN SELECT key, value FROM jsonb_each(document)
			LOOP
				normalized_key := regexp_replace(
					decodex.ascii_lower(entry.key) COLLATE "C",
					'[^a-z0-9]',
					'',
					'g'
				);
				IF (normalized_key COLLATE "C") ~ '(credentials?|password|passphrase|privatekey|secret|authorization|bearer|apikey|cookie|token|session)$'
					OR decodex.has_credential_material(entry.value) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'array' THEN
			FOR entry IN SELECT value FROM jsonb_array_elements(document)
			LOOP
				IF decodex.has_credential_material(entry.value) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'string' THEN
			IF decodex.has_credential_material(document #>> '{}') THEN
				RETURN true;
			END IF;
		ELSE
			NULL;
	END CASE;
	RETURN false;
END
$$;

CREATE FUNCTION decodex.is_meaningful_evidence(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	entry jsonb;
BEGIN
	CASE jsonb_typeof(document)
		WHEN 'object' THEN
			FOR entry IN SELECT value FROM jsonb_each(document)
			LOOP
				IF decodex.is_meaningful_evidence(entry) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'array' THEN
			FOR entry IN SELECT value FROM jsonb_array_elements(document)
			LOOP
				IF decodex.is_meaningful_evidence(entry) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'string' THEN
			RETURN length(btrim(decodex.normalize_unicode_whitespace(document #>> '{}'))) > 0;
		WHEN 'number', 'boolean' THEN
			RETURN true;
		ELSE
			NULL;
	END CASE;

	RETURN false;
END
$$;

CREATE FUNCTION decodex.rfc3339_utc(value timestamptz)
RETURNS text
LANGUAGE sql
STABLE
STRICT
AS $$
	SELECT to_char(value AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
$$;

CREATE FUNCTION decodex.is_valid_operation_duration(value interval)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT extract(year FROM value) = 0
		AND extract(month FROM value) = 0
		AND extract(epoch FROM value) * 1000 BETWEEN 1 AND 31536000000
		AND extract(epoch FROM value) * 1000 = trunc(extract(epoch FROM value) * 1000)
$$;

CREATE TABLE decodex.accounts (
	account_id uuid PRIMARY KEY,
	display_label text NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128),
	state decodex.account_state NOT NULL DEFAULT 'unknown',
	metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT accounts_finite_timestamps CHECK (
		isfinite(observed_at) AND isfinite(updated_at)
	),
	CONSTRAINT accounts_no_credentials CHECK (
		NOT decodex.has_credential_material(display_label)
		AND NOT decodex.has_credential_material(metadata)
	)
);

CREATE TABLE decodex.quota_windows (
	account_id uuid NOT NULL REFERENCES decodex.accounts(account_id) ON DELETE CASCADE,
	window_class text NOT NULL CHECK (window_class ~ '^[a-z][a-z0-9_]{0,63}$'),
	duration_seconds bigint NOT NULL CHECK (duration_seconds > 0),
	remaining_amount double precision CHECK (remaining_amount IS NULL OR remaining_amount >= 0),
	resets_at timestamptz,
	observed_at timestamptz NOT NULL,
	confidence double precision NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
	metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT quota_windows_finite_timestamps CHECK (
		(resets_at IS NULL OR isfinite(resets_at))
		AND isfinite(observed_at)
		AND isfinite(updated_at)
	),
	PRIMARY KEY (account_id, window_class, duration_seconds),
	CONSTRAINT quota_windows_no_credentials CHECK (
		NOT decodex.has_credential_material(window_class)
		AND NOT decodex.has_credential_material(metadata)
	)
);

CREATE TABLE decodex.command_receipts (
	idempotency_key text PRIMARY KEY CHECK (length(idempotency_key) BETWEEN 1 AND 256),
	request_hash text NOT NULL CHECK (request_hash ~ '^[0-9a-f]{64}$'),
	response jsonb,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	completed_at timestamptz,
	CONSTRAINT command_receipts_finite_timestamps CHECK (
		isfinite(created_at) AND (completed_at IS NULL OR isfinite(completed_at))
	),
	CONSTRAINT command_receipts_timestamp_order CHECK (
		completed_at IS NULL OR completed_at >= created_at
	),
	CONSTRAINT command_receipts_no_credentials CHECK (
		NOT decodex.has_credential_material(idempotency_key)
		AND (response IS NULL OR NOT decodex.has_credential_material(response))
	),
	CHECK ((response IS NOT NULL) = (completed_at IS NOT NULL))
);

CREATE TABLE decodex.activity (
	sequence bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	aggregate_kind text NOT NULL CHECK (aggregate_kind ~ '^[a-z][a-z0-9_]{0,63}$'),
	aggregate_id text NOT NULL CHECK (length(aggregate_id) BETWEEN 1 AND 256),
	revision bigint NOT NULL CHECK (revision > 0),
	event_kind text NOT NULL CHECK (event_kind ~ '^[a-z][a-z0-9_]{0,127}$'),
	correlation_key text NOT NULL CHECK (length(correlation_key) BETWEEN 1 AND 256),
	payload jsonb NOT NULL,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT activity_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT activity_no_credentials CHECK (
		NOT decodex.has_credential_material(aggregate_kind)
		AND NOT decodex.has_credential_material(aggregate_id)
		AND NOT decodex.has_credential_material(event_kind)
		AND NOT decodex.has_credential_material(correlation_key)
		AND NOT decodex.has_credential_material(payload)
	),
	UNIQUE (aggregate_kind, aggregate_id, revision, event_kind)
);

CREATE TABLE decodex.leases (
	resource_key text PRIMARY KEY CHECK (length(resource_key) BETWEEN 1 AND 256),
	holder_id uuid NOT NULL,
	lease_token uuid NOT NULL DEFAULT gen_random_uuid(),
	expires_at timestamptz NOT NULL,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT leases_finite_timestamps CHECK (isfinite(expires_at) AND isfinite(updated_at)),
	CONSTRAINT leases_bounded_expiration CHECK (
		expires_at <= updated_at
		OR decodex.is_valid_operation_duration(expires_at - updated_at)
	),
	CONSTRAINT leases_no_credentials CHECK (
		NOT decodex.has_credential_material(resource_key)
	)
);

CREATE TABLE decodex.outbox (
	id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	effect_key text NOT NULL UNIQUE CHECK (length(effect_key) BETWEEN 1 AND 256),
	aggregate_kind text NOT NULL CHECK (aggregate_kind ~ '^[a-z][a-z0-9_]{0,63}$'),
	aggregate_id text NOT NULL CHECK (length(aggregate_id) BETWEEN 1 AND 256),
	aggregate_revision bigint NOT NULL CHECK (aggregate_revision > 0),
	payload jsonb NOT NULL,
	state decodex.outbox_state NOT NULL DEFAULT 'pending',
	effect_state decodex.effect_state NOT NULL DEFAULT 'not_started',
	attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
	max_attempts integer NOT NULL DEFAULT 16 CHECK (max_attempts > 0),
	available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	lease_holder uuid,
	claim_token uuid,
	lease_acquired_at timestamptz,
	lease_expires_at timestamptz,
	receipt jsonb,
	reconciliation jsonb,
	last_failure_code text CHECK (
		last_failure_code IS NULL OR last_failure_code ~ '^[a-z][a-z0-9_]{0,127}$'
	),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	delivered_at timestamptz,
	dead_lettered_at timestamptz,
	retain_until timestamptz,
	CONSTRAINT outbox_finite_timestamps CHECK (
		isfinite(available_at)
		AND (lease_acquired_at IS NULL OR isfinite(lease_acquired_at))
		AND (lease_expires_at IS NULL OR isfinite(lease_expires_at))
		AND isfinite(created_at)
		AND (delivered_at IS NULL OR isfinite(delivered_at))
		AND (dead_lettered_at IS NULL OR isfinite(dead_lettered_at))
		AND (retain_until IS NULL OR isfinite(retain_until))
	),
	CONSTRAINT outbox_no_credentials CHECK (
		NOT decodex.has_credential_material(effect_key)
		AND NOT decodex.has_credential_material(aggregate_kind)
		AND NOT decodex.has_credential_material(aggregate_id)
		AND NOT decodex.has_credential_material(payload)
		AND (receipt IS NULL OR NOT decodex.has_credential_material(receipt))
		AND (reconciliation IS NULL OR NOT decodex.has_credential_material(reconciliation))
		AND (last_failure_code IS NULL OR NOT decodex.has_credential_material(last_failure_code))
	),
	CONSTRAINT outbox_claim_shape CHECK (
		(state = 'in_flight' AND lease_holder IS NOT NULL AND claim_token IS NOT NULL
			AND lease_acquired_at IS NOT NULL AND lease_expires_at IS NOT NULL)
		OR (state <> 'in_flight' AND lease_holder IS NULL AND claim_token IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL)
	),
	CONSTRAINT outbox_in_flight_lease_duration CHECK (
		state <> 'in_flight'
		OR (
			lease_acquired_at >= created_at
			AND decodex.is_valid_operation_duration(lease_expires_at - lease_acquired_at)
		)
	),
	CONSTRAINT outbox_meaningful_receipt_state CHECK (
		(effect_state = 'receipt_recorded')
			= COALESCE(decodex.is_meaningful_evidence(receipt), false)
	),
	CONSTRAINT outbox_delivered_evidence CHECK (
		state <> 'delivered'
		OR (
			effect_state = 'receipt_recorded'
			AND COALESCE(decodex.is_meaningful_evidence(receipt), false)
			AND COALESCE(decodex.is_meaningful_evidence(reconciliation), false)
			AND NOT decodex.has_credential_material(receipt)
			AND NOT decodex.has_credential_material(reconciliation)
		)
	),
	CONSTRAINT outbox_delivered_timestamps CHECK (
		(state = 'delivered') = (delivered_at IS NOT NULL)
		AND (state = 'delivered') = (retain_until IS NOT NULL)
	),
	CONSTRAINT outbox_delivered_retention CHECK (
		state <> 'delivered'
		OR decodex.is_valid_operation_duration(retain_until - delivered_at)
	),
	CONSTRAINT outbox_terminal_chronology CHECK (
		(delivered_at IS NULL OR delivered_at >= created_at)
		AND (dead_lettered_at IS NULL OR dead_lettered_at >= created_at)
	),
	CHECK ((state = 'dead_letter') = (dead_lettered_at IS NOT NULL))
);

CREATE FUNCTION decodex.enforce_lease_operation_time()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
	write_at timestamptz := clock_timestamp();
BEGIN
	IF NEW.updated_at > write_at
		OR NEW.expires_at > write_at + 31536000000 * interval '1 millisecond'
	THEN
		RAISE EXCEPTION 'lease timestamps must be anchored to the database write time'
			USING ERRCODE = '23514', CONSTRAINT = 'leases_operation_time';
	END IF;

	RETURN NEW;
END
$$;

CREATE TRIGGER leases_operation_time
BEFORE INSERT OR UPDATE ON decodex.leases
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_lease_operation_time();

CREATE FUNCTION decodex.enforce_outbox_operation_time()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
	write_at timestamptz := clock_timestamp();
	latest_deadline timestamptz := write_at + 31536000000 * interval '1 millisecond';
BEGIN
	IF NEW.created_at > write_at
		OR NEW.available_at > latest_deadline
		OR (NEW.lease_acquired_at IS NOT NULL AND NEW.lease_acquired_at > write_at)
		OR (NEW.lease_expires_at IS NOT NULL AND NEW.lease_expires_at > latest_deadline)
		OR (NEW.delivered_at IS NOT NULL AND NEW.delivered_at > write_at)
		OR (NEW.dead_lettered_at IS NOT NULL AND NEW.dead_lettered_at > write_at)
		OR (NEW.retain_until IS NOT NULL AND NEW.retain_until > latest_deadline)
	THEN
		RAISE EXCEPTION 'outbox timestamps must be anchored to the database write time'
			USING ERRCODE = '23514', CONSTRAINT = 'outbox_operation_time';
	END IF;

	RETURN NEW;
END
$$;

CREATE TRIGGER outbox_operation_time
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_outbox_operation_time();

CREATE FUNCTION decodex.forbid_mutation_of_activity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	RAISE EXCEPTION 'decodex.activity is append-only';
END
$$;

CREATE TRIGGER activity_append_only
BEFORE UPDATE OR DELETE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_mutation_of_activity();

CREATE FUNCTION decodex.enforce_outbox_terminal_retention()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	IF TG_OP = 'DELETE' THEN
		IF OLD.state <> 'delivered' OR OLD.retain_until > clock_timestamp() THEN
			RAISE EXCEPTION 'outbox rows may be deleted only after delivered retention is due'
				USING ERRCODE = '55000', CONSTRAINT = 'outbox_retention_pruning_only';
		END IF;

		RETURN OLD;
	END IF;

	IF OLD.state = 'delivered' AND NEW IS DISTINCT FROM OLD THEN
		RAISE EXCEPTION 'delivered outbox rows are immutable until retention pruning'
			USING ERRCODE = '55000', CONSTRAINT = 'outbox_delivered_is_terminal';
	END IF;

	RETURN NEW;
END
$$;

CREATE TRIGGER outbox_terminal_retention
BEFORE UPDATE OR DELETE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_outbox_terminal_retention();

CREATE FUNCTION decodex.lease_ttl_milliseconds(value interval)
RETURNS bigint
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	milliseconds numeric;
BEGIN
	IF extract(year FROM value) <> 0 OR extract(month FROM value) <> 0 THEN
		RAISE EXCEPTION 'lease TTL must be a fixed duration' USING ERRCODE = '22023';
	END IF;

	milliseconds := extract(epoch FROM value) * 1000;

	IF milliseconds < 1
		OR milliseconds > 31536000000
		OR milliseconds <> trunc(milliseconds)
	THEN
		RAISE EXCEPTION 'lease TTL must be a positive whole number of milliseconds no greater than 365 days'
			USING ERRCODE = '22023';
	END IF;

	RETURN milliseconds::bigint;
END
$$;

CREATE FUNCTION decodex.try_acquire_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_ttl interval
)
RETURNS TABLE (acquired boolean, lease_token uuid, revision bigint)
LANGUAGE plpgsql
AS $$
DECLARE
	ttl_milliseconds bigint := decodex.lease_ttl_milliseconds(p_ttl);
	write_at timestamptz := clock_timestamp();
BEGIN
	RETURN QUERY
	WITH acquired AS (
		INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at)
		VALUES (
			p_resource_key,
			p_holder_id,
			write_at + ttl_milliseconds * interval '1 millisecond',
			write_at
		)
		ON CONFLICT (resource_key) DO UPDATE
		SET holder_id = EXCLUDED.holder_id,
			lease_token = gen_random_uuid(),
			expires_at = EXCLUDED.expires_at,
			revision = decodex.leases.revision + 1,
			updated_at = EXCLUDED.updated_at
		WHERE decodex.leases.expires_at <= write_at
			OR decodex.leases.holder_id = EXCLUDED.holder_id
		RETURNING decodex.leases.lease_token, decodex.leases.revision
	)
	SELECT true, acquired.lease_token, acquired.revision FROM acquired
	UNION ALL
	SELECT false, NULL::uuid, NULL::bigint WHERE NOT EXISTS (SELECT 1 FROM acquired);
END
$$;

CREATE FUNCTION decodex.renew_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_lease_token uuid,
	p_ttl interval
)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
	ttl_milliseconds bigint := decodex.lease_ttl_milliseconds(p_ttl);
	write_at timestamptz := clock_timestamp();
	updated_count bigint;
BEGIN
	UPDATE decodex.leases
	SET expires_at = write_at + ttl_milliseconds * interval '1 millisecond',
		revision = revision + 1,
		updated_at = write_at
	WHERE resource_key = p_resource_key
		AND holder_id = p_holder_id
		AND lease_token = p_lease_token
		AND expires_at > write_at;

	GET DIAGNOSTICS updated_count = ROW_COUNT;

	RETURN updated_count = 1;
END
$$;

CREATE FUNCTION decodex.release_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_lease_token uuid
)
RETURNS boolean
LANGUAGE sql
AS $$
	WITH write_time AS (SELECT clock_timestamp() AS value)
	UPDATE decodex.leases
	SET expires_at = write_time.value,
		revision = revision + 1,
		updated_at = write_time.value
	FROM write_time
	WHERE resource_key = p_resource_key
		AND holder_id = p_holder_id
		AND lease_token = p_lease_token
		AND expires_at > write_time.value
	RETURNING true
$$;
