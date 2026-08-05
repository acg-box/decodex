#!/usr/bin/env python3
"""Run the canonical product-native latest-schema gate."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "crates/decodex-postgres/schema.sql"
SCHEMA_OWNER = "decodex_schema_owner"
RUNTIME_ROLE = "decodex_runtime"
ADMIN_ROLE = "decodex_gate_admin"
DATABASE = "decodex"
PORT = 55_432
POSTGRES_MAJOR = 18
COMMAND_TIMEOUT_SECONDS = 30 * 60
OUTPUT_TAIL_BYTES = 4_096
SOURCE_LIST_BYTES = 4 * 1_024 * 1_024
SOURCE_FILE_BYTES = 2 * 1_024 * 1_024
SOURCE_TOTAL_BYTES = 16 * 1_024 * 1_024
SECOND_BOOTSTRAP_REFUSAL_DIAGNOSTIC = "Error: Database(Incompatible)"
DIAGNOSTIC_COMMAND_BYTES = 32 * 1_024
DIAGNOSTIC_LOG_BYTES = 128 * 1_024
FAILURE_LOG_FILE_BYTES = 1 * 1_024 * 1_024
FAILURE_LOG_TOTAL_BYTES = 16 * 1_024 * 1_024
COPY_BUFFER_BYTES = 64 * 1_024
FAILURE_EVIDENCE_PARENT = Path("/private/tmp")
BOOTSTRAP_AUTHORITY_REPORT_PREFIX = b"DECODEX_BOOTSTRAP_AUTHORITY_REPORT="
BOOTSTRAP_AUTHORITY_REPORT_SCHEMA = "decodex/bootstrap-authority-report/1"
BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES = 16 * 1024
BOOTSTRAP_PLATFORM_NAMES = (
    "postgres_major",
    "data_checksums",
    "trusted_search_path",
    "trusted_time_zone",
    "trusted_time_zone_offset",
    "database_envelope",
    "pgcrypto_present",
    "pgcrypto_version",
)
BOOTSTRAP_NAMESPACE_NAMES = ("namespace_present", "namespace_owner")
BOOTSTRAP_SEMANTIC_NAMES = (
    "configured_runtime_session",
    "no_forbidden_role_attributes",
    "no_database_create",
    "no_schema_create",
    "no_effective_object_ownership",
    "no_function_grant_option",
    "no_trigger_bypass",
    "no_alter_system_bypass",
    "session_replication_role_origin",
    "no_membership_admin",
    "exact_table_authority",
    "no_unsafe_table_authority",
    "exact_sequence_contract",
    "sequence_usage",
    "no_unsafe_sequence_authority",
    "process_generation_type_usage",
    "no_public_process_generation_type_usage",
    "no_process_generation_type_grant_option",
    "provider_attempt_type_usage",
    "no_public_provider_attempt_type_usage",
    "no_provider_attempt_type_grant_option",
    "no_extension_control",
    "schema_usage",
    "identity_cast_closed",
    "exact_trigger_inventory",
    "no_relation_rules",
    "no_relation_policies",
    "closed_function_dependencies",
    "exact_function_inventory",
    "function_metadata",
    "function_semantics",
    "function_execute_authority",
    "retention_inventory",
    "retention_trigger_bindings",
    "retention_function_metadata",
    "retention_function_semantics",
    "no_unexpected_runtime_security_definer_authority",
)
BOOTSTRAP_SEMANTIC_INCOMPATIBLE = frozenset(
    {
        "exact_table_authority",
        "exact_sequence_contract",
        "sequence_usage",
        "process_generation_type_usage",
        "provider_attempt_type_usage",
        "schema_usage",
        "function_semantics",
        "function_execute_authority",
        "retention_inventory",
        "retention_function_metadata",
        "retention_function_semantics",
    }
)
BOOTSTRAP_PLATFORM_INCOMPATIBLE = frozenset(
    {
        "postgres_major",
        "data_checksums",
        "database_envelope",
        "pgcrypto_present",
        "pgcrypto_version",
    }
)
BOOTSTRAP_NAMESPACE_INCOMPATIBLE = frozenset({"namespace_present"})
BOOTSTRAP_QUERY_FAILURE_CATEGORIES = frozenset(
    {
        "authentication",
        "authorization",
        "authority",
        "catalog",
        "constraint",
        "evidence",
        "host_path",
        "internal",
        "server",
        "transaction",
        "transport",
    }
)
BOOTSTRAP_QUERY_FAILURE_CLASSIFICATIONS = {
    "authentication": "authentication",
    "authorization": "incompatible",
    "authority": "unsafe_authority",
    "catalog": "incompatible",
    "constraint": "incompatible",
    "evidence": "incompatible",
    "host_path": "unsafe_host_path",
    "internal": "unreachable",
    "server": "incompatible",
    "transaction": "incompatible",
    "transport": "unreachable",
}
BOOTSTRAP_QUERY_FAILURE_OPERATIONS = {
    "platform": ("platform", False, 0),
    "initial_authorization": ("initial_authorization", True, 0),
    "namespace": ("authority", True, 0),
    "semantic": ("authority", True, 1),
    "configured_authority": ("authority", True, 2),
    "schema_contract": ("authority", True, 3),
}
GATE_LOG_NAMES = frozenset(
    {
        "account-contract.log",
        "bootstrap-diagnostic.log",
        "bootstrap.log",
        "build-decodexd.log",
        "changed-adapter-sql.log",
        "cluster-init.log",
        "cluster-setup.log",
        "postgres.log",
        "quick-task.log",
        "runtime-validation.log",
        "second-bootstrap.log",
        "validation-after-refusal.log",
    }
)


class GateFailure(RuntimeError):
    """One bounded gate stage failed."""


class GateLogDirectory:
    """Descriptor-pinned owner for the gate's fixed log files."""

    def __init__(self, descriptor: int, identity: tuple[int, int, int, int]):
        self._descriptor = descriptor
        self._identity = identity

    @classmethod
    def create(cls, path: Path) -> GateLogDirectory:
        path.mkdir(mode=0o700)
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
        descriptor = os.open(path, flags)
        try:
            os.fchmod(descriptor, 0o700)
            named = path.lstat()
            pinned = os.fstat(descriptor)
            identity = (pinned.st_dev, pinned.st_ino, pinned.st_uid, pinned.st_mode)
            if (
                not stat.S_ISDIR(pinned.st_mode)
                or pinned.st_uid != os.geteuid()
                or stat.S_IMODE(pinned.st_mode) != 0o700
                or identity != (named.st_dev, named.st_ino, named.st_uid, named.st_mode)
            ):
                raise GateFailure("gate log directory could not be pinned")
            return cls(descriptor, identity)
        except Exception:
            os.close(descriptor)
            raise

    def close(self) -> None:
        if self._descriptor >= 0:
            os.close(self._descriptor)
            self._descriptor = -1

    def _verify(self) -> None:
        if self._descriptor < 0:
            raise GateFailure("gate log directory is closed")
        current = os.fstat(self._descriptor)
        identity = (current.st_dev, current.st_ino, current.st_uid, current.st_mode)
        if identity != self._identity or not stat.S_ISDIR(current.st_mode):
            raise GateFailure("gate log directory identity changed")

    def _require_name(self, name: str) -> None:
        if name not in GATE_LOG_NAMES:
            raise GateFailure(f"unowned gate log name: {name}")

    @staticmethod
    def _validate_regular(metadata: os.stat_result, name: str) -> None:
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            raise GateFailure(f"gate log is not a private owner regular file: {name}")

    def open_append(self, name: str):
        self._verify()
        self._require_name(name)
        flags = os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_CLOEXEC | os.O_NOFOLLOW
        descriptor = os.open(name, flags, 0o600, dir_fd=self._descriptor)
        try:
            metadata = os.fstat(descriptor)
            self._validate_regular(metadata, name)
            return os.fdopen(descriptor, "ab")
        except Exception:
            os.close(descriptor)
            raise

    def _open_existing(
        self,
        name: str,
        access: int = os.O_RDONLY,
    ) -> tuple[int, os.stat_result] | None:
        self._verify()
        self._require_name(name)
        try:
            before = os.stat(name, dir_fd=self._descriptor, follow_symlinks=False)
        except FileNotFoundError:
            return None
        self._validate_regular(before, name)
        descriptor = os.open(
            name,
            access | os.O_CLOEXEC | os.O_NOFOLLOW,
            dir_fd=self._descriptor,
        )
        try:
            current = os.fstat(descriptor)
            self._validate_regular(current, name)
            before_identity = (
                before.st_dev,
                before.st_ino,
                before.st_uid,
                before.st_mode,
                before.st_size,
                before.st_nlink,
            )
            current_identity = (
                current.st_dev,
                current.st_ino,
                current.st_uid,
                current.st_mode,
                current.st_size,
                current.st_nlink,
            )
            if current_identity != before_identity:
                raise GateFailure(f"gate log identity changed while opening: {name}")
            return descriptor, current
        except Exception:
            os.close(descriptor)
            raise

    def concise(self, name: str) -> str:
        try:
            opened = self._open_existing(name)
            if opened is None:
                return "command failed without readable output"
            descriptor, _ = opened
            with os.fdopen(descriptor, "rb") as stream:
                stream.seek(0, os.SEEK_END)
                stream.seek(max(0, stream.tell() - OUTPUT_TAIL_BYTES))
                text = stream.read().decode("utf-8", errors="replace")
        except (OSError, GateFailure):
            return "command failed without readable output"
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        return " | ".join(lines[-6:])[-1_000:] or "command failed without output"

    def has_exact_diagnostic(self, name: str, expected: str) -> bool:
        try:
            opened = self._open_existing(name)
            if opened is None:
                return False
            descriptor, _ = opened
            with os.fdopen(descriptor, "rb") as stream:
                output = stream.read(OUTPUT_TAIL_BYTES + 1)
        except (OSError, GateFailure):
            return False
        if len(output) > OUTPUT_TAIL_BYTES:
            return False
        text = output.decode("utf-8", errors="replace")
        return [line.strip() for line in text.splitlines() if line.strip()] == [expected]

    def read_bounded(self, name: str, limit: int) -> bytes:
        opened = self._open_existing(name)
        if opened is None:
            raise GateFailure(f"gate log is absent: {name}")
        descriptor, metadata = opened
        try:
            if metadata.st_size > limit:
                raise GateFailure(f"gate log exceeds its read bound: {name}")
            with os.fdopen(descriptor, "rb") as stream:
                descriptor = -1
                body = stream.read(limit + 1)
        finally:
            if descriptor >= 0:
                os.close(descriptor)
        if len(body) > limit:
            raise GateFailure(f"gate log exceeds its read bound: {name}")
        return body

    def write_diagnostic(self, name: str, body: bytes) -> None:
        self._verify()
        self._require_name(name)
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW
        descriptor = os.open(name, flags, 0o600, dir_fd=self._descriptor)
        try:
            self._validate_regular(os.fstat(descriptor), name)
            view = memoryview(body)
            while view:
                written = os.write(descriptor, view)
                view = view[written:]
            os.fsync(descriptor)
        finally:
            os.close(descriptor)

    def retention_source(self, name: str) -> tuple[int, os.stat_result] | None:
        return self._open_existing(name)

    def flush(self) -> None:
        for name in sorted(GATE_LOG_NAMES):
            opened = self._open_existing(name, os.O_WRONLY)
            if opened is None:
                continue
            descriptor, _ = opened
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)


