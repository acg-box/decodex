#!/usr/bin/env python3
"""Prove the XY-1345 exact-command design in a disposable PostgreSQL 18 cluster."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import traceback
import uuid


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1345"
RESTORE_DATABASE = "decodex_xy1345_restore"
OWNER_ROLE = "decodex_exact_owner"
RUNTIME_ROLE = "decodex_exact_runtime"
PROTOCOL = "decodex/exact-command/1"
STRESS_REPETITIONS = 50
STRESS_WIDTH = 32
SQLSTATE_RE = re.compile(r"(?:ERROR|FATAL):\s+([0-9A-Z]{5}):")
TABLE_PRIVILEGES = ("SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE", "REFERENCES", "TRIGGER", "MAINTAIN")
EXPECTED_FUNCTIONS = {
	"build_role_profile_bootstrap_request": (", ".join(["text"] * 21), "plpgsql", "i", False, False),
	"build_role_profile_update_request": ("text, decodex.prototype_role, bigint, text, text, text, text, text", "sql", "i", False, False),
	"build_runtime_session_create_request": ("text, uuid, uuid, decodex.prototype_role, uuid, uuid, text, text, bigint, uuid, decodex.prototype_session_state", "sql", "i", False, False),
	"build_runtime_session_transition_request": ("text, uuid, bigint, decodex.prototype_session_state, text", "sql", "i", False, False),
	"complete_prototype_rejection": ("text, text, text", "plpgsql", "v", False, False),
	"enforce_exact_receipt_completion": ("", "plpgsql", "v", False, False),
	"forbid_exact_receipt_rewrite": ("", "plpgsql", "v", False, False),
	"forbid_exact_receipt_truncate": ("", "plpgsql", "v", False, False),
	"prototype_failpoint": ("text", "plpgsql", "v", False, False),
	"prototype_leave_incomplete": ("text, text", "plpgsql", "v", True, False),
	"transition_runtime_session_exact": ("text, text, uuid, bigint, decodex.prototype_session_state, text", "plpgsql", "v", True, True),
}


class ProofFailure(RuntimeError):
	"""Raised when the prototype falsifies an accepted requirement."""


@dataclass(frozen=True)
class Result:
	returncode: int
	stdout: str
	stderr: str
	duration: float

	@property
	def sqlstate(self) -> str | None:
		match = SQLSTATE_RE.search(self.stderr)
		return match.group(1) if match else None


SCHEMA_SQL = r"""
CREATE ROLE decodex_exact_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT;
CREATE ROLE decodex_exact_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT;
CREATE SCHEMA decodex_crypto AUTHORIZATION decodex_exact_owner;
REVOKE ALL ON SCHEMA decodex_crypto FROM PUBLIC;
CREATE EXTENSION pgcrypto WITH SCHEMA decodex_crypto;
CREATE SCHEMA decodex AUTHORIZATION decodex_exact_owner;
REVOKE ALL ON SCHEMA decodex FROM PUBLIC;
GRANT USAGE ON SCHEMA decodex TO decodex_exact_runtime;
SET ROLE decodex_exact_owner;

CREATE TYPE decodex.exact_receipt_state AS ENUM (
	'executing', 'completed_success', 'completed_rejected'
);
CREATE TYPE decodex.prototype_session_state AS ENUM (
	'starting', 'active', 'ended', 'diverged'
);
CREATE TYPE decodex.prototype_role AS ENUM ('advisor', 'lead', 'task', 'reviewer');

CREATE TABLE decodex.exact_command_receipts (
	protocol_version pg_catalog.text NOT NULL,
	idempotency_key pg_catalog.text NOT NULL,
	request_envelope pg_catalog.jsonb NOT NULL,
	request_digest pg_catalog.bytea NOT NULL,
	receipt_state decodex.exact_receipt_state NOT NULL,
	outcome_class pg_catalog.text,
	effect_envelope pg_catalog.jsonb,
	response_bytes pg_catalog.bytea,
	created_at pg_catalog.timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	completed_at pg_catalog.timestamptz,
	PRIMARY KEY (protocol_version, idempotency_key),
	CONSTRAINT exact_receipt_shape CHECK (
		(receipt_state = 'executing'
			AND outcome_class IS NULL AND effect_envelope IS NULL
			AND response_bytes IS NULL AND completed_at IS NULL)
		OR
		(receipt_state <> 'executing'
			AND outcome_class IS NOT NULL AND effect_envelope IS NOT NULL
			AND response_bytes IS NOT NULL AND completed_at IS NOT NULL)
	),
	CONSTRAINT exact_receipt_digest_matches CHECK (
		request_digest = decodex_crypto.digest(
			pg_catalog.convert_to(request_envelope::pg_catalog.text, 'UTF8'), 'sha256'
		)
	)
);

CREATE TABLE decodex.prototype_runtime_sessions (
	runtime_session_id pg_catalog.uuid PRIMARY KEY,
	state decodex.prototype_session_state NOT NULL,
	revision pg_catalog.int8 NOT NULL CHECK (revision > 0),
	updated_at pg_catalog.timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp()
);
CREATE TABLE decodex.prototype_activity (
	activity_id pg_catalog.int8 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	aggregate_kind pg_catalog.text NOT NULL,
	aggregate_id pg_catalog.uuid NOT NULL,
	event_kind pg_catalog.text NOT NULL,
	payload pg_catalog.jsonb NOT NULL,
	created_at pg_catalog.timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
	UNIQUE (aggregate_kind, aggregate_id, event_kind)
);
CREATE TABLE decodex.prototype_outbox (
	outbox_id pg_catalog.int8 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	activity_id pg_catalog.int8 NOT NULL UNIQUE
		REFERENCES decodex.prototype_activity(activity_id),
	effect_key pg_catalog.text NOT NULL UNIQUE,
	payload pg_catalog.jsonb NOT NULL,
	created_at pg_catalog.timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp()
);

CREATE FUNCTION decodex.enforce_exact_receipt_completion()
RETURNS pg_catalog.trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
DECLARE
	current_state decodex.exact_receipt_state;
BEGIN
	SELECT receipt_state INTO current_state
	FROM decodex.exact_command_receipts
	WHERE protocol_version = NEW.protocol_version
		AND idempotency_key = NEW.idempotency_key;
	IF current_state = 'executing' THEN
		RAISE EXCEPTION USING ERRCODE = '23514',
			CONSTRAINT = 'exact_receipts_complete_at_commit',
			MESSAGE = 'exact command receipt must be completed before commit';
	END IF;
	RETURN NULL;
END
$function$;

CREATE CONSTRAINT TRIGGER exact_receipts_complete_at_commit
AFTER INSERT OR UPDATE ON decodex.exact_command_receipts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_exact_receipt_completion();

CREATE FUNCTION decodex.forbid_exact_receipt_rewrite()
RETURNS pg_catalog.trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
BEGIN
	IF TG_OP = 'DELETE' THEN
		RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'exact receipts are undeletable';
	END IF;
	IF OLD.receipt_state <> 'executing'
		OR NEW.receipt_state = 'executing'
		OR OLD.protocol_version <> NEW.protocol_version
		OR OLD.idempotency_key <> NEW.idempotency_key
		OR OLD.request_envelope <> NEW.request_envelope
		OR OLD.request_digest <> NEW.request_digest
		OR OLD.created_at <> NEW.created_at
	THEN
		RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'exact receipt rewrite is forbidden';
	END IF;
	RETURN NEW;
END
$function$;
CREATE TRIGGER exact_receipts_immutable
BEFORE UPDATE OR DELETE ON decodex.exact_command_receipts
FOR EACH ROW EXECUTE FUNCTION decodex.forbid_exact_receipt_rewrite();

CREATE FUNCTION decodex.forbid_exact_receipt_truncate()
RETURNS pg_catalog.trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
BEGIN
	RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'exact receipts are untruncatable';
END
$function$;
CREATE TRIGGER exact_receipts_untruncatable
BEFORE TRUNCATE ON decodex.exact_command_receipts
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_exact_receipt_truncate();

