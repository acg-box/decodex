DO $$
BEGIN
	IF current_setting('server_version_num')::integer < 180000
		OR current_setting('server_version_num')::integer >= 190000
	THEN
		RAISE EXCEPTION 'Decodex vNext storage proof requires PostgreSQL 18.x, found %', version();
	END IF;
END
$$;

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE SCHEMA decodex;

CREATE TYPE decodex.outbox_state AS ENUM ('pending', 'in_flight', 'delivered');
CREATE TYPE decodex.artifact_state AS ENUM ('active', 'expired', 'deleted');

CREATE FUNCTION decodex.has_credential_key(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	entry record;
BEGIN
	CASE jsonb_typeof(document)
		WHEN 'object' THEN
			FOR entry IN SELECT key, value FROM jsonb_each(document)
			LOOP
				IF lower(entry.key) IN (
					'credential', 'credentials', 'password', 'private_key', 'secret',
					'access_token', 'refresh_token', 'api_key'
				) OR decodex.has_credential_key(entry.value) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'array' THEN
			FOR entry IN SELECT value FROM jsonb_array_elements(document)
			LOOP
				IF decodex.has_credential_key(entry.value) THEN
					RETURN true;
				END IF;
			END LOOP;
		ELSE
			NULL;
	END CASE;
	RETURN false;
END
$$;

CREATE TABLE decodex.probe_entities (
	entity_key text PRIMARY KEY,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	value jsonb NOT NULL CHECK (NOT decodex.has_credential_key(value)),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE decodex.leases (
	resource_key text PRIMARY KEY,
	holder_id uuid NOT NULL,
	lease_token uuid NOT NULL DEFAULT gen_random_uuid(),
	expires_at timestamptz NOT NULL,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE decodex.outbox (
	id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	aggregate_key text NOT NULL,
	payload jsonb NOT NULL CHECK (NOT decodex.has_credential_key(payload)),
	state decodex.outbox_state NOT NULL DEFAULT 'pending',
	attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
	available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	lease_holder uuid,
	lease_expires_at timestamptz,
	last_error text,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	delivered_at timestamptz,
	CHECK (
		(state = 'in_flight' AND lease_holder IS NOT NULL AND lease_expires_at IS NOT NULL)
		OR (state <> 'in_flight' AND lease_holder IS NULL AND lease_expires_at IS NULL)
	),
	CHECK ((state = 'delivered') = (delivered_at IS NOT NULL))
);

CREATE INDEX outbox_claim_idx
	ON decodex.outbox (available_at, id)
	WHERE state <> 'delivered';

CREATE TABLE decodex.command_receipts (
	idempotency_key text PRIMARY KEY,
	request_hash text NOT NULL CHECK (request_hash ~ '^[0-9a-f]{64}$'),
	response jsonb CHECK (response IS NULL OR NOT decodex.has_credential_key(response)),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE decodex.artifacts (
	content_hash text PRIMARY KEY CHECK (content_hash ~ '^[0-9a-f]{64}$'),
	byte_size bigint NOT NULL CHECK (byte_size >= 0),
	relative_path text NOT NULL UNIQUE,
	state decodex.artifact_state NOT NULL DEFAULT 'active',
	integrity_verified_at timestamptz,
	delete_after timestamptz,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	deleted_at timestamptz,
	CHECK (relative_path = 'blobs/sha256/' || left(content_hash, 2) || '/' || content_hash),
	CHECK (relative_path !~ '(^/|(^|/)\.\.(/|$))'),
	CHECK ((state = 'deleted') = (deleted_at IS NOT NULL))
);

CREATE TABLE decodex.account_metadata (
	account_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
	display_name text NOT NULL,
	health text NOT NULL CHECK (health IN (
		'available', 'depleted', 'unknown', 'auth_failed', 'plugin_unready', 'disabled'
	)),
	observed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE FUNCTION decodex.try_acquire_lease(
	p_resource_key text,
	p_holder_id uuid,
	p_ttl interval
)
RETURNS TABLE (acquired boolean, lease_token uuid, revision bigint)
LANGUAGE sql
AS $$
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
	SELECT false, NULL::uuid, NULL::bigint WHERE NOT EXISTS (SELECT 1 FROM acquired)
$$;

CREATE FUNCTION decodex.update_probe_entity(
	p_entity_key text,
	p_expected_revision bigint,
	p_value jsonb
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
	next_revision bigint;
BEGIN
	UPDATE decodex.probe_entities
	SET value = p_value,
		revision = revision + 1,
		updated_at = clock_timestamp()
	WHERE entity_key = p_entity_key AND revision = p_expected_revision
	RETURNING revision INTO next_revision;
	IF next_revision IS NULL THEN
		RAISE EXCEPTION 'optimistic revision conflict for % at expected revision %',
			p_entity_key, p_expected_revision USING ERRCODE = '40001';
	END IF;
	RETURN next_revision;
END
$$;

CREATE FUNCTION decodex.claim_outbox(
	p_worker_id uuid,
	p_limit integer,
	p_lease interval
)
RETURNS SETOF decodex.outbox
LANGUAGE sql
AS $$
	WITH candidates AS (
		SELECT id
		FROM decodex.outbox
		WHERE available_at <= clock_timestamp()
			AND (
				state = 'pending'
				OR (state = 'in_flight' AND lease_expires_at <= clock_timestamp())
			)
		ORDER BY id
		FOR UPDATE SKIP LOCKED
		LIMIT greatest(p_limit, 0)
	)
	UPDATE decodex.outbox AS work
	SET state = 'in_flight',
		attempt_count = work.attempt_count + 1,
		lease_holder = p_worker_id,
		lease_expires_at = clock_timestamp() + p_lease,
		last_error = CASE WHEN work.state = 'in_flight' THEN 'recovered_expired_claim' ELSE NULL END
	FROM candidates
	WHERE work.id = candidates.id
	RETURNING work.*
$$;

CREATE FUNCTION decodex.complete_outbox(p_id bigint, p_worker_id uuid)
RETURNS boolean
LANGUAGE sql
AS $$
	UPDATE decodex.outbox
	SET state = 'delivered',
		lease_holder = NULL,
		lease_expires_at = NULL,
		delivered_at = clock_timestamp()
	WHERE id = p_id AND state = 'in_flight' AND lease_holder = p_worker_id
	RETURNING true
$$;

CREATE FUNCTION decodex.retry_outbox(
	p_id bigint,
	p_worker_id uuid,
	p_error text,
	p_delay interval
)
RETURNS boolean
LANGUAGE sql
AS $$
	UPDATE decodex.outbox
	SET state = 'pending',
		available_at = clock_timestamp() + greatest(p_delay, interval '0 seconds'),
		lease_holder = NULL,
		lease_expires_at = NULL,
		last_error = p_error
	WHERE id = p_id AND state = 'in_flight' AND lease_holder = p_worker_id
	RETURNING true
$$;

CREATE FUNCTION decodex.apply_probe_command(
	p_idempotency_key text,
	p_request_hash text,
	p_entity_key text,
	p_expected_revision bigint,
	p_value jsonb
)
RETURNS jsonb
LANGUAGE plpgsql
AS $$
DECLARE
	existing_hash text;
	existing_response jsonb;
	next_revision bigint;
	command_response jsonb;
BEGIN
	INSERT INTO decodex.command_receipts (idempotency_key, request_hash)
	VALUES (p_idempotency_key, p_request_hash)
	ON CONFLICT DO NOTHING;

	IF NOT FOUND THEN
		SELECT request_hash, response
		INTO existing_hash, existing_response
		FROM decodex.command_receipts
		WHERE idempotency_key = p_idempotency_key;
		IF existing_hash <> p_request_hash THEN
			RAISE EXCEPTION 'idempotency key reused with different request hash'
				USING ERRCODE = '22000';
		END IF;
		RETURN existing_response;
	END IF;

	next_revision := decodex.update_probe_entity(
		p_entity_key, p_expected_revision, p_value
	);
	INSERT INTO decodex.outbox (aggregate_key, payload)
	VALUES (
		p_entity_key,
		jsonb_build_object('kind', 'probe_entity_updated', 'revision', next_revision)
	);
	command_response := jsonb_build_object(
		'entity_key', p_entity_key,
		'revision', next_revision
	);
	UPDATE decodex.command_receipts
	SET response = command_response
	WHERE idempotency_key = p_idempotency_key;
	RETURN command_response;
END
$$;