def clean_environment() -> dict[str, str]:
    environment = os.environ.copy()
    for name in list(environment):
        if name.startswith("PG") or "SCHEMA_OWNER" in name:
            environment.pop(name)
    return environment


def terminate(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=10)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=10)


def bounded_command_diagnostic(
    label: str,
    command: list[str],
    environment: dict[str, str],
) -> tuple[bytes, str | None]:
    failure = None
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
            timeout=10,
        )
        return_code = str(completed.returncode)
        output = completed.stdout
    except subprocess.TimeoutExpired as error:
        return_code = "timeout"
        output = error.stdout or b""
        failure = f"{label} timed out"
    except OSError as error:
        return_code = "not-executed"
        output = b""
        failure = f"{label} could not execute: {type(error).__name__}"

    truncated = len(output) > DIAGNOSTIC_COMMAND_BYTES
    retained = output[:DIAGNOSTIC_COMMAND_BYTES]
    header = (
        f"[{label}]\n"
        f"return_code={return_code}\n"
        f"output_size={len(retained)}\n"
        f"output_truncated={str(truncated).lower()}\n"
    ).encode("ascii")
    return header + retained + (b"\n" if retained and not retained.endswith(b"\n") else b""), failure


def socket_file_type(mode: int) -> str:
    if stat.S_ISSOCK(mode):
        return "socket"
    if stat.S_ISREG(mode):
        return "regular"
    if stat.S_ISDIR(mode):
        return "directory"
    if stat.S_ISLNK(mode):
        return "symlink"
    if stat.S_ISFIFO(mode):
        return "fifo"
    if stat.S_ISCHR(mode):
        return "character-device"
    if stat.S_ISBLK(mode):
        return "block-device"
    return "unknown"