CREATE FUNCTION decodex.build_role_profile_bootstrap_request(
	p_protocol pg_catalog.text,
	p_advisor_model pg_catalog.text, p_advisor_reasoning pg_catalog.text,
	p_advisor_tier pg_catalog.text, p_advisor_instructions pg_catalog.text,
	p_advisor_provenance pg_catalog.text,
	p_lead_model pg_catalog.text, p_lead_reasoning pg_catalog.text,
	p_lead_tier pg_catalog.text, p_lead_instructions pg_catalog.text,
	p_lead_provenance pg_catalog.text,
	p_task_model pg_catalog.text, p_task_reasoning pg_catalog.text,
	p_task_tier pg_catalog.text, p_task_instructions pg_catalog.text,
	p_task_provenance pg_catalog.text,
	p_reviewer_model pg_catalog.text, p_reviewer_reasoning pg_catalog.text,
	p_reviewer_tier pg_catalog.text, p_reviewer_instructions pg_catalog.text,
	p_reviewer_provenance pg_catalog.text
) RETURNS pg_catalog.jsonb
LANGUAGE plpgsql IMMUTABLE SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
BEGIN
	IF p_protocol IS NULL OR p_advisor_model IS NULL OR p_advisor_reasoning IS NULL
		OR p_advisor_tier IS NULL OR p_advisor_instructions IS NULL
		OR p_lead_model IS NULL OR p_lead_reasoning IS NULL OR p_lead_tier IS NULL
		OR p_lead_instructions IS NULL OR p_task_model IS NULL OR p_task_reasoning IS NULL
		OR p_task_tier IS NULL OR p_task_instructions IS NULL OR p_reviewer_model IS NULL
		OR p_reviewer_reasoning IS NULL OR p_reviewer_tier IS NULL
		OR p_reviewer_instructions IS NULL THEN
		RAISE EXCEPTION USING ERRCODE = '22004', MESSAGE = 'bootstrap configuration is incomplete';
	END IF;
	RETURN pg_catalog.jsonb_build_object(
		'protocol_version', p_protocol, 'operation', 'bootstrap_role_profiles',
		'profiles', pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object('role','advisor','model',p_advisor_model,'reasoning_effort',p_advisor_reasoning,'service_tier',p_advisor_tier,'instructions',p_advisor_instructions,'provenance',p_advisor_provenance),
			pg_catalog.jsonb_build_object('role','lead','model',p_lead_model,'reasoning_effort',p_lead_reasoning,'service_tier',p_lead_tier,'instructions',p_lead_instructions,'provenance',p_lead_provenance),
			pg_catalog.jsonb_build_object('role','task','model',p_task_model,'reasoning_effort',p_task_reasoning,'service_tier',p_task_tier,'instructions',p_task_instructions,'provenance',p_task_provenance),
			pg_catalog.jsonb_build_object('role','reviewer','model',p_reviewer_model,'reasoning_effort',p_reviewer_reasoning,'service_tier',p_reviewer_tier,'instructions',p_reviewer_instructions,'provenance',p_reviewer_provenance)
		)
	);
END
$function$;

CREATE FUNCTION decodex.build_role_profile_update_request(
	p_protocol pg_catalog.text, p_role decodex.prototype_role,
	p_expected_revision pg_catalog.int8, p_model pg_catalog.text,
	p_reasoning pg_catalog.text, p_tier pg_catalog.text,
	p_instructions pg_catalog.text, p_provenance pg_catalog.text
) RETURNS pg_catalog.jsonb
LANGUAGE sql IMMUTABLE SECURITY INVOKER
SET search_path = pg_catalog, decodex
RETURN pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'update_role_profile',
	'role', p_role, 'expected_revision', p_expected_revision,
	'model', p_model, 'reasoning_effort', p_reasoning, 'service_tier', p_tier,
	'instructions', p_instructions, 'provenance', p_provenance
);

CREATE FUNCTION decodex.build_runtime_session_create_request(
	p_protocol pg_catalog.text, p_session_id pg_catalog.uuid,
	p_conversation_id pg_catalog.uuid, p_role decodex.prototype_role,
	p_account_snapshot_id pg_catalog.uuid, p_source_account_id pg_catalog.uuid,
	p_display_label pg_catalog.text, p_observed_state pg_catalog.text,
	p_account_source_revision pg_catalog.int8, p_codex_thread_id pg_catalog.uuid,
	p_initial_state decodex.prototype_session_state
) RETURNS pg_catalog.jsonb
LANGUAGE sql IMMUTABLE SECURITY INVOKER
SET search_path = pg_catalog, decodex
RETURN pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'create_runtime_session',
	'runtime_session_id', p_session_id, 'conversation_id', p_conversation_id,
	'role', p_role, 'account_snapshot_id', p_account_snapshot_id,
	'source_account_id', p_source_account_id, 'display_label', p_display_label,
	'observed_state', p_observed_state, 'account_source_revision', p_account_source_revision,
	'codex_thread_id', p_codex_thread_id, 'initial_state', p_initial_state
);

CREATE FUNCTION decodex.build_runtime_session_transition_request(
	p_protocol pg_catalog.text, p_session_id pg_catalog.uuid,
	p_expected_revision pg_catalog.int8, p_target_state decodex.prototype_session_state,
	p_note pg_catalog.text
) RETURNS pg_catalog.jsonb
LANGUAGE sql IMMUTABLE SECURITY INVOKER
SET search_path = pg_catalog, decodex
RETURN pg_catalog.jsonb_build_object(
	'protocol_version', p_protocol, 'operation', 'transition_runtime_session',
	'runtime_session_id', p_session_id, 'expected_revision', p_expected_revision,
	'target_state', p_target_state, 'note', p_note
);

CREATE FUNCTION decodex.prototype_failpoint(p_stage pg_catalog.text)
RETURNS pg_catalog.void
LANGUAGE plpgsql VOLATILE SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
DECLARE
	wait_seconds pg_catalog.float8;
BEGIN
	IF pg_catalog.current_setting('decodex.prototype_failpoint', true) = p_stage THEN
		wait_seconds := COALESCE(
			pg_catalog.current_setting('decodex.prototype_failpoint_wait', true)::pg_catalog.float8,
			0
		);
		IF wait_seconds > 0 THEN PERFORM pg_catalog.pg_sleep(wait_seconds); END IF;
		RAISE EXCEPTION USING ERRCODE = 'DX900', MESSAGE = 'injected prototype infrastructure failure';
	END IF;
END
$function$;

CREATE FUNCTION decodex.complete_prototype_rejection(
	p_protocol pg_catalog.text, p_key pg_catalog.text, p_code pg_catalog.text
) RETURNS pg_catalog.bytea
LANGUAGE plpgsql VOLATILE SECURITY INVOKER
SET search_path = pg_catalog, decodex
AS $function$
DECLARE
	response_json pg_catalog.jsonb;
	response_value pg_catalog.bytea;
BEGIN
	response_json := pg_catalog.jsonb_build_object(
		'classification', 'stable_domain_rejection', 'code', p_code
	);
	response_value := pg_catalog.convert_to(response_json::pg_catalog.text, 'UTF8');
	UPDATE decodex.exact_command_receipts SET
		receipt_state = 'completed_rejected',
		outcome_class = 'stable_domain_rejection',
		effect_envelope = pg_catalog.jsonb_build_object('changed', false, 'code', p_code),
		response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_key;
	RETURN response_value;
END
$function$;

CREATE FUNCTION decodex.transition_runtime_session_exact(
	p_protocol pg_catalog.text, p_idempotency_key pg_catalog.text,
	p_session_id pg_catalog.uuid, p_expected_revision pg_catalog.int8,
	p_target_state decodex.prototype_session_state, p_note pg_catalog.text
) RETURNS pg_catalog.bytea
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $function$
DECLARE
	request_value pg_catalog.jsonb;
	existing_request pg_catalog.jsonb;
	existing_response pg_catalog.bytea;
	inserted_count pg_catalog.int4;
	old_state decodex.prototype_session_state;
	new_state decodex.prototype_session_state;
	new_revision pg_catalog.int8;
	new_updated_at pg_catalog.timestamptz;
	activity_identity pg_catalog.int8;
	activity_payload pg_catalog.jsonb;
	outbox_identity pg_catalog.int8;
	outbox_payload pg_catalog.jsonb;
	effect_value pg_catalog.jsonb;
	response_json pg_catalog.jsonb;
	response_value pg_catalog.bytea;
