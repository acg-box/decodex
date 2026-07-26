"""Shared policy, validation, process, and private-file primitives."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import selectors
import signal
import subprocess
import tempfile
import time
from typing import Any, Mapping, Sequence


STATE_SCHEMA = "decodex/codex-upstream-state/2"
RESULT_SCHEMA = "decodex/codex-upstream-command-result/1"
POLICY_SCHEMA = "decodex/codex-upstream-policy/2"
POLICY_KEYS = {
    "schema",
    "upstream_repository",
    "upstream_branch",
    "target_repository",
    "target_branch",
    "branch_prefix",
    "decodex_executable_sha256",
    "accepted_schema_marker_path",
    "max_batch_commits",
    "max_attempts",
    "lease_seconds",
    "lease_write_guard_seconds",
    "max_lease_renewals",
    "retry_backoff_seconds",
    "validation_profiles",
    "required_validation_profiles",
    "required_experimental_request_methods",
    "required_stable_request_methods",
    "required_notification_methods",
    "relevant_path_prefixes",
}
REPO_ROOT = Path(__file__).resolve().parents[4]
DEFAULT_POLICY_PATH = REPO_ROOT / "automations/upstream/policy.json"
TERMINAL_STATUSES = {"landed", "no_change", "rejected"}
CONTENT_DEGRADATION_CODES = (
    "account_restore_failed",
    "candidate_unresolved",
    "daily_strategy_overdue",
    "outcome_24h_overdue",
    "outcome_7d_overdue",
    "reservation_expired",
    "social_validation_failed",
    "weekly_strategy_overdue",
)
SHA_PATTERN = re.compile(r"^(?:[0-9a-f]{40}|[0-9a-f]{64})$")
REASON_PATTERN = re.compile(r"^[a-z0-9][a-z0-9_]{0,63}$")
PR_PATTERN = re.compile(r"^https://github\.com/hack-ink/decodex/pull/[1-9][0-9]*$")
TAG_PATTERN = re.compile(
    r"^rust-v(?P<major>[0-9]+)\.(?P<minor>[0-9]+)\.(?P<patch>[0-9]+)"
    r"(?:-(?P<label>alpha|beta|rc)\.(?P<number>[0-9]+))?$"
)
MAX_STATE_CANDIDATES = 512
MAX_ACTIVE_SOURCE_CANDIDATES = 128
MAX_EVENTS = 2048
METRIC_BUCKET_SECONDS = 300
MAX_METRIC_BUCKETS = (7 * 24 * 60 * 60 // METRIC_BUCKET_SECONDS) + 2
MAX_SCHEMA_FILES = 512
MAX_SCHEMA_BYTES = 32 * 1024 * 1024
MAX_SCHEMA_EVIDENCE_FILES = 512
MAX_SCHEMA_EVIDENCE_BYTES = 512 * 1024 * 1024
MAX_GIT_TEXT_BYTES = 8 * 1024 * 1024
MAX_STATE_BYTES = 4 * 1024 * 1024
MAX_UPSTREAM_COMMITS = 65_536
MAX_COMMAND_OUTPUT_BYTES = 64 * 1024 * 1024
MAX_EXECUTABLE_BYTES = 512 * 1024 * 1024
COMMAND_TIMEOUT_SECONDS = 300
VALIDATION_LEASE_BUDGET_SECONDS = 11_700
SIDE_EFFECT_LEASE_BUDGET_SECONDS = 9_000
LAND_EFFECT_LEASE_BUDGET_SECONDS = 21_000
MAX_LAND_RECOVERY_WORKTREES = 4
REQUIRED_VALIDATION_PROFILES = (
    "focused_tests",
    "cargo_make_check_upstream_automation",
)
FULL_VALIDATION_PROFILE = "cargo_make_check"
VALIDATION_PROFILE_COMMANDS = {
    "focused_tests": ["cargo", "make", "test-automations"],
    "cargo_make_check_upstream_automation": [
        "cargo",
        "make",
        "check-upstream-automation",
    ],
    FULL_VALIDATION_PROFILE: ["cargo", "make", "check"],
}
VALIDATION_AUTHORITY_PATHS = {
    "Makefile.toml",
    "automations/upstream/policy.json",
    "scripts/audit_node_lock.py",
}
VALIDATION_AUTHORITY_PREFIXES = (
    "automations/upstream/scripts/",
    "automations/upstream/tests/",
)
FULL_GATE_EXACT_PATHS = {
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain.toml",
    "site/.nvmrc",
    "site/package-lock.json",
    "site/package.json",
}
FULL_GATE_PREFIXES = (
    ".github/workflows/",
    "apps/decodex-cli/",
    "apps/decodex/",
    "apps/decodex-gpui/",
)
TRUSTED_SYSTEM_TOOL_DIRECTORIES = (
    Path("/run/current-system/sw/bin"),
    Path("/nix/var/nix/profiles/default/bin"),
    Path("/usr/bin"),
    Path("/bin"),
    Path("/usr/sbin"),
    Path("/sbin"),
)
TRUSTED_SYSTEM_EXECUTABLE_ROOTS = (
    Path("/nix/store"),
    Path("/usr/bin"),
    Path("/bin"),
    Path("/usr/sbin"),
    Path("/sbin"),
)
ALLOWED_CANDIDATE_KINDS = {
    "bootstrap",
    "upstream_range",
    "stable_release",
    "prerelease_release",
    "local_build",
    "automation_repair",
}
CODEX_VERSION_PATTERN = re.compile(r"^codex-cli [0-9][0-9A-Za-z.+-]{0,127}$")
METHOD_PATTERN = re.compile(r"^[A-Za-z][A-Za-z0-9]*(?:/[A-Za-z][A-Za-z0-9]*)*$")
SAFE_FACT_PATTERN = re.compile(r"^[A-Za-z0-9_.:/+-]{1,256}$")
STATE_KEYS = {
    "schema",
    "persistence_generation",
    "created_at",
    "updated_at",
    "last_observed_at",
    "source",
    "local_build",
    "candidates",
    "events",
    "metrics",
}
SOURCE_KEYS = {
    "observed_head_sha",
    "queued_head_sha",
    "cursor_sha",
    "cursor_sequence",
    "next_sequence",
    "next_discovery_sequence",
    "next_lease_generation",
    "observation_started_generation",
    "observation_applied_generation",
    "stable_tag",
    "stable_tag_sha",
    "prerelease_tag",
    "prerelease_tag_sha",
    "schema_fingerprints",
}
CANDIDATE_KEYS = {
    "id",
    "discovery_sequence",
    "kind",
    "status",
    "priority",
    "source_sequence",
    "from_sha",
    "to_sha",
    "release_tag",
    "codex_version",
    "codex_executable_sha256",
    "policy_fingerprint",
    "accepted_marker_fingerprint",
    "schema_fingerprints",
    "schema_evidence",
    "contract_missing",
    "path_summary",
    "repair_of",
    "branch_name",
    "attempts",
    "created_at",
    "updated_at",
    "next_retry_at",
    "retry_role",
    "lease",
    "effect",
    "commit_receipt",
    "pull_request",
    "retired_pull_requests",
    "decision",
    "result",
}


class AutopilotError(RuntimeError):
    """A bounded machine-readable automation failure."""

    def __init__(self, code: str) -> None:
        if not REASON_PATTERN.fullmatch(code):
            code = "unclassified_failure"
        self.code = code
        super().__init__(code)


@dataclass(frozen=True)
class Observation:
    """Public-safe facts observed from official upstream and the local Codex CLI."""

    upstream_head_sha: str
    stable_tag: str | None
    stable_tag_sha: str | None
    prerelease_tag: str | None
    prerelease_tag_sha: str | None
    codex_version: str
    codex_executable_sha256: str
    policy_fingerprint: str
    accepted_marker_fingerprint: str
    stable_schema_fingerprint: str
    experimental_schema_fingerprint: str
    stable_schema_evidence_sha256: str
    experimental_schema_evidence_sha256: str
    upstream_main_schema_fingerprint: str
    stable_release_schema_fingerprint: str | None
    prerelease_schema_fingerprint: str | None
    stable_missing_request_methods: tuple[str, ...]
    stable_missing_notification_methods: tuple[str, ...]
    experimental_missing_request_methods: tuple[str, ...]
    experimental_missing_notification_methods: tuple[str, ...]
    repository_contract_drift: tuple[str, ...]
    upstream_main_contract_missing: tuple[str, ...]
    stable_release_contract_missing: tuple[str, ...]
    prerelease_contract_missing: tuple[str, ...]

    @property
    def contract_missing(self) -> list[str]:
        return [
            *(f"stable_request:{value}" for value in self.stable_missing_request_methods),
            *(
                f"stable_notification:{value}"
                for value in self.stable_missing_notification_methods
            ),
            *(
                f"experimental_request:{value}"
                for value in self.experimental_missing_request_methods
            ),
            *(
                f"experimental_notification:{value}"
                for value in self.experimental_missing_notification_methods
            ),
            *(f"repository_digest:{value}" for value in self.repository_contract_drift),
            *(f"upstream:{value}" for value in self.upstream_main_contract_missing),
            *(f"upstream:{value}" for value in self.stable_release_contract_missing),
            *(f"upstream:{value}" for value in self.prerelease_contract_missing),
        ]

    def contract_missing_for(self, kind: str) -> list[str]:
        local = [
            *(f"stable_request:{value}" for value in self.stable_missing_request_methods),
            *(
                f"stable_notification:{value}"
                for value in self.stable_missing_notification_methods
            ),
            *(
                f"experimental_request:{value}"
                for value in self.experimental_missing_request_methods
            ),
            *(
                f"experimental_notification:{value}"
                for value in self.experimental_missing_notification_methods
            ),
            *(f"repository_digest:{value}" for value in self.repository_contract_drift),
        ]
        upstream = {
            "bootstrap": self.upstream_main_contract_missing,
            "upstream_range": self.upstream_main_contract_missing,
            "stable_release": self.stable_release_contract_missing,
            "prerelease_release": self.prerelease_contract_missing,
            "local_build": (),
            "automation_repair": (),
        }.get(kind)
        if upstream is None:
            raise AutopilotError("candidate_kind_invalid")
        return [*local, *(f"upstream:{value}" for value in upstream)]


def utc_now() -> int:
    return int(datetime.now(timezone.utc).timestamp())


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256_value(value: Any) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def real_home_directory() -> Path:
    try:
        home = Path(pwd.getpwuid(os.getuid()).pw_dir).resolve(strict=True)
    except (KeyError, OSError) as error:
        raise AutopilotError("trusted_home_unavailable") from error
    if not home.is_dir():
        raise AutopilotError("trusted_home_unavailable")
    return home


def _path_is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (OSError, ValueError):
        return False
    return True


def verify_openai_codex_signature(executable: Path) -> None:
    bundle = Path("/Applications/ChatGPT.app")
    expected = bundle / "Contents/Resources/codex"
    if executable != expected:
        raise AutopilotError("trusted_executable_provenance_invalid")
    codesign = Path("/usr/bin/codesign")
    try:
        verification = subprocess.run(
            [str(codesign), "--verify", "--deep", "--strict", str(bundle)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={"PATH": "/usr/bin:/bin"},
            timeout=60,
        )
        details = subprocess.run(
            [str(codesign), "-dv", "--verbose=4", str(bundle)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={"PATH": "/usr/bin:/bin"},
            timeout=60,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AutopilotError("trusted_executable_provenance_invalid") from error
    detail_text = details.stderr.decode("utf-8", errors="replace")
    if (
        verification.returncode != 0
        or details.returncode != 0
        or len(details.stderr) > 64 * 1024
        or "Identifier=com.openai.codex\n" not in detail_text
        or "TeamIdentifier=2DC432GLL2\n" not in detail_text
        or (
            "Authority=Developer ID Application: OpenAI OpCo, LLC "
            "(2DC432GLL2)\n"
        )
        not in detail_text
    ):
        raise AutopilotError("trusted_executable_provenance_invalid")


def _trusted_user_executable(command: str) -> Path | None:
    relative = {
        "codex": Path(".codex/shims/codex"),
        "decodex": Path(".cargo/bin/decodex"),
    }.get(command)
    if relative is None:
        return None
    candidate = real_home_directory() / relative
    if not candidate.exists():
        return None
    try:
        metadata = candidate.lstat()
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise AutopilotError("trusted_executable_unavailable") from error
    if command == "codex":
        if (
            not candidate.is_symlink()
            or metadata.st_uid != os.getuid()
            or metadata.st_mode & 0o022
        ):
            raise AutopilotError("trusted_executable_provenance_invalid")
        verify_openai_codex_signature(resolved)
    elif (
        candidate.is_symlink()
        or not candidate.is_file()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o022
    ):
        raise AutopilotError("trusted_executable_provenance_invalid")
    hash_file_bounded(resolved)
    return candidate


def trusted_executable(command: str) -> Path:
    if (
        not isinstance(command, str)
        or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}", command) is None
    ):
        raise AutopilotError("trusted_executable_name_invalid")
    user_executable = _trusted_user_executable(command)
    if user_executable is not None:
        return user_executable
    for directory in TRUSTED_SYSTEM_TOOL_DIRECTORIES:
        candidate = directory / command
        if not candidate.exists():
            continue
        try:
            metadata = candidate.lstat()
            resolved = candidate.resolve(strict=True)
        except OSError as error:
            raise AutopilotError("trusted_executable_unavailable") from error
        if (
            candidate.name != command
            or not resolved.is_file()
            or metadata.st_uid != 0
            or metadata.st_mode & 0o022
            or not any(
                _path_is_within(resolved, root)
                for root in TRUSTED_SYSTEM_EXECUTABLE_ROOTS
            )
        ):
            raise AutopilotError("trusted_executable_provenance_invalid")
        hash_file_bounded(resolved)
        return candidate
    raise AutopilotError("trusted_executable_unavailable")


def trusted_command_arguments(arguments: Sequence[str]) -> list[str]:
    if (
        not arguments
        or not isinstance(arguments[0], str)
        or not all(isinstance(value, str) and "\x00" not in value for value in arguments)
    ):
        raise AutopilotError("command_arguments_invalid")
    executable = Path(arguments[0])
    if not executable.is_absolute():
        if len(executable.parts) != 1:
            raise AutopilotError("trusted_executable_name_invalid")
        executable = trusted_executable(arguments[0])
    return [str(executable), *arguments[1:]]


def trusted_operational_environment() -> dict[str, str]:
    if os.environ.get("DECODEX_CANDIDATE_SANDBOX") == "1":
        try:
            home = Path(os.environ["HOME"]).resolve(strict=True)
            temporary = Path(os.environ["TMPDIR"]).resolve(strict=True)
            home.relative_to(temporary)
        except (KeyError, OSError, ValueError) as error:
            raise AutopilotError("candidate_sandbox_environment_invalid") from error
        return {
            "DECODEX_CANDIDATE_SANDBOX": "1",
            "GCM_INTERACTIVE": "never",
            "GH_PROMPT_DISABLED": "1",
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "HOME": str(home),
            "LANG": os.environ.get("LANG", "C.UTF-8"),
            "PATH": os.pathsep.join(
                str(path) for path in TRUSTED_SYSTEM_TOOL_DIRECTORIES
            ),
            "TMPDIR": str(temporary),
        }
    environment = {
        "GCM_INTERACTIVE": "never",
        "GH_PROMPT_DISABLED": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "HOME": str(real_home_directory()),
        "LANG": os.environ.get("LANG", "C.UTF-8"),
        "PATH": os.pathsep.join(
            str(path) for path in TRUSTED_SYSTEM_TOOL_DIRECTORIES
        ),
    }
    for name in (
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GPG_TTY",
        "LC_ALL",
        "NIX_SSL_CERT_FILE",
        "SSH_AUTH_SOCK",
        "SSL_CERT_DIR",
        "SSL_CERT_FILE",
        "TERM",
        "TMPDIR",
    ):
        value = os.environ.get(name)
        if value:
            environment[name] = value
    return environment


def run_command(
    arguments: Sequence[str],
    *,
    cwd: Path | None = None,
    environment: Mapping[str, str] | None = None,
    inherit_environment: bool = True,
    failure_code: str,
    allow_failure: bool = False,
    timeout_seconds: int = COMMAND_TIMEOUT_SECONDS,
    max_output_bytes: int = MAX_COMMAND_OUTPUT_BYTES,
) -> str:
    if max_output_bytes < 1 or max_output_bytes > MAX_COMMAND_OUTPUT_BYTES:
        raise AutopilotError("command_output_budget_invalid")
    command = trusted_command_arguments(arguments)
    process_environment = {
        **(trusted_operational_environment() if inherit_environment else {}),
        **dict(environment or {}),
    }
    process: subprocess.Popen[bytes] | None = None
    selector: selectors.BaseSelector | None = None
    output = bytearray()
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=process_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        if process.stdout is None or process.stderr is None:
            raise AutopilotError(failure_code)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        total_bytes = 0
        deadline = time.monotonic() + timeout_seconds
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(command, timeout_seconds)
            events = selector.select(min(remaining, 0.25))
            if not events and process.poll() is not None:
                events = [
                    (key, selectors.EVENT_READ)
                    for key in list(selector.get_map().values())
                ]
            for key, _mask in events:
                chunk = os.read(key.fd, 64 * 1024)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                total_bytes += len(chunk)
                if total_bytes > max_output_bytes:
                    raise AutopilotError("command_output_budget_exceeded")
                if key.data == "stdout":
                    output.extend(chunk)
        return_code = process.wait(timeout=max(0.01, deadline - time.monotonic()))
    except (OSError, subprocess.TimeoutExpired) as error:
        if process is not None and process.poll() is None:
            terminate_process_group(process)
        if allow_failure:
            return ""
        raise AutopilotError(failure_code) from error
    except AutopilotError:
        if process is not None and process.poll() is None:
            terminate_process_group(process)
        raise
    finally:
        if selector is not None:
            selector.close()
        if process is not None:
            if process.stdout is not None:
                process.stdout.close()
            if process.stderr is not None:
                process.stderr.close()
    if return_code != 0:
        if allow_failure:
            return ""
        raise AutopilotError(failure_code)
    try:
        return output.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise AutopilotError(failure_code) from error


def terminate_process_group(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except OSError:
        process.kill()
    process.wait()


def command_succeeds(
    arguments: Sequence[str],
    *,
    cwd: Path | None = None,
    failure_code: str,
    timeout_seconds: int = COMMAND_TIMEOUT_SECONDS,
) -> bool:
    command = trusted_command_arguments(arguments)
    process: subprocess.Popen[bytes] | None = None
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=trusted_operational_environment(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        return_code = process.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as error:
        if process is not None and process.poll() is None:
            terminate_process_group(process)
        raise AutopilotError(failure_code) from error
    except OSError as error:
        if process is not None and process.poll() is None:
            terminate_process_group(process)
        raise AutopilotError(failure_code) from error
    if return_code == 0:
        return True
    if return_code == 1:
        return False
    raise AutopilotError(failure_code)


def bounded_string_list(
    value: Any,
    *,
    pattern: re.Pattern[str] | None = None,
    maximum: int = 128,
) -> bool:
    if not isinstance(value, list) or len(value) > maximum:
        return False
    if not all(isinstance(item, str) for item in value):
        return False
    return len(value) == len(set(value)) and all(
        0 < len(item) <= 256
        and "\n" not in item
        and "\r" not in item
        and (pattern is None or pattern.fullmatch(item) is not None)
        for item in value
    )


def has_exact_keys(value: Any, keys: set[str]) -> bool:
    return isinstance(value, dict) and set(value) == keys


def is_sha256(value: Any) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def validate_path_summary(candidate: dict[str, Any]) -> None:
    summary = candidate.get("path_summary")
    kind = candidate["kind"]
    if kind == "upstream_range":
        if not has_exact_keys(
            summary,
            {"changed_path_count", "relevant_path_count", "affected_trusted_prefixes"},
        ):
            raise AutopilotError("candidate_path_summary_invalid")
        changed_count = summary["changed_path_count"]
        relevant_count = summary["relevant_path_count"]
        prefixes = summary["affected_trusted_prefixes"]
        if (
            not isinstance(changed_count, int)
            or not 0 <= changed_count <= 100_000
            or not isinstance(relevant_count, int)
            or not 0 <= relevant_count <= changed_count
            or not bounded_string_list(prefixes, pattern=SAFE_FACT_PATTERN, maximum=64)
            or any(not prefix.endswith("/") for prefix in prefixes)
        ):
            raise AutopilotError("candidate_path_summary_invalid")
        return
    if kind == "automation_repair":
        reason_code = summary.get("reason_code") if isinstance(summary, dict) else None
        expected_keys = {"repair_of", "reason_code", "evidence_sha256"}
        if reason_code == "content_loop_degraded":
            expected_keys.add("degradation_codes")
        if (
            not has_exact_keys(summary, expected_keys)
            or summary["repair_of"] != candidate.get("repair_of")
            or REASON_PATTERN.fullmatch(str(summary["reason_code"])) is None
            or not is_sha256(summary["evidence_sha256"])
        ):
            raise AutopilotError("candidate_path_summary_invalid")
        if reason_code == "content_loop_degraded" and (
            not bounded_string_list(
                summary["degradation_codes"],
                pattern=REASON_PATTERN,
                maximum=len(CONTENT_DEGRADATION_CODES),
            )
            or not summary["degradation_codes"]
            or any(
                code not in CONTENT_DEGRADATION_CODES
                for code in summary["degradation_codes"]
            )
        ):
            raise AutopilotError("candidate_path_summary_invalid")
        return
    if summary is not None:
        raise AutopilotError("candidate_path_summary_invalid")


def validate_candidate_result(candidate: dict[str, Any]) -> None:
    result = candidate.get("result")
    if result is None:
        return
    if not isinstance(result, dict):
        raise AutopilotError("candidate_result_invalid")
    outcome = result.get("outcome")
    if outcome in TERMINAL_STATUSES:
        if (
            not has_exact_keys(
                result,
                {
                    "outcome",
                    "reason_code",
                    "merge_sha",
                    "land_intent_sha256",
                    "land_execution_receipt",
                    "land_execution_receipt_sha256",
                    "decision_receipt_sha256",
                    "reviewer_receipt",
                    "resolved_at",
                },
            )
            or REASON_PATTERN.fullmatch(str(result["reason_code"])) is None
            or not isinstance(result["resolved_at"], int)
            or (
                outcome == "landed"
                and (
                    not SHA_PATTERN.fullmatch(str(result["merge_sha"]))
                    or not is_sha256(result["land_intent_sha256"])
                    or not isinstance(
                        result["land_execution_receipt"],
                        dict,
                    )
                    or not is_sha256(
                        result["land_execution_receipt_sha256"]
                    )
                    or sha256_value(result["land_execution_receipt"])
                    != result["land_execution_receipt_sha256"]
                    or result["decision_receipt_sha256"] is not None
                )
            )
            or (
                outcome != "landed"
                and (
                    result["merge_sha"] is not None
                    or result["land_intent_sha256"] is not None
                    or result["land_execution_receipt"] is not None
                    or result["land_execution_receipt_sha256"] is not None
                    or not is_sha256(result["decision_receipt_sha256"])
                )
            )
        ):
            raise AutopilotError("candidate_result_invalid")
        return
    if outcome == "blocked":
        if (
            not has_exact_keys(
                result,
                {"outcome", "reason_code", "error_digest", "at"},
            )
            or REASON_PATTERN.fullmatch(str(result["reason_code"])) is None
            or not is_sha256(result["error_digest"])
            or not isinstance(result["at"], int)
        ):
            raise AutopilotError("candidate_result_invalid")
        return
    if outcome == "repair_requested":
        if (
            not has_exact_keys(result, {"outcome", "finding_codes", "at"})
            or not bounded_string_list(
                result["finding_codes"],
                pattern=REASON_PATTERN,
                maximum=16,
            )
            or not result["finding_codes"]
            or not isinstance(result["at"], int)
        ):
            raise AutopilotError("candidate_result_invalid")
        return
    if outcome == "automation_repair_resolved":
        if (
            not has_exact_keys(
                result,
                {
                    "outcome",
                    "repair_candidate_id",
                    "merge_sha",
                    "repair_outcome",
                    "blocked_role",
                    "resumed_role",
                    "at",
                },
            )
            or re.fullmatch(
                r"[0-9a-f]{16}",
                str(result["repair_candidate_id"]),
            )
            is None
            or result["repair_outcome"] not in {"landed", "no_change"}
            or result["blocked_role"] not in {"maintainer", "reviewer"}
            or result["resumed_role"] not in {"maintainer", "reviewer"}
            or (
                result["repair_outcome"] == "landed"
                and not SHA_PATTERN.fullmatch(str(result["merge_sha"]))
            )
            or (
                result["repair_outcome"] == "no_change"
                and result["merge_sha"] is not None
            )
            or not isinstance(result["at"], int)
        ):
            raise AutopilotError("candidate_result_invalid")
        return
    raise AutopilotError("candidate_result_invalid")


def load_policy(path: Path = DEFAULT_POLICY_PATH) -> dict[str, Any]:
    try:
        policy = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AutopilotError("policy_unavailable") from error
    if not has_exact_keys(policy, POLICY_KEYS):
        raise AutopilotError("policy_shape_invalid")
    if policy.get("schema") != POLICY_SCHEMA:
        raise AutopilotError("policy_schema_invalid")
    if policy.get("upstream_repository") != "https://github.com/openai/codex.git":
        raise AutopilotError("policy_upstream_repository_invalid")
    if policy.get("upstream_branch") != "main":
        raise AutopilotError("policy_upstream_branch_invalid")
    if policy.get("target_repository") != "hack-ink/decodex":
        raise AutopilotError("policy_target_repository_invalid")
    if policy.get("target_branch") != "main":
        raise AutopilotError("policy_target_branch_invalid")
    if policy.get("branch_prefix") != "xv/codex-upstream-":
        raise AutopilotError("policy_branch_prefix_invalid")
    if not is_sha256(policy.get("decodex_executable_sha256")):
        raise AutopilotError("policy_decodex_identity_invalid")
    marker = policy.get("accepted_schema_marker_path")
    if (
        not isinstance(marker, str)
        or Path(marker).is_absolute()
        or ".." in Path(marker).parts
        or not marker.startswith("crates/decodex-codex/schema/")
    ):
        raise AutopilotError("policy_schema_marker_invalid")
    try:
        max_batch_commits = int(policy.get("max_batch_commits", 0))
        max_attempts = int(policy.get("max_attempts", 0))
        lease_seconds = int(policy.get("lease_seconds", 0))
        lease_write_guard_seconds = int(policy.get("lease_write_guard_seconds", 0))
        max_lease_renewals = int(policy.get("max_lease_renewals", -1))
        retry_backoff = [int(value) for value in policy.get("retry_backoff_seconds", [])]
    except (TypeError, ValueError) as error:
        raise AutopilotError("policy_numeric_value_invalid") from error
    if not 1 <= max_batch_commits <= 128:
        raise AutopilotError("policy_batch_limit_invalid")
    if not 1 <= max_attempts <= 10:
        raise AutopilotError("policy_attempt_limit_invalid")
    if not (
        LAND_EFFECT_LEASE_BUDGET_SECONDS
        <= lease_seconds
        <= 21_600
    ):
        raise AutopilotError("policy_lease_invalid")
    if not (
        SIDE_EFFECT_LEASE_BUDGET_SECONDS
        <= lease_write_guard_seconds
        < lease_seconds
    ):
        raise AutopilotError("policy_lease_invalid")
    if not 0 <= max_lease_renewals <= 12:
        raise AutopilotError("policy_lease_invalid")
    if (
        len(retry_backoff) != max_attempts
        or any(value < 1 or value > 86400 for value in retry_backoff)
        or retry_backoff != sorted(retry_backoff)
    ):
        raise AutopilotError("policy_retry_backoff_invalid")
    profiles = policy.get("validation_profiles")
    required_profiles = policy.get("required_validation_profiles")
    if (
        not has_exact_keys(profiles, set(VALIDATION_PROFILE_COMMANDS))
        or required_profiles != list(REQUIRED_VALIDATION_PROFILES)
    ):
        raise AutopilotError("policy_validation_profiles_invalid")
    for name in VALIDATION_PROFILE_COMMANDS:
        arguments = profiles.get(name)
        if (
            not isinstance(arguments, list)
            or not 1 <= len(arguments) <= 16
            or any(
                not isinstance(argument, str)
                or not argument
                or len(argument) > 256
                or "\n" in argument
                or "\r" in argument
                for argument in arguments
            )
        ):
            raise AutopilotError("policy_validation_profiles_invalid")
    if profiles != VALIDATION_PROFILE_COMMANDS:
        raise AutopilotError("policy_validation_profiles_invalid")
    for key in (
        "required_experimental_request_methods",
        "required_stable_request_methods",
        "required_notification_methods",
    ):
        if not bounded_string_list(policy.get(key), pattern=METHOD_PATTERN):
            raise AutopilotError("policy_required_methods_invalid")
    prefixes = policy.get("relevant_path_prefixes")
    if not bounded_string_list(prefixes):
        raise AutopilotError("policy_relevant_paths_invalid")
    if any(
        Path(prefix).is_absolute()
        or ".." in Path(prefix).parts
        or not prefix.endswith("/")
        for prefix in prefixes
    ):
        raise AutopilotError("policy_relevant_paths_invalid")
    return policy


def hash_file_bounded(path: Path, *, maximum_bytes: int = MAX_EXECUTABLE_BYTES) -> str:
    if path.is_symlink() or not path.is_file():
        raise AutopilotError("executable_invalid")
    try:
        if path.stat().st_size > maximum_bytes:
            raise AutopilotError("executable_byte_budget")
        digest = hashlib.sha256()
        total = 0
        with path.open("rb") as handle:
            while chunk := handle.read(1024 * 1024):
                total += len(chunk)
                if total > maximum_bytes:
                    raise AutopilotError("executable_byte_budget")
                digest.update(chunk)
    except OSError as error:
        raise AutopilotError("executable_unavailable") from error
    return digest.hexdigest()


def resolve_executable(command: str) -> tuple[Path, str]:
    execution_path = trusted_executable(command)
    try:
        resolved = execution_path.resolve(strict=True)
    except OSError as error:
        raise AutopilotError("trusted_executable_unavailable") from error
    return resolved, hash_file_bounded(resolved)


def target_remote_matches(remote: str, repository: str) -> bool:
    owner, name = repository.split("/", 1)
    return remote in {
        f"git@github.com:{owner}/{name}.git",
        f"git@github.com:{owner}/{name}",
        f"https://github.com/{owner}/{name}.git",
        f"https://github.com/{owner}/{name}",
        f"ssh://git@github.com/{owner}/{name}.git",
        f"ssh://git@github.com/{owner}/{name}",
    }


def target_origin_urls(repo_root: Path, repository: str) -> tuple[str, tuple[str, ...]]:
    fetch_url = run_command(
        ["git", "remote", "get-url", "origin"],
        cwd=repo_root,
        failure_code="target_origin_unavailable",
    )
    push_output = run_command(
        ["git", "remote", "get-url", "--push", "--all", "origin"],
        cwd=repo_root,
        failure_code="target_origin_unavailable",
    )
    push_urls = tuple(value for value in push_output.splitlines() if value)
    if (
        not target_remote_matches(fetch_url, repository)
        or not push_urls
        or any(not target_remote_matches(value, repository) for value in push_urls)
    ):
        raise AutopilotError("target_origin_mismatch")
    return fetch_url, push_urls


def resolve_primary_checkout(source_root: Path, branch: str = "main") -> Path:
    output = run_command(
        ["git", "worktree", "list", "--porcelain"],
        cwd=source_root,
        failure_code="worktree_inventory_unavailable",
    )
    current_path: Path | None = None
    for line in [*output.splitlines(), ""]:
        if line.startswith("worktree "):
            current_path = Path(line.removeprefix("worktree ")).resolve()
            continue
        if line == f"branch refs/heads/{branch}" and current_path is not None:
            return current_path
        if not line:
            current_path = None
    raise AutopilotError("primary_checkout_unavailable")


def assert_primary_clean_main(repo_root: Path, policy: dict[str, Any]) -> dict[str, str]:
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=repo_root,
            failure_code="repository_unavailable",
        )
    ).resolve()
    if root != repo_root.resolve():
        raise AutopilotError("repository_root_mismatch")
    if ".worktrees" in root.parts:
        raise AutopilotError("automation_worktree_binding")
    git_dir_text = run_command(
        ["git", "rev-parse", "--git-dir"],
        cwd=root,
        failure_code="git_directory_unavailable",
    )
    git_dir = Path(git_dir_text)
    if not git_dir.is_absolute():
        git_dir = root / git_dir
    if git_dir.resolve() != (root / ".git").resolve():
        raise AutopilotError("automation_not_primary_checkout")
    branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=root,
        failure_code="branch_unavailable",
    )
    if branch != policy["target_branch"]:
        raise AutopilotError("automation_not_on_main")
    status = run_command(
        ["git", "status", "--porcelain=v1"],
        cwd=root,
        failure_code="git_status_unavailable",
    )
    if status:
        raise AutopilotError("primary_checkout_dirty")
    target_origin_urls(root, policy["target_repository"])
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        failure_code="head_unavailable",
    )
    if not SHA_PATTERN.fullmatch(head):
        raise AutopilotError("head_invalid")
    run_command(
        [
            "git",
            "fetch",
            "--quiet",
            "origin",
            f"refs/heads/{policy['target_branch']}:refs/remotes/origin/{policy['target_branch']}",
        ],
        cwd=root,
        failure_code="target_main_fetch_failed",
    )
    remote_head = run_command(
        ["git", "rev-parse", f"refs/remotes/origin/{policy['target_branch']}"],
        cwd=root,
        failure_code="target_main_unavailable",
    )
    if head != remote_head:
        raise AutopilotError("primary_main_stale")
    return {"repo_root": str(root), "branch": branch, "head": head}


def assert_primary_snapshot(
    repo_root: Path,
    policy: dict[str, Any],
    expected_head: str,
) -> None:
    if not SHA_PATTERN.fullmatch(expected_head):
        raise AutopilotError("head_invalid")
    branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=repo_root,
        failure_code="branch_unavailable",
    )
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        failure_code="head_unavailable",
    )
    status = run_command(
        ["git", "status", "--porcelain=v1"],
        cwd=repo_root,
        failure_code="git_status_unavailable",
    )
    try:
        target_origin_urls(repo_root, policy["target_repository"])
        origin_matches = True
    except AutopilotError as error:
        if error.code != "target_origin_mismatch":
            raise
        origin_matches = False
    if (
        branch != policy["target_branch"]
        or head != expected_head
        or status
        or not origin_matches
    ):
        raise AutopilotError("primary_snapshot_changed")


def refresh_primary_snapshot(
    repo_root: Path,
    policy: dict[str, Any],
    expected_head: str,
) -> str:
    assert_primary_snapshot(repo_root, policy, expected_head)
    run_command(
        [
            "git",
            "fetch",
            "--quiet",
            "origin",
            (
                f"refs/heads/{policy['target_branch']}:"
                f"refs/remotes/origin/{policy['target_branch']}"
            ),
        ],
        cwd=repo_root,
        failure_code="target_main_fetch_failed",
    )
    remote_head = run_command(
        ["git", "rev-parse", f"refs/remotes/origin/{policy['target_branch']}"],
        cwd=repo_root,
        failure_code="target_main_unavailable",
    )
    if remote_head != expected_head:
        raise AutopilotError("primary_snapshot_changed")
    return remote_head


def ensure_cache_root(cache_root: Path) -> Path:
    if cache_root.exists() and cache_root.is_symlink():
        raise AutopilotError("cache_root_symlink")
    try:
        cache_root.mkdir(parents=True, exist_ok=True)
        resolved = cache_root.resolve()
        os.chmod(resolved, 0o700)
    except OSError as error:
        raise AutopilotError("cache_root_unavailable") from error
    if ".agent" not in resolved.parts or "automations" not in resolved.parts:
        raise AutopilotError("cache_root_outside_automation_state")
    return resolved


def atomic_write_json(path: Path, value: Any) -> None:
    if path.exists() and path.is_symlink():
        raise AutopilotError("state_path_symlink")
    if path.parent.exists() and (
        path.parent.is_symlink() or not path.parent.is_dir()
    ):
        raise AutopilotError("state_parent_invalid")
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.parent.is_symlink():
        raise AutopilotError("state_parent_invalid")
    temporary_name: str | None = None
    try:
        descriptor, temporary_name = tempfile.mkstemp(
            prefix=f".{path.name}.",
            suffix=".tmp",
            dir=path.parent,
        )
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary_name, 0o600)
        os.replace(temporary_name, path)
        directory_descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory_descriptor)
        finally:
            os.close(directory_descriptor)
    except OSError as error:
        if temporary_name is not None:
            try:
                Path(temporary_name).unlink(missing_ok=True)
            except OSError:
                pass
        raise AutopilotError("state_write_failed") from error