def _exact_keys(value: object, keys: set[str], label: str) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != keys:
        raise GateFailure(f"bootstrap authority report has an invalid {label} shape")
    return value


def _validate_observations(
    value: object,
    expected_names: tuple[str, ...],
    label: str,
) -> list[dict[str, object]]:
    if not isinstance(value, list) or len(value) != len(expected_names):
        raise GateFailure(f"bootstrap authority report has an invalid {label} count")
    observations = [
        _exact_keys(item, {"class", "name", "pass"}, f"{label} observation")
        for item in value
    ]
    for item in observations:
        if (
            not isinstance(item["name"], str)
            or not isinstance(item["class"], str)
            or not isinstance(item["pass"], bool)
        ):
            raise GateFailure(f"bootstrap authority report has invalid {label} values")
    names = tuple(item["name"] for item in observations)
    if names != expected_names or len(set(names)) != len(names):
        raise GateFailure(f"bootstrap authority report has invalid {label} identities")
    for item in observations:
        if item["class"] not in {"unsafe", "incompatible"}:
            raise GateFailure(f"bootstrap authority report has invalid {label} values")
    return observations


def _require_observation_classes(
    observations: list[dict[str, object]],
    incompatible_names: frozenset[str],
    dynamic_names: frozenset[str] = frozenset(),
) -> None:
    for item in observations:
        if item["name"] in dynamic_names:
            continue
        expected = "incompatible" if item["name"] in incompatible_names else "unsafe"
        if item["class"] != expected:
            raise GateFailure("bootstrap authority report changes an observation class")


def _validate_digest(value: object, label: str) -> dict[str, object]:
    digest = _exact_keys(
        value,
        {"actual_sha256", "class", "complete", "expected_sha256", "pass"},
        label,
    )
    if (
        not isinstance(digest["class"], str)
        or digest["class"] not in {"unsafe", "incompatible"}
        or not isinstance(digest["complete"], bool)
        or not isinstance(digest["pass"], bool)
        or not isinstance(digest["expected_sha256"], str)
        or re.fullmatch(r"[0-9a-f]{64}", digest["expected_sha256"]) is None
        or (
            digest["actual_sha256"] is not None
            and (
                not isinstance(digest["actual_sha256"], str)
                or re.fullmatch(r"[0-9a-f]{64}", digest["actual_sha256"]) is None
            )
        )
    ):
        raise GateFailure(f"bootstrap authority report has invalid {label} values")
    expected_pass = (
        digest["complete"]
        and digest["actual_sha256"] == digest["expected_sha256"]
    )
    if digest["pass"] is not expected_pass:
        raise GateFailure(f"bootstrap authority report has inconsistent {label} evidence")
    return digest


def validate_bootstrap_authority_report(logs: GateLogDirectory) -> tuple[str, tuple[str, ...]]:
    body = logs.read_bounded("bootstrap.log", FAILURE_LOG_FILE_BYTES)
    report_lines = [
        line[len(BOOTSTRAP_AUTHORITY_REPORT_PREFIX) :]
        for line in body.splitlines()
        if line.startswith(BOOTSTRAP_AUTHORITY_REPORT_PREFIX)
    ]
    if len(report_lines) != 1:
        raise GateFailure("bootstrap emitted no unique authority report")
    encoded = report_lines[0]
    if not encoded or len(encoded) > BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES:
        raise GateFailure("bootstrap authority report exceeds its bound")
    try:
        report = json.loads(encoded)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise GateFailure("bootstrap authority report is not canonical JSON") from error
    report = _exact_keys(
        report,
        {
            "classification",
            "complete",
            "configured_authority",
            "namespace",
            "platform",
            "query_failure",
            "schema",
            "schema_contract",
            "semantic",
        },
        "top-level",
    )
    try:
        canonical = json.dumps(report, sort_keys=True, separators=(",", ":")).encode("ascii")
    except UnicodeEncodeError as error:
        raise GateFailure("bootstrap authority report contains non-ASCII values") from error
    if canonical != encoded:
        raise GateFailure("bootstrap authority report JSON is not canonical")
    if report["schema"] != BOOTSTRAP_AUTHORITY_REPORT_SCHEMA or not isinstance(
        report["complete"], bool
    ):
        raise GateFailure("bootstrap authority report has invalid closure metadata")
    if not isinstance(report["classification"], str) or report["classification"] not in {
        "authentication",
        "unreachable",
        "incompatible",
        "unsafe_authority",
        "unsafe_host_path",
    }:
        raise GateFailure("bootstrap authority report has an invalid classification")

    if not report["complete"]:
        failure = _exact_keys(
            report["query_failure"],
            {"category", "operation", "phase"},
            "query failure",
        )
        if (
            not isinstance(failure["phase"], str)
            or not isinstance(failure["operation"], str)
            or not isinstance(failure["category"], str)
            or failure["operation"] not in BOOTSTRAP_QUERY_FAILURE_OPERATIONS
            or failure["category"] not in BOOTSTRAP_QUERY_FAILURE_CATEGORIES
        ):
            raise GateFailure("bootstrap authority report has an invalid query failure")
        expected_phase, platform_complete, completed_authority = (
            BOOTSTRAP_QUERY_FAILURE_OPERATIONS[failure["operation"]]
        )
        if failure["phase"] != expected_phase:
            raise GateFailure("bootstrap authority report has an invalid query failure")
        if report["classification"] != BOOTSTRAP_QUERY_FAILURE_CLASSIFICATIONS[
            failure["category"]
        ]:
            raise GateFailure(
                "bootstrap authority report changes the query failure classification"
            )

        if platform_complete:
            platform = _validate_observations(
                report["platform"], BOOTSTRAP_PLATFORM_NAMES, "platform"
            )
            _require_observation_classes(platform, BOOTSTRAP_PLATFORM_INCOMPATIBLE)
        elif report["platform"] != []:
            raise GateFailure("partial bootstrap authority report contains platform evidence")

        if completed_authority >= 1:
            namespace = _validate_observations(
                report["namespace"], BOOTSTRAP_NAMESPACE_NAMES, "namespace"
            )
            _require_observation_classes(namespace, BOOTSTRAP_NAMESPACE_INCOMPATIBLE)
        elif report["namespace"] != []:
            raise GateFailure("partial bootstrap authority report contains namespace evidence")

        if completed_authority >= 2:
            semantic = _validate_observations(
                report["semantic"], BOOTSTRAP_SEMANTIC_NAMES, "semantic"
            )
            _require_observation_classes(
                semantic,
                BOOTSTRAP_SEMANTIC_INCOMPATIBLE,
                frozenset({"exact_function_inventory"}),
            )
        elif report["semantic"] != []:
            raise GateFailure("partial bootstrap authority report contains semantic evidence")

        if completed_authority >= 3:
            configured = _validate_digest(
                report["configured_authority"], "configured authority"
            )
            if configured["class"] != (
                "unsafe" if configured["complete"] else "incompatible"
            ):
                raise GateFailure(
                    "bootstrap authority report misclassifies configured authority"
                )
        elif report["configured_authority"] is not None:
            raise GateFailure(
                "partial bootstrap authority report contains configured authority evidence"
            )

        if report["schema_contract"] is not None:
            raise GateFailure(
                "partial bootstrap authority report contains schema contract evidence"
            )
        return encoded.decode("ascii"), (
            f"query:{failure['phase']}:{failure['operation']}:{failure['category']}",
        )

    if report["query_failure"] is not None:
        raise GateFailure("complete bootstrap authority report contains a query failure")
    platform = _validate_observations(report["platform"], BOOTSTRAP_PLATFORM_NAMES, "platform")
    namespace = _validate_observations(
        report["namespace"], BOOTSTRAP_NAMESPACE_NAMES, "namespace"
    )
    semantic = _validate_observations(
        report["semantic"], BOOTSTRAP_SEMANTIC_NAMES, "semantic"
    )
    _require_observation_classes(platform, BOOTSTRAP_PLATFORM_INCOMPATIBLE)
    _require_observation_classes(namespace, BOOTSTRAP_NAMESPACE_INCOMPATIBLE)
    _require_observation_classes(
        semantic,
        BOOTSTRAP_SEMANTIC_INCOMPATIBLE,
        frozenset({"exact_function_inventory"}),
    )
    configured = _validate_digest(report["configured_authority"], "configured authority")
    schema = _validate_digest(report["schema_contract"], "schema contract")
    if configured["class"] != ("unsafe" if configured["complete"] else "incompatible"):
        raise GateFailure("bootstrap authority report misclassifies configured authority")
    if schema["class"] != "incompatible":
        raise GateFailure("bootstrap authority report misclassifies the schema contract")

    failed = tuple(
        f"{group}:{item['name']}:{item['class']}"
        for group, observations in (
            ("platform", platform),
            ("namespace", namespace),
            ("semantic", semantic),
        )
        for item in observations
        if not item["pass"]
    ) + tuple(
        f"{name}:{evidence['class']}"
        for name, evidence in (
            ("configured_authority", configured),
            ("schema_contract", schema),
        )
        if not evidence["pass"]
    )
    if not failed:
        raise GateFailure("failed bootstrap emitted a passing authority report")

    primary_class = next(
        (item["class"] for item in platform if not item["pass"]),
        None,
    )
    if primary_class is None:
        primary_class = next((item["class"] for item in namespace if not item["pass"]), None)
    if primary_class is None:
        if any(not item["pass"] and item["class"] == "unsafe" for item in semantic):
            primary_class = "unsafe"
        elif any(not item["pass"] for item in semantic):
            primary_class = "incompatible"
    if primary_class is None and not configured["pass"]:
        primary_class = configured["class"]
    if primary_class is None and not schema["pass"]:
        primary_class = schema["class"]
    expected_classification = {
        "unsafe": "unsafe_authority",
        "incompatible": "incompatible",
    }[primary_class]
    if report["classification"] != expected_classification:
        raise GateFailure("bootstrap authority report changes the primary failure classification")
    return encoded.decode("ascii"), failed


