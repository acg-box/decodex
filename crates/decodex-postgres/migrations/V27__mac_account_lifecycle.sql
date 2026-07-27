-- XY-1422 makes PostgreSQL the credential-negative account registry while the daemon coordinates
-- finite operations against a versioned macOS Keychain item. This is deliberately account-specific
-- state, not a general transaction or effect framework.

CREATE TABLE decodex.account_migration_receipts (
	singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
	manifest_sha256 text NOT NULL UNIQUE CHECK (manifest_sha256 ~ '^[0-9a-f]{64}$'),
	manifest jsonb NOT NULL,
	phase text NOT NULL CHECK (phase IN ('prepared','completed')),
	destination_receipt jsonb,
	retirement_receipt jsonb,
	account_count integer NOT NULL CHECK (account_count BETWEEN 0 AND 512),
	prepared_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	completed_at timestamptz,
	CHECK ((manifest->>'schema'='decodex/account-migration-manifest/1') IS TRUE),
	CHECK ((manifest->>'quota_policy'='reset_to_unknown') IS TRUE),
	CHECK ((manifest->>'usage_profile_policy'='start_empty') IS TRUE),
	CHECK ((manifest->>'history_policy'='do_not_import') IS TRUE),
	CHECK ((pg_catalog.jsonb_typeof(manifest->'decision_fingerprints')='object'
		AND manifest->'decision_fingerprints'=pg_catalog.jsonb_build_object(
			'credentials_sha256',manifest #> '{decision_fingerprints,credentials_sha256}',
			'labels_sha256',manifest #> '{decision_fingerprints,labels_sha256}',
			'enabled_sha256',manifest #> '{decision_fingerprints,enabled_sha256}',
			'routing_sha256',manifest #> '{decision_fingerprints,routing_sha256}',
			'provider_sha256',manifest #> '{decision_fingerprints,provider_sha256}',
			'quota_sha256',manifest #> '{decision_fingerprints,quota_sha256}',
			'usage_profile_sha256',manifest #> '{decision_fingerprints,usage_profile_sha256}',
			'history_sha256',manifest #> '{decision_fingerprints,history_sha256}'
		)) IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,credentials_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,labels_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,enabled_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,routing_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,provider_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,quota_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,usage_profile_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK (((manifest #>> '{decision_fingerprints,history_sha256}')
		~ '^[0-9a-f]{64}$') IS TRUE),
	CHECK ((pg_catalog.jsonb_typeof(manifest->'sources')='array'
		AND pg_catalog.jsonb_array_length(manifest->'sources')=4) IS TRUE),
	CHECK ((pg_catalog.jsonb_typeof(manifest->'accounts')='array'
		AND pg_catalog.jsonb_array_length(manifest->'accounts')=account_count) IS TRUE),
	CHECK ((phase='prepared' AND destination_receipt IS NULL AND retirement_receipt IS NULL
			AND completed_at IS NULL)
		OR (phase='completed' AND destination_receipt IS NOT NULL
			AND retirement_receipt IS NOT NULL AND completed_at IS NOT NULL)),
	CHECK (destination_receipt IS NULL OR (
		(destination_receipt->>'schema'='decodex/account-migration-destination/1') IS TRUE
		AND (pg_catalog.jsonb_typeof(destination_receipt->'accounts')='array'
			AND pg_catalog.jsonb_array_length(destination_receipt->'accounts')=account_count) IS TRUE
		AND (pg_catalog.jsonb_typeof(destination_receipt->'routing')='object'
			AND destination_receipt->'routing'=pg_catalog.jsonb_build_object(
				'mode',destination_receipt #> '{routing,mode}',
				'fixed_account_id',destination_receipt #> '{routing,fixed_account_id}',
				'order',destination_receipt #> '{routing,order}',
				'revision',destination_receipt #> '{routing,revision}'
			)) IS TRUE
		AND ((destination_receipt #>> '{routing,mode}') IN ('balanced','fixed')) IS TRUE
		AND ((destination_receipt #> '{routing,fixed_account_id}')='null'::jsonb)
			=(destination_receipt #>> '{routing,mode}'='balanced')
		AND (pg_catalog.jsonb_typeof(destination_receipt #> '{routing,order}')='array') IS TRUE
		AND ((destination_receipt #>> '{routing,revision}')::bigint > 0) IS TRUE
		AND (destination_receipt->>'keychain_verified')::boolean IS TRUE
		AND (destination_receipt->>'postgresql_verified')::boolean IS TRUE)),
	CHECK (retirement_receipt IS NULL OR (
		(retirement_receipt->>'schema'='decodex/account-runtime-retirement/1') IS TRUE
		AND (retirement_receipt->>'supervisor_profile'='postgres_and_daemon_only_v1') IS TRUE
		AND (retirement_receipt->>'legacy_source_untouched')::boolean IS TRUE
		AND (retirement_receipt->>'runtime_legacy_authority_removed')::boolean IS TRUE
		AND (retirement_receipt->>'final_config_swapped')::boolean IS TRUE
		AND (retirement_receipt->>'staging_secrets_retired')::boolean IS TRUE
		AND (retirement_receipt->>'active_legacy_sources_retired')::boolean IS TRUE
		AND (retirement_receipt->>'installed_assets_verified')::boolean IS TRUE)),
	CHECK (pg_catalog.octet_length(manifest::text) <= 1048576),
	CHECK (destination_receipt IS NULL
		OR pg_catalog.octet_length(destination_receipt::text) <= 262144),
	CHECK (retirement_receipt IS NULL
		OR pg_catalog.octet_length(retirement_receipt::text) <= 65536),
	CHECK (NOT decodex.has_credential_material(manifest)
		AND (destination_receipt IS NULL OR NOT decodex.has_credential_material(destination_receipt))
		AND (retirement_receipt IS NULL OR NOT decodex.has_credential_material(retirement_receipt))),
	CHECK (isfinite(prepared_at) AND (completed_at IS NULL OR isfinite(completed_at)))
);

CREATE FUNCTION decodex.record_account_migration_receipt_exact(
	p_manifest_sha256 text,p_manifest jsonb,p_destination_receipt jsonb,
	p_retirement_receipt jsonb,p_account_count integer
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE existing decodex.account_migration_receipts%ROWTYPE;
BEGIN
	SELECT * INTO existing FROM decodex.account_migration_receipts
	WHERE singleton FOR UPDATE;
	IF FOUND THEN
		IF (existing.manifest_sha256,existing.manifest,existing.account_count)
			IS DISTINCT FROM (p_manifest_sha256,p_manifest,p_account_count) THEN
			RETURN 'identity_conflict';
		END IF;
		IF p_destination_receipt IS NULL AND p_retirement_receipt IS NULL THEN
			RETURN 'replayed';
		END IF;
		IF p_destination_receipt IS NULL OR p_retirement_receipt IS NULL THEN
			RETURN 'identity_conflict';
		END IF;
		IF existing.phase='completed' THEN
			RETURN CASE WHEN (existing.destination_receipt,existing.retirement_receipt)
				IS NOT DISTINCT FROM (p_destination_receipt,p_retirement_receipt)
				THEN 'replayed' ELSE 'identity_conflict' END;
		END IF;
		UPDATE decodex.account_migration_receipts SET phase='completed',
			destination_receipt=p_destination_receipt,retirement_receipt=p_retirement_receipt,
			completed_at=pg_catalog.clock_timestamp() WHERE singleton;
		RETURN 'recorded';
	END IF;
	RETURN CASE
		WHEN p_destination_receipt IS NULL AND p_retirement_receipt IS NULL THEN 'absent'
		ELSE 'identity_conflict'
	END;
END
$$;

DO $$
DECLARE handoff_relation regclass;
DECLARE handoff_columns text[];
DECLARE handoff_types text[];
DECLARE handoff_not_null boolean[];
DECLARE handoff_count bigint;
DECLARE handoff_manifest_sha256 text;
DECLARE handoff_manifest jsonb;
DECLARE handoff_account_count integer;
DECLARE existing_receipt decodex.account_migration_receipts%ROWTYPE;
DECLARE receipt_result text;
DECLARE consumed_count bigint;
BEGIN
	handoff_relation:=pg_catalog.to_regclass('pg_temp.decodex_account_migration_handoff');
	IF handoff_relation IS NULL THEN
		IF EXISTS (SELECT 1 FROM decodex.accounts) THEN
			RAISE EXCEPTION 'populated V26 account migration requires an exact manifest handoff'
				USING ERRCODE='55000';
		END IF;
		RETURN;
	END IF;
	IF NOT EXISTS (
		SELECT 1 FROM pg_catalog.pg_class AS relation
		WHERE relation.oid=handoff_relation
			AND relation.relnamespace=pg_catalog.pg_my_temp_schema()
			AND relation.relkind='r' AND relation.relpersistence='t'
	) THEN
		RAISE EXCEPTION 'account migration handoff relation is invalid' USING ERRCODE='55000';
	END IF;
	SELECT pg_catalog.array_agg(attribute.attname::text ORDER BY attribute.attnum),
		pg_catalog.array_agg(
			pg_catalog.format_type(attribute.atttypid,attribute.atttypmod)
			ORDER BY attribute.attnum
		),
		pg_catalog.array_agg(attribute.attnotnull ORDER BY attribute.attnum)
	INTO handoff_columns,handoff_types,handoff_not_null
	FROM pg_catalog.pg_attribute AS attribute
	WHERE attribute.attrelid=handoff_relation
		AND attribute.attnum>0 AND NOT attribute.attisdropped;
	IF handoff_columns IS DISTINCT FROM
			ARRAY['manifest_sha256','manifest','account_count']::text[]
		OR handoff_types IS DISTINCT FROM ARRAY['text','jsonb','integer']::text[]
		OR handoff_not_null IS DISTINCT FROM ARRAY[true,true,true]::boolean[]
	THEN
		RAISE EXCEPTION 'account migration handoff shape is invalid' USING ERRCODE='55000';
	END IF;
	EXECUTE 'SELECT pg_catalog.count(*) FROM pg_temp.decodex_account_migration_handoff'
		INTO handoff_count;
	IF handoff_count<>1 THEN
		RAISE EXCEPTION 'account migration handoff cardinality is invalid' USING ERRCODE='55000';
	END IF;
	EXECUTE 'SELECT manifest_sha256,manifest,account_count
		FROM pg_temp.decodex_account_migration_handoff'
	INTO handoff_manifest_sha256,handoff_manifest,handoff_account_count;
	SELECT * INTO existing_receipt FROM decodex.account_migration_receipts
	WHERE singleton FOR UPDATE;
	IF FOUND THEN
		receipt_result:=CASE
			WHEN (existing_receipt.manifest_sha256,existing_receipt.manifest,
				existing_receipt.account_count) IS NOT DISTINCT FROM
				(handoff_manifest_sha256,handoff_manifest,handoff_account_count)
			THEN 'replayed'
			ELSE 'identity_conflict'
		END;
	ELSE
		INSERT INTO decodex.account_migration_receipts(
			manifest_sha256,manifest,phase,destination_receipt,retirement_receipt,account_count
		) VALUES (
			handoff_manifest_sha256,handoff_manifest,'prepared',NULL,NULL,
			handoff_account_count
		);
		receipt_result:='prepared';
	END IF;
	IF receipt_result NOT IN ('prepared','replayed') THEN
		RAISE EXCEPTION 'account migration handoff identity conflicts' USING ERRCODE='55000';
	END IF;
	EXECUTE 'DELETE FROM pg_temp.decodex_account_migration_handoff';
	GET DIAGNOSTICS consumed_count = ROW_COUNT;
	IF consumed_count<>1 THEN
		RAISE EXCEPTION 'account migration handoff consumption is invalid' USING ERRCODE='55000';
	END IF;
END
$$;

CREATE TYPE decodex.account_provider_kind AS ENUM ('chatgpt');
CREATE TYPE decodex.account_operation_kind AS ENUM ('enroll','import','refresh','logout');
CREATE TYPE decodex.account_operation_phase AS ENUM (
	'prepared','provider_effect_pending','store_applied','committed','cancelled','recovery_required'
);
CREATE TYPE decodex.account_selection_mode AS ENUM ('fixed','balanced');
CREATE TYPE decodex.account_store_observation AS ENUM (
	'unknown','exact','missing','mismatch','provider_mismatch','unavailable'
);
CREATE TYPE decodex.account_quota_observation_error AS ENUM (
	'provider_unavailable','protocol_unavailable','account_mismatch','unsupported_window'
);

ALTER TABLE decodex.command_receipts
	DROP CONSTRAINT command_receipts_protocol,
	ADD CONSTRAINT command_receipts_protocol CHECK (
		protocol_version IN ('decodex/store-command/1','decodex/account-command/1')
		AND operation ~ '^[a-z][a-z0-9_]{0,63}$'
		AND project_scope ~ '^[a-z][a-z0-9_]{0,31}$'
		AND octet_length(scope_id) BETWEEN 1 AND 256
		AND octet_length(entity_id) BETWEEN 1 AND 256
	);

DO $$
BEGIN
	IF (SELECT pg_catalog.count(*) FROM decodex.accounts) > 512 THEN
		RAISE EXCEPTION 'account registry cardinality exceeds the supported bound'
			USING ERRCODE='54000';
	END IF;
END
$$;

-- Administrative disablement becomes an independent column in this migration. Existing rows that
-- encoded it in observed health lose that stale observation and start as unknown.
UPDATE decodex.accounts SET state='unknown' WHERE state='disabled';

ALTER TABLE decodex.accounts
	ADD COLUMN enabled boolean NOT NULL DEFAULT false,
	ADD COLUMN provider_kind decodex.account_provider_kind,
	ADD COLUMN provider_account_id text,
	ADD COLUMN credential_store_schema_version integer,
	ADD COLUMN credential_version bigint,
	ADD COLUMN credential_fingerprint text,
	ADD COLUMN credential_writer_operation_id uuid,
	ADD COLUMN credential_store_observation decodex.account_store_observation NOT NULL DEFAULT 'unknown',
	ADD COLUMN credential_store_observed_at timestamptz,
	ADD COLUMN tombstoned_at timestamptz,
	ADD CONSTRAINT accounts_v27_display_label_shape CHECK (
		pg_catalog.octet_length(display_label) BETWEEN 1 AND 128
		AND display_label !~ '[[:cntrl:]]'
	),
	ADD CONSTRAINT accounts_observed_state_not_administrative CHECK (state<>'disabled'),
	ADD CONSTRAINT accounts_provider_identity_shape CHECK (
		(provider_kind IS NULL AND provider_account_id IS NULL)
		OR (provider_kind IS NOT NULL
			AND pg_catalog.octet_length(provider_account_id) >= 1
			AND pg_catalog.octet_length(provider_account_id) <= 512
			AND provider_account_id !~ '[[:cntrl:]]')
	),
	ADD CONSTRAINT accounts_credential_binding_shape CHECK (
		(credential_store_schema_version IS NULL
			AND credential_version IS NULL
			AND credential_fingerprint IS NULL
			AND credential_writer_operation_id IS NULL)
		OR (credential_store_schema_version = 1
			AND credential_version > 0
			AND credential_fingerprint ~ '^[0-9a-f]{64}$'
			AND credential_writer_operation_id IS NOT NULL
			AND provider_kind IS NOT NULL)
	),
	ADD CONSTRAINT accounts_tombstone_shape CHECK (
		(tombstoned_at IS NULL)
		OR (NOT enabled
			AND credential_store_schema_version IS NULL
			AND credential_version IS NULL
			AND credential_fingerprint IS NULL)
	),
	ADD CONSTRAINT accounts_store_observation_shape CHECK (
		(credential_store_observation='unknown')=(credential_store_observed_at IS NULL)
		AND (credential_store_observed_at IS NULL OR isfinite(credential_store_observed_at))
	),
	ADD CONSTRAINT accounts_lifecycle_no_credentials CHECK (
		(provider_account_id IS NULL OR NOT decodex.has_credential_material(provider_account_id))
		AND (credential_fingerprint IS NULL
			OR NOT decodex.has_credential_material(credential_fingerprint))
	);

CREATE UNIQUE INDEX accounts_provider_identity_unique
	ON decodex.accounts(provider_kind,provider_account_id)
	WHERE tombstoned_at IS NULL AND provider_kind IS NOT NULL;

CREATE TABLE decodex.account_operations (
	operation_id uuid PRIMARY KEY,
	account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	kind decodex.account_operation_kind NOT NULL,
	phase decodex.account_operation_phase NOT NULL DEFAULT 'prepared',
	expected_account_revision bigint,
	expected_store_schema_version integer,
	expected_credential_version bigint,
	expected_credential_fingerprint text,
	expected_credential_writer_operation_id uuid,
	target_store_schema_version integer,
	target_credential_version bigint,
	target_credential_fingerprint text,
	target_credential_writer_operation_id uuid,
	provider_kind decodex.account_provider_kind NOT NULL,
	provider_account_id text NOT NULL,
	recovery_code text,
	created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	completed_at timestamptz,
	requested_display_label text,
	requested_enabled boolean,
	CONSTRAINT account_operations_expected_binding_shape CHECK (
		(expected_store_schema_version IS NULL
			AND expected_credential_version IS NULL
			AND expected_credential_fingerprint IS NULL
			AND expected_credential_writer_operation_id IS NULL)
		OR (expected_store_schema_version = 1
			AND expected_credential_version > 0
			AND expected_credential_fingerprint ~ '^[0-9a-f]{64}$'
			AND expected_credential_writer_operation_id IS NOT NULL)
	),
	CONSTRAINT account_operations_target_binding_shape CHECK (
		(target_store_schema_version IS NULL
			AND target_credential_version IS NULL
			AND target_credential_fingerprint IS NULL
			AND target_credential_writer_operation_id IS NULL)
		OR (target_store_schema_version = 1
			AND target_credential_version > 0
			AND target_credential_fingerprint ~ '^[0-9a-f]{64}$'
			AND target_credential_writer_operation_id IS NOT NULL)
	),
	CONSTRAINT account_operations_provider_shape CHECK (
		pg_catalog.octet_length(provider_account_id) >= 1
		AND pg_catalog.octet_length(provider_account_id) <= 512
		AND provider_account_id !~ '[[:cntrl:]]'
		AND NOT decodex.has_credential_material(provider_account_id)
	),
	CONSTRAINT account_operations_install_request_shape CHECK (
		((kind IN ('enroll','import')
			AND requested_display_label IS NOT NULL AND requested_enabled IS NOT NULL)
		OR (kind NOT IN ('enroll','import')
			AND requested_display_label IS NULL AND requested_enabled IS NULL))
		AND (requested_display_label IS NULL OR (
			pg_catalog.octet_length(requested_display_label) BETWEEN 1 AND 128
			AND requested_display_label !~ '[[:cntrl:]]'
			AND NOT decodex.has_credential_material(requested_display_label)
		))
	),
	CONSTRAINT account_operations_recovery_shape CHECK (
		(phase='recovery_required')=(recovery_code IS NOT NULL)
		AND (recovery_code IS NULL OR recovery_code ~ '^[a-z][a-z0-9_]{0,127}$')
	),
	CONSTRAINT account_operations_terminal_time CHECK (
		(completed_at IS NOT NULL)=(phase IN ('committed','cancelled'))
	),
	CONSTRAINT account_operations_finite_times CHECK (
		isfinite(created_at) AND isfinite(updated_at)
		AND (completed_at IS NULL OR isfinite(completed_at))
	)
);
CREATE UNIQUE INDEX account_operations_one_unsettled_per_account
	ON decodex.account_operations(account_id)
	WHERE phase NOT IN ('committed','cancelled');

CREATE TABLE decodex.account_routing_control (
	singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
	mode decodex.account_selection_mode NOT NULL DEFAULT 'balanced',
	fixed_account_id uuid REFERENCES decodex.accounts(account_id),
	revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CHECK ((mode='fixed')=(fixed_account_id IS NOT NULL)),
	CHECK (isfinite(updated_at))
);
INSERT INTO decodex.account_routing_control(singleton) VALUES (true);

CREATE TABLE decodex.account_routing_order (
	account_id uuid PRIMARY KEY REFERENCES decodex.accounts(account_id),
	position integer NOT NULL UNIQUE CHECK (position >= 0),
	updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CHECK (isfinite(updated_at))
);

INSERT INTO decodex.account_routing_order(account_id,position)
SELECT account_id,pg_catalog.row_number() OVER (ORDER BY account_id)-1
FROM decodex.accounts;

CREATE TABLE decodex.account_quota_facts (
	account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	duration_minutes integer NOT NULL CHECK (duration_minutes IN (300,10080)),
	used_percent integer CHECK (used_percent >= 0 AND used_percent <= 100),
	resets_at_micros bigint,
	error_code decodex.account_quota_observation_error,
	observed_at_micros bigint NOT NULL CHECK (observed_at_micros > 0),
	PRIMARY KEY(account_id,duration_minutes),
	CHECK ((error_code IS NULL AND used_percent IS NOT NULL
			AND resets_at_micros > observed_at_micros)
		OR (error_code IS NOT NULL AND used_percent IS NULL AND resets_at_micros IS NULL))
);

CREATE TABLE decodex.codex_account_capability (
	singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
	build_identity text NOT NULL,
	executable_sha256 text NOT NULL CHECK (executable_sha256 ~ '^[0-9a-f]{64}$'),
	schema_sha256 text NOT NULL CHECK (schema_sha256 ~ '^[0-9a-f]{64}$'),
	callback_profile_sha256 text NOT NULL CHECK (callback_profile_sha256 ~ '^[0-9a-f]{64}$'),
	login_chatgpt_auth_tokens boolean NOT NULL,
	refresh_callback boolean NOT NULL,
	observed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	CHECK (pg_catalog.octet_length(build_identity) >= 1
		AND pg_catalog.octet_length(build_identity) <= 256),
	CHECK (isfinite(observed_at))
);

ALTER TABLE decodex.process_generations
	ADD COLUMN initial_account_revision bigint,
	ADD COLUMN credential_store_schema_version integer,
	ADD COLUMN credential_version bigint,
	ADD COLUMN credential_fingerprint text,
	ADD COLUMN credential_writer_operation_id uuid,
	ADD COLUMN provider_kind decodex.account_provider_kind,
	ADD COLUMN provider_account_id text,
	ADD COLUMN refresh_callback_profile_sha256 text,
	ADD CONSTRAINT process_generations_account_binding_shape CHECK (
		(initial_account_revision IS NULL
			AND credential_store_schema_version IS NULL
			AND credential_version IS NULL
			AND credential_fingerprint IS NULL
			AND credential_writer_operation_id IS NULL
			AND provider_kind IS NULL
			AND provider_account_id IS NULL
			AND refresh_callback_profile_sha256 IS NULL)
		OR (initial_account_revision > 0
			AND credential_store_schema_version = 1
			AND credential_version > 0
			AND credential_fingerprint ~ '^[0-9a-f]{64}$'
			AND credential_writer_operation_id IS NOT NULL
			AND provider_kind IS NOT NULL
			AND pg_catalog.octet_length(provider_account_id) >= 1
			AND pg_catalog.octet_length(provider_account_id) <= 512
			AND refresh_callback_profile_sha256 ~ '^[0-9a-f]{64}$')
	);

CREATE FUNCTION decodex.read_account_registry_exact(
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
					AND capability.build_identity='codex-cli 0.145.0-alpha.18'
					AND capability.executable_sha256='f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9'
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

CREATE FUNCTION decodex.read_reset_card_account_admission_exact(
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
			WHERE capability.singleton AND capability.login_chatgpt_auth_tokens
				AND capability.refresh_callback
				AND capability.callback_profile_sha256=p_callback_profile_sha256
		)
	FROM decodex.accounts AS account
	WHERE account.account_id=p_account_id
	FOR SHARE OF account;
END
$$;

CREATE FUNCTION decodex.prepare_account_operation_exact(
	p_operation_id uuid,p_account_id uuid,p_kind decodex.account_operation_kind,
	p_display_label text,p_enabled boolean,p_expected_account_revision bigint,
	p_expected_store_schema_version integer,p_expected_credential_version bigint,
	p_expected_credential_fingerprint text,p_expected_credential_writer_operation_id uuid,
	p_target_store_schema_version integer,p_target_credential_version bigint,
	p_target_credential_fingerprint text,p_target_credential_writer_operation_id uuid,
	p_provider_kind decodex.account_provider_kind,p_provider_account_id text
) RETURNS TABLE(result_code text,account_revision bigint,phase decodex.account_operation_phase)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE existing decodex.account_operations%ROWTYPE;
DECLARE account decodex.accounts%ROWTYPE;
BEGIN
	IF p_kind='logout' THEN
		PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
		PERFORM pg_catalog.pg_advisory_xact_lock(1271);
		PERFORM pg_catalog.pg_advisory_xact_lock(
			1401,pg_catalog.hashtext(p_account_id::text)
		);
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1422,pg_catalog.hashtext(p_account_id::text));
	SELECT * INTO existing FROM decodex.account_operations
	WHERE operation_id=p_operation_id FOR UPDATE;
	IF FOUND THEN
		IF (existing.account_id,existing.kind,existing.expected_account_revision,
			existing.expected_store_schema_version,existing.expected_credential_version,
			existing.expected_credential_fingerprint,
			existing.expected_credential_writer_operation_id,
			existing.target_store_schema_version,existing.target_credential_version,
			existing.target_credential_fingerprint,
			existing.target_credential_writer_operation_id,
			existing.provider_kind,existing.provider_account_id,
			existing.requested_display_label,existing.requested_enabled) IS DISTINCT FROM
			(p_account_id,p_kind,p_expected_account_revision,p_expected_store_schema_version,
			p_expected_credential_version,p_expected_credential_fingerprint,
			p_expected_credential_writer_operation_id,
			p_target_store_schema_version,p_target_credential_version,
			p_target_credential_fingerprint,p_target_credential_writer_operation_id,
			p_provider_kind,p_provider_account_id,p_display_label,p_enabled)
		THEN
			RETURN QUERY SELECT 'identity_conflict',0::bigint,existing.phase;
		ELSE
			SELECT * INTO account FROM decodex.accounts WHERE account_id=p_account_id;
			RETURN QUERY SELECT 'replayed',account.revision,existing.phase;
		END IF;
		RETURN;
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.account_operations AS operation
		WHERE operation.account_id=p_account_id
		AND operation.phase NOT IN ('committed','cancelled')) THEN
		RETURN QUERY SELECT 'operation_unsettled',0::bigint,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	IF p_kind IN ('enroll','import') AND (
		p_target_store_schema_version<>1 OR p_target_credential_version<>1
		OR p_target_credential_fingerprint !~ '^[0-9a-f]{64}$'
		OR p_target_credential_writer_operation_id IS DISTINCT FROM p_operation_id
	) THEN
		RETURN QUERY SELECT 'invalid_request',0::bigint,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	SELECT * INTO account FROM decodex.accounts WHERE account_id=p_account_id FOR UPDATE;
	IF p_kind IN ('enroll','import') AND NOT FOUND THEN
		PERFORM 1 FROM decodex.account_routing_control WHERE singleton FOR UPDATE;
		IF p_expected_account_revision IS NOT NULL
			OR p_enabled IS NULL
			OR p_display_label IS NULL OR pg_catalog.octet_length(p_display_label) < 1
			OR pg_catalog.octet_length(p_display_label) > 128
			OR p_display_label ~ '[[:cntrl:]]'
			OR decodex.has_credential_material(p_display_label)
			OR (SELECT pg_catalog.count(*) FROM decodex.accounts
				WHERE tombstoned_at IS NULL)>=512
		THEN
			RETURN QUERY SELECT 'invalid_request',0::bigint,'prepared'::decodex.account_operation_phase;
			RETURN;
		END IF;
		INSERT INTO decodex.accounts(
			account_id,display_label,state,enabled,provider_kind,provider_account_id
		) VALUES (
			p_account_id,p_display_label,'unknown',p_enabled,p_provider_kind,p_provider_account_id
		) RETURNING * INTO account;
		IF NOT (
			p_kind='import'
			AND EXISTS (
				SELECT 1 FROM decodex.account_migration_receipts AS receipt
				CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
					receipt.manifest->'accounts'
				) AS migration_account
				WHERE receipt.singleton AND receipt.phase='prepared'
					AND migration_account->>'account_id'=p_account_id::text
					AND migration_account->>'operation_id'=p_operation_id::text
			)
		) THEN
			INSERT INTO decodex.account_routing_order(account_id,position)
			SELECT p_account_id,COALESCE(pg_catalog.max(position)+1,0)
			FROM decodex.account_routing_order;
			UPDATE decodex.account_routing_control SET revision=revision+1,
				updated_at=pg_catalog.clock_timestamp() WHERE singleton;
		END IF;
	ELSIF p_kind='import' AND FOUND
		AND p_expected_account_revision=account.revision
		AND p_display_label=account.display_label
		AND p_enabled=account.enabled
		AND account.tombstoned_at IS NULL
		AND account.provider_kind IS NULL
		AND account.provider_account_id IS NULL
		AND account.credential_store_schema_version IS NULL
		AND account.credential_version IS NULL
		AND account.credential_fingerprint IS NULL
		AND account.credential_writer_operation_id IS NULL
	THEN
		NULL;
	ELSIF NOT FOUND THEN
		RETURN QUERY SELECT 'account_missing',0::bigint,'prepared'::decodex.account_operation_phase;
		RETURN;
	ELSIF account.revision IS DISTINCT FROM p_expected_account_revision
		OR account.tombstoned_at IS NOT NULL
		OR account.provider_kind IS DISTINCT FROM p_provider_kind
		OR account.provider_account_id IS DISTINCT FROM p_provider_account_id
		OR account.credential_store_schema_version IS DISTINCT FROM p_expected_store_schema_version
		OR account.credential_version IS DISTINCT FROM p_expected_credential_version
		OR account.credential_fingerprint IS DISTINCT FROM p_expected_credential_fingerprint
		OR account.credential_writer_operation_id IS DISTINCT FROM
			p_expected_credential_writer_operation_id
	THEN
		RETURN QUERY SELECT 'stale_account',account.revision,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	IF p_kind='logout' AND (
		EXISTS (SELECT 1 FROM decodex.process_generations AS generation
			WHERE generation.account_id=p_account_id AND generation.state<>'dead')
		OR EXISTS (SELECT 1 FROM decodex.provider_attempts AS attempt
			WHERE attempt.selected_account_id=p_account_id
				AND attempt.state IN ('prepared','dispatch_authorized','unknown'))
		OR EXISTS (SELECT 1 FROM decodex.outbox AS work
			WHERE work.aggregate_kind='reset_card_operation'
				AND work.payload #>> '{payload,account_id}'=p_account_id::text
				AND work.state IN ('pending','in_flight'))
	) THEN
		RETURN QUERY SELECT 'account_in_use',account.revision,
			'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	IF p_kind='refresh' AND p_target_credential_version IS NOT NULL
		AND (p_target_credential_version<>p_expected_credential_version+1
			OR p_target_credential_writer_operation_id IS DISTINCT FROM p_operation_id) THEN
		RETURN QUERY SELECT 'stale_account',account.revision,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	INSERT INTO decodex.account_operations(
		operation_id,account_id,kind,expected_account_revision,
		expected_store_schema_version,expected_credential_version,
		expected_credential_fingerprint,expected_credential_writer_operation_id,
		target_store_schema_version,target_credential_version,
		target_credential_fingerprint,target_credential_writer_operation_id,
		provider_kind,provider_account_id,requested_display_label,requested_enabled
	) VALUES (
		p_operation_id,p_account_id,p_kind,p_expected_account_revision,
		p_expected_store_schema_version,p_expected_credential_version,
		p_expected_credential_fingerprint,p_expected_credential_writer_operation_id,
		p_target_store_schema_version,p_target_credential_version,
		p_target_credential_fingerprint,p_target_credential_writer_operation_id,
		p_provider_kind,p_provider_account_id,p_display_label,p_enabled
	);
	RETURN QUERY SELECT 'prepared',account.revision,'prepared'::decodex.account_operation_phase;
END
$$;

CREATE FUNCTION decodex.set_account_operation_target_exact(
	p_operation_id uuid,p_target_store_schema_version integer,
	p_target_credential_version bigint,p_target_credential_fingerprint text,
	p_target_credential_writer_operation_id uuid
) RETURNS TABLE(result_code text,account_revision bigint,phase decodex.account_operation_phase)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE operation decodex.account_operations%ROWTYPE;
DECLARE current_revision bigint;
BEGIN
	SELECT * INTO operation FROM decodex.account_operations
	WHERE operation_id=p_operation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'operation_missing',0::bigint,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	SELECT revision INTO current_revision FROM decodex.accounts
	WHERE account_id=operation.account_id;
	IF operation.target_credential_version IS NOT NULL THEN
		IF (operation.target_store_schema_version,operation.target_credential_version,
			operation.target_credential_fingerprint,
			operation.target_credential_writer_operation_id) IS NOT DISTINCT FROM
			(p_target_store_schema_version,p_target_credential_version,
			p_target_credential_fingerprint,p_target_credential_writer_operation_id)
		THEN RETURN QUERY SELECT 'replayed',current_revision,operation.phase;
		ELSE RETURN QUERY SELECT 'identity_conflict',current_revision,operation.phase;
		END IF;
		RETURN;
	END IF;
	IF operation.kind<>'refresh' OR operation.phase<>'provider_effect_pending'
		OR p_target_store_schema_version<>1
		OR p_target_credential_version<>operation.expected_credential_version+1
		OR p_target_credential_fingerprint !~ '^[0-9a-f]{64}$'
		OR p_target_credential_writer_operation_id IS DISTINCT FROM p_operation_id
	THEN
		RETURN QUERY SELECT 'stale_operation',current_revision,operation.phase;
		RETURN;
	END IF;
	UPDATE decodex.account_operations SET
		target_store_schema_version=p_target_store_schema_version,
		target_credential_version=p_target_credential_version,
		target_credential_fingerprint=p_target_credential_fingerprint,
		target_credential_writer_operation_id=p_target_credential_writer_operation_id,
		updated_at=pg_catalog.clock_timestamp()
	WHERE operation_id=p_operation_id;
	RETURN QUERY SELECT 'advanced',current_revision,operation.phase;
END
$$;

CREATE FUNCTION decodex.advance_account_operation_exact(
	p_operation_id uuid,p_expected_phase decodex.account_operation_phase,
	p_target_phase decodex.account_operation_phase,p_recovery_code text
) RETURNS TABLE(result_code text,account_revision bigint,phase decodex.account_operation_phase)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE operation decodex.account_operations%ROWTYPE;
DECLARE next_revision bigint;
BEGIN
	SELECT * INTO operation FROM decodex.account_operations
	WHERE operation_id=p_operation_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'operation_missing',0::bigint,'prepared'::decodex.account_operation_phase;
		RETURN;
	END IF;
	IF operation.phase=p_target_phase THEN
		SELECT revision INTO next_revision FROM decodex.accounts WHERE account_id=operation.account_id;
		RETURN QUERY SELECT 'replayed',next_revision,operation.phase;
		RETURN;
	END IF;
	IF operation.phase<>p_expected_phase OR NOT (
		(p_expected_phase='prepared' AND p_target_phase IN ('provider_effect_pending','store_applied','cancelled','recovery_required'))
		OR (p_expected_phase='provider_effect_pending' AND p_target_phase IN ('store_applied','cancelled','recovery_required'))
		OR (p_expected_phase='store_applied' AND p_target_phase IN ('committed','recovery_required'))
			OR (p_expected_phase='recovery_required' AND p_target_phase IN ('store_applied','cancelled'))
	) THEN
		SELECT revision INTO next_revision FROM decodex.accounts WHERE account_id=operation.account_id;
		RETURN QUERY SELECT 'stale_operation',next_revision,operation.phase;
		RETURN;
	END IF;
	IF p_target_phase='cancelled'
		AND operation.kind='import'
		AND EXISTS (
			SELECT 1 FROM decodex.account_migration_receipts AS receipt
			CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
				receipt.manifest->'accounts'
			) AS migration_account
			WHERE receipt.singleton AND receipt.phase='prepared'
				AND migration_account->>'account_id'=operation.account_id::text
				AND migration_account->>'operation_id'=operation.operation_id::text
		)
	THEN
		SELECT revision INTO next_revision FROM decodex.accounts
		WHERE account_id=operation.account_id;
		RETURN QUERY SELECT 'operation_unsettled',next_revision,operation.phase;
		RETURN;
	END IF;
	IF p_target_phase='committed' THEN
		IF operation.kind='logout' THEN
			UPDATE decodex.accounts SET enabled=false,
				credential_store_schema_version=NULL,credential_version=NULL,
				credential_fingerprint=NULL,credential_writer_operation_id=NULL,
				credential_store_observation='missing',
				credential_store_observed_at=pg_catalog.clock_timestamp(),
				tombstoned_at=pg_catalog.clock_timestamp(),revision=revision+1,
				updated_at=pg_catalog.clock_timestamp()
			WHERE account_id=operation.account_id RETURNING revision INTO next_revision;
			DELETE FROM decodex.account_routing_order WHERE account_id=operation.account_id;
			UPDATE decodex.account_routing_control SET
				mode=CASE WHEN fixed_account_id=operation.account_id THEN 'balanced' ELSE mode END,
				fixed_account_id=CASE WHEN fixed_account_id=operation.account_id THEN NULL ELSE fixed_account_id END,
				revision=revision+1,updated_at=pg_catalog.clock_timestamp()
			WHERE singleton;
		ELSE
			UPDATE decodex.accounts SET
				credential_store_schema_version=operation.target_store_schema_version,
				credential_version=operation.target_credential_version,
				credential_fingerprint=operation.target_credential_fingerprint,
				credential_writer_operation_id=operation.target_credential_writer_operation_id,
				provider_kind=operation.provider_kind,
				provider_account_id=operation.provider_account_id,
				credential_store_observation='exact',
				credential_store_observed_at=pg_catalog.clock_timestamp(),
				revision=revision+1,updated_at=pg_catalog.clock_timestamp()
			WHERE account_id=operation.account_id RETURNING revision INTO next_revision;
			IF NOT (
				operation.kind='import'
				AND EXISTS (
					SELECT 1 FROM decodex.account_migration_receipts AS receipt
					CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
						receipt.manifest->'accounts'
					) AS migration_account
					WHERE receipt.singleton AND receipt.phase='prepared'
						AND migration_account->>'account_id'=operation.account_id::text
						AND migration_account->>'operation_id'=operation.operation_id::text
				)
			) THEN
				INSERT INTO decodex.account_routing_order(account_id,position)
				SELECT operation.account_id,COALESCE(pg_catalog.max(position)+1,0)
				FROM decodex.account_routing_order
				ON CONFLICT(account_id) DO NOTHING;
				IF FOUND THEN
					UPDATE decodex.account_routing_control SET revision=revision+1,
						updated_at=pg_catalog.clock_timestamp() WHERE singleton;
				END IF;
			END IF;
		END IF;
	ELSE
		SELECT revision INTO next_revision FROM decodex.accounts WHERE account_id=operation.account_id;
	END IF;
	UPDATE decodex.account_operations SET phase=p_target_phase,recovery_code=p_recovery_code,
		updated_at=pg_catalog.clock_timestamp(),completed_at=CASE
			WHEN p_target_phase IN ('committed','cancelled') THEN pg_catalog.clock_timestamp()
			ELSE NULL END
	WHERE operation_id=p_operation_id;
	RETURN QUERY SELECT 'advanced',next_revision,p_target_phase;
END
$$;

CREATE FUNCTION decodex.read_unsettled_account_operations_exact(p_limit bigint)
RETURNS SETOF decodex.account_operations
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	IF p_limit IS NULL OR p_limit < 1 OR p_limit > 512 THEN
		RAISE EXCEPTION 'account operation read limit is invalid' USING ERRCODE='22023';
	END IF;
	RETURN QUERY SELECT * FROM decodex.account_operations
	WHERE phase NOT IN ('committed','cancelled')
		AND NOT (
			kind='import'
			AND EXISTS (
				SELECT 1 FROM decodex.account_migration_receipts AS receipt
				CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
					receipt.manifest->'accounts'
				) AS migration_account
				WHERE receipt.singleton AND receipt.phase='prepared'
					AND migration_account->>'account_id'=account_operations.account_id::text
					AND migration_account->>'operation_id'=account_operations.operation_id::text
			)
		)
	ORDER BY operation_id LIMIT p_limit;
END
$$;

CREATE FUNCTION decodex.read_account_operation_exact(p_operation_id uuid)
RETURNS SETOF decodex.account_operations
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
	SELECT * FROM decodex.account_operations WHERE operation_id=p_operation_id
$$;

CREATE FUNCTION decodex.update_account_administration_exact(
	p_account_id uuid,p_expected_revision bigint,p_display_label text,p_enabled boolean
) RETURNS TABLE(result_code text,revision bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE current_revision bigint;
BEGIN
	IF p_display_label IS NOT NULL AND (
		pg_catalog.octet_length(p_display_label)<1 OR pg_catalog.octet_length(p_display_label)>128
		OR p_display_label ~ '[[:cntrl:]]'
		OR decodex.has_credential_material(p_display_label)
	) THEN
		RETURN QUERY SELECT 'invalid_request',0::bigint;
		RETURN;
	END IF;
	SELECT account.revision INTO current_revision FROM decodex.accounts AS account
	WHERE account.account_id=p_account_id AND account.tombstoned_at IS NULL FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'account_missing',0::bigint; RETURN;
	END IF;
	IF current_revision<>p_expected_revision THEN
		RETURN QUERY SELECT 'stale_account',current_revision; RETURN;
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.accounts AS account WHERE account.account_id=p_account_id
		AND account.display_label IS NOT DISTINCT FROM
			COALESCE(p_display_label,account.display_label)
		AND account.enabled IS NOT DISTINCT FROM COALESCE(p_enabled,account.enabled)) THEN
		RETURN QUERY SELECT 'updated',current_revision; RETURN;
	END IF;
	UPDATE decodex.accounts AS account SET
		display_label=COALESCE(p_display_label,account.display_label),
		enabled=COALESCE(p_enabled,account.enabled),revision=account.revision+1,
		updated_at=pg_catalog.clock_timestamp()
	WHERE account.account_id=p_account_id RETURNING account.revision INTO current_revision;
	RETURN QUERY SELECT 'updated',current_revision;
END
$$;

CREATE FUNCTION decodex.replace_account_routing_control_exact(
	p_expected_revision bigint,p_mode decodex.account_selection_mode,
	p_fixed_account_id uuid,p_order uuid[]
) RETURNS TABLE(result_code text,revision bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
DECLARE current_revision bigint;
DECLARE current_mode decodex.account_selection_mode;
DECLARE current_fixed uuid;
DECLARE current_order uuid[];
BEGIN
	SELECT control.revision,control.mode,control.fixed_account_id
	INTO current_revision,current_mode,current_fixed
	FROM decodex.account_routing_control AS control
	WHERE singleton FOR UPDATE;
	SELECT COALESCE(pg_catalog.array_agg(ordering.account_id ORDER BY ordering.position),'{}'::uuid[])
	INTO current_order FROM decodex.account_routing_order AS ordering;
	IF current_revision<>p_expected_revision THEN
		RETURN QUERY SELECT 'stale_control',current_revision; RETURN;
	END IF;
	IF (p_mode='fixed')<>(p_fixed_account_id IS NOT NULL)
		OR p_order IS NULL OR pg_catalog.cardinality(p_order)<>(SELECT pg_catalog.count(*)
			FROM decodex.accounts WHERE tombstoned_at IS NULL)
		OR pg_catalog.cardinality(p_order)<>(SELECT pg_catalog.count(DISTINCT value)
			FROM pg_catalog.unnest(p_order) AS value)
		OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_order) AS value
			LEFT JOIN decodex.accounts AS account ON account.account_id=value
			WHERE account.account_id IS NULL OR account.tombstoned_at IS NOT NULL)
		OR (p_fixed_account_id IS NOT NULL AND NOT p_fixed_account_id=ANY(p_order))
	THEN
		RETURN QUERY SELECT 'invalid_order',current_revision; RETURN;
	END IF;
	IF (current_mode,current_fixed,current_order) IS NOT DISTINCT FROM
		(p_mode,p_fixed_account_id,p_order) THEN
		RETURN QUERY SELECT 'updated',current_revision; RETURN;
	END IF;
	DELETE FROM decodex.account_routing_order;
	INSERT INTO decodex.account_routing_order(account_id,position)
	SELECT value,ordinality-1 FROM pg_catalog.unnest(p_order) WITH ORDINALITY AS entry(value,ordinality);
	UPDATE decodex.account_routing_control AS control
	SET mode=p_mode,fixed_account_id=p_fixed_account_id,
		revision=control.revision+1,updated_at=pg_catalog.clock_timestamp()
	WHERE control.singleton RETURNING control.revision INTO current_revision;
	RETURN QUERY SELECT 'updated',current_revision;
END
$$;

CREATE FUNCTION decodex.read_account_routing_control_exact()
RETURNS TABLE(mode decodex.account_selection_mode,fixed_account_id uuid,revision bigint,account_order uuid[])
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
	SELECT control.mode,control.fixed_account_id,control.revision,
		COALESCE(pg_catalog.array_agg(ordering.account_id ORDER BY ordering.position)
			FILTER (WHERE ordering.account_id IS NOT NULL),'{}'::uuid[])
	FROM decodex.account_routing_control AS control
	LEFT JOIN decodex.account_routing_order AS ordering ON true
	WHERE control.singleton GROUP BY control.mode,control.fixed_account_id,control.revision
$$;

CREATE FUNCTION decodex.observe_account_quota_exact(
	p_account_id uuid,p_duration_minutes integer,p_used_percent integer,
	p_resets_at_micros bigint,p_observed_at_micros bigint
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	IF p_duration_minutes NOT IN (300,10080) OR p_used_percent<0 OR p_used_percent>100
		OR p_observed_at_micros IS NULL OR p_observed_at_micros<=0
		OR p_resets_at_micros<=p_observed_at_micros THEN RETURN 'invalid_fact'; END IF;
	INSERT INTO decodex.account_quota_facts(
		account_id,duration_minutes,used_percent,resets_at_micros,observed_at_micros
	) VALUES (
		p_account_id,p_duration_minutes,p_used_percent,p_resets_at_micros,p_observed_at_micros
		) ON CONFLICT(account_id,duration_minutes) DO UPDATE SET
			used_percent=EXCLUDED.used_percent,resets_at_micros=EXCLUDED.resets_at_micros,
			error_code=NULL,observed_at_micros=EXCLUDED.observed_at_micros
	WHERE account_quota_facts.observed_at_micros<=EXCLUDED.observed_at_micros;
	UPDATE decodex.accounts AS account SET
		state=CASE WHEN EXISTS (
			SELECT 1 FROM decodex.account_quota_facts AS quota
			WHERE quota.account_id=p_account_id AND quota.error_code IS NULL
				AND quota.used_percent>=100
		) THEN 'depleted'::decodex.account_state ELSE 'available'::decodex.account_state END,
		updated_at=pg_catalog.clock_timestamp()
	WHERE account.account_id=p_account_id AND account.tombstoned_at IS NULL;
	RETURN 'observed';
END
$$;

CREATE FUNCTION decodex.observe_account_quota_error_exact(
	p_account_id uuid,p_duration_minutes integer,
	p_error_code decodex.account_quota_observation_error,p_observed_at_micros bigint
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	IF p_duration_minutes NOT IN (300,10080) OR p_error_code IS NULL
		OR p_observed_at_micros IS NULL OR p_observed_at_micros<=0 THEN
		RETURN 'invalid_fact';
	END IF;
	INSERT INTO decodex.account_quota_facts(
		account_id,duration_minutes,used_percent,resets_at_micros,error_code,observed_at_micros
	) VALUES (
		p_account_id,p_duration_minutes,NULL,NULL,p_error_code,p_observed_at_micros
	) ON CONFLICT(account_id,duration_minutes) DO UPDATE SET
		used_percent=NULL,resets_at_micros=NULL,error_code=EXCLUDED.error_code,
		observed_at_micros=EXCLUDED.observed_at_micros
	WHERE account_quota_facts.observed_at_micros<=EXCLUDED.observed_at_micros;
	RETURN 'observed';
END
$$;

CREATE FUNCTION decodex.observe_account_store_exact(
	p_account_id uuid,p_expected_revision bigint,p_expected_schema integer,
	p_expected_version bigint,p_expected_fingerprint text,p_expected_writer_operation_id uuid,
	p_expected_provider decodex.account_provider_kind,p_expected_provider_account_id text,
	p_observation decodex.account_store_observation
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	UPDATE decodex.accounts SET credential_store_observation=p_observation,
		credential_store_observed_at=pg_catalog.clock_timestamp()
	WHERE account_id=p_account_id AND tombstoned_at IS NULL
		AND revision=p_expected_revision
		AND credential_store_schema_version=p_expected_schema
		AND credential_version=p_expected_version
		AND credential_fingerprint=p_expected_fingerprint
		AND credential_writer_operation_id=p_expected_writer_operation_id
		AND provider_kind=p_expected_provider
		AND provider_account_id=p_expected_provider_account_id;
	RETURN CASE WHEN FOUND THEN 'observed' ELSE 'stale_account' END;
END
$$;

CREATE FUNCTION decodex.attest_codex_account_capability_exact(
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
	RETURN CASE WHEN p_build_identity='codex-cli 0.145.0-alpha.18'
		AND p_executable_sha256='f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9'
		AND p_schema_sha256 ~ '^[0-9a-f]{64}$'
		AND p_callback_profile_sha256 ~ '^[0-9a-f]{64}$'
		AND p_login_chatgpt_auth_tokens AND p_refresh_callback
		THEN 'ready' ELSE 'unready' END;
END
$$;

DROP FUNCTION decodex.prepare_process_generation_exact(
	uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,
	decodex.process_generation_isolation_kind
);
CREATE FUNCTION decodex.prepare_process_generation_exact(
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
		RETURN QUERY SELECT 'restore_authority_unavailable',0::bigint,'starting',0::bigint,0::bigint;
		RETURN;
	END IF;
	PERFORM 1 FROM decodex.process_generation_execution_epochs AS epoch
	WHERE epoch.execution_epoch_id=p_execution_epoch_id
		AND epoch.authorization_digest=p_authorization_digest AND epoch.retired_at IS NULL FOR SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'restore_authority_unavailable',0::bigint,
		'starting',0::bigint,0::bigint; RETURN; END IF;
	SELECT * INTO account FROM decodex.accounts WHERE account_id=p_account_id FOR KEY SHARE;
	IF NOT FOUND THEN RETURN QUERY SELECT 'account_missing',0::bigint,'starting',0::bigint,0::bigint; RETURN; END IF;
	IF (p_reset_card_outbox_id IS NULL,p_reset_card_worker_id IS NULL,
		p_reset_card_claim_token IS NULL) NOT IN ((true,true,true),(false,false,false))
	THEN RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,'starting',0::bigint,0::bigint; RETURN; END IF;
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
			RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,'starting',0::bigint,0::bigint;
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
	THEN RETURN QUERY SELECT 'account_lifecycle_unready',0::bigint,'starting',0::bigint,0::bigint; RETURN; END IF;
	IF NOT reconciliation_authorized AND NOT EXISTS (
		SELECT 1 FROM decodex.codex_account_capability AS capability
		WHERE capability.singleton AND capability.login_chatgpt_auth_tokens
			AND capability.refresh_callback
			AND capability.build_identity='codex-cli 0.145.0-alpha.18'
			AND capability.executable_sha256='f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9'
			AND capability.callback_profile_sha256=p_refresh_callback_profile_sha256)
	THEN RETURN QUERY SELECT 'callback_capability_unready',0::bigint,'starting',0::bigint,0::bigint; RETURN; END IF;
	IF EXISTS (SELECT 1 FROM decodex.process_generations AS generation
		WHERE generation.account_id=p_account_id AND generation.state<>'dead')
	THEN RETURN QUERY SELECT 'account_quarantined',0::bigint,'starting',0::bigint,0::bigint; RETURN; END IF;
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
	RETURN QUERY SELECT 'prepared',1::bigint,'starting',
		(extract(epoch FROM now_value)*1000000)::bigint,
		(extract(epoch FROM now_value)*1000000)::bigint;
END
$$;

DROP FUNCTION decodex.read_process_generations_exact(uuid,boolean,uuid,bigint);
CREATE FUNCTION decodex.read_process_generations_exact(
	p_account_id uuid,p_include_dead boolean,p_after_generation_id uuid,p_limit bigint
) RETURNS TABLE(
	generation_id uuid,account_id uuid,execution_epoch_id uuid,runner_identity text,
	intended_boot_id text,control_kind decodex.process_generation_control_kind,
	isolation_kind decodex.process_generation_isolation_kind,bound_boot_id text,
	process_id bigint,process_start_id text,process_group_id bigint,session_id bigint,
	state decodex.process_generation_state,revision bigint,
	authority_loss_reason decodex.process_generation_loss_reason,death_evidence_id uuid,
	created_at_micros bigint,updated_at_micros bigint,initial_account_revision bigint,
	credential_store_schema_version integer,credential_version bigint,
	credential_fingerprint text,credential_writer_operation_id uuid,
	provider_kind decodex.account_provider_kind,
	provider_account_id text,refresh_callback_profile_sha256 text
) LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$
BEGIN
	IF p_limit IS NULL OR p_limit<1 OR p_limit>256 THEN
		RAISE EXCEPTION 'process generation read limit is invalid' USING ERRCODE='22023';
	END IF;
	RETURN QUERY SELECT generation.generation_id,generation.account_id,
		generation.execution_epoch_id,generation.runner_identity,generation.intended_boot_id,
		generation.control_kind,generation.isolation_kind,generation.bound_boot_id,
		generation.process_id,generation.process_start_id,generation.process_group_id,
		generation.session_id,generation.state,generation.revision,
		generation.authority_loss_reason,generation.death_evidence_id,
		(extract(epoch FROM generation.created_at)*1000000)::bigint,
		(extract(epoch FROM generation.updated_at)*1000000)::bigint,
		generation.initial_account_revision,generation.credential_store_schema_version,
		generation.credential_version,generation.credential_fingerprint,
		generation.credential_writer_operation_id,
		generation.provider_kind,generation.provider_account_id,
		generation.refresh_callback_profile_sha256
	FROM decodex.process_generations AS generation
	WHERE (p_account_id IS NULL OR generation.account_id=p_account_id)
		AND (p_include_dead OR generation.state<>'dead')
		AND (p_after_generation_id IS NULL OR generation.generation_id>p_after_generation_id)
	ORDER BY generation.generation_id LIMIT p_limit;
END
$$;

REVOKE ALL ON TYPE decodex.account_provider_kind,decodex.account_operation_kind,
	decodex.account_operation_phase,decodex.account_selection_mode,
	decodex.account_store_observation,
	decodex.account_quota_observation_error FROM PUBLIC;
REVOKE ALL ON TABLE decodex.account_operations,decodex.account_routing_control,
	decodex.account_routing_order,decodex.account_quota_facts,
	decodex.codex_account_capability,decodex.account_migration_receipts FROM PUBLIC;
REVOKE ALL ON FUNCTION
	decodex.read_account_registry_exact(uuid,bigint),
	decodex.read_reset_card_account_admission_exact(uuid,text),
	decodex.prepare_account_operation_exact(uuid,uuid,decodex.account_operation_kind,text,boolean,bigint,integer,bigint,text,uuid,integer,bigint,text,uuid,decodex.account_provider_kind,text),
	decodex.set_account_operation_target_exact(uuid,integer,bigint,text,uuid),
	decodex.advance_account_operation_exact(uuid,decodex.account_operation_phase,decodex.account_operation_phase,text),
	decodex.read_unsettled_account_operations_exact(bigint),
	decodex.read_account_operation_exact(uuid),
	decodex.update_account_administration_exact(uuid,bigint,text,boolean),
	decodex.replace_account_routing_control_exact(bigint,decodex.account_selection_mode,uuid,uuid[]),
	decodex.read_account_routing_control_exact(),
	decodex.observe_account_quota_exact(uuid,integer,integer,bigint,bigint),
	decodex.observe_account_quota_error_exact(uuid,integer,decodex.account_quota_observation_error,bigint),
	decodex.observe_account_store_exact(uuid,bigint,integer,bigint,text,uuid,decodex.account_provider_kind,text,decodex.account_store_observation),
	decodex.attest_codex_account_capability_exact(text,text,text,text,boolean,boolean),
	decodex.record_account_migration_receipt_exact(text,jsonb,jsonb,jsonb,integer),
	decodex.prepare_process_generation_exact(uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,bigint,integer,bigint,text,uuid,decodex.account_provider_kind,text,text,bigint,uuid,uuid),
	decodex.read_process_generations_exact(uuid,boolean,uuid,bigint)
	FROM PUBLIC;

-- Derive the existing runtime principal from the V24 anchor and grant only closed functions.
DO $$
DECLARE anchor_oid pg_catalog.oid;
DECLARE migration_role_oid pg_catalog.oid;
DECLARE runtime_role pg_catalog.name;
BEGIN
	SELECT role.oid INTO migration_role_oid FROM pg_catalog.pg_roles AS role
	WHERE role.rolname=current_user;
	anchor_oid:=pg_catalog.to_regprocedure(
		'decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text)'
	);
	SELECT role.rolname INTO runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
		procedure.proacl,pg_catalog.acldefault('f',procedure.proowner))) AS privilege
	JOIN pg_catalog.pg_roles AS role ON role.oid=privilege.grantee
	WHERE procedure.oid=anchor_oid AND privilege.privilege_type='EXECUTE'
		AND privilege.grantee<>migration_role_oid;
	IF runtime_role IS NOT NULL THEN
		EXECUTE pg_catalog.format(
			'REVOKE INSERT,UPDATE,DELETE ON TABLE decodex.accounts FROM %I',runtime_role
		);
		EXECUTE pg_catalog.format('GRANT USAGE ON TYPE decodex.account_provider_kind,decodex.account_operation_kind,decodex.account_operation_phase,decodex.account_selection_mode,decodex.account_store_observation,decodex.account_quota_observation_error TO %I',runtime_role);
		EXECUTE pg_catalog.format('GRANT EXECUTE ON FUNCTION decodex.read_account_registry_exact(uuid,bigint),decodex.read_reset_card_account_admission_exact(uuid,text),decodex.prepare_account_operation_exact(uuid,uuid,decodex.account_operation_kind,text,boolean,bigint,integer,bigint,text,uuid,integer,bigint,text,uuid,decodex.account_provider_kind,text),decodex.set_account_operation_target_exact(uuid,integer,bigint,text,uuid),decodex.advance_account_operation_exact(uuid,decodex.account_operation_phase,decodex.account_operation_phase,text),decodex.read_unsettled_account_operations_exact(bigint),decodex.read_account_operation_exact(uuid),decodex.update_account_administration_exact(uuid,bigint,text,boolean),decodex.replace_account_routing_control_exact(bigint,decodex.account_selection_mode,uuid,uuid[]),decodex.read_account_routing_control_exact(),decodex.observe_account_quota_exact(uuid,integer,integer,bigint,bigint),decodex.observe_account_quota_error_exact(uuid,integer,decodex.account_quota_observation_error,bigint),decodex.observe_account_store_exact(uuid,bigint,integer,bigint,text,uuid,decodex.account_provider_kind,text,decodex.account_store_observation),decodex.attest_codex_account_capability_exact(text,text,text,text,boolean,boolean),decodex.record_account_migration_receipt_exact(text,jsonb,jsonb,jsonb,integer),decodex.prepare_process_generation_exact(uuid,uuid,uuid,text,text,text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,bigint,integer,bigint,text,uuid,decodex.account_provider_kind,text,text,bigint,uuid,uuid),decodex.read_process_generations_exact(uuid,boolean,uuid,bigint) TO %I',runtime_role);
	END IF;
END
$$;