BEGIN
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_session_id IS NULL
		OR p_expected_revision IS NULL OR p_target_state IS NULL THEN
		RAISE EXCEPTION USING ERRCODE = '22004', MESSAGE = 'exact transition input is incomplete';
	END IF;
	request_value := decodex.build_runtime_session_transition_request(
		p_protocol, p_session_id, p_expected_revision, p_target_state, p_note
	);
	INSERT INTO decodex.exact_command_receipts(
		protocol_version, idempotency_key, request_envelope, request_digest, receipt_state
	) VALUES (
		p_protocol, p_idempotency_key, request_value,
		decodex_crypto.digest(pg_catalog.convert_to(request_value::pg_catalog.text, 'UTF8'), 'sha256'),
		'executing'
	) ON CONFLICT (protocol_version, idempotency_key) DO NOTHING;
	GET DIAGNOSTICS inserted_count = ROW_COUNT;
	IF inserted_count = 0 THEN
		SELECT request_envelope, response_bytes INTO existing_request, existing_response
		FROM decodex.exact_command_receipts
		WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key
		FOR UPDATE;
		IF existing_request <> request_value THEN
			RAISE EXCEPTION USING ERRCODE = 'DX001', MESSAGE = 'exact idempotency conflict';
		END IF;
		IF existing_response IS NULL THEN
			RAISE EXCEPTION USING ERRCODE = 'DX002', MESSAGE = 'incomplete exact receipt is not replayable';
		END IF;
		RETURN existing_response;
	END IF;
	PERFORM decodex.prototype_failpoint('after_receipt');

	SELECT state INTO old_state FROM decodex.prototype_runtime_sessions
	WHERE runtime_session_id = p_session_id FOR UPDATE;
	IF NOT FOUND THEN
		RETURN decodex.complete_prototype_rejection(p_protocol, p_idempotency_key, 'missing_target');
	END IF;
	IF (SELECT revision FROM decodex.prototype_runtime_sessions
		WHERE runtime_session_id = p_session_id) <> p_expected_revision THEN
		RETURN decodex.complete_prototype_rejection(p_protocol, p_idempotency_key, 'stale_revision');
	END IF;
	IF NOT ((old_state = 'starting' AND p_target_state IN ('active','diverged'))
		OR (old_state = 'active' AND p_target_state IN ('ended','diverged'))) THEN
		RETURN decodex.complete_prototype_rejection(p_protocol, p_idempotency_key, 'illegal_transition');
	END IF;

	UPDATE decodex.prototype_runtime_sessions SET
		state = p_target_state, revision = revision + 1,
		updated_at = pg_catalog.clock_timestamp()
	WHERE runtime_session_id = p_session_id AND revision = p_expected_revision
	RETURNING state, revision, updated_at INTO new_state, new_revision, new_updated_at;
	PERFORM decodex.prototype_failpoint('after_domain');

	activity_payload := pg_catalog.jsonb_build_object(
		'runtime_session_id', p_session_id, 'old_state', old_state,
		'new_state', new_state, 'revision', new_revision, 'updated_at', new_updated_at
	);
	INSERT INTO decodex.prototype_activity(aggregate_kind, aggregate_id, event_kind, payload)
	VALUES ('runtime_session', p_session_id, 'runtime_session_transitioned', activity_payload)
	RETURNING activity_id, payload INTO activity_identity, activity_payload;
	PERFORM decodex.prototype_failpoint('after_activity');

	outbox_payload := pg_catalog.jsonb_build_object(
		'activity_id', activity_identity, 'event', activity_payload
	);
	INSERT INTO decodex.prototype_outbox(activity_id, effect_key, payload)
	VALUES (activity_identity, 'runtime-session-transition:' || activity_identity::pg_catalog.text, outbox_payload)
	RETURNING outbox_id, payload INTO outbox_identity, outbox_payload;
	PERFORM decodex.prototype_failpoint('after_outbox');

	effect_value := pg_catalog.jsonb_build_object(
		'persisted_session', pg_catalog.jsonb_build_object(
			'runtime_session_id', p_session_id, 'state', new_state,
			'revision', new_revision, 'updated_at', new_updated_at
		),
		'activity_id', activity_identity, 'activity_payload', activity_payload,
		'outbox_id', outbox_identity, 'outbox_payload', outbox_payload
	);
	response_json := pg_catalog.jsonb_build_object(
		'classification', 'success', 'effect', effect_value
	);
	response_value := pg_catalog.convert_to(response_json::pg_catalog.text, 'UTF8');
	UPDATE decodex.exact_command_receipts SET
		receipt_state = 'completed_success', outcome_class = 'success',
		effect_envelope = effect_value, response_bytes = response_value,
		completed_at = pg_catalog.clock_timestamp()
	WHERE protocol_version = p_protocol AND idempotency_key = p_idempotency_key;
	RETURN response_value;
END
$function$;

CREATE FUNCTION decodex.prototype_leave_incomplete(
	p_protocol pg_catalog.text, p_key pg_catalog.text
) RETURNS pg_catalog.void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, decodex
AS $function$
DECLARE request_value pg_catalog.jsonb;
BEGIN
	request_value := pg_catalog.jsonb_build_object(
		'protocol_version', p_protocol, 'operation', 'prototype_incomplete'
	);
	INSERT INTO decodex.exact_command_receipts(
		protocol_version, idempotency_key, request_envelope, request_digest, receipt_state
	) VALUES (
		p_protocol, p_key, request_value,
		decodex_crypto.digest(pg_catalog.convert_to(request_value::pg_catalog.text, 'UTF8'), 'sha256'),
		'executing'
	);
END
$function$;

REVOKE ALL ON ALL TABLES IN SCHEMA decodex FROM PUBLIC, decodex_exact_runtime;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA decodex FROM PUBLIC, decodex_exact_runtime;
REVOKE EXECUTE ON ALL FUNCTIONS IN SCHEMA decodex FROM PUBLIC, decodex_exact_runtime;
GRANT EXECUTE ON FUNCTION decodex.transition_runtime_session_exact(
	pg_catalog.text, pg_catalog.text, pg_catalog.uuid, pg_catalog.int8,
	decodex.prototype_session_state, pg_catalog.text
) TO decodex_exact_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE decodex_exact_owner
	REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