def capture_bootstrap_diagnostic(
    tools: dict[str, Path],
    fixture: Path,
    logs: GateLogDirectory,
    environment: dict[str, str],
    process: subprocess.Popen[bytes],
) -> str | None:
    poll_result = process.poll()
    socket_path = fixture / "socket" / f".s.PGSQL.{PORT}"
    metadata_lines = [
        f"effective_uid={os.geteuid()}",
        f"configured_port={PORT}",
        f"postmaster_poll={'running' if poll_result is None else poll_result}",
        f"postmaster_alive={str(poll_result is None).lower()}",
        f"schema_owner_user={SCHEMA_OWNER}",
        f"schema_owner_query=SELECT 1",
    ]
    try:
        socket_metadata = socket_path.lstat()
        metadata_lines.extend(
            [
                f"socket_type={socket_file_type(socket_metadata.st_mode)}",
                f"socket_uid={socket_metadata.st_uid}",
                f"socket_mode={stat.S_IMODE(socket_metadata.st_mode):04o}",
                f"socket_device={socket_metadata.st_dev}",
                f"socket_inode={socket_metadata.st_ino}",
            ]
        )
    except OSError as error:
        metadata_lines.extend(
            [
                "socket_type=unavailable",
                f"socket_stat_errno={error.errno}",
            ]
        )

    pg_isready, ready_failure = bounded_command_diagnostic(
        "pg_isready",
        [
            str(tools["pg_isready"]),
            "--host",
            str(fixture / "socket"),
            "--port",
            str(PORT),
            "--dbname",
            DATABASE,
            "--username",
            SCHEMA_OWNER,
        ],
        environment,
    )
    select_one, select_failure = bounded_command_diagnostic(
        "schema-owner-select-1",
        [
            str(tools["psql"]),
            "-X",
            "--no-password",
            "--set=ON_ERROR_STOP=1",
            "--tuples-only",
            "--no-align",
            "--host",
            str(fixture / "socket"),
            "--port",
            str(PORT),
            "--username",
            SCHEMA_OWNER,
            "--dbname",
            DATABASE,
            "--command",
            "SELECT 1",
        ],
        environment,
    )
    body = ("\n".join(metadata_lines) + "\n").encode("ascii") + pg_isready + select_one
    if len(body) > DIAGNOSTIC_LOG_BYTES:
        raise GateFailure("bootstrap diagnostic exceeded its output bound")

    logs.write_diagnostic("bootstrap-diagnostic.log", body)

    failures = [failure for failure in (ready_failure, select_failure) if failure]
    return "; ".join(failures) or None


