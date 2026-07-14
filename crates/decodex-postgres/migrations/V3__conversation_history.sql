CREATE FUNCTION decodex.is_canonical_media_type(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT octet_length(value) BETWEEN 1 AND 128
		AND translate(
			value,
			'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$&^_.+-/',
			''
		) = ''
		AND length(value) - length(replace(value, '/', '')) = 1
		AND split_part(value, '/', 1) <> ''
		AND split_part(value, '/', 2) <> ''
$$;

CREATE FUNCTION decodex.is_history_metadata_projection(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	entry record;
	field_count integer := 0;
BEGIN
	IF pg_catalog.jsonb_typeof(document) <> 'object'
		OR decodex.has_credential_material(document)
	THEN
		RETURN false;
	END IF;
	FOR entry IN SELECT key, value FROM pg_catalog.jsonb_each(document) LOOP
		field_count := field_count + 1;
		IF field_count > 32 OR pg_catalog.octet_length(entry.key) NOT BETWEEN 1 AND 64 THEN
			RETURN false;
		END IF;
		IF pg_catalog.jsonb_typeof(entry.value) = 'string' THEN
			IF pg_catalog.octet_length(entry.value OPERATOR(pg_catalog.#>>) '{}') > 256 THEN
				RETURN false;
			END IF;
		ELSIF pg_catalog.jsonb_typeof(entry.value) <> 'boolean' THEN
			RETURN false;
		END IF;
	END LOOP;
	RETURN true;
END
$$;

CREATE TYPE decodex.conversation_status AS ENUM ('open', 'archived');
CREATE TYPE decodex.runtime_session_state AS ENUM ('starting', 'active', 'ended', 'diverged');
CREATE TYPE decodex.turn_role AS ENUM ('user', 'assistant', 'system', 'tool');
CREATE TYPE decodex.side_effect_state AS ENUM ('none', 'possible', 'unknown');
CREATE TYPE decodex.history_item_kind AS ENUM (
	'message', 'reasoning', 'tool_call', 'tool_result', 'artifact', 'status'
);
CREATE TYPE decodex.history_item_status AS ENUM ('streaming', 'completed', 'failed');
CREATE TYPE decodex.turn_status AS ENUM ('active', 'completed', 'failed');
CREATE TYPE decodex.artifact_status AS ENUM ('active', 'expired', 'deleted');
CREATE TYPE decodex.context_source_kind AS ENUM (
	'pinned_revision', 'repository_instructions', 'openwiki', 'decision', 'fact',
	'artifact', 'recent_raw'
);
CREATE TYPE decodex.transition_kind AS ENUM ('rollover', 'fallback');
CREATE TYPE decodex.context_source_disposition AS ENUM ('complete', 'truncated', 'omitted');
CREATE TYPE decodex.command_receipt_state AS ENUM ('pending', 'completed');

ALTER TABLE decodex.command_receipts
	ADD COLUMN protocol_version text NOT NULL DEFAULT 'decodex/store-command/1',
	ADD COLUMN operation text NOT NULL DEFAULT 'legacy',
	ADD COLUMN project_scope text NOT NULL DEFAULT 'global',
	ADD COLUMN scope_id text NOT NULL DEFAULT 'global',
	ADD COLUMN entity_id text NOT NULL DEFAULT 'legacy',
	ADD COLUMN expected_revision bigint,
	ADD COLUMN payload_hash text,
	ADD COLUMN payload_length bigint,
	ADD COLUMN receipt_state decodex.command_receipt_state NOT NULL DEFAULT 'pending',
	ADD COLUMN claim_token uuid,
	ADD COLUMN completion_claim_token uuid,
	ADD COLUMN claim_expires_at timestamptz,
	ADD COLUMN response_bytes bytea;

UPDATE decodex.command_receipts SET
	operation = 'legacy',
	entity_id = idempotency_key,
	receipt_state = CASE
		WHEN response IS NULL THEN 'pending'::decodex.command_receipt_state
		ELSE 'completed'::decodex.command_receipt_state
	END,
	claim_token = CASE WHEN response IS NULL THEN pg_catalog.gen_random_uuid() ELSE NULL END,
	completion_claim_token = CASE WHEN response IS NULL THEN NULL ELSE pg_catalog.gen_random_uuid() END,
	claim_expires_at = CASE
		WHEN response IS NULL THEN pg_catalog.clock_timestamp() + interval '5 minutes'
		ELSE NULL
	END,
	response_bytes = CASE WHEN response IS NULL THEN NULL ELSE pg_catalog.convert_to(response::text, 'UTF8') END;

ALTER TABLE decodex.command_receipts
	ADD CONSTRAINT command_receipts_protocol CHECK (
		protocol_version = 'decodex/store-command/1'
		AND operation ~ '^[a-z][a-z0-9_]{0,63}$'
		AND project_scope ~ '^[a-z][a-z0-9_]{0,31}$'
		AND octet_length(scope_id) BETWEEN 1 AND 256
		AND octet_length(entity_id) BETWEEN 1 AND 256
	),
	ADD CONSTRAINT command_receipts_payload CHECK (
		(payload_hash IS NULL AND payload_length IS NULL)
		OR (payload_hash ~ '^[0-9a-f]{64}$' AND payload_length BETWEEN 1 AND 67108864)
	),
	ADD CONSTRAINT command_receipts_claim CHECK (
		(receipt_state = 'pending' AND response IS NULL AND response_bytes IS NULL
			AND completed_at IS NULL AND claim_token IS NOT NULL
			AND completion_claim_token IS NULL
			AND claim_expires_at IS NOT NULL AND isfinite(claim_expires_at)
			AND claim_expires_at > created_at)
		OR (receipt_state = 'completed' AND response IS NOT NULL AND response_bytes IS NOT NULL
			AND completed_at IS NOT NULL AND claim_token IS NULL
			AND completion_claim_token IS NOT NULL AND claim_expires_at IS NULL)
	),
	ADD CONSTRAINT command_receipts_saga_no_credentials CHECK (
		NOT decodex.has_credential_material(operation)
		AND NOT decodex.has_credential_material(project_scope)
		AND NOT decodex.has_credential_material(scope_id)
		AND NOT decodex.has_credential_material(entity_id)
	);

CREATE FUNCTION decodex.enforce_command_receipt_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE write_time timestamptz := pg_catalog.clock_timestamp();
BEGIN
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION 'command receipts are durable and cannot be deleted'
			USING ERRCODE = '55000', CONSTRAINT = 'command_receipts_durable';
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.created_at > write_time
			OR NEW.created_at < write_time - interval '1 minute'
			OR (NEW.receipt_state = 'pending' AND (
				NEW.claim_expires_at <= write_time
				OR NEW.claim_expires_at > write_time + interval '5 minutes'
			))
			OR NEW.receipt_state = 'completed' THEN
			RAISE EXCEPTION 'new command receipts must be canonical pending reservations'
				USING ERRCODE = '23514', CONSTRAINT = 'command_receipts_canonical_insert';
		END IF;
		RETURN NEW;
	END IF;
	IF NEW.idempotency_key IS DISTINCT FROM OLD.idempotency_key
		OR NEW.request_hash IS DISTINCT FROM OLD.request_hash
		OR NEW.protocol_version IS DISTINCT FROM OLD.protocol_version
		OR NEW.operation IS DISTINCT FROM OLD.operation
		OR NEW.project_scope IS DISTINCT FROM OLD.project_scope
		OR NEW.scope_id IS DISTINCT FROM OLD.scope_id
		OR NEW.entity_id IS DISTINCT FROM OLD.entity_id
		OR NEW.expected_revision IS DISTINCT FROM OLD.expected_revision
		OR NEW.payload_hash IS DISTINCT FROM OLD.payload_hash
		OR NEW.payload_length IS DISTINCT FROM OLD.payload_length
		OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
		RAISE EXCEPTION 'command receipt request identity is immutable'
			USING ERRCODE = '55000', CONSTRAINT = 'command_receipts_identity_immutable';
	END IF;
	IF OLD.receipt_state = 'completed' THEN
		RAISE EXCEPTION 'completed command receipts are immutable'
			USING ERRCODE = '55000', CONSTRAINT = 'command_receipts_completed_immutable';
	END IF;
	IF NEW.receipt_state = 'pending' THEN
		IF OLD.claim_expires_at > write_time
			OR NEW.claim_token IS NOT DISTINCT FROM OLD.claim_token
			OR NEW.claim_expires_at <= write_time
			OR NEW.claim_expires_at > write_time + interval '5 minutes' THEN
			RAISE EXCEPTION 'pending command claim can rotate only after expiry'
				USING ERRCODE = '55000', CONSTRAINT = 'command_receipts_claim_fence';
		END IF;
	ELSIF NEW.receipt_state = 'completed' THEN
		IF NEW.completion_claim_token IS DISTINCT FROM OLD.claim_token
			OR OLD.claim_expires_at <= write_time
			OR NEW.completed_at > write_time
			OR NEW.completed_at < write_time - interval '1 minute' THEN
			RAISE EXCEPTION 'command completion lost its live claim fence'
				USING ERRCODE = '55000', CONSTRAINT = 'command_receipts_completion_fence';
		END IF;
	END IF;
	RETURN NEW;
END;
$$;

CREATE TRIGGER command_receipts_state_guard
BEFORE INSERT OR UPDATE OR DELETE ON decodex.command_receipts
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_command_receipt_state();

CREATE TABLE decodex.conversations (
	conversation_id uuid PRIMARY KEY,
	title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 512),
	status decodex.conversation_status NOT NULL DEFAULT 'open',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT conversations_finite_timestamps CHECK (isfinite(created_at) AND isfinite(updated_at)),
	CONSTRAINT conversations_timestamp_order CHECK (updated_at >= created_at),
	CONSTRAINT conversations_no_credentials CHECK (NOT decodex.has_credential_material(title))
);

CREATE TABLE decodex.profile_snapshots (
	profile_snapshot_id uuid PRIMARY KEY,
	source_profile_id text NOT NULL CHECK (octet_length(source_profile_id) BETWEEN 1 AND 128),
	role text NOT NULL CHECK (role ~ '^[a-z][a-z0-9_]{0,31}$'),
	model text NOT NULL CHECK (octet_length(model) BETWEEN 1 AND 128),
	reasoning_effort text NOT NULL CHECK (reasoning_effort ~ '^[a-z][a-z0-9_]{0,31}$'),
	service_tier text NOT NULL CHECK (service_tier ~ '^[a-z][a-z0-9_]{0,31}$'),
	instructions_digest text NOT NULL CHECK (instructions_digest ~ '^[0-9a-f]{64}$'),
	source_revision bigint NOT NULL CHECK (source_revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT profile_snapshots_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT profile_snapshots_no_credentials CHECK (
		NOT decodex.has_credential_material(source_profile_id)
		AND NOT decodex.has_credential_material(role)
		AND NOT decodex.has_credential_material(model)
		AND NOT decodex.has_credential_material(reasoning_effort)
		AND NOT decodex.has_credential_material(service_tier)
	)
);

CREATE TABLE decodex.account_snapshots (
	account_snapshot_id uuid PRIMARY KEY,
	source_account_id text NOT NULL CHECK (octet_length(source_account_id) BETWEEN 1 AND 128),
	display_label text NOT NULL CHECK (octet_length(display_label) BETWEEN 1 AND 128),
	observed_state text NOT NULL CHECK (observed_state ~ '^[a-z][a-z0-9_]{0,63}$'),
	source_revision bigint NOT NULL CHECK (source_revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT account_snapshots_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT account_snapshots_no_credentials CHECK (
		NOT decodex.has_credential_material(source_account_id)
		AND NOT decodex.has_credential_material(display_label)
		AND NOT decodex.has_credential_material(observed_state)
	)
);

CREATE TABLE decodex.runtime_sessions (
	runtime_session_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	profile_snapshot_id uuid NOT NULL REFERENCES decodex.profile_snapshots(profile_snapshot_id) ON DELETE RESTRICT,
	account_snapshot_id uuid NOT NULL REFERENCES decodex.account_snapshots(account_snapshot_id) ON DELETE RESTRICT,
	codex_thread_id text CHECK (
		codex_thread_id IS NULL OR octet_length(codex_thread_id) BETWEEN 1 AND 256
	),
	state decodex.runtime_session_state NOT NULL DEFAULT 'starting',
	last_known_turn_id text CHECK (
		last_known_turn_id IS NULL OR octet_length(last_known_turn_id) BETWEEN 1 AND 256
	),
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	ended_at timestamptz,
	CONSTRAINT runtime_sessions_finite_timestamps CHECK (
		isfinite(created_at) AND isfinite(updated_at) AND (ended_at IS NULL OR isfinite(ended_at))
	),
	CONSTRAINT runtime_sessions_timestamp_order CHECK (
		updated_at >= created_at AND (ended_at IS NULL OR ended_at >= created_at)
	),
	CONSTRAINT runtime_sessions_no_credentials CHECK (
		(codex_thread_id IS NULL OR NOT decodex.has_credential_material(codex_thread_id))
		AND (last_known_turn_id IS NULL OR NOT decodex.has_credential_material(last_known_turn_id))
	),
	UNIQUE (runtime_session_id, conversation_id),
	UNIQUE (codex_thread_id)
);

CREATE TABLE decodex.blob_objects (
	blob_hash text PRIMARY KEY CHECK (blob_hash ~ '^[0-9a-f]{64}$'),
	byte_length bigint NOT NULL CHECK (byte_length BETWEEN 1 AND 67108864),
	verified_at timestamptz NOT NULL,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT blob_objects_finite_timestamps CHECK (isfinite(verified_at) AND isfinite(created_at)),
	CONSTRAINT blob_objects_timestamp_order CHECK (verified_at >= created_at)
);

CREATE TABLE decodex.artifacts (
	artifact_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	status decodex.artifact_status NOT NULL DEFAULT 'active',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT artifacts_finite_timestamps CHECK (isfinite(created_at) AND isfinite(updated_at)),
	CONSTRAINT artifacts_timestamp_order CHECK (updated_at >= created_at),
	UNIQUE (artifact_id, conversation_id),
	UNIQUE (artifact_id, conversation_id, revision)
);

CREATE TABLE decodex.artifact_revisions (
	artifact_id uuid NOT NULL,
	conversation_id uuid NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	blob_hash text NOT NULL REFERENCES decodex.blob_objects(blob_hash) ON DELETE RESTRICT,
	media_type text NOT NULL CHECK (decodex.is_canonical_media_type(media_type)),
	display_name text CHECK (display_name IS NULL OR octet_length(display_name) BETWEEN 1 AND 256),
	status decodex.artifact_status NOT NULL,
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT artifact_revisions_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT artifact_revisions_no_credentials CHECK (
		NOT decodex.has_credential_material(media_type)
		AND (display_name IS NULL OR NOT decodex.has_credential_material(display_name))
	),
	CONSTRAINT artifact_revisions_parent_fk FOREIGN KEY (artifact_id, conversation_id)
		REFERENCES decodex.artifacts(artifact_id, conversation_id) ON DELETE RESTRICT,
	PRIMARY KEY (artifact_id, revision),
	UNIQUE (artifact_id, conversation_id, revision)
);

ALTER TABLE decodex.artifacts ADD CONSTRAINT artifacts_current_revision_fk
	FOREIGN KEY (artifact_id, conversation_id, revision)
	REFERENCES decodex.artifact_revisions(artifact_id, conversation_id, revision)
	ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE decodex.turns (
	turn_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	runtime_session_id uuid NOT NULL,
	sequence bigint NOT NULL CHECK (sequence > 0),
	role decodex.turn_role NOT NULL,
	possible_side_effects decodex.side_effect_state NOT NULL DEFAULT 'none',
	status decodex.turn_status NOT NULL DEFAULT 'active',
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	completed_at timestamptz,
	CONSTRAINT turns_finite_timestamps CHECK (
		isfinite(created_at) AND isfinite(updated_at)
		AND (completed_at IS NULL OR isfinite(completed_at))
	),
	CONSTRAINT turns_timestamp_order CHECK (
		updated_at >= created_at AND (completed_at IS NULL OR completed_at >= created_at)
	),
	CONSTRAINT turns_session_conversation_fk FOREIGN KEY (runtime_session_id, conversation_id)
		REFERENCES decodex.runtime_sessions(runtime_session_id, conversation_id) ON DELETE RESTRICT,
	UNIQUE (conversation_id, sequence),
	UNIQUE (turn_id, conversation_id),
	UNIQUE (turn_id, conversation_id, runtime_session_id)
);

CREATE TABLE decodex.history_items (
	history_item_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL,
	history_position bigint NOT NULL CHECK (history_position > 0),
	turn_id uuid NOT NULL,
	ordinal integer NOT NULL CHECK (ordinal >= 0 AND ordinal <= 1000000),
	kind decodex.history_item_kind NOT NULL,
	status decodex.history_item_status NOT NULL,
	inline_text text CHECK (inline_text IS NULL OR octet_length(inline_text) <= 16384),
	blob_hash text REFERENCES decodex.blob_objects(blob_hash) ON DELETE RESTRICT,
	media_type text NOT NULL CHECK (decodex.is_canonical_media_type(media_type)),
	metadata jsonb NOT NULL DEFAULT '{}'::jsonb
		CHECK (decodex.is_history_metadata_projection(metadata)),
	artifact_id uuid,
	artifact_revision bigint,
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT history_items_payload_shape CHECK (
		(inline_text IS NOT NULL AND blob_hash IS NULL)
		OR (inline_text IS NULL AND blob_hash IS NOT NULL)
	),
	CONSTRAINT history_items_finite_timestamps CHECK (isfinite(created_at) AND isfinite(updated_at)),
	CONSTRAINT history_items_timestamp_order CHECK (updated_at >= created_at),
	CONSTRAINT history_items_no_credentials CHECK (
		(inline_text IS NULL OR NOT decodex.has_credential_material(inline_text))
		AND NOT decodex.has_credential_material(media_type)
		AND NOT decodex.has_credential_material(metadata)
	),
	CONSTRAINT history_items_artifact_shape CHECK (
		(kind = 'artifact') = (artifact_id IS NOT NULL AND artifact_revision IS NOT NULL)
	),
	CONSTRAINT history_items_turn_fk FOREIGN KEY (turn_id, conversation_id)
		REFERENCES decodex.turns(turn_id, conversation_id) ON DELETE RESTRICT,
	CONSTRAINT history_items_artifact_fk FOREIGN KEY (artifact_id, conversation_id, artifact_revision)
		REFERENCES decodex.artifact_revisions(artifact_id, conversation_id, revision) ON DELETE RESTRICT,
	UNIQUE (turn_id, ordinal),
	UNIQUE (conversation_id, history_position),
	UNIQUE (conversation_id, history_position, history_item_id)
);

CREATE TABLE decodex.history_item_versions (
	version_sequence bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	history_item_id uuid NOT NULL,
	conversation_id uuid NOT NULL,
	history_position bigint NOT NULL CHECK (history_position > 0),
	turn_id uuid NOT NULL,
	ordinal integer NOT NULL CHECK (ordinal BETWEEN 0 AND 1000000),
	kind decodex.history_item_kind NOT NULL,
	status decodex.history_item_status NOT NULL,
	inline_text text CHECK (inline_text IS NULL OR octet_length(inline_text) <= 16384),
	blob_hash text REFERENCES decodex.blob_objects(blob_hash) ON DELETE RESTRICT,
	media_type text NOT NULL CHECK (decodex.is_canonical_media_type(media_type)),
	metadata jsonb NOT NULL CHECK (decodex.is_history_metadata_projection(metadata)),
	artifact_id uuid,
	artifact_revision bigint,
	revision bigint NOT NULL CHECK (revision > 0),
	captured_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT history_item_versions_payload_shape CHECK (
		(inline_text IS NOT NULL AND blob_hash IS NULL)
		OR (inline_text IS NULL AND blob_hash IS NOT NULL)
	),
	CONSTRAINT history_item_versions_finite_timestamp CHECK (isfinite(captured_at)),
	CONSTRAINT history_item_versions_artifact_shape CHECK (
		(kind = 'artifact') = (artifact_id IS NOT NULL AND artifact_revision IS NOT NULL)
	),
	CONSTRAINT history_item_versions_turn_fk FOREIGN KEY (turn_id, conversation_id)
		REFERENCES decodex.turns(turn_id, conversation_id) ON DELETE RESTRICT,
	CONSTRAINT history_item_versions_artifact_fk FOREIGN KEY (
		artifact_id, conversation_id, artifact_revision
	) REFERENCES decodex.artifact_revisions(artifact_id, conversation_id, revision) ON DELETE RESTRICT,
	UNIQUE (history_item_id, revision),
	UNIQUE (conversation_id, history_position, version_sequence)
);

CREATE TABLE decodex.history_cursors (
	cursor_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	snapshot_high_water bigint NOT NULL CHECK (snapshot_high_water > 0),
	snapshot_version_sequence bigint NOT NULL CHECK (snapshot_version_sequence > 0),
	page_size integer NOT NULL CHECK (page_size BETWEEN 1 AND 100),
	last_position bigint NOT NULL CHECK (
		last_position > 0 AND last_position < snapshot_high_water
	),
	history_item_id uuid NOT NULL,
	parent_cursor_id uuid REFERENCES decodex.history_cursors(cursor_id) ON DELETE CASCADE,
	created_at timestamptz NOT NULL,
	expires_at timestamptz NOT NULL,
	CONSTRAINT history_cursors_finite_timestamps CHECK (
		isfinite(created_at) AND isfinite(expires_at)
	),
	CONSTRAINT history_cursors_expiry_order CHECK (
		expires_at > created_at AND expires_at <= created_at + interval '1 hour 1 second'
	),
	CONSTRAINT history_cursors_item_fk FOREIGN KEY (
		conversation_id, last_position, history_item_id
	) REFERENCES decodex.history_items(conversation_id, history_position, history_item_id)
		ON DELETE RESTRICT,
	CONSTRAINT history_cursors_boundary_key UNIQUE NULLS NOT DISTINCT (
		conversation_id, snapshot_high_water, snapshot_version_sequence, page_size, last_position,
		history_item_id, parent_cursor_id
	)
);

CREATE TABLE decodex.context_packs (
	context_pack_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	pack_revision bigint NOT NULL CHECK (pack_revision > 0),
	compiled_digest text NOT NULL CHECK (compiled_digest ~ '^[0-9a-f]{64}$'),
	manifest_digest text NOT NULL CHECK (manifest_digest ~ '^[0-9a-f]{64}$'),
	inline_bytes bytea CHECK (inline_bytes IS NULL OR octet_length(inline_bytes) <= 16384),
	blob_hash text REFERENCES decodex.blob_objects(blob_hash) ON DELETE RESTRICT,
	byte_length bigint NOT NULL CHECK (byte_length BETWEEN 1 AND 262144),
	max_bytes integer NOT NULL CHECK (max_bytes BETWEEN 1024 AND 262144),
	recent_item_limit integer NOT NULL CHECK (recent_item_limit BETWEEN 1 AND 256),
	possible_side_effects decodex.side_effect_state NOT NULL,
	truncated boolean NOT NULL,
	omitted_source_count integer NOT NULL CHECK (omitted_source_count >= 0),
	source_count integer NOT NULL CHECK (source_count BETWEEN 1 AND 512),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT context_packs_payload_shape CHECK (
		(inline_bytes IS NOT NULL AND blob_hash IS NULL)
		OR (inline_bytes IS NULL AND blob_hash IS NOT NULL)
	),
	CONSTRAINT context_packs_finite_created_at CHECK (isfinite(created_at)),
	UNIQUE (context_pack_id, conversation_id),
	UNIQUE (conversation_id, pack_revision)
);

CREATE TABLE decodex.context_pack_sources (
	context_pack_id uuid NOT NULL,
	conversation_id uuid NOT NULL,
	position integer NOT NULL CHECK (position >= 0 AND position < 512),
	kind decodex.context_source_kind NOT NULL,
	source_id text NOT NULL CHECK (octet_length(source_id) BETWEEN 1 AND 256),
	source_revision bigint NOT NULL CHECK (source_revision > 0),
	content_digest text NOT NULL CHECK (content_digest ~ '^[0-9a-f]{64}$'),
	original_byte_length bigint NOT NULL CHECK (original_byte_length BETWEEN 0 AND 2097152),
	included_byte_length bigint NOT NULL CHECK (
		included_byte_length >= 0 AND included_byte_length <= original_byte_length
	),
	included_digest text NOT NULL CHECK (included_digest ~ '^[0-9a-f]{64}$'),
	disposition decodex.context_source_disposition NOT NULL,
	artifact_id uuid,
	artifact_revision bigint,
	CONSTRAINT context_pack_sources_no_credentials CHECK (
		NOT decodex.has_credential_material(source_id)
	),
	CONSTRAINT context_pack_sources_disposition_shape CHECK (
		(disposition = 'omitted' AND included_byte_length = 0
			AND included_digest = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
		OR (disposition = 'complete' AND original_byte_length > 0
			AND included_byte_length = original_byte_length AND included_digest = content_digest)
		OR (disposition = 'truncated' AND included_byte_length > 0
			AND included_byte_length < original_byte_length)
	),
	CONSTRAINT context_pack_sources_artifact_shape CHECK (
		(kind = 'artifact') = (artifact_id IS NOT NULL AND artifact_revision IS NOT NULL)
		AND (kind <> 'artifact' OR (source_id = artifact_id::text AND source_revision = artifact_revision))
	),
	CONSTRAINT context_pack_sources_pinned_shape CHECK (
		(position = 0) = (kind = 'pinned_revision')
		AND (position <> 0 OR (disposition = 'complete' AND included_byte_length > 0))
	),
	CONSTRAINT context_pack_sources_pack_fk FOREIGN KEY (context_pack_id, conversation_id)
		REFERENCES decodex.context_packs(context_pack_id, conversation_id)
		ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT context_pack_sources_artifact_fk FOREIGN KEY (
		artifact_id, conversation_id, artifact_revision
	) REFERENCES decodex.artifact_revisions(artifact_id, conversation_id, revision) ON DELETE RESTRICT,
	PRIMARY KEY (context_pack_id, position)
);

CREATE TABLE decodex.transition_proposals (
	transition_id uuid PRIMARY KEY,
	conversation_id uuid NOT NULL REFERENCES decodex.conversations(conversation_id) ON DELETE RESTRICT,
	from_runtime_session_id uuid NOT NULL,
	context_pack_id uuid NOT NULL,
	kind decodex.transition_kind NOT NULL,
	reason text NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 512),
	dispatch_enabled boolean NOT NULL DEFAULT false CHECK (NOT dispatch_enabled),
	created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
	CONSTRAINT transition_proposals_finite_created_at CHECK (isfinite(created_at)),
	CONSTRAINT transition_proposals_no_credentials CHECK (NOT decodex.has_credential_material(reason)),
	CONSTRAINT transition_proposals_session_fk FOREIGN KEY (from_runtime_session_id, conversation_id)
		REFERENCES decodex.runtime_sessions(runtime_session_id, conversation_id) ON DELETE RESTRICT,
	CONSTRAINT transition_proposals_pack_fk FOREIGN KEY (context_pack_id, conversation_id)
		REFERENCES decodex.context_packs(context_pack_id, conversation_id) ON DELETE RESTRICT,
	UNIQUE (from_runtime_session_id, context_pack_id, kind)
);

CREATE INDEX runtime_sessions_conversation_idx
	ON decodex.runtime_sessions (conversation_id, created_at, runtime_session_id);
CREATE INDEX turns_conversation_sequence_idx
	ON decodex.turns (conversation_id, sequence, turn_id);
CREATE INDEX history_items_turn_order_idx
	ON decodex.history_items (turn_id, ordinal, history_item_id);
CREATE INDEX history_items_conversation_position_idx
	ON decodex.history_items (conversation_id, history_position);
CREATE INDEX history_cursors_expiry_idx
	ON decodex.history_cursors (expires_at, cursor_id);
CREATE INDEX artifacts_conversation_idx
	ON decodex.artifacts (conversation_id, created_at, artifact_id);
CREATE INDEX context_packs_conversation_revision_idx
	ON decodex.context_packs (conversation_id, pack_revision DESC);
CREATE INDEX transition_proposals_conversation_idx
	ON decodex.transition_proposals (conversation_id, created_at, transition_id);

-- PostgreSQL locks an UPDATE target tuple before invoking its row-level BEFORE trigger.
-- Every hierarchy-mutating statement therefore takes the coordinator in a statement-level
-- BEFORE trigger, before the executor can identify or lock any target tuple. Row triggers may
-- subsequently lock child/parent rows, but never acquire this outer coordinator themselves.
CREATE FUNCTION decodex.acquire_hierarchy_coordinator() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	RETURN NULL;
END;
$$;

CREATE FUNCTION decodex.enforce_conversation_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.status <> 'open' OR NEW.revision <> 1 THEN
			RAISE EXCEPTION 'conversation must be created open at revision 1';
		END IF;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		RETURN NEW;
	END IF;
	IF OLD.status = 'archived' THEN
		RAISE EXCEPTION 'archived conversation is immutable';
	END IF;
	IF NEW.conversation_id <> OLD.conversation_id
		OR NEW.title IS DISTINCT FROM OLD.title
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1
		OR OLD.status <> 'open' OR NEW.status <> 'archived' THEN
		RAISE EXCEPTION 'illegal conversation transition';
	END IF;
	IF NEW.status = 'archived' AND EXISTS (
		SELECT 1 FROM decodex.runtime_sessions WHERE conversation_id = NEW.conversation_id
		AND state IN ('starting', 'active')
	) THEN RAISE EXCEPTION 'conversation has nonterminal runtime sessions'; END IF;
	IF NEW.status = 'archived' AND EXISTS (
		SELECT 1 FROM decodex.artifacts WHERE conversation_id = NEW.conversation_id
		AND status = 'active'
	) THEN RAISE EXCEPTION 'conversation has active Artifacts'; END IF;
	NEW.updated_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_runtime_session_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE parent_status decodex.conversation_status;
DECLARE transition_time timestamptz;
BEGIN
	SELECT status INTO parent_status FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	IF parent_status <> 'open' THEN
		RAISE EXCEPTION 'runtime session requires an open conversation';
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.state NOT IN ('starting', 'active') OR NEW.revision <> 1 THEN
			RAISE EXCEPTION 'illegal initial runtime session state';
		END IF;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		NEW.ended_at := NULL;
		RETURN NEW;
	END IF;
	IF OLD.state IN ('ended', 'diverged') THEN
		RAISE EXCEPTION 'terminal runtime session is immutable';
	END IF;
	IF NEW.runtime_session_id <> OLD.runtime_session_id
		OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.profile_snapshot_id <> OLD.profile_snapshot_id
		OR NEW.account_snapshot_id <> OLD.account_snapshot_id
		OR NEW.codex_thread_id IS DISTINCT FROM OLD.codex_thread_id
		OR NEW.last_known_turn_id IS DISTINCT FROM OLD.last_known_turn_id
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1
		OR NOT ((OLD.state = 'starting' AND NEW.state IN ('active', 'ended', 'diverged'))
			OR (OLD.state = 'active' AND NEW.state IN ('ended', 'diverged'))) THEN
		RAISE EXCEPTION 'illegal runtime session transition';
	END IF;
	IF NEW.state IN ('ended', 'diverged') AND EXISTS (
		SELECT 1 FROM decodex.turns WHERE runtime_session_id = NEW.runtime_session_id
		AND status = 'active'
	) THEN RAISE EXCEPTION 'runtime session has active turns'; END IF;
	transition_time := pg_catalog.clock_timestamp();
	NEW.updated_at := transition_time;
	NEW.ended_at := CASE WHEN NEW.state IN ('ended', 'diverged') THEN transition_time ELSE NULL END;
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_turn_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE parent_state decodex.runtime_session_state;
DECLARE parent_status decodex.conversation_status;
DECLARE transition_time timestamptz;
BEGIN
	SELECT c.status, rs.state INTO parent_status, parent_state
		FROM decodex.conversations c JOIN decodex.runtime_sessions rs
		ON rs.conversation_id = c.conversation_id
		WHERE c.conversation_id = NEW.conversation_id
			AND rs.runtime_session_id = NEW.runtime_session_id FOR UPDATE OF c, rs;
	IF TG_OP = 'INSERT' THEN
		IF parent_status <> 'open' OR parent_state <> 'active'
			OR NEW.status <> 'active' OR NEW.revision <> 1 THEN
			RAISE EXCEPTION 'turn requires an active parent';
		END IF;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		NEW.completed_at := NULL;
		RETURN NEW;
	END IF;
	IF OLD.status IN ('completed', 'failed') THEN RAISE EXCEPTION 'terminal turn is immutable'; END IF;
	IF NEW.turn_id <> OLD.turn_id OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.runtime_session_id <> OLD.runtime_session_id OR NEW.sequence <> OLD.sequence
		OR NEW.role <> OLD.role OR NEW.possible_side_effects <> OLD.possible_side_effects
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1 OR NEW.status NOT IN ('completed', 'failed') THEN
		RAISE EXCEPTION 'illegal turn transition';
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.history_items WHERE turn_id = NEW.turn_id
		AND status = 'streaming') THEN RAISE EXCEPTION 'turn has streaming items'; END IF;
	transition_time := pg_catalog.clock_timestamp();
	NEW.updated_at := transition_time;
	NEW.completed_at := transition_time;
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_history_item_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE turn_state decodex.turn_status;
DECLARE session_state decodex.runtime_session_state;
DECLARE conversation_state decodex.conversation_status;
DECLARE artifact_state decodex.artifact_status;
DECLARE next_position bigint;
BEGIN
	SELECT t.status, rs.state, c.status INTO turn_state, session_state, conversation_state
		FROM decodex.turns t JOIN decodex.runtime_sessions rs
		ON (rs.runtime_session_id, rs.conversation_id) = (t.runtime_session_id, t.conversation_id)
		JOIN decodex.conversations c ON c.conversation_id = t.conversation_id
		WHERE (t.turn_id, t.conversation_id) = (NEW.turn_id, NEW.conversation_id)
			FOR UPDATE OF c, rs, t;
	IF turn_state <> 'active' OR session_state <> 'active' OR conversation_state <> 'open' THEN
		RAISE EXCEPTION 'history write requires active parents';
	END IF;
	IF NEW.kind = 'artifact' THEN
		SELECT status INTO artifact_state FROM decodex.artifacts
			WHERE (artifact_id, conversation_id) = (NEW.artifact_id, NEW.conversation_id)
			FOR UPDATE;
		IF artifact_state <> 'active' THEN
			RAISE EXCEPTION 'history Artifact reference requires active Artifact';
		END IF;
	END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.revision <> 1 THEN RAISE EXCEPTION 'history item must start at revision 1'; END IF;
		SELECT COALESCE(max(history_position), 0) + 1 INTO next_position
			FROM decodex.history_items WHERE conversation_id = NEW.conversation_id;
		NEW.history_position := next_position;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		RETURN NEW;
	END IF;
	IF OLD.status IN ('completed', 'failed') THEN RAISE EXCEPTION 'terminal history item is immutable'; END IF;
	IF NEW.history_item_id <> OLD.history_item_id OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.history_position <> OLD.history_position OR NEW.turn_id <> OLD.turn_id
		OR NEW.ordinal <> OLD.ordinal OR NEW.kind <> OLD.kind
		OR NEW.artifact_id IS DISTINCT FROM OLD.artifact_id
		OR NEW.artifact_revision IS DISTINCT FROM OLD.artifact_revision
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1 THEN
		RAISE EXCEPTION 'illegal history item transition';
	END IF;
	NEW.updated_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.capture_history_item_version() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
BEGIN
	INSERT INTO decodex.history_item_versions (
		history_item_id, conversation_id, history_position, turn_id, ordinal, kind, status,
		inline_text, blob_hash, media_type, metadata, artifact_id, artifact_revision, revision,
		captured_at
	) VALUES (
		NEW.history_item_id, NEW.conversation_id, NEW.history_position, NEW.turn_id, NEW.ordinal,
		NEW.kind, NEW.status, NEW.inline_text, NEW.blob_hash, NEW.media_type, NEW.metadata,
		NEW.artifact_id, NEW.artifact_revision, NEW.revision, pg_catalog.clock_timestamp()
	);
	RETURN NULL;
END;
$$;

CREATE FUNCTION decodex.enforce_artifact_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE parent_status decodex.conversation_status;
DECLARE predecessor_status decodex.artifact_status;
BEGIN
	SELECT status INTO parent_status FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	IF parent_status <> 'open' THEN RAISE EXCEPTION 'artifact requires open conversation'; END IF;
	IF TG_OP = 'INSERT' THEN
		IF NEW.status <> 'active' OR NEW.revision <> 1 THEN RAISE EXCEPTION 'illegal initial artifact state'; END IF;
		NEW.created_at := pg_catalog.clock_timestamp();
		NEW.updated_at := NEW.created_at;
		RETURN NEW;
	END IF;
	IF OLD.status = 'deleted' THEN RAISE EXCEPTION 'deleted artifact is immutable'; END IF;
	SELECT status INTO predecessor_status FROM decodex.artifact_revisions
		WHERE (artifact_id, conversation_id, revision) =
			(OLD.artifact_id, OLD.conversation_id, OLD.revision);
	IF predecessor_status IS DISTINCT FROM OLD.status THEN
		RAISE EXCEPTION 'Artifact advance requires its exact immutable current revision';
	END IF;
	IF NEW.artifact_id <> OLD.artifact_id OR NEW.conversation_id <> OLD.conversation_id
		OR NEW.created_at IS DISTINCT FROM OLD.created_at
		OR NEW.revision <> OLD.revision + 1
		OR NOT ((OLD.status = 'active' AND NEW.status IN ('expired', 'deleted'))
			OR (OLD.status = 'expired' AND NEW.status = 'deleted')) THEN
		RAISE EXCEPTION 'illegal artifact transition';
	END IF;
	NEW.updated_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_artifact_revision_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE parent_status decodex.conversation_status;
DECLARE artifact_state decodex.artifact_status;
DECLARE artifact_revision bigint;
DECLARE predecessor_status decodex.artifact_status;
BEGIN
	SELECT c.status, a.status, a.revision INTO parent_status, artifact_state, artifact_revision
		FROM decodex.conversations c JOIN decodex.artifacts a
		ON a.conversation_id = c.conversation_id
		WHERE (a.artifact_id, a.conversation_id) = (NEW.artifact_id, NEW.conversation_id)
		FOR UPDATE OF c, a;
	IF parent_status <> 'open' OR NEW.status <> artifact_state
		OR NEW.revision <> artifact_revision THEN
		RAISE EXCEPTION 'Artifact revision requires eligible matching parents';
	END IF;
	IF NEW.revision = 1 THEN
		IF NEW.status <> 'active' THEN
			RAISE EXCEPTION 'initial Artifact revision must be active';
		END IF;
	ELSE
		SELECT status INTO predecessor_status FROM decodex.artifact_revisions
			WHERE (artifact_id, conversation_id, revision) =
				(NEW.artifact_id, NEW.conversation_id, NEW.revision - 1);
		IF predecessor_status IS NULL
			OR NOT ((predecessor_status = 'active' AND NEW.status IN ('expired', 'deleted'))
				OR (predecessor_status = 'expired' AND NEW.status = 'deleted')) THEN
			RAISE EXCEPTION 'Artifact revision requires its exact legal immutable predecessor';
		END IF;
	END IF;
	NEW.created_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_context_pack_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE parent_status decodex.conversation_status;
DECLARE actual_source_count bigint;
DECLARE first_position integer;
DECLARE last_position integer;
BEGIN
	IF TG_OP <> 'INSERT' THEN RAISE EXCEPTION 'Context Pack is immutable'; END IF;
	SELECT status INTO parent_status FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	IF parent_status <> 'open' THEN RAISE EXCEPTION 'Context Pack requires open conversation'; END IF;
	SELECT count(*), min(position), max(position)
		INTO actual_source_count, first_position, last_position
		FROM decodex.context_pack_sources
		WHERE (context_pack_id, conversation_id) = (NEW.context_pack_id, NEW.conversation_id);
	IF actual_source_count <> NEW.source_count OR first_position <> 0
		OR last_position <> NEW.source_count - 1 THEN
		RAISE EXCEPTION 'Context Pack requires one exact contiguous source manifest';
	END IF;
	NEW.created_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_context_pack_source_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE artifact_state decodex.artifact_status;
DECLARE conversation_state decodex.conversation_status;
DECLARE pack_exists boolean;
BEGIN
	IF TG_OP <> 'INSERT' THEN RAISE EXCEPTION 'Context Pack source is immutable'; END IF;
	SELECT EXISTS (
		SELECT 1 FROM decodex.context_packs
		WHERE (context_pack_id, conversation_id) = (NEW.context_pack_id, NEW.conversation_id)
	) INTO pack_exists;
	IF pack_exists THEN RAISE EXCEPTION 'Context Pack source manifest is sealed'; END IF;
	SELECT status INTO conversation_state FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	IF conversation_state <> 'open' THEN
		RAISE EXCEPTION 'Context Pack source requires open Conversation';
	END IF;
	IF NEW.kind = 'artifact' THEN
		SELECT status INTO artifact_state FROM decodex.artifacts
			WHERE (artifact_id, conversation_id) = (NEW.artifact_id, NEW.conversation_id)
			FOR UPDATE;
		IF artifact_state <> 'active' THEN
			RAISE EXCEPTION 'Context Pack Artifact source requires active Artifact';
		END IF;
	END IF;
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_history_cursor_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE current_high_water bigint;
DECLARE current_version_sequence bigint;
DECLARE parent_conversation_id uuid;
DECLARE parent_high_water bigint;
DECLARE parent_version_sequence bigint;
DECLARE parent_position bigint;
DECLARE parent_page_size integer;
DECLARE parent_expires_at timestamptz;
DECLARE canonical_position bigint;
DECLARE canonical_item_id uuid;
DECLARE insertion_time timestamptz := pg_catalog.clock_timestamp();
BEGIN
	PERFORM 1 FROM decodex.conversations
		WHERE conversation_id = NEW.conversation_id FOR UPDATE;
	SELECT COALESCE(max(history_position), 0) INTO current_high_water
		FROM decodex.history_items WHERE conversation_id = NEW.conversation_id;
	SELECT COALESCE(max(version_sequence), 0) INTO current_version_sequence
		FROM decodex.history_item_versions WHERE conversation_id = NEW.conversation_id;
	IF NEW.parent_cursor_id IS NULL THEN
		IF NEW.snapshot_high_water <> current_high_water
			OR NEW.snapshot_version_sequence <> current_version_sequence THEN
			RAISE EXCEPTION 'root history cursor requires the current snapshot boundary';
		END IF;
	ELSE
		SELECT conversation_id, snapshot_high_water, snapshot_version_sequence,
				last_position, page_size, expires_at
			INTO parent_conversation_id, parent_high_water, parent_version_sequence, parent_position,
				parent_page_size, parent_expires_at
			FROM decodex.history_cursors WHERE cursor_id = NEW.parent_cursor_id;
		IF parent_conversation_id IS DISTINCT FROM NEW.conversation_id
			OR parent_high_water IS DISTINCT FROM NEW.snapshot_high_water
			OR parent_version_sequence IS DISTINCT FROM NEW.snapshot_version_sequence
			OR parent_position IS NULL OR NEW.last_position <= parent_position
			OR parent_page_size IS DISTINCT FROM NEW.page_size
			OR parent_expires_at <= insertion_time
			OR NEW.expires_at IS DISTINCT FROM parent_expires_at THEN
			RAISE EXCEPTION 'continued history cursor requires one issued earlier boundary';
		END IF;
	END IF;
	IF NEW.created_at > insertion_time
		OR NEW.created_at < insertion_time - interval '1 minute'
		OR (NEW.parent_cursor_id IS NULL
			AND NEW.expires_at IS DISTINCT FROM NEW.created_at + interval '1 hour') THEN
		RAISE EXCEPTION 'history cursor timestamps are not canonical';
	END IF;
	SELECT history_position, history_item_id INTO canonical_position, canonical_item_id FROM (
		SELECT DISTINCT ON (history_position) history_position, history_item_id
		FROM decodex.history_item_versions
		WHERE conversation_id = NEW.conversation_id
			AND version_sequence <= NEW.snapshot_version_sequence
		ORDER BY history_position, version_sequence DESC
	) AS snapshot_items
		WHERE history_position > COALESCE(parent_position, 0)
			AND history_position <= NEW.snapshot_high_water
		ORDER BY history_position
		OFFSET NEW.page_size - 1 LIMIT 1;
	IF canonical_position IS DISTINCT FROM NEW.last_position
		OR canonical_item_id IS DISTINCT FROM NEW.history_item_id THEN
		RAISE EXCEPTION 'history cursor is not one canonical issued page boundary';
	END IF;
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.prune_history_snapshots()
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE maintenance_time timestamptz := pg_catalog.clock_timestamp();
DECLARE removed_count bigint;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock(1272);
	DELETE FROM decodex.history_cursors WHERE expires_at <= maintenance_time;
	WITH obsolete AS (
		SELECT candidate.version_sequence
		FROM decodex.history_item_versions AS candidate
		WHERE NOT EXISTS (
			SELECT 1 FROM decodex.history_items AS current
			WHERE current.history_item_id = candidate.history_item_id
				AND current.revision = candidate.revision
		) AND NOT EXISTS (
			SELECT 1 FROM decodex.command_receipts AS receipt
			WHERE receipt.receipt_state = 'completed'
				AND receipt.response->>'kind' = 'history_item'
				AND receipt.response->>'history_item_id' = candidate.history_item_id::text
				AND (receipt.response->>'revision')::bigint = candidate.revision
		) AND NOT EXISTS (
			SELECT 1 FROM decodex.history_cursors AS cursor
			WHERE cursor.conversation_id = candidate.conversation_id
				AND cursor.expires_at > maintenance_time
				AND candidate.version_sequence = (
					SELECT max(snapshot.version_sequence)
					FROM decodex.history_item_versions AS snapshot
					WHERE snapshot.history_item_id = candidate.history_item_id
						AND snapshot.version_sequence <= cursor.snapshot_version_sequence
				)
		)
		ORDER BY candidate.version_sequence
		LIMIT 4096
	)
	DELETE FROM decodex.history_item_versions AS candidate
	USING obsolete
	WHERE candidate.version_sequence = obsolete.version_sequence;
	GET DIAGNOSTICS removed_count = ROW_COUNT;
	RETURN removed_count;
END;
$$;

CREATE FUNCTION decodex.issue_history_cursor(
	p_conversation_id uuid,
	p_parent_cursor_id uuid,
	p_page_size integer
)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE current_high_water bigint;
DECLARE v_snapshot_high_water bigint;
DECLARE v_snapshot_version_sequence bigint;
DECLARE start_position bigint := 0;
DECLARE boundary_position bigint;
DECLARE boundary_item_id uuid;
DECLARE issued_cursor_id uuid;
DECLARE parent_page_size integer;
DECLARE cursor_expiry timestamptz;
DECLARE issuance_time timestamptz := pg_catalog.clock_timestamp();
DECLARE cursor_created_at timestamptz;
DECLARE global_cursor_count bigint;
DECLARE conversation_cursor_count bigint;
BEGIN
	IF p_conversation_id IS NULL OR p_page_size NOT BETWEEN 1 AND 100 THEN
		RAISE EXCEPTION 'history cursor page size is invalid' USING ERRCODE = '22023';
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1272);
	PERFORM 1 FROM decodex.conversations
		WHERE conversation_id = p_conversation_id FOR UPDATE;
	IF NOT FOUND THEN
		RAISE EXCEPTION 'history cursor Conversation does not exist' USING ERRCODE = '22023';
	END IF;
	PERFORM decodex.prune_history_snapshots();
	SELECT COALESCE(max(history_position), 0) INTO current_high_water
		FROM decodex.history_items WHERE conversation_id = p_conversation_id;
	IF p_parent_cursor_id IS NULL THEN
		v_snapshot_high_water := current_high_water;
		SELECT COALESCE(max(version_sequence), 0) INTO v_snapshot_version_sequence
			FROM decodex.history_item_versions WHERE conversation_id = p_conversation_id;
		cursor_expiry := issuance_time + interval '1 hour';
	ELSE
		SELECT snapshot_high_water, snapshot_version_sequence, last_position, page_size, expires_at
			INTO v_snapshot_high_water, v_snapshot_version_sequence, start_position,
				parent_page_size, cursor_expiry
			FROM decodex.history_cursors
			WHERE cursor_id = p_parent_cursor_id
				AND conversation_id = p_conversation_id
				AND expires_at > issuance_time;
		IF v_snapshot_high_water IS NULL THEN
			RAISE EXCEPTION 'history cursor parent was not issued for this Conversation'
				USING ERRCODE = '22023';
		END IF;
		IF p_page_size <> parent_page_size THEN
			RAISE EXCEPTION 'history cursor page size must match its issued parent'
				USING ERRCODE = '22023';
		END IF;
	END IF;
	cursor_created_at := CASE
		WHEN issuance_time > cursor_expiry - interval '1 hour' THEN issuance_time
		ELSE cursor_expiry - interval '1 hour'
	END;
	SELECT history_position, history_item_id INTO boundary_position, boundary_item_id FROM (
		SELECT DISTINCT ON (history_position) history_position, history_item_id
		FROM decodex.history_item_versions
		WHERE conversation_id = p_conversation_id
			AND version_sequence <= v_snapshot_version_sequence
		ORDER BY history_position, version_sequence DESC
	) AS snapshot_items
		WHERE history_position > start_position
			AND history_position <= v_snapshot_high_water
		ORDER BY history_position
		OFFSET p_page_size - 1 LIMIT 1;
	IF boundary_position IS NULL THEN
		RAISE EXCEPTION 'history cursor requires a canonical page boundary'
			USING ERRCODE = '22023';
	END IF;
	IF boundary_position >= v_snapshot_high_water THEN
		RAISE EXCEPTION 'history cursor requires another page in its snapshot'
			USING ERRCODE = '22023';
	END IF;
	SELECT cursor_id INTO issued_cursor_id FROM decodex.history_cursors AS cursor
	WHERE cursor.conversation_id = p_conversation_id
		AND cursor.snapshot_high_water = v_snapshot_high_water
		AND cursor.snapshot_version_sequence = v_snapshot_version_sequence
		AND cursor.page_size = p_page_size
		AND cursor.last_position = boundary_position
		AND cursor.history_item_id = boundary_item_id
		AND cursor.parent_cursor_id IS NOT DISTINCT FROM p_parent_cursor_id;
	IF issued_cursor_id IS NOT NULL THEN
		RETURN issued_cursor_id;
	END IF;
	SELECT count(*), count(*) FILTER (WHERE conversation_id = p_conversation_id)
		INTO global_cursor_count, conversation_cursor_count
		FROM decodex.history_cursors;
	IF global_cursor_count >= 4096 OR conversation_cursor_count >= 512 THEN
		RAISE EXCEPTION 'history cursor capacity is exhausted' USING ERRCODE = '54000';
	END IF;
	INSERT INTO decodex.history_cursors (
		conversation_id, snapshot_high_water, snapshot_version_sequence, page_size, last_position,
		history_item_id, parent_cursor_id, created_at, expires_at
	) VALUES (
		p_conversation_id, v_snapshot_high_water, v_snapshot_version_sequence,
		p_page_size, boundary_position,
		boundary_item_id, p_parent_cursor_id, cursor_created_at, cursor_expiry
	)
	RETURNING cursor_id INTO issued_cursor_id;
	RETURN issued_cursor_id;
END;
$$;

CREATE FUNCTION decodex.canonicalize_created_at() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	NEW.created_at := pg_catalog.clock_timestamp();
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_blob_object_state() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	NEW.created_at := pg_catalog.clock_timestamp();
	NEW.verified_at := NEW.created_at;
	RETURN NEW;
END;
$$;

CREATE TRIGGER conversations_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.conversations
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER conversations_state_guard BEFORE INSERT OR UPDATE ON decodex.conversations
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_conversation_state();
CREATE TRIGGER profile_snapshots_created_at_guard BEFORE INSERT ON decodex.profile_snapshots
	FOR EACH ROW EXECUTE FUNCTION decodex.canonicalize_created_at();
CREATE TRIGGER account_snapshots_created_at_guard BEFORE INSERT ON decodex.account_snapshots
	FOR EACH ROW EXECUTE FUNCTION decodex.canonicalize_created_at();
CREATE TRIGGER runtime_sessions_state_guard BEFORE INSERT OR UPDATE ON decodex.runtime_sessions
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_runtime_session_state();
CREATE TRIGGER runtime_sessions_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.runtime_sessions
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER blob_objects_state_guard BEFORE INSERT ON decodex.blob_objects
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_blob_object_state();
CREATE TRIGGER turns_state_guard BEFORE INSERT OR UPDATE ON decodex.turns
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_turn_state();
CREATE TRIGGER turns_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.turns
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER history_items_state_guard BEFORE INSERT OR UPDATE ON decodex.history_items
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_history_item_state();
CREATE TRIGGER history_items_version_capture AFTER INSERT OR UPDATE ON decodex.history_items
	FOR EACH ROW EXECUTE FUNCTION decodex.capture_history_item_version();
CREATE TRIGGER history_items_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.history_items
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER history_cursors_state_guard BEFORE INSERT ON decodex.history_cursors
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_history_cursor_state();
CREATE TRIGGER artifacts_state_guard BEFORE INSERT OR UPDATE ON decodex.artifacts
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_artifact_state();
CREATE TRIGGER artifacts_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.artifacts
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER artifact_revisions_state_guard BEFORE INSERT ON decodex.artifact_revisions
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_artifact_revision_state();
CREATE TRIGGER artifact_revisions_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.artifact_revisions
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER context_packs_state_guard BEFORE INSERT OR UPDATE OR DELETE ON decodex.context_packs
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_context_pack_state();
CREATE TRIGGER context_packs_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.context_packs
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER context_pack_sources_state_guard BEFORE INSERT OR UPDATE OR DELETE ON decodex.context_pack_sources
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_context_pack_source_state();
CREATE TRIGGER context_pack_sources_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.context_pack_sources
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();
CREATE TRIGGER transition_proposals_created_at_guard BEFORE INSERT ON decodex.transition_proposals
	FOR EACH ROW EXECUTE FUNCTION decodex.canonicalize_created_at();
CREATE TRIGGER transition_proposals_coordinator BEFORE INSERT OR UPDATE OR DELETE ON decodex.transition_proposals
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.acquire_hierarchy_coordinator();

ALTER FUNCTION decodex.normalize_unicode_whitespace(text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.ascii_lower(text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.has_credential_material(text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.has_credential_material(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_meaningful_evidence(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.rfc3339_utc(timestamptz) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_valid_operation_duration(interval) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_lease_operation_time() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_outbox_operation_time() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.forbid_mutation_of_activity() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_outbox_terminal_retention() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.forbid_outbox_truncate() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.lease_ttl_milliseconds(interval) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.try_acquire_lease(text, uuid, interval) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.renew_lease(text, uuid, uuid, interval) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.release_lease(text, uuid, uuid) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_canonical_media_type(text) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.is_history_metadata_projection(jsonb) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_command_receipt_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.acquire_hierarchy_coordinator() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_conversation_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_runtime_session_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_turn_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_history_item_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.capture_history_item_version() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_artifact_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_artifact_revision_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_context_pack_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_context_pack_source_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_history_cursor_state() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.prune_history_snapshots() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.issue_history_cursor(uuid, uuid, integer) SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.canonicalize_created_at() SET search_path = pg_catalog, decodex;
ALTER FUNCTION decodex.enforce_blob_object_state() SET search_path = pg_catalog, decodex;
REVOKE ALL ON FUNCTION decodex.issue_history_cursor(uuid, uuid, integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION decodex.prune_history_snapshots() FROM PUBLIC;
REVOKE ALL ON FUNCTION decodex.capture_history_item_version() FROM PUBLIC;
