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

CREATE FUNCTION decodex.has_credential_material(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT value ~* '(^|[[:space:][:punct:]])(bearer[[:space:]]+[[:alnum:]_.~+/-]{8,}|basic[[:space:]]+[[:alnum:]+/]{8,}={0,2})|(^|[^[:alnum:]])(sk-[[:alnum:]_-]{8,}|(sk|pk|rk)_(live|test|proj)?[[:alnum:]_-]{8,}|xox[baprs]-[[:alnum:]-]{8,}|glpat-[[:alnum:]_-]{8,}|npm_[[:alnum:]]{8,})|gh[pousr]_[[:alnum:]]{20,}|eyj[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}|-----begin[^-]*private key-----|(password|passphrase|secret|token|authorization)[[:space:]]*[:=][[:space:]]*[^[:space:]]{4,}|[a-z][a-z0-9+.-]*://[^/:[:space:]]+:[^@[:space:]]+@|akia[0-9a-z]{16}'
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
				normalized_key := regexp_replace(lower(entry.key), '[^a-z0-9]', '', 'g');
				IF normalized_key ~ '(credentials?|password|passphrase|privatekey|secret|authorization|bearer|apikey|cookie|token|session)$'
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

CREATE TABLE decodex.accounts (
	account_id uuid PRIMARY KEY,
	display_label text NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128),
	state decodex.account_state NOT NULL DEFAULT 'unknown',
	metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
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
	CONSTRAINT outbox_no_credentials CHECK (
		NOT decodex.has_credential_material(effect_key)
		AND NOT decodex.has_credential_material(aggregate_kind)
		AND NOT decodex.has_credential_material(aggregate_id)
		AND NOT decodex.has_credential_material(payload)
		AND (receipt IS NULL OR NOT decodex.has_credential_material(receipt))
		AND (reconciliation IS NULL OR NOT decodex.has_credential_material(reconciliation))
		AND (last_failure_code IS NULL OR NOT decodex.has_credential_material(last_failure_code))
	),
	CHECK (
		(state = 'in_flight' AND lease_holder IS NOT NULL AND claim_token IS NOT NULL
			AND lease_expires_at IS NOT NULL)
		OR (state <> 'in_flight' AND lease_holder IS NULL AND claim_token IS NULL
			AND lease_expires_at IS NULL)
	),
	CHECK ((state = 'delivered') = (delivered_at IS NOT NULL)),
	CHECK ((state = 'dead_letter') = (dead_lettered_at IS NOT NULL)),
	CHECK ((effect_state = 'receipt_recorded') = (receipt IS NOT NULL)),
	CHECK (state <> 'delivered' OR (effect_state = 'receipt_recorded' AND receipt IS NOT NULL))
);

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

CREATE FUNCTION decodex.try_acquire_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_ttl interval
)
RETURNS TABLE (acquired boolean, lease_token uuid, revision bigint)
LANGUAGE plpgsql
AS $$
BEGIN
	IF p_ttl <= interval '0 seconds' THEN
		RAISE EXCEPTION 'lease TTL must be positive' USING ERRCODE = '22023';
	END IF;

	RETURN QUERY
	WITH acquired AS (
		INSERT INTO decodex.leases (resource_key, holder_id, expires_at)
		VALUES (p_resource_key, p_holder_id, clock_timestamp() + p_ttl)
		ON CONFLICT (resource_key) DO UPDATE
		SET holder_id = EXCLUDED.holder_id,
			lease_token = gen_random_uuid(),
			expires_at = EXCLUDED.expires_at,
			revision = decodex.leases.revision + 1,
			updated_at = clock_timestamp()
		WHERE decodex.leases.expires_at <= clock_timestamp()
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
LANGUAGE sql
AS $$
	UPDATE decodex.leases
	SET expires_at = clock_timestamp() + p_ttl,
		revision = revision + 1,
		updated_at = clock_timestamp()
	WHERE resource_key = p_resource_key
		AND holder_id = p_holder_id
		AND lease_token = p_lease_token
		AND expires_at > clock_timestamp()
		AND p_ttl > interval '0 seconds'
	RETURNING true
$$;

CREATE FUNCTION decodex.release_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_lease_token uuid
)
RETURNS boolean
LANGUAGE sql
AS $$
	UPDATE decodex.leases
	SET expires_at = clock_timestamp(),
		revision = revision + 1,
		updated_at = clock_timestamp()
	WHERE resource_key = p_resource_key
		AND holder_id = p_holder_id
		AND lease_token = p_lease_token
		AND expires_at > clock_timestamp()
	RETURNING true
$$;