def retain_failure_logs(
    logs: GateLogDirectory,
) -> tuple[Path, list[tuple[str, str, int, int, int]], list[str]]:
    if not FAILURE_EVIDENCE_PARENT.is_dir():
        raise GateFailure(f"failure evidence parent is unavailable: {FAILURE_EVIDENCE_PARENT}")

    evidence = Path(
        tempfile.mkdtemp(
            prefix="decodex-latest-schema-gate-failure-",
            dir=FAILURE_EVIDENCE_PARENT,
        )
    )
    evidence.chmod(0o700)
    evidence_descriptor = os.open(
        evidence,
        os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
    )
    try:
        named_evidence = evidence.lstat()
        pinned_evidence = os.fstat(evidence_descriptor)
        if (
            not stat.S_ISDIR(pinned_evidence.st_mode)
            or pinned_evidence.st_uid != os.geteuid()
            or stat.S_IMODE(pinned_evidence.st_mode) != 0o700
            or (
                pinned_evidence.st_dev,
                pinned_evidence.st_ino,
                pinned_evidence.st_uid,
                pinned_evidence.st_mode,
            )
            != (
                named_evidence.st_dev,
                named_evidence.st_ino,
                named_evidence.st_uid,
                named_evidence.st_mode,
            )
        ):
            raise GateFailure("failure evidence directory could not be pinned")
    except Exception:
        os.close(evidence_descriptor)
        raise

    records: list[tuple[str, str, int, int, int]] = []
    warnings: list[str] = []
    total = 0
    try:
        for name in sorted(GATE_LOG_NAMES):
            try:
                opened = logs.retention_source(name)
            except (OSError, GateFailure) as error:
                warnings.append(f"could not open {name}: {type(error).__name__}")
                continue
            if opened is None:
                continue
            source_descriptor, current = opened
            remaining = FAILURE_LOG_TOTAL_BYTES - total
            if remaining <= 0:
                os.close(source_descriptor)
                warnings.append("failure log total bound reached")
                break
            limit = min(FAILURE_LOG_FILE_BYTES, remaining)
            destination_descriptor = -1
            try:
                retained_offset = max(0, current.st_size - limit)
                os.lseek(source_descriptor, retained_offset, os.SEEK_SET)
                destination_descriptor = os.open(
                    name,
                    os.O_WRONLY
                    | os.O_CREAT
                    | os.O_EXCL
                    | os.O_CLOEXEC
                    | os.O_NOFOLLOW,
                    0o600,
                    dir_fd=evidence_descriptor,
                )
                os.fchmod(destination_descriptor, 0o600)
                destination_metadata = os.fstat(destination_descriptor)
                if (
                    not stat.S_ISREG(destination_metadata.st_mode)
                    or destination_metadata.st_uid != os.geteuid()
                    or stat.S_IMODE(destination_metadata.st_mode) != 0o600
                    or destination_metadata.st_nlink != 1
                ):
                    raise GateFailure(f"retained log is not private owner authority: {name}")
                digest = hashlib.sha256()
                copied = 0
                while copied < limit:
                    chunk = os.read(
                        source_descriptor,
                        min(COPY_BUFFER_BYTES, limit - copied),
                    )
                    if not chunk:
                        break
                    view = memoryview(chunk)
                    while view:
                        written = os.write(destination_descriptor, view)
                        view = view[written:]
                    digest.update(chunk)
                    copied += len(chunk)
                os.fsync(destination_descriptor)
                records.append(
                    (name, digest.hexdigest(), copied, current.st_size, retained_offset)
                )
                total += copied
            except (OSError, GateFailure) as error:
                warnings.append(f"could not retain {name}: {type(error).__name__}")
                if destination_descriptor >= 0:
                    os.close(destination_descriptor)
                    destination_descriptor = -1
                    try:
                        os.unlink(name, dir_fd=evidence_descriptor)
                    except FileNotFoundError:
                        pass
            finally:
                if destination_descriptor >= 0:
                    os.close(destination_descriptor)
                os.close(source_descriptor)
        os.fsync(evidence_descriptor)
    finally:
        os.close(evidence_descriptor)

    return evidence, records, warnings


def run_command(
    name: str,
    command: list[str],
    logs: GateLogDirectory,
    environment: dict[str, str],
    *,
    timeout: int = COMMAND_TIMEOUT_SECONDS,
) -> tuple[int, str]:
    log_name = f"{name}.log"
    with logs.open_append(log_name) as output:
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            return_code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            terminate(process)
            return 124, f"timed out after {timeout} seconds; {logs.concise(log_name)}"
    return return_code, logs.concise(log_name)


def require_command(
    name: str,
    command: list[str],
    logs: GateLogDirectory,
    environment: dict[str, str],
    *,
    timeout: int = COMMAND_TIMEOUT_SECONDS,
) -> None:
    return_code, detail = run_command(name, command, logs, environment, timeout=timeout)
    if return_code != 0:
        raise GateFailure(detail)


def resolve_postgres_tools(environment: dict[str, str]) -> dict[str, Path]:
    configured = os.environ.get("DECODEX_POSTGRES_18_BINDIR")
    if configured:
        bindir = Path(configured).expanduser().resolve()
    else:
        pg_config = shutil.which("pg_config", path=environment.get("PATH"))
        if pg_config is None:
            raise GateFailure(
                "set DECODEX_POSTGRES_18_BINDIR or put PostgreSQL 18 pg_config on PATH"
            )
        completed = subprocess.run(
            [pg_config, "--bindir"],
            check=False,
            capture_output=True,
            env=environment,
            text=True,
            timeout=10,
        )
        if completed.returncode != 0 or not completed.stdout.strip():
            raise GateFailure("pg_config did not resolve a PostgreSQL binary directory")
        bindir = Path(completed.stdout.strip()).resolve()

    tools = {name: bindir / name for name in ("initdb", "pg_isready", "postgres", "psql")}
    version_pattern = re.compile(r"\b([0-9]+)(?:\.[0-9]+)?\b")
    for name, path in tools.items():
        if not path.is_file() or not os.access(path, os.X_OK):
            raise GateFailure(f"PostgreSQL tool is unavailable: {path}")
        completed = subprocess.run(
            [str(path), "--version"],
            check=False,
            capture_output=True,
            env=environment,
            text=True,
            timeout=10,
        )
        match = version_pattern.search(completed.stdout + completed.stderr)
        if completed.returncode != 0 or match is None or int(match.group(1)) != POSTGRES_MAJOR:
            raise GateFailure(f"{name} is not PostgreSQL {POSTGRES_MAJOR}: {path}")
    return tools