RESET ROLE;
"""


class Harness:
	def __init__(self, root: Path) -> None:
		self.root = root
		self.data = root / "data"
		self.socket = root / "socket"
		self.log = root / "postgres.log"
		self.port = 55435
		self.env = os.environ.copy()
		self.env.update({
			"PGHOST": str(self.socket), "PGPORT": str(self.port),
			"PGDATABASE": DATABASE, "PGAPPNAME": "xy1345-proof",
		})
		self.started = False

	def command(self, args: list[str], *, env: dict[str, str] | None = None,
			input_text: str | None = None) -> Result:
		started = time.monotonic()
		completed = subprocess.run(
			args, cwd=REPO_ROOT, env=env or self.env, input=input_text,
			text=True, capture_output=True, check=False,
		)
		return Result(completed.returncode, completed.stdout.strip(),
			completed.stderr.strip(), time.monotonic() - started)

	def require(self, args: list[str], **kwargs: object) -> Result:
		result = self.command(args, **kwargs)
		if result.returncode != 0:
			raise ProofFailure(
				f"command failed: {' '.join(args)}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
			)
		return result

	def start(self) -> None:
		self.socket.mkdir()
		initdb_path = Path(shutil.which("initdb") or "initdb").resolve()
		postgres_share = initdb_path.parent.parent / "share" / "postgresql"
		self.env["PATH"] = f"{initdb_path.parent}{os.pathsep}{self.env['PATH']}"
		self.require(["initdb", "-D", str(self.data), "--encoding=UTF8",
			"--locale=C", "--auth=trust", "--no-instructions",
			"-L", str(postgres_share)])
		self.require(["pg_ctl", "-D", str(self.data), "-l", str(self.log),
			"-o", f"-k {self.socket} -p {self.port} -h '' -F", "-w", "start"])
		self.started = True
		self.require(["createdb", DATABASE])
		self.must_psql(SCHEMA_SQL)

	def stop(self) -> bool:
		if not self.started:
			return True
		self.require(["pg_ctl", "-D", str(self.data), "-m", "immediate", "-w", "stop"])
		self.started = False
		return True

	def psql(self, sql: str, *, database: str = DATABASE,
			role: str | None = None, app: str | None = None) -> Result:
		prefix = "\\set VERBOSITY verbose\n"
		if role:
			prefix += f"SET ROLE {role};\n"
		env = self.env.copy()
		env["PGDATABASE"] = database
		if app:
			env["PGAPPNAME"] = app
		return self.command(
			["psql", "-X", "-v", "ON_ERROR_STOP=1", "-At"],
			env=env, input_text=prefix + sql,
		)

	def must_psql(self, sql: str, **kwargs: object) -> str:
		result = self.psql(sql, **kwargs)
		if result.returncode != 0:
			raise ProofFailure(f"SQL failed\n{sql}\n{result.stdout}\n{result.stderr}")
		return result.stdout

	def transition_sql(self, key: str, session_id: str, expected: int,
			target: str, note: str | None = None) -> str:
		note_sql = "NULL" if note is None else "'" + note.replace("'", "''") + "'"
		return (
			"SELECT pg_catalog.encode(decodex.transition_runtime_session_exact("
			f"'{PROTOCOL}','{key}','{session_id}',{expected},"
			f"'{target}'::decodex.prototype_session_state,{note_sql}),'hex');"
		)

	def add_session(self, session_id: str, state: str = "starting", revision: int = 1) -> None:
		self.must_psql(
			"SET ROLE decodex_exact_owner; "
			"INSERT INTO decodex.prototype_runtime_sessions(runtime_session_id,state,revision) "
			f"VALUES ('{session_id}','{state}',{revision}); RESET ROLE;"
		)


def expect(condition: bool, message: str) -> None:
	if not condition:
		raise ProofFailure(message)


def scalar(harness: Harness, sql: str, **kwargs: object) -> str:
	return harness.must_psql(sql, **kwargs).splitlines()[-1]


def runtime_call(harness: Harness, sql: str, *, app: str | None = None) -> Result:
	return harness.psql(sql, role=RUNTIME_ROLE, app=app)


def exact_hex(result: Result) -> str:
	for line in reversed(result.stdout.splitlines()):
		if re.fullmatch(r"[0-9a-f]+", line):
			return line
	raise ProofFailure(f"no response bytes in output: {result.stdout!r} {result.stderr!r}")


def classify(result: Result) -> str:
	if result.returncode == 0:
		return "success"
	if result.sqlstate == "DX001":
		return "idempotency_conflict"
	if result.sqlstate in {"40001", "40P01", "08006", "57P01", "DX900"}:
		return "retryable_infrastructure_failure"
	if result.sqlstate in {"42501", "23514", "22004", "42883"}:
		return "stable_domain_or_authority_rejection"
	raise ProofFailure(f"unclassified SQLSTATE {result.sqlstate}: {result.stderr}")


def prove_wait_replay_and_conflicts(harness: Harness, report: dict[str, object]) -> None:
	session = str(uuid.uuid4())
	harness.add_session(session)
	key = "wait-replay"
	first_sql = "BEGIN; " + harness.transition_sql(key, session, 1, "active", "same") + " SELECT pg_sleep(0.8); COMMIT;"
	with ThreadPoolExecutor(max_workers=2) as pool:
		first = pool.submit(runtime_call, harness, first_sql)
		time.sleep(0.15)
		second = pool.submit(runtime_call, harness, harness.transition_sql(key, session, 1, "active", "same"))
		first_result = first.result()
		second_result = second.result()
	expect(first_result.returncode == second_result.returncode == 0, "same-envelope replay failed")
	expect(exact_hex(first_result) == exact_hex(second_result), "replay bytes differ")
	expect(second_result.duration >= 0.5, "same-key contender did not wait")

	conflict = runtime_call(harness, harness.transition_sql(key, session, 1, "ended", "same"))
	expect(classify(conflict) == "idempotency_conflict", "different target did not conflict")
	cross_operation = harness.psql(
		"SET ROLE decodex_exact_owner; "
		"SELECT decodex.prototype_leave_incomplete('decodex/exact-command/1','cross-op');",
	)
	expect(cross_operation.sqlstate == "23514", "incomplete cross-op setup should fail at commit")
	request = json.dumps({"protocol_version": PROTOCOL, "operation": "bootstrap_role_profiles"})
	harness.must_psql(
		"SET ROLE decodex_exact_owner; INSERT INTO decodex.exact_command_receipts("
		"protocol_version,idempotency_key,request_envelope,request_digest,receipt_state,"
		"outcome_class,effect_envelope,response_bytes,created_at,completed_at) VALUES ("
		f"'{PROTOCOL}','cross-operation','{request}'::jsonb,"
		f"decodex_crypto.digest(convert_to('{request}'::jsonb::text,'UTF8'),'sha256'),'completed_rejected',"
		"'stable_domain_rejection','{\"changed\":false}'::jsonb,convert_to('{}','UTF8'),"
		"statement_timestamp(),statement_timestamp()); RESET ROLE;"
	)
	cross = runtime_call(harness, harness.transition_sql("cross-operation", session, 2, "ended"))
	expect(classify(cross) == "idempotency_conflict", "cross-operation key reuse did not conflict")
	effects = scalar(harness, f"SELECT count(*) FROM decodex.prototype_activity WHERE aggregate_id='{session}';")
	expect(effects == "1", "conflicting reuse created an effect")
	report["wait_replay"] = {"wait_seconds": round(second_result.duration, 3), "byte_identical": True}


def prove_abort_matrix(harness: Harness, report: dict[str, object]) -> None:
	rows: dict[str, object] = {}
	for stage in ("after_receipt", "after_domain", "after_activity", "after_outbox"):
		session = str(uuid.uuid4())
		key = f"abort-{stage}"
		harness.add_session(session)
		failing = (
			"BEGIN; SET LOCAL decodex.prototype_failpoint='" + stage + "'; "
			"SET LOCAL decodex.prototype_failpoint_wait='0.35'; "
			+ harness.transition_sql(key, session, 1, "active") + " COMMIT;"
		)
		with ThreadPoolExecutor(max_workers=2) as pool:
			first = pool.submit(runtime_call, harness, failing)
			time.sleep(0.1)
			waiter = pool.submit(runtime_call, harness, harness.transition_sql(key, session, 1, "active"))
			failed = first.result()
			waited = waiter.result()
		expect(classify(failed) == "retryable_infrastructure_failure", f"{stage} not retryable")
		expect(waited.returncode == 0, f"waiter did not become executor after {stage}")
		counts = scalar(
			harness,
			"SELECT (SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key='"
			+ key + "') || ',' || (SELECT count(*) FROM decodex.prototype_activity WHERE aggregate_id='"
			+ session + "') || ',' || (SELECT count(*) FROM decodex.prototype_outbox o JOIN decodex.prototype_activity a USING(activity_id) WHERE a.aggregate_id='"
			+ session + "');",
		)
		expect(counts == "1,1,1", f"{stage} did not converge exactly once: {counts}")
		rows[stage] = {"first": failed.sqlstate, "waiter_executed": True}
	report["abort_matrix"] = rows


def prove_connection_and_lost_result(harness: Harness, report: dict[str, object]) -> None:
	session = str(uuid.uuid4())
	key = "terminated-before-commit"
	harness.add_session(session)
	sql = "BEGIN; " + harness.transition_sql(key, session, 1, "active") + " SELECT pg_sleep(10); COMMIT;"
	with ThreadPoolExecutor(max_workers=1) as pool:
		future = pool.submit(runtime_call, harness, sql, app="xy1345-kill")
		deadline = time.monotonic() + 5
		pid = ""
		while time.monotonic() < deadline:
			pid = scalar(harness, "SELECT COALESCE(max(pid)::text,'-') FROM pg_stat_activity WHERE application_name='xy1345-kill' AND query LIKE '%pg_sleep(10)%';")
			if pid != "-":
				break
			time.sleep(0.05)
		expect(pid != "-", "terminable backend did not reach pre-commit wait")
		harness.must_psql(f"SELECT pg_terminate_backend({pid});")
		terminated = future.result()
	expect(terminated.returncode != 0, "terminated connection unexpectedly committed")
	expect(classify(terminated) == "retryable_infrastructure_failure", "terminated connection was not classified")
	zero = scalar(harness, f"SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key='{key}';")
	expect(zero == "0", "connection termination left a durable receipt")
	retry = runtime_call(harness, harness.transition_sql(key, session, 1, "active"))
	expect(retry.returncode == 0, "retry after termination failed")

	lost_session = str(uuid.uuid4())
	lost_key = "commit-result-discarded"
	harness.add_session(lost_session)
	discarded = runtime_call(harness, harness.transition_sql(lost_key, lost_session, 1, "active"))
	expect(discarded.returncode == 0, "discarded-result transaction failed")
	stored = scalar(harness, f"SELECT encode(response_bytes,'hex') FROM decodex.exact_command_receipts WHERE idempotency_key='{lost_key}';")
	replayed = runtime_call(harness, harness.transition_sql(lost_key, lost_session, 1, "active"))
	expect(exact_hex(replayed) == stored, "lost-result retry did not return stored bytes")
	report["connection_loss"] = {"precommit_sqlstate": terminated.sqlstate,
		"precommit_rows": 0, "retry_effects": 1, "postcommit_replay": "byte_identical"}


def prove_stable_rejections(harness: Harness, report: dict[str, object]) -> None:
	cases: dict[str, str] = {}

	missing = str(uuid.uuid4())
	key = "reject-missing"
	first = runtime_call(harness, harness.transition_sql(key, missing, 1, "active"))
	expect(first.returncode == 0, "missing-target rejection did not commit")
	harness.add_session(missing)
	replay = runtime_call(harness, harness.transition_sql(key, missing, 1, "active"))
	expect(exact_hex(first) == exact_hex(replay), "missing-target rejection changed")
	cases["missing_target"] = "stable_domain_rejection"

	stale = str(uuid.uuid4())
	harness.add_session(stale, revision=2)
	key = "reject-stale"
	first = runtime_call(harness, harness.transition_sql(key, stale, 1, "active"))
	harness.must_psql(f"SET ROLE {OWNER_ROLE}; UPDATE decodex.prototype_runtime_sessions SET revision=1 WHERE runtime_session_id='{stale}'; RESET ROLE;")
	replay = runtime_call(harness, harness.transition_sql(key, stale, 1, "active"))
	expect(exact_hex(first) == exact_hex(replay), "stale rejection changed")
	cases["stale_revision"] = "stable_domain_rejection"

	illegal = str(uuid.uuid4())
	harness.add_session(illegal, state="active")
	key = "reject-illegal"
	first = runtime_call(harness, harness.transition_sql(key, illegal, 1, "starting"))
	harness.must_psql(f"SET ROLE {OWNER_ROLE}; UPDATE decodex.prototype_runtime_sessions SET state='starting' WHERE runtime_session_id='{illegal}'; RESET ROLE;")
	replay = runtime_call(harness, harness.transition_sql(key, illegal, 1, "starting"))
	expect(exact_hex(first) == exact_hex(replay), "illegal-transition rejection changed")
	cases["illegal_transition"] = "stable_domain_rejection"
	report["stable_rejections"] = cases


def prove_deferred_and_privileges(harness: Harness, report: dict[str, object]) -> None:
	incomplete = harness.psql(
		f"BEGIN; SET ROLE {OWNER_ROLE}; SELECT decodex.prototype_leave_incomplete('{PROTOCOL}','incomplete'); COMMIT;"
	)
	expect(incomplete.sqlstate == "23514", "deferred completion invariant did not reject commit")
	expect(scalar(harness, "SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key='incomplete';") == "0", "incomplete receipt committed")
	owner_immutable = {
		"update_completed": "SET ROLE decodex_exact_owner; UPDATE decodex.exact_command_receipts SET response_bytes=response_bytes WHERE idempotency_key='wait-replay'",
		"delete_completed": "SET ROLE decodex_exact_owner; DELETE FROM decodex.exact_command_receipts WHERE idempotency_key='wait-replay'",
		"truncate_completed": "SET ROLE decodex_exact_owner; TRUNCATE decodex.exact_command_receipts",
	}
	for name, sql in owner_immutable.items():
		result = harness.psql(sql)
		expect(result.sqlstate == "23514", f"owner could {name.replace('_', ' ')}")

	privilege_sql = {
		"select": "SELECT * FROM decodex.exact_command_receipts",
		"insert": "INSERT INTO decodex.exact_command_receipts(protocol_version,idempotency_key,request_envelope,request_digest,receipt_state) VALUES ('x','x','{}',decodex_crypto.digest(convert_to('{}','UTF8'),'sha256'),'executing')",
		"update": "UPDATE decodex.exact_command_receipts SET outcome_class='x'",
		"delete": "DELETE FROM decodex.exact_command_receipts",
		"truncate": "TRUNCATE decodex.exact_command_receipts",
	}
	privileges: dict[str, str] = {}
	for name, sql in privilege_sql.items():
		result = runtime_call(harness, sql)
		expect(result.sqlstate == "42501", f"runtime exact receipt {name} was not denied")
		privileges[name] = result.sqlstate or ""

	forgery_sql = {
		"activity_kind": "INSERT INTO decodex.prototype_activity(aggregate_kind,aggregate_id,event_kind,payload) VALUES ('runtime_session',gen_random_uuid(),'runtime_session_transitioned','{}')",
		"activity_structured": "INSERT INTO decodex.prototype_activity(aggregate_kind,aggregate_id,event_kind,payload) VALUES ('other',gen_random_uuid(),'other','{\"aggregate_kind\":\"runtime_session\",\"event_kind\":\"runtime_session_transitioned\"}')",
		"outbox_effect": "INSERT INTO decodex.prototype_outbox(activity_id,effect_key,payload) VALUES (1,'runtime-session-transition:forged','{}')",
		"outbox_link": "INSERT INTO decodex.prototype_outbox(activity_id,effect_key,payload) VALUES (1,'other','{\"activity_id\":1,\"event\":{\"aggregate_kind\":\"runtime_session\"}}')",
	}
	for name, sql in forgery_sql.items():
		result = runtime_call(harness, sql)
		expect(result.sqlstate == "42501", f"runtime canonical forgery {name} was not denied")
		privileges[name] = result.sqlstate or ""

	closure = scalar(
		harness,
		"SELECT (SELECT rolname FROM pg_roles r JOIN pg_proc p ON p.proowner=r.oid WHERE p.oid='decodex.transition_runtime_session_exact(text,text,uuid,bigint,decodex.prototype_session_state,text)'::regprocedure) || ',' || "
		"(SELECT prosecdef::text FROM pg_proc WHERE oid='decodex.transition_runtime_session_exact(text,text,uuid,bigint,decodex.prototype_session_state,text)'::regprocedure) || ',' || "
		"pg_has_role('decodex_exact_runtime','decodex_exact_owner','MEMBER')::text || ',' || "
		"(SELECT count(*) FROM pg_proc WHERE pronamespace='decodex'::regnamespace AND proname='transition_runtime_session_exact');"
	)
	expect(closure == f"{OWNER_ROLE},true,false,1", f"catalog closure mismatch: {closure}")
	report["authority"] = {"runtime_denials": privileges, "definer_closure": closure,
		"incomplete_commit_sqlstate": incomplete.sqlstate,
		"completed_owner_rewrite_denials": {name: "23514" for name in owner_immutable}}


def prove_envelopes(harness: Harness, report: dict[str, object]) -> None:
	u = [str(uuid.uuid4()) for _ in range(5)]
	create = scalar(
		harness,
		"SELECT decodex.build_runtime_session_create_request("
		f"'{PROTOCOL}','{u[0]}','{u[1]}','task','{u[2]}','{u[3]}','label','available',7,NULL,'starting')::text;",
		role=OWNER_ROLE,
	)
	create_json = json.loads(create)
	expect("codex_thread_id" in create_json and create_json["codex_thread_id"] is None, "optional create key is absent")
	transition = scalar(
		harness,
		f"SELECT decodex.build_runtime_session_transition_request('{PROTOCOL}','{u[0]}',1,'active',NULL)::text;",
		role=OWNER_ROLE,
	)
	expect(json.loads(transition).get("note", "absent") is None, "optional transition key is absent")

	literal = scalar(harness, f"SELECT decodex.build_runtime_session_transition_request('{PROTOCOL}','{u[0]}',1,'active',NULL)::text;", role=OWNER_ROLE)
	bound = scalar(harness, f"PREPARE typed(bigint) AS SELECT decodex.build_runtime_session_transition_request('{PROTOCOL}','{u[0]}',$1,'active',NULL)::text; EXECUTE typed(1);", role=OWNER_ROLE)
	casted = scalar(harness, f"SELECT decodex.build_runtime_session_transition_request('{PROTOCOL}','{u[0]}',('1'::text)::bigint,'active',NULL)::text;", role=OWNER_ROLE)
	expect(literal == bound == casted, "typed integer inputs did not converge")

	bootstrap_args = ",".join(["'m'","'medium'","'standard'","'instructions'","NULL"] * 4)
	bootstrap = scalar(harness, f"SELECT decodex.build_role_profile_bootstrap_request('{PROTOCOL}',{bootstrap_args})::text;", role=OWNER_ROLE)
	profiles = json.loads(bootstrap)["profiles"]
	expect([item["role"] for item in profiles] == ["advisor","lead","task","reviewer"], "bootstrap role order is ambiguous")
	expect(all("provenance" in item and item["provenance"] is None for item in profiles), "bootstrap optional null is absent")
	incomplete = harness.psql(
		f"SELECT decodex.build_role_profile_bootstrap_request('{PROTOCOL}',NULL,{','.join(["'x'"] * 19)});",
		role=OWNER_ROLE,
	)
	expect(incomplete.sqlstate == "22004", "incomplete bootstrap shape was accepted")

	base_session = str(uuid.uuid4())
	harness.add_session(base_session)
	base = runtime_call(harness, harness.transition_sql("exact-text", base_session, 1, "active", "café"))
	expect(base.returncode == 0, "exact-text baseline failed")
	variants = ["café", "Café", "café "]
	for value in variants:
		result = runtime_call(harness, harness.transition_sql("exact-text", base_session, 1, "active", value))
		expect(classify(result) == "idempotency_conflict", f"text variant did not conflict: {value!r}")
	report["envelopes"] = {"optional_keys_explicit_null": True, "typed_integer_convergence": True, "bootstrap_fixed_roles": True, "exact_text_conflicts": len(variants)}


def run_isolation_pair(harness: Harness, isolation: str, key: str, session: str) -> tuple[Result, Result]:
	call = harness.transition_sql(key, session, 1, "active")
	first_sql = f"BEGIN ISOLATION LEVEL {isolation}; {call} SELECT pg_sleep(0.6); COMMIT;"
	second_sql = f"BEGIN ISOLATION LEVEL {isolation}; {call} COMMIT;"
	with ThreadPoolExecutor(max_workers=2) as pool:
		first = pool.submit(runtime_call, harness, first_sql)
		time.sleep(0.12)
		second = pool.submit(runtime_call, harness, second_sql)
		return first.result(), second.result()


def prove_isolation_and_deadlock(harness: Harness, report: dict[str, object]) -> None:
	results: dict[str, object] = {}
	for isolation in ("READ COMMITTED", "REPEATABLE READ", "SERIALIZABLE"):
		session = str(uuid.uuid4())
		key = "isolation-" + isolation.lower().replace(" ", "-")
		harness.add_session(session)
		first, second = run_isolation_pair(harness, isolation, key, session)
		classes = [classify(first), classify(second)]
		expect(classes[0] == "success", f"first {isolation} transaction failed")
		expect(classes[1] in {"success", "retryable_infrastructure_failure"}, f"unexpected {isolation} outcome")
		if second.returncode != 0:
			expect(second.sqlstate == "40001", f"unexpected isolation SQLSTATE {second.sqlstate}")
			retry = runtime_call(harness, f"BEGIN ISOLATION LEVEL {isolation}; {harness.transition_sql(key, session, 1, 'active')} COMMIT;")
			expect(retry.returncode == 0, f"whole transaction retry failed for {isolation}")
		effects = scalar(harness, f"SELECT count(*) FROM decodex.prototype_activity WHERE aggregate_id='{session}';")
		expect(effects == "1", f"duplicate effect under {isolation}")
		results[isolation] = {"initial": classes, "second_sqlstate": second.sqlstate, "effects": 1}

	a, b = str(uuid.uuid4()), str(uuid.uuid4())
	harness.add_session(a)
	harness.add_session(b)
	call_a = harness.transition_sql("deadlock-a", a, 1, "active")
	call_b = harness.transition_sql("deadlock-b", b, 1, "active")
	tx1 = "BEGIN; " + call_a + " SELECT pg_sleep(0.35); " + call_b + " COMMIT;"
	tx2 = "BEGIN; " + call_b + " SELECT pg_sleep(0.35); " + call_a + " COMMIT;"
	with ThreadPoolExecutor(max_workers=2) as pool:
		one = pool.submit(runtime_call, harness, tx1)
		two = pool.submit(runtime_call, harness, tx2)
		deadlock_results = [one.result(), two.result()]
	expect(sorted(r.sqlstate or "success" for r in deadlock_results) == ["40P01", "success"], "opposite-order schedule did not produce one classified deadlock")
	loser_sql = tx1 if deadlock_results[0].returncode != 0 else tx2
	retry = runtime_call(harness, loser_sql)
	expect(retry.returncode == 0, "deadlock whole-transaction retry failed")
	expect(scalar(harness, f"SELECT count(*) FROM decodex.prototype_activity WHERE aggregate_id IN ('{a}','{b}');") == "2", "deadlock retry duplicated or lost effects")
	results["opposite_order"] = {"sqlstate": "40P01", "retry": "converged", "effects": 2}
	report["isolation"] = results


def stress_round(harness: Harness, repetition: int, mixed: bool) -> dict[str, object]:
	session = str(uuid.uuid4())
	key = f"stress-{'mixed' if mixed else 'same'}-{repetition}"
	harness.add_session(session)
	requests = []
	for index in range(STRESS_WIDTH):
		note = "alpha" if not mixed or index < STRESS_WIDTH // 2 else "beta"
		requests.append(harness.transition_sql(key, session, 1, "active", note))
	with ThreadPoolExecutor(max_workers=STRESS_WIDTH) as pool:
		observed = list(pool.map(lambda sql: runtime_call(harness, sql), requests))
	classes = [classify(item) for item in observed]
	successes = [exact_hex(item) for item in observed if item.returncode == 0]
	expect(successes and len(set(successes)) == 1, "stress responses mismatched")
	if mixed:
		expect(classes.count("success") == STRESS_WIDTH // 2, "mixed stress winner cardinality changed")
		expect(classes.count("idempotency_conflict") == STRESS_WIDTH // 2, "mixed stress conflict cardinality changed")
	else:
		expect(classes == ["success"] * STRESS_WIDTH, "identical stress did not fully replay")
	counts = scalar(
		harness,
		"SELECT (SELECT count(*) FROM decodex.prototype_activity WHERE aggregate_id='" + session + "') || ',' || "
		"(SELECT count(*) FROM decodex.prototype_outbox o JOIN decodex.prototype_activity a USING(activity_id) WHERE a.aggregate_id='" + session + "') || ',' || "
		"(SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key='" + key + "' AND receipt_state='executing');",
	)
	expect(counts == "1,1,0", f"stress invariant mismatch: {counts}")
	return {"success": classes.count("success"), "conflict": classes.count("idempotency_conflict")}


def prove_stress(harness: Harness, report: dict[str, object]) -> None:
	identical = [stress_round(harness, i, False) for i in range(STRESS_REPETITIONS)]
	mixed = [stress_round(harness, i, True) for i in range(STRESS_REPETITIONS)]
	report["stress"] = {
		"identical": {"repetitions": STRESS_REPETITIONS, "width": STRESS_WIDTH,
			"successes": sum(item["success"] for item in identical), "anomalies": 0},
		"mixed": {"repetitions": STRESS_REPETITIONS, "width": STRESS_WIDTH,
			"successes": sum(item["success"] for item in mixed),
			"conflicts": sum(item["conflict"] for item in mixed), "anomalies": 0},
	}


def prove_effect_binding_and_restore(harness: Harness, report: dict[str, object]) -> dict[str, int]:
	mismatch = int(scalar(
		harness,
		"WITH decoded AS (SELECT r.*, pg_catalog.convert_from(r.response_bytes,'UTF8')::pg_catalog.jsonb AS response "
		"FROM decodex.exact_command_receipts r WHERE r.receipt_state='completed_success') "
		"SELECT count(*) FROM decoded r "
		"LEFT JOIN decodex.prototype_runtime_sessions s ON s.runtime_session_id=(r.effect_envelope->'persisted_session'->>'runtime_session_id')::pg_catalog.uuid "
		"LEFT JOIN decodex.prototype_activity a ON a.activity_id=(r.effect_envelope->>'activity_id')::pg_catalog.int8 "
		"LEFT JOIN decodex.prototype_outbox o ON o.outbox_id=(r.effect_envelope->>'outbox_id')::pg_catalog.int8 "
		"WHERE r.response->>'classification' IS DISTINCT FROM 'success' "
		"OR r.response->'effect' IS DISTINCT FROM r.effect_envelope "
		"OR s.runtime_session_id IS NULL OR r.effect_envelope->'persisted_session' IS DISTINCT FROM pg_catalog.jsonb_build_object("
		"'runtime_session_id',s.runtime_session_id,'state',s.state,'revision',s.revision,'updated_at',s.updated_at) "
		"OR a.activity_id IS NULL OR a.aggregate_kind IS DISTINCT FROM 'runtime_session' "
		"OR a.aggregate_id IS DISTINCT FROM s.runtime_session_id OR a.event_kind IS DISTINCT FROM 'runtime_session_transitioned' "
		"OR a.payload IS DISTINCT FROM r.effect_envelope->'activity_payload' "
		"OR o.outbox_id IS NULL OR o.activity_id IS DISTINCT FROM a.activity_id "
		"OR o.effect_key IS DISTINCT FROM 'runtime-session-transition:' || a.activity_id::pg_catalog.text "
		"OR o.payload IS DISTINCT FROM r.effect_envelope->'outbox_payload';",
	))
	expect(mismatch == 0, f"decoded response/effect rows have {mismatch} persisted binding mismatches")
	catalog_sql = """
	WITH function_rows AS (
		SELECT pg_catalog.jsonb_build_object(
			'name', p.proname,
			'signature', p.oid::pg_catalog.regprocedure::pg_catalog.text,
			'identity_types', pg_catalog.oidvectortypes(p.proargtypes),
			'owner', owner_role.rolname,
			'prosecdef', p.prosecdef,
			'language', language.lanname,
			'volatility', p.provolatile,
			'parallel', p.proparallel,
			'settings', p.proconfig,
			'source_sha256', pg_catalog.encode(decodex_crypto.digest(pg_catalog.convert_to(p.prosrc,'UTF8'),'sha256'),'hex'),
			'definition_sha256', pg_catalog.encode(decodex_crypto.digest(pg_catalog.convert_to(pg_catalog.pg_get_functiondef(p.oid),'UTF8'),'sha256'),'hex'),
			'public_execute', pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE'),
			'runtime_execute', pg_catalog.has_function_privilege('decodex_exact_runtime', p.oid, 'EXECUTE'),
			'owner_execute', pg_catalog.has_function_privilege('decodex_exact_owner', p.oid, 'EXECUTE'),
			'overloads', (SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc q WHERE q.pronamespace=p.pronamespace AND q.proname=p.proname),
			'semantic_acl', COALESCE((
				SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
					'grantor', grantor_role.rolname,
					'grantee', COALESCE(grantee_role.rolname,'PUBLIC'),
					'privilege', privilege.privilege_type,
					'grantable', privilege.is_grantable
				) ORDER BY grantor_role.rolname,COALESCE(grantee_role.rolname,'PUBLIC'),privilege.privilege_type,privilege.is_grantable)
				FROM pg_catalog.aclexplode(COALESCE(p.proacl,pg_catalog.acldefault('f',p.proowner))) privilege
				JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=privilege.grantor
				LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=privilege.grantee
			), '[]'::pg_catalog.jsonb),
			'dependencies', COALESCE((
				SELECT pg_catalog.jsonb_agg(pg_catalog.pg_describe_object(d.refclassid,d.refobjid,d.refobjsubid) || ':' || d.deptype::pg_catalog.text
					ORDER BY pg_catalog.pg_describe_object(d.refclassid,d.refobjid,d.refobjsubid),d.deptype)
				FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_proc'::pg_catalog.regclass AND d.objid=p.oid
			), '[]'::pg_catalog.jsonb)
		) AS item
		FROM pg_catalog.pg_proc p
		JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=p.proowner
		JOIN pg_catalog.pg_language language ON language.oid=p.prolang
		WHERE p.pronamespace='decodex'::pg_catalog.regnamespace
	), receipt_relation AS (
		SELECT c.* FROM pg_catalog.pg_class c WHERE c.oid='decodex.exact_command_receipts'::pg_catalog.regclass
	)
	SELECT pg_catalog.jsonb_build_object(
		'functions', (SELECT pg_catalog.jsonb_agg(item ORDER BY item->>'signature') FROM function_rows),
		'relation_owner', (SELECT r.rolname FROM receipt_relation c JOIN pg_catalog.pg_roles r ON r.oid=c.relowner),
		'relation_effective', pg_catalog.jsonb_build_object(
			'owner', pg_catalog.jsonb_build_object(
				'SELECT',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','SELECT'),
				'INSERT',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','INSERT'),
				'UPDATE',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','UPDATE'),
				'DELETE',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','DELETE'),
				'TRUNCATE',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','TRUNCATE'),
				'REFERENCES',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','REFERENCES'),
				'TRIGGER',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','TRIGGER'),
				'MAINTAIN',pg_catalog.has_table_privilege('decodex_exact_owner','decodex.exact_command_receipts','MAINTAIN')),
			'runtime', pg_catalog.jsonb_build_object(
				'SELECT',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','SELECT'),
				'INSERT',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','INSERT'),
				'UPDATE',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','UPDATE'),
				'DELETE',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','DELETE'),
				'TRUNCATE',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','TRUNCATE'),
				'REFERENCES',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','REFERENCES'),
				'TRIGGER',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','TRIGGER'),
				'MAINTAIN',pg_catalog.has_table_privilege('decodex_exact_runtime','decodex.exact_command_receipts','MAINTAIN')),
			'public', pg_catalog.jsonb_build_object(
				'SELECT',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','SELECT'),
				'INSERT',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','INSERT'),
				'UPDATE',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','UPDATE'),
				'DELETE',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','DELETE'),
				'TRUNCATE',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','TRUNCATE'),
				'REFERENCES',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','REFERENCES'),
				'TRIGGER',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','TRIGGER'),
				'MAINTAIN',pg_catalog.has_table_privilege('public','decodex.exact_command_receipts','MAINTAIN'))
		),
		'relation_semantic_acl', (SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'grantor',grantor_role.rolname,'grantee',COALESCE(grantee_role.rolname,'PUBLIC'),
			'privilege',privilege.privilege_type,'grantable',privilege.is_grantable
		) ORDER BY grantor_role.rolname,COALESCE(grantee_role.rolname,'PUBLIC'),privilege.privilege_type,privilege.is_grantable)
		FROM receipt_relation c
		CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) privilege
		JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=privilege.grantor
		LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=privilege.grantee),
		'default_privileges', COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'role',owner_role.rolname,'schema',namespace.nspname,'type',d.defaclobjtype,
			'grantor',grantor_role.rolname,'grantee',COALESCE(grantee_role.rolname,'PUBLIC'),
			'privilege',privilege.privilege_type,'grantable',privilege.is_grantable
		) ORDER BY owner_role.rolname,namespace.nspname,d.defaclobjtype,grantor_role.rolname,COALESCE(grantee_role.rolname,'PUBLIC'),privilege.privilege_type)
		FROM pg_catalog.pg_default_acl d
		JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=d.defaclrole
		LEFT JOIN pg_catalog.pg_namespace namespace ON namespace.oid=d.defaclnamespace
		CROSS JOIN LATERAL pg_catalog.aclexplode(d.defaclacl) privilege
		JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=privilege.grantor
		LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=privilege.grantee), '[]'::pg_catalog.jsonb),
		'role_memberships', COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'role',role_role.rolname,'member',member_role.rolname,'admin',membership.admin_option
		) ORDER BY role_role.rolname,member_role.rolname)
		FROM pg_catalog.pg_auth_members membership
		JOIN pg_catalog.pg_roles role_role ON role_role.oid=membership.roleid
		JOIN pg_catalog.pg_roles member_role ON member_role.oid=membership.member
		WHERE role_role.rolname IN ('decodex_exact_owner','decodex_exact_runtime')
			OR member_role.rolname IN ('decodex_exact_owner','decodex_exact_runtime')), '[]'::pg_catalog.jsonb),
		'triggers', (SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
			'name',t.tgname,'constraint',t.tgconstraint<>0,'deferrable',t.tgdeferrable,
			'initially_deferred',t.tginitdeferred,'enabled',t.tgenabled,
			'function',t.tgfoid::pg_catalog.regprocedure::pg_catalog.text,
			'definition',pg_catalog.pg_get_triggerdef(t.oid,true)
		) ORDER BY t.tgname) FROM pg_catalog.pg_trigger t
		WHERE t.tgrelid='decodex.exact_command_receipts'::pg_catalog.regclass AND NOT t.tgisinternal)
	)::pg_catalog.text;
	"""
	catalog_before_text = scalar(harness, catalog_sql)
	catalog_before = json.loads(catalog_before_text)
	functions = {item["name"]: item for item in catalog_before["functions"]}
	expect(set(functions) == set(EXPECTED_FUNCTIONS), f"function identity set drifted: {sorted(functions)}")
	for name, (identity_types, language, volatility, prosecdef, runtime_execute) in EXPECTED_FUNCTIONS.items():
		item = functions[name]
		expect(item["identity_types"] == identity_types, f"{name} signature drifted: {item['identity_types']}")
		expected_regprocedure = f"decodex.{name}({identity_types.replace(', ', ',')})"
		expect(item["signature"] == expected_regprocedure, f"{name} regprocedure identity drifted: {item['signature']}")
		expect(item["owner"] == OWNER_ROLE and item["overloads"] == 1, f"{name} owner/overload closure failed")
		expect(item["language"] == language and item["volatility"] == volatility, f"{name} language/volatility drifted")
		expect(item["parallel"] == "u" and item["prosecdef"] is prosecdef, f"{name} execution metadata drifted")
		expect(item["settings"] == ["search_path=pg_catalog, decodex"], f"{name} trusted search path drifted")
		expect(item["owner_execute"] and not item["public_execute"] and item["runtime_execute"] is runtime_execute,
			f"{name} effective execute closure failed")
		expected_grantees = {OWNER_ROLE} | ({RUNTIME_ROLE} if runtime_execute else set())
		expect({entry["grantee"] for entry in item["semantic_acl"]} == expected_grantees,
			f"{name} has unexpected function grantees: {item['semantic_acl']}")
		expect(all(entry["grantor"] == OWNER_ROLE and entry["privilege"] == "EXECUTE" and not entry["grantable"]
			for entry in item["semantic_acl"]), f"{name} semantic function ACL drifted")
		expect(re.fullmatch(r"[0-9a-f]{64}", item["definition_sha256"]) is not None and item["dependencies"],
			f"{name} source/dependency closure is incomplete")
	expect(catalog_before["relation_owner"] == OWNER_ROLE, "receipt relation owner drifted")
	expect(all(catalog_before["relation_effective"]["owner"].get(privilege) is True for privilege in TABLE_PRIVILEGES),
		"receipt owner-effective privilege closure failed")
	for role in ("runtime", "public"):
		expect(all(catalog_before["relation_effective"][role].get(privilege) is False for privilege in TABLE_PRIVILEGES),
			f"receipt {role} effective privilege closure failed")
	relation_acl = catalog_before["relation_semantic_acl"]
	expect({entry["privilege"] for entry in relation_acl} == set(TABLE_PRIVILEGES),
		f"receipt semantic privilege set drifted: {relation_acl}")
	expect(all(entry["grantor"] == OWNER_ROLE and entry["grantee"] == OWNER_ROLE and not entry["grantable"]
		for entry in relation_acl), f"receipt relation has an unexpected grantee/grant option: {relation_acl}")
	expect(catalog_before["default_privileges"] == [{"role": OWNER_ROLE, "schema": None, "type": "f",
		"grantor": OWNER_ROLE, "grantee": OWNER_ROLE, "privilege": "EXECUTE", "grantable": False}],
		f"default function privilege closure drifted: {catalog_before['default_privileges']}")
	expect(catalog_before["role_memberships"] == [], f"owner/runtime role membership path exists: {catalog_before['role_memberships']}")
	triggers = {item["name"]: item for item in catalog_before["triggers"]}
	expect(set(triggers) == {"exact_receipts_complete_at_commit", "exact_receipts_immutable", "exact_receipts_untruncatable"},
		f"receipt trigger identity set drifted: {sorted(triggers)}")
	expect(triggers["exact_receipts_complete_at_commit"]["function"] == "decodex.enforce_exact_receipt_completion()"
		and triggers["exact_receipts_complete_at_commit"]["constraint"]
		and triggers["exact_receipts_complete_at_commit"]["deferrable"]
		and triggers["exact_receipts_complete_at_commit"]["initially_deferred"], "deferred completion trigger closure failed")
	expect(triggers["exact_receipts_immutable"]["function"] == "decodex.forbid_exact_receipt_rewrite()"
		and not triggers["exact_receipts_immutable"]["constraint"], "immutable trigger closure failed")
	expect(triggers["exact_receipts_untruncatable"]["function"] == "decodex.forbid_exact_receipt_truncate()"
		and not triggers["exact_receipts_untruncatable"]["constraint"], "truncate trigger closure failed")
	expect(all(item["enabled"] == "O" and item["definition"] for item in triggers.values()), "receipt trigger enablement/definition drifted")

	before = scalar(
		harness,
		"SELECT count(*)||','||encode(decodex_crypto.digest(string_agg(encode(response_bytes,'hex'),'' ORDER BY protocol_version,idempotency_key),'sha256'),'hex') "
		"FROM decodex.exact_command_receipts WHERE receipt_state<>'executing';",
	)
	dump = harness.root / "xy1345.dump"
	harness.require(["pg_dump", "-Fc", "-f", str(dump), DATABASE])
	harness.require(["createdb", RESTORE_DATABASE])
	harness.require(["pg_restore", "-d", RESTORE_DATABASE, str(dump)])
	after = scalar(
		harness,
		"SELECT count(*)||','||encode(decodex_crypto.digest(string_agg(encode(response_bytes,'hex'),'' ORDER BY protocol_version,idempotency_key),'sha256'),'hex') "
		"FROM decodex.exact_command_receipts WHERE receipt_state<>'executing';",
		database=RESTORE_DATABASE,
	)
	expect(before == after, "restore changed receipt count or response bytes")
	data_sql = """
	SELECT pg_catalog.jsonb_build_object(
		'receipts', (SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(r) ORDER BY protocol_version,idempotency_key),'[]'::pg_catalog.jsonb) FROM decodex.exact_command_receipts r),
		'sessions', (SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(s) ORDER BY runtime_session_id),'[]'::pg_catalog.jsonb) FROM decodex.prototype_runtime_sessions s),
		'activity', (SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(a) ORDER BY activity_id),'[]'::pg_catalog.jsonb) FROM decodex.prototype_activity a),
		'outbox', (SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(o) ORDER BY outbox_id),'[]'::pg_catalog.jsonb) FROM decodex.prototype_outbox o)
	)::pg_catalog.text;
	"""
	data_before_text = scalar(harness, data_sql)
	data_after_text = scalar(harness, data_sql, database=RESTORE_DATABASE)
	expect(data_before_text == data_after_text, "restore changed persisted receipts, sessions, activity, or outbox rows")
	catalog_after_text = scalar(harness, catalog_sql, database=RESTORE_DATABASE)
	catalog_after = json.loads(catalog_after_text)
	drift = {key: [catalog_before.get(key), catalog_after.get(key)]
		for key in catalog_before if catalog_before.get(key) != catalog_after.get(key)}
	expect(not drift, f"restore changed catalog/source/dependency/ACL closure: {drift}")
	duplicate_domain = int(scalar(harness, "SELECT count(*) FROM (SELECT aggregate_id,event_kind FROM decodex.prototype_activity GROUP BY aggregate_id,event_kind HAVING count(*)>1) duplicate;"))
	duplicate_pairs = int(scalar(harness, "SELECT count(*) FROM (SELECT activity_id FROM decodex.prototype_outbox GROUP BY activity_id HAVING count(*)>1) duplicate;"))
	unexplained = int(scalar(harness,
		"SELECT (SELECT count(*) FROM decodex.prototype_activity a LEFT JOIN decodex.exact_command_receipts r ON (r.effect_envelope->>'activity_id')::bigint=a.activity_id WHERE r.protocol_version IS NULL) + "
		"(SELECT count(*) FROM decodex.prototype_outbox o LEFT JOIN decodex.exact_command_receipts r ON (r.effect_envelope->>'outbox_id')::bigint=o.outbox_id WHERE r.protocol_version IS NULL);"))
	authority_bypasses = sum(1 for role in ("runtime", "public") for privilege in TABLE_PRIVILEGES
		if catalog_before["relation_effective"][role][privilege]) + sum(1 for item in functions.values()
		if item["public_execute"] or item["runtime_execute"] != (item["name"] == "transition_runtime_session_exact")) + len(catalog_before["role_memberships"])
	anomalies = {"duplicate_domain_effects": duplicate_domain, "duplicate_activity_outbox_pairs": duplicate_pairs,
		"mismatched_responses": mismatch, "authority_bypasses": authority_bypasses, "unexplained_rows": unexplained}
	expect(not any(anomalies.values()), f"final persisted anomaly counters are nonzero: {anomalies}")
	report["effect_restore"] = {"binding_mismatches": mismatch,
		"receipt_bytes_digest": before.split(",", 1)[1], "restored_equal": True,
		"persisted_data_sha256": hashlib.sha256(data_before_text.encode("utf-8")).hexdigest(),
		"catalog_source_acl_dependency_restore_equal": True,
		"attested_function_count": len(functions),
		"function_definition_manifest_sha256": hashlib.sha256(json.dumps(
			{name: item["definition_sha256"] for name, item in sorted(functions.items())},
			sort_keys=True, separators=(",", ":"),
		).encode("utf-8")).hexdigest(),
		"function_definition_sha256": {name: item["definition_sha256"] for name, item in sorted(functions.items())},
		"transition_function_source_sha256": functions["transition_runtime_session_exact"]["source_sha256"]}
	return anomalies


def build_report(harness: Harness) -> dict[str, object]:
	version = harness.require(["postgres", "--version"]).stdout
	expect(version == "postgres (PostgreSQL) 18.4", f"exact PostgreSQL version changed: {version}")
	report: dict[str, object] = {
		"schema": "decodex/xy-1345-exact-command-proof/1",
		"postgres": version,
		"protocol": PROTOCOL,
		"harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
		"schema_sql_sha256": hashlib.sha256(SCHEMA_SQL.encode("utf-8")).hexdigest(),
		"classification": {
			"stable_domain_rejection": ["missing_target", "stale_revision", "illegal_transition"],
			"idempotency_conflict": ["DX001"],
			"retryable_infrastructure_failure": ["40001", "40P01", "08006", "57P01", "DX900"],
		},
	}
	prove_wait_replay_and_conflicts(harness, report)
	prove_abort_matrix(harness, report)
	prove_connection_and_lost_result(harness, report)
	prove_stable_rejections(harness, report)
	prove_deferred_and_privileges(harness, report)
	prove_envelopes(harness, report)
	prove_isolation_and_deadlock(harness, report)
	prove_stress(harness, report)
	anomalies = prove_effect_binding_and_restore(harness, report)
	committed_executing = int(scalar(harness, "SELECT count(*) FROM decodex.exact_command_receipts WHERE receipt_state='executing';"))
	expect(committed_executing == 0, f"{committed_executing} committed executing receipts remain")
	report["final_anomalies"] = {**anomalies, "committed_executing_rows": committed_executing,
		"unclassified_sqlstates": 0}
	return report


def main() -> int:
	root = Path(tempfile.mkdtemp(prefix="decodex-xy1345-")).resolve()
	harness = Harness(root)
	report: dict[str, object] | None = None
	error: Exception | None = None
	try:
		harness.start()
		report = build_report(harness)
	except Exception as exc:  # cleanup must also cover architectural falsifiers
		error = exc
	finally:
		try:
			cluster_stopped = harness.stop()
		except Exception as cleanup_exc:
			cluster_stopped = False
			if error is None:
				error = cleanup_exc
			else:
				error = ProofFailure(f"proof failed: {error}; PostgreSQL shutdown also failed: {cleanup_exc}")
		if cluster_stopped:
			try:
				shutil.rmtree(root)
			except Exception as cleanup_exc:
				if error is None:
					error = ProofFailure(f"temporary proof root removal failed: {cleanup_exc}")
		cleaned = not root.exists()
		if not cleaned and error is None:
			error = ProofFailure("temporary proof root was not removed")
	if error is not None:
		print(json.dumps({"result": "FAILED", "error": str(error),
			"traceback": "".join(traceback.format_exception(error)),
			"cleanup": {"cluster_stopped": cluster_stopped, "temporary_root_removed": cleaned}}, indent=2), file=sys.stderr)
		return 1
	assert report is not None
	report["cleanup"] = {"cluster_stopped": cluster_stopped, "temporary_root_removed": cleaned,
		"tcp_disabled": True, "existing_services_enumerated_or_modified": False}
	report["result"] = "PASSED"
	print(json.dumps(report, indent=2, sort_keys=True))
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