def reverse_scan(environment: dict[str, str]) -> None:
    completed = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        env=environment,
        timeout=10,
    )
    if completed.returncode != 0:
        raise GateFailure("git source listing failed")
    if len(completed.stdout) > SOURCE_LIST_BYTES:
        raise GateFailure("git source listing exceeds the reverse-scan bound")

    paths = [Path(item.decode("utf-8")) for item in completed.stdout.split(b"\0") if item]
    canonical = SCHEMA.relative_to(ROOT)
    sql_paths = sorted(path for path in paths if path.suffix == ".sql")
    if sql_paths != [canonical] or not SCHEMA.is_file():
        raise GateFailure(f"sole SQL authority mismatch: {[str(path) for path in sql_paths]}")

    prohibited_paths = []
    for path in paths:
        text = path.as_posix()
        postgres_owner = text.startswith("crates/decodex-postgres/")
        vnext_script = text.startswith("scripts/vnext/")
        retired_spike = text.startswith("spikes/vnext-storage/")
        if (
            postgres_owner
            and (
                "migrations" in path.parts
                or re.search(r"(?:^|/)V[0-9]+__[^/]*\.sql$", text)
                or text
                in {
                    "crates/decodex-postgres/build.rs",
                    "crates/decodex-postgres/src/migrations.rs",
                }
            )
            or vnext_script
            and text == "scripts/vnext/postgres_store_test.py"
            or retired_spike
            and path.suffix in {".py", ".rs", ".sql", ".toml"}
        ):
            prohibited_paths.append(text)
    if prohibited_paths:
        raise GateFailure(f"prohibited migration artifact: {prohibited_paths[0]}")

    scan_paths = [
        path
        for path in paths
        if path.as_posix() in {"Cargo.lock", "Cargo.toml", "Makefile.toml"}
        or path.as_posix() == "crates/decodex-postgres/Cargo.toml"
        or path.as_posix().startswith("crates/decodex-postgres/src/")
        or path.as_posix().startswith("apps/decodexd/src/")
        or path.as_posix() == "crates/decodex-runtime/src/bootstrap.rs"
        or path.as_posix().startswith("scripts/vnext/")
        and path.name != Path(__file__).name
    ]
    forbidden = re.compile(
        r"refinery_schema_history|\brefinery\b|include_migrations!|"
        r"\b(?:run|apply)_migrations?\b|\bmigration_history\b|\bschema_history\b",
        re.IGNORECASE,
    )
    schema_ddl = re.compile(
        r"\bCREATE\s+(?:SCHEMA|EXTENSION|TYPE|TABLE|SEQUENCE|FUNCTION|TRIGGER)\b",
        re.IGNORECASE,
    )
    total = 0
    for relative in scan_paths:
        source = ROOT / relative
        size = source.stat().st_size
        if size > SOURCE_FILE_BYTES:
            raise GateFailure(f"reverse-scan file exceeds bound: {relative}")
        total += size
        if total > SOURCE_TOTAL_BYTES:
            raise GateFailure("reverse-scan input exceeds total bound")
        source_text = source.read_text(encoding="utf-8")
        match = forbidden.search(source_text)
        if match is not None:
            raise GateFailure(f"prohibited migration source in {relative}: {match.group(0)}")
        active_vnext_python = (
            relative.as_posix().startswith("scripts/vnext/")
            and relative.suffix == ".py"
            and relative.name != Path(__file__).name
        )
        ddl_match = schema_ddl.search(source_text) if active_vnext_python else None
        if ddl_match is not None:
            raise GateFailure(
                f"prohibited schema DDL in {relative}: {ddl_match.group(0)}"
            )


def built_decodexd(
    cargo: str,
    logs: GateLogDirectory,
    environment: dict[str, str],
) -> Path:
    require_command(
        "build-decodexd",
        [cargo, "build", "-p", "decodexd", "--bin", "decodexd"],
        logs,
        environment,
    )
    target = Path(environment.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    build_target = environment.get("CARGO_BUILD_TARGET")
    binary = target / "debug" / "decodexd"
    if build_target:
        binary = target / build_target / "debug" / "decodexd"
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise GateFailure(f"built decodexd binary is unavailable: {binary}")
    return binary.resolve()


def write_root_config(root: Path, socket: Path) -> None:
    root.mkdir(mode=0o700)
    config = root / "config.toml"
    config.write_text(
        f'''version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {os.geteuid()}

[postgres]
socket_directory = "{socket}"
expected_peer_uid = {os.geteuid()}
port = {PORT}
database = "{DATABASE}"

[postgres.runtime]
user = "{RUNTIME_ROLE}"

[cache]
max_entries = 1
max_bytes = 1
max_entry_bytes = 1
''',
        encoding="utf-8",
    )
    config.chmod(0o600)


def psql(
    tools: dict[str, Path],
    logs: GateLogDirectory,
    environment: dict[str, str],
    database: str,
    sql: str,
) -> None:
    require_command(
        "cluster-setup",
        [
            str(tools["psql"]),
            "-X",
            "--set=ON_ERROR_STOP=1",
            "--host",
            environment["PGHOST"],
            "--port",
            environment["PGPORT"],
            "--username",
            ADMIN_ROLE,
            "--dbname",
            database,
            "--command",
            sql,
        ],
        logs,
        environment,
        timeout=60,
    )


def start_cluster(
    fixture: Path,
    tools: dict[str, Path],
    logs: GateLogDirectory,
    environment: dict[str, str],
    started_processes: list[subprocess.Popen[bytes]],
) -> tuple[subprocess.Popen[bytes], dict[str, str]]:
    data = fixture / "postgres"
    socket = fixture / "socket"
    socket.mkdir(mode=0o700)
    require_command(
        "cluster-init",
        [
            str(tools["initdb"]),
            "-D",
            str(data),
            f"--username={ADMIN_ROLE}",
            "--auth-local=trust",
            "--auth-host=reject",
            "--encoding=UTF8",
            "--locale=C",
            "--data-checksums",
            "--no-instructions",
        ],
        logs,
        environment,
        timeout=120,
    )
    server_log = logs.open_append("postgres.log")
    try:
        process = subprocess.Popen(
            [
                str(tools["postgres"]),
                "-D",
                str(data),
                "-h",
                "",
                "-k",
                str(socket),
                "-p",
                str(PORT),
                "-c",
                "unix_socket_permissions=0700",
            ],
            cwd=ROOT,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=server_log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        started_processes.append(process)
    finally:
        server_log.close()

    database_environment = environment.copy()
    database_environment.update(
        {
            "PGHOST": str(socket),
            "PGPORT": str(PORT),
            "PGUSER": ADMIN_ROLE,
            "PGDATABASE": "postgres",
            "PGOPTIONS": "-csearch_path=pg_catalog -cTimeZone=+05:00",
        }
    )
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise GateFailure(
                f"PostgreSQL exited during startup: {logs.concise('postgres.log')}"
            )
        ready = subprocess.run(
            [
                str(tools["pg_isready"]),
                "--host",
                str(socket),
                "--port",
                str(PORT),
                "--dbname",
                "postgres",
                "--username",
                ADMIN_ROLE,
            ],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=database_environment,
            timeout=2,
        )
        if ready.returncode == 0:
            break
        time.sleep(0.1)
    else:
        raise GateFailure("PostgreSQL did not become ready within 30 seconds")

    role_attributes = (
        "LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION "
        "NOBYPASSRLS CONNECTION LIMIT -1 VALID UNTIL 'infinity'"
    )
    psql(
        tools,
        logs,
        database_environment,
        "postgres",
        f"CREATE ROLE {SCHEMA_OWNER} {role_attributes}",
    )
    psql(
        tools,
        logs,
        database_environment,
        "postgres",
        f"CREATE ROLE {RUNTIME_ROLE} {role_attributes}",
    )
    psql(
        tools,
        logs,
        database_environment,
        "postgres",
        f"CREATE DATABASE {DATABASE} WITH TEMPLATE template0 ENCODING 'UTF8' OWNER {SCHEMA_OWNER}",
    )
    psql(
        tools,
        logs,
        database_environment,
        DATABASE,
        f"GRANT USAGE, CREATE ON SCHEMA public TO {SCHEMA_OWNER}",
    )
    psql(
        tools,
        logs,
        database_environment,
        "postgres",
        f"REVOKE CREATE ON DATABASE {DATABASE} FROM PUBLIC; "
        f"GRANT CONNECT, CREATE ON DATABASE {DATABASE} TO {SCHEMA_OWNER}; "
        f"GRANT CONNECT ON DATABASE {DATABASE} TO {RUNTIME_ROLE}",
    )
    return process, database_environment


def test_environment(base: dict[str, str], fixture: Path) -> dict[str, str]:
    environment = base.copy()
    options = "options='-csearch_path=pg_catalog -cTimeZone=+05:00'"
    endpoint = f"host={fixture / 'socket'} port={PORT} dbname={DATABASE}"
    environment.update(
        {
            "DECODEX_TEST_SCHEMA_OWNER_DATABASE_URL": f"{endpoint} user={SCHEMA_OWNER} {options}",
            "DECODEX_TEST_RUNTIME_DATABASE_URL": f"{endpoint} user={RUNTIME_ROLE} {options}",
            "DECODEX_TEST_BLOB_ROOT": str(fixture / "blobs"),
            "DECODEX_TEST_ROOT": str(fixture / "root"),
            "DECODEX_TEST_SCHEMA_OWNER_USER": SCHEMA_OWNER,
            "PGHOST": str(fixture / "socket"),
            "PGPORT": str(PORT),
            "PGUSER": SCHEMA_OWNER,
            "PGDATABASE": DATABASE,
        }
    )
    (fixture / "blobs").mkdir(mode=0o700)
    return environment


def main() -> int:
    if (
        os.name != "posix"
        or not hasattr(os, "geteuid")
        or any(
            not hasattr(os, name)
            for name in ("O_CLOEXEC", "O_DIRECTORY", "O_NOFOLLOW")
        )
    ):
        print("latest-schema gate requires a Unix host", file=sys.stderr)
        return 1

    old_umask = os.umask(0o077)
    results: dict[str, tuple[str, str]] = {}
    postgres_processes: list[subprocess.Popen[bytes]] = []
    diagnostic_failure: str | None = None
    bootstrap_report_json: str | None = None
    bootstrap_report_failures: tuple[str, ...] = ()
    evidence_directory: Path | None = None
    evidence_records: list[tuple[str, str, int, int, int]] = []
    evidence_warnings: list[str] = []
    evidence_failure: str | None = None
    fixture: Path | None = None
    logs: GateLogDirectory | None = None
    try:
        fixture = Path(tempfile.mkdtemp(prefix="decodex-gate-", dir="/tmp")).resolve()
        fixture.chmod(0o700)
        try:
            logs = GateLogDirectory.create(fixture / "logs")
        except Exception:
            shutil.rmtree(fixture)
            raise
        try:
            root = fixture / "root"
            isolated_home = fixture / "home"
            isolated_home.mkdir(mode=0o700)
            base = clean_environment()

            def stage(name: str, action, dependencies: tuple[str, ...] = ()):
                blocked = [
                    dependency
                    for dependency in dependencies
                    if results[dependency][0] != "PASS"
                ]
                if blocked:
                    results[name] = ("BLOCKED", ", ".join(blocked))
                    return None
                try:
                    value = action()
                except Exception as error:
                    detail = str(error) or type(error).__name__
                    results[name] = ("FAIL", detail[:1_000])
                    return None
                results[name] = ("PASS", "")
                return value

            stage("reverse-scan", lambda: reverse_scan(base))
            tools = stage("postgres-tools", lambda: resolve_postgres_tools(base))
            cargo = shutil.which("cargo", path=base.get("PATH"))
            if cargo is None:
                results["build-decodexd"] = ("FAIL", "cargo is unavailable on PATH")
                binary = None
            else:
                binary = stage("build-decodexd", lambda: built_decodexd(cargo, logs, base))

            def cluster_action() -> bool:
                assert tools is not None
                start_cluster(fixture, tools, logs, base, postgres_processes)
                write_root_config(root, fixture / "socket")
                return True

            stage("cluster", cluster_action, ("postgres-tools", "build-decodexd"))
            product_environment = base.copy()
            product_environment.update({"HOME": str(isolated_home), "TMPDIR": str(fixture)})
            bootstrap_command = (
                [
                    str(binary),
                    "bootstrap-latest-schema",
                    "--root",
                    str(root),
                    "--schema-owner-user",
                    SCHEMA_OWNER,
                ]
                if binary is not None
                else []
            )
            validate_command = (
                [str(binary), "validate-current-authority", "--root", str(root)]
                if binary is not None
                else []
            )

            stage(
                "bootstrap",
                lambda: require_command(
                    "bootstrap", bootstrap_command, logs, product_environment, timeout=10 * 60
                ),
                ("cluster",),
            )
            if results["bootstrap"][0] == "FAIL":
                diagnostic_failures: list[str] = []
                try:
                    bootstrap_report_json, bootstrap_report_failures = (
                        validate_bootstrap_authority_report(logs)
                    )
                except Exception as error:
                    detail = str(error) or type(error).__name__
                    diagnostic_failures.append(
                        f"authority-report {type(error).__name__}: {detail[:500]}"
                    )
                try:
                    if tools is None or not postgres_processes:
                        raise GateFailure("bootstrap diagnostic authority is unavailable")
                    external_failure = capture_bootstrap_diagnostic(
                        tools,
                        fixture,
                        logs,
                        product_environment,
                        postgres_processes[-1],
                    )
                    if external_failure is not None:
                        diagnostic_failures.append(external_failure)
                except Exception as error:
                    detail = str(error) or type(error).__name__
                    diagnostic_failures.append(f"{type(error).__name__}: {detail[:500]}")
                diagnostic_failure = "; ".join(diagnostic_failures) or None
            stage(
                "runtime-validation",
                lambda: require_command(
                    "runtime-validation",
                    validate_command,
                    logs,
                    product_environment,
                    timeout=5 * 60,
                ),
                ("bootstrap",),
            )

            def refuse_second_bootstrap() -> None:
                return_code, detail = run_command(
                    "second-bootstrap", bootstrap_command, logs, product_environment, timeout=5 * 60
                )
                if return_code == 0:
                    raise GateFailure("second bootstrap unexpectedly succeeded")
                if return_code == 124:
                    raise GateFailure(detail)
                if return_code != 1:
                    raise GateFailure(
                        f"second bootstrap exited with unexpected status {return_code}"
                    )
                if not logs.has_exact_diagnostic(
                    "second-bootstrap.log", SECOND_BOOTSTRAP_REFUSAL_DIAGNOSTIC
                ):
                    raise GateFailure(
                        "second bootstrap did not report the exact empty PostgreSQL target "
                        "refusal classification"
                    )

            stage("second-bootstrap-refusal", refuse_second_bootstrap, ("bootstrap",))
            stage(
                "validation-after-refusal",
                lambda: require_command(
                    "validation-after-refusal",
                    validate_command,
                    logs,
                    product_environment,
                    timeout=5 * 60,
                ),
                ("bootstrap",),
            )

            if cargo is not None:
                tests = test_environment(base, fixture)
                stage(
                    "changed-adapter-sql",
                    lambda: require_command(
                        "changed-adapter-sql",
                        [
                            cargo,
                            "test",
                            "-p",
                            "decodex-postgres",
                            "--features",
                            "test-support",
                            "--lib",
                            "launch_gate_tests::changed_adapter_sql_prepares_against_current_authority",
                            "--",
                            "--exact",
                            "--test-threads=1",
                        ],
                        logs,
                        tests,
                    ),
                    ("bootstrap",),
                )
                stage(
                    "quick-task",
                    lambda: require_command(
                        "quick-task",
                        [cargo, "test", "-p", "decodex-runtime", "--lib", "quick_task::tests::"],
                        logs,
                        tests,
                    ),
                    ("bootstrap",),
                )
                def account_contract() -> None:
                    require_command(
                        "account-contract",
                        [
                            cargo,
                            "test",
                            "-p",
                            "decodex-runtime",
                            "--lib",
                            "local_account_authority::tests::"
                            "local_account_restore_command_proves_two_exact_credential_"
                            "fences_and_readback",
                            "--",
                            "--ignored",
                            "--exact",
                            "--test-threads=1",
                        ],
                        logs,
                        tests,
                    )
                    require_command(
                        "account-contract",
                        [
                            cargo,
                            "test",
                            "-p",
                            "decodex-postgres",
                            "--features",
                            "test-support",
                            "--test",
                            "postgres_store",
                            "postgres_account_routing_contract",
                            "--",
                            "--ignored",
                            "--exact",
                            "--test-threads=1",
                        ],
                        logs,
                        tests,
                    )

                stage(
                    "account-contract",
                    account_contract,
                    ("bootstrap",),
                )
            else:
                for name in ("changed-adapter-sql", "quick-task", "account-contract"):
                    results[name] = ("BLOCKED", "build-decodexd")

        finally:
            # Finalization order is PostgreSQL shutdown, log flush/retention, then deletion.
            shutdown_failures = []
            for process in reversed(postgres_processes):
                try:
                    terminate(process)
                except (OSError, subprocess.SubprocessError) as error:
                    detail = str(error) or type(error).__name__
                    shutdown_failures.append(f"{type(error).__name__}: {detail[:500]}")
                if process.poll() is None:
                    shutdown_failures.append("postmaster exit was not confirmed")

            postmasters_exited = all(process.poll() is not None for process in postgres_processes)
            if shutdown_failures:
                detail = "; ".join(shutdown_failures)
                if not postmasters_exited:
                    detail = f"{detail}; fixture deletion skipped"
                results["postgres-cleanup"] = ("FAIL", detail[:1_000])

            logs_flushed = False
            if postmasters_exited:
                try:
                    logs.flush()
                    logs_flushed = True
                except (OSError, GateFailure) as error:
                    detail = str(error) or type(error).__name__
                    results["gate-log-flush"] = ("FAIL", detail[:1_000])
            else:
                evidence_failure = (
                    "postmaster shutdown was not confirmed; retention and fixture deletion skipped"
                )

            if (
                postmasters_exited
                and logs_flushed
                and any(status != "PASS" for status, _ in results.values())
            ):
                try:
                    evidence_directory, evidence_records, evidence_warnings = (
                        retain_failure_logs(logs)
                    )
                except Exception as error:
                    detail = str(error) or type(error).__name__
                    evidence_failure = f"{type(error).__name__}: {detail[:500]}"
            elif postmasters_exited and not logs_flushed:
                evidence_failure = "gate logs could not be flushed; retention skipped"

            logs_closed = False
            try:
                logs.close()
                logs_closed = True
            except OSError as error:
                detail = str(error) or type(error).__name__
                results["gate-log-close"] = ("FAIL", detail[:1_000])

            if postmasters_exited and logs_closed:
                try:
                    shutil.rmtree(fixture)
                except OSError as error:
                    detail = str(error) or type(error).__name__
                    results["fixture-cleanup"] = ("FAIL", detail[:1_000])
    finally:
        os.umask(old_umask)

    failed = False
    for name, (status, detail) in results.items():
        failed |= status != "PASS"
        suffix = f": {detail}" if detail else ""
        print(f"{status:7} {name}{suffix}")
    if diagnostic_failure is not None:
        print(f"DIAGNOSTIC bootstrap: FAIL: {diagnostic_failure}")
    if bootstrap_report_json is not None:
        print(f"BOOTSTRAP-AUTHORITY-REPORT {bootstrap_report_json}")
        print(
            "BOOTSTRAP-AUTHORITY-FAILURES "
            + ",".join(bootstrap_report_failures)
        )
    if evidence_directory is not None:
        print(f"FAILURE-EVIDENCE directory={evidence_directory}")
        for name, digest, size, source_size, offset in evidence_records:
            print(
                f"FAILURE-EVIDENCE file={evidence_directory / name} "
                f"sha256={digest} size={size} source_size={source_size} "
                f"retained_offset={offset}"
            )
        for warning in evidence_warnings:
            print(f"FAILURE-EVIDENCE warning={warning}")
    if evidence_failure is not None:
        print(f"FAILURE-EVIDENCE error={evidence_failure}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
