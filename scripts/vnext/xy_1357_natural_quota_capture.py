#!/usr/bin/env python3
"""Capture one lossless, allowlisted XY-1357 quota timestamp receipt."""

from __future__ import annotations

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import stat
import subprocess
import tempfile
import time
from typing import Any


RECEIPT_SCHEMA = "decodex/xy-1357-natural-quota-evidence/1"
RATE_LIMIT_METHOD = "account/rateLimits/read"
REQUEST_SEQUENCE = ("initialize", "initialized", RATE_LIMIT_METHOD)
MAX_FRAME_BYTES = 1_024 * 1_024
MAX_SCHEMA_BYTES = 16 * 1_024 * 1_024
MAX_TIMESTAMP_MICROS = 253_402_300_799_999_999
NUMBER_PATTERN = re.compile(
    r"(?P<sign>-?)(?P<integer>0|[1-9][0-9]*)"
    r"(?:\.(?P<fraction>[0-9]+))?"
    r"(?:[eE](?P<exponent>[+-]?[0-9]+))?\Z"
)
UNSAFE_TEXT_PATTERNS = (
    re.compile(r"(?i)\b(?:bearer|authorization)\b"),
    re.compile(r"(?i)\bsk-[a-z0-9_-]{8,}\b"),
    re.compile(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b"),
    re.compile(r"/(?:Users|home)/"),
    re.compile(r"(?i)\.codex(?:/|\\)"),
)


class CaptureError(RuntimeError):
    """A closed failure that never contains upstream data."""

    def __init__(self, code: str) -> None:
        super().__init__(code)
        self.code = code


class JsonNumberToken(str):
    """An exact JSON number lexeme retained before numeric conversion."""


def reject_json_constant(_value: str) -> None:
    raise CaptureError("non_finite_json_number")


def unique_json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CaptureError("duplicate_json_key")
        result[key] = value
    return result


def decode_json_frame(raw: bytes) -> dict[str, Any]:
    """Decode one frame while retaining every numeric lexeme exactly."""
    if not raw or len(raw) > MAX_FRAME_BYTES:
        raise CaptureError("invalid_frame_size")
    try:
        text = raw.decode("utf-8", errors="strict")
        value = json.loads(
            text,
            parse_int=JsonNumberToken,
            parse_float=JsonNumberToken,
            parse_constant=reject_json_constant,
            object_pairs_hook=unique_json_object,
        )
    except UnicodeDecodeError as error:
        raise CaptureError("invalid_frame_utf8") from error
    except json.JSONDecodeError as error:
        raise CaptureError("invalid_frame_json") from error
    if not isinstance(value, dict):
        raise CaptureError("invalid_frame_shape")
    return value


def exact_integer(value: Any, code: str) -> int:
    if not isinstance(value, JsonNumberToken) or not re.fullmatch(r"-?(?:0|[1-9][0-9]*)", value):
        raise CaptureError(code)
    return int(value)


def utc_text(timestamp_micros: int) -> str:
    seconds, micros = divmod(timestamp_micros, 1_000_000)
    instant = datetime(1970, 1, 1, tzinfo=timezone.utc) + timedelta(
        seconds=seconds, microseconds=micros
    )
    return f"{instant:%Y-%m-%dT%H:%M:%S}.{micros:06d}Z"


def wall_clock_text(timestamp_ns: int) -> str:
    seconds, nanos = divmod(timestamp_ns, 1_000_000_000)
    instant = datetime.fromtimestamp(seconds, timezone.utc)
    return f"{instant:%Y-%m-%dT%H:%M:%S}.{nanos:09d}Z"


def convert_timestamp_token(token: JsonNumberToken) -> dict[str, Any]:
    """Convert exact Unix seconds to microseconds without rounding or truncation."""
    match = NUMBER_PATTERN.fullmatch(token)
    if match is None:
        return {"status": "precision_incompatible", "reason": "invalid_number_lexeme"}

    integer = match.group("integer")
    fraction = match.group("fraction") or ""
    exponent_text = match.group("exponent")
    exponent = int(exponent_text or "0")
    if len(integer) + len(fraction) > 40 or abs(exponent) > 30:
        return {"status": "precision_incompatible", "reason": "unsupported_magnitude"}

    numerator = int(integer + fraction)
    if match.group("sign") == "-":
        numerator = -numerator
    decimal_places = len(fraction) - exponent
    if decimal_places >= 0:
        denominator = 10**decimal_places
    else:
        numerator *= 10 ** (-decimal_places)
        denominator = 1

    scaled_numerator = numerator * 1_000_000
    microseconds, remainder = divmod(scaled_numerator, denominator)
    lexical_form = "exponent" if exponent_text is not None else "decimal" if fraction else "integer"
    arithmetic = {
        "seconds_numerator": numerator,
        "seconds_denominator": denominator,
        "microsecond_scale": 1_000_000,
        "scaled_numerator": scaled_numerator,
        "division_remainder": remainder,
    }
    representation = {
        "json_type": "number",
        "lexical_form": lexical_form,
        "fractional_digits": len(fraction),
        "decimal_exponent": exponent,
    }
    if remainder != 0:
        return {
            "status": "precision_incompatible",
            "reason": "would_round_or_truncate",
            "representation": representation,
            "exact_arithmetic": arithmetic,
        }
    if not 0 <= microseconds <= MAX_TIMESTAMP_MICROS:
        return {
            "status": "precision_incompatible",
            "reason": "outside_product_range",
            "representation": representation,
            "exact_arithmetic": arithmetic,
        }
    return {
        "status": "exact",
        "representation": representation,
        "exact_arithmetic": arithmetic,
        "utc_unix_microseconds": microseconds,
        "utc": utc_text(microseconds),
    }


def count_reset_fields(value: Any) -> int:
    if isinstance(value, dict):
        return sum(
            (1 if key == "resetsAt" else 0) + count_reset_fields(child)
            for key, child in value.items()
        )
    if isinstance(value, list):
        return sum(count_reset_fields(child) for child in value)
    return 0


def extract_observations(result: dict[str, Any]) -> tuple[list[dict[str, Any]], list[str]]:
    observations: list[dict[str, Any]] = []
    limitations: list[str] = []
    classified_reset_fields = 0

    snapshots: list[tuple[str, str, dict[str, Any]]] = []
    legacy = result.get("rateLimits")
    if isinstance(legacy, dict):
        snapshots.append(("rateLimits", "legacy-view", legacy))
    elif legacy is not None:
        limitations.append("rateLimits was not an object")

    buckets = result.get("rateLimitsByLimitId")
    if isinstance(buckets, dict):
        for index, raw_key in enumerate(sorted(buckets), start=1):
            snapshot = buckets[raw_key]
            if isinstance(snapshot, dict):
                snapshots.append(("rateLimitsByLimitId", f"bucket-{index}", snapshot))
            else:
                limitations.append("one aliased rate-limit bucket was not an object")
    elif buckets is not None:
        limitations.append("rateLimitsByLimitId was not an object")

    for source_view, bucket_alias, snapshot in snapshots:
        for window_name in ("primary", "secondary"):
            window = snapshot.get(window_name)
            if not isinstance(window, dict) or "resetsAt" not in window:
                continue
            classified_reset_fields += 1
            token = window["resetsAt"]
            if token is None:
                limitations.append(f"{bucket_alias} {window_name} reset was null")
                continue
            if not isinstance(token, JsonNumberToken):
                limitations.append(f"{bucket_alias} {window_name} reset was not numeric")
                continue
            duration = window.get("windowDurationMins")
            try:
                duration_minutes = None if duration is None else exact_integer(
                    duration, "invalid_window_duration"
                )
            except CaptureError:
                duration_minutes = None
                limitations.append(f"{bucket_alias} {window_name} duration was not an integer")
            observations.append(
                {
                    "source_view": source_view,
                    "bucket_alias": bucket_alias,
                    "timestamp_role": f"{window_name}_window_reset",
                    "window_duration_minutes": duration_minutes,
                    "raw_json_token": str(token),
                    "source_unit": "unix_seconds",
                    "conversion": convert_timestamp_token(token),
                }
            )

        individual = snapshot.get("individualLimit")
        if isinstance(individual, dict) and "resetsAt" in individual:
            classified_reset_fields += 1
            token = individual["resetsAt"]
            if isinstance(token, JsonNumberToken):
                observations.append(
                    {
                        "source_view": source_view,
                        "bucket_alias": bucket_alias,
                        "timestamp_role": "individual_spend_control_reset",
                        "window_duration_minutes": None,
                        "raw_json_token": str(token),
                        "source_unit": "unix_seconds",
                        "conversion": convert_timestamp_token(token),
                    }
                )
            elif token is None:
                limitations.append(f"{bucket_alias} individual reset was null")
            else:
                limitations.append(f"{bucket_alias} individual reset was not numeric")

    total_reset_fields = count_reset_fields(result)
    if total_reset_fields != classified_reset_fields:
        limitations.append(
            f"{total_reset_fields - classified_reset_fields} reset field(s) were outside the allowlisted quota paths"
        )
    if not observations:
        limitations.append("the receipt contained no usable reset timestamp token")
    return observations, limitations


def receipt_verdict(
    observations: list[dict[str, Any]], limitations: list[str]
) -> str:
    if not observations or any(
        "not numeric" in item or "outside the allowlisted" in item for item in limitations
    ):
        return "insufficient_evidence"
    if any(item["conversion"]["status"] != "exact" for item in observations):
        return "precision_incompatible"
    return "exact_microseconds_compatible"


def safe_text(value: Any, code: str) -> str:
    if not isinstance(value, str) or not value or len(value.encode("utf-8")) > 256:
        raise CaptureError(code)
    if any(character in value for character in "\r\n\0"):
        raise CaptureError(code)
    if any(pattern.search(value) for pattern in UNSAFE_TEXT_PATTERNS):
        raise CaptureError(code)
    return value


def sha256_file(path: Path, byte_limit: int | None = None) -> str:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            total += len(chunk)
            if byte_limit is not None and total > byte_limit:
                raise CaptureError("file_size_limit_exceeded")
            digest.update(chunk)
    return digest.hexdigest()


def schema_contains_method(value: Any, method: str) -> bool:
    if isinstance(value, dict):
        if value.get("enum") == [method]:
            return True
        return any(schema_contains_method(child, method) for child in value.values())
    if isinstance(value, list):
        return any(schema_contains_method(child, method) for child in value)
    return False


def load_schema(path: Path) -> dict[str, Any]:
    if not path.is_file() or path.stat().st_size > MAX_SCHEMA_BYTES:
        raise CaptureError("generated_schema_unavailable")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CaptureError("generated_schema_invalid") from error
    if not isinstance(value, dict):
        raise CaptureError("generated_schema_invalid")
    return value


def attest_schema(executable: Path, timeout: float) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="xy-1357-schema-") as directory:
        output = Path(directory)
        try:
            completed = subprocess.run(
                [
                    str(executable),
                    "app-server",
                    "generate-json-schema",
                    "--experimental",
                    "--out",
                    str(output),
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=timeout,
                check=False,
                start_new_session=True,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise CaptureError("schema_generation_failed") from error
        if completed.returncode != 0:
            raise CaptureError("schema_generation_failed")

        request_path = output / "ClientRequest.json"
        response_path = output / "v2" / "GetAccountRateLimitsResponse.json"
        aggregate_path = output / "codex_app_server_protocol.v2.schemas.json"
        request = load_schema(request_path)
        response = load_schema(response_path)
        if not schema_contains_method(request, RATE_LIMIT_METHOD):
            raise CaptureError("rate_limit_method_not_advertised")
        try:
            reset_types = response["definitions"]["RateLimitWindow"]["properties"][
                "resetsAt"
            ]["type"]
        except (KeyError, TypeError) as error:
            raise CaptureError("reset_schema_missing") from error
        if reset_types != ["integer", "null"]:
            raise CaptureError("reset_schema_incompatible")
        return {
            "rate_limit_method_advertised": True,
            "window_reset_json_types": reset_types,
            "rate_limit_response_schema_sha256": sha256_file(
                response_path, MAX_SCHEMA_BYTES
            ),
            "aggregate_v2_schema_sha256": sha256_file(
                aggregate_path, MAX_SCHEMA_BYTES
            ),
        }


def resolve_executable(command: str) -> Path:
    candidate = shutil.which(command)
    if candidate is None:
        raise CaptureError("codex_executable_unavailable")
    try:
        path = Path(candidate).resolve(strict=True)
        mode = path.stat().st_mode
    except OSError as error:
        raise CaptureError("codex_executable_unavailable") from error
    if not stat.S_ISREG(mode) or not os.access(path, os.X_OK):
        raise CaptureError("codex_executable_unavailable")
    return path


def codex_version(executable: Path, timeout: float) -> str:
    try:
        completed = subprocess.run(
            [str(executable), "--version"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=timeout,
            check=False,
            start_new_session=True,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise CaptureError("codex_version_unavailable") from error
    if completed.returncode != 0 or len(completed.stdout) > 256:
        raise CaptureError("codex_version_unavailable")
    try:
        value = completed.stdout.decode("utf-8", errors="strict").strip()
    except UnicodeDecodeError as error:
        raise CaptureError("codex_version_unavailable") from error
    return safe_text(value, "codex_version_unsafe")


def shutdown_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=1)
    except (OSError, subprocess.TimeoutExpired):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except OSError:
            pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired as error:
            raise CaptureError("app_server_cleanup_failed") from error


class AppServerSession:
    def __init__(self, process: subprocess.Popen[bytes], timeout: float) -> None:
        self.process = process
        self.timeout = timeout
        self.next_id = 1
        self.rate_limit_read_count = 0

    def send(self, message: dict[str, Any]) -> None:
        if self.process.stdin is None:
            raise CaptureError("app_server_stdin_unavailable")
        encoded = json.dumps(message, separators=(",", ":")).encode("utf-8") + b"\n"
        try:
            self.process.stdin.write(encoded)
            self.process.stdin.flush()
        except (BrokenPipeError, OSError) as error:
            raise CaptureError("app_server_write_failed") from error

    def receive(self, deadline: float) -> dict[str, Any]:
        if self.process.stdout is None:
            raise CaptureError("app_server_stdout_unavailable")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise CaptureError("app_server_response_timeout")
        ready, _, _ = select.select([self.process.stdout], [], [], remaining)
        if not ready:
            raise CaptureError("app_server_response_timeout")
        raw = self.process.stdout.readline(MAX_FRAME_BYTES + 2)
        if not raw:
            raise CaptureError("app_server_exited")
        if len(raw) > MAX_FRAME_BYTES or not raw.endswith(b"\n"):
            raise CaptureError("app_server_frame_limit")
        return decode_json_frame(raw[:-1])

    def request(self, method: str, params: dict[str, Any] | None = None) -> Any:
        request_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            message["params"] = params
        if method == RATE_LIMIT_METHOD:
            if self.rate_limit_read_count != 0:
                raise CaptureError("rate_limit_read_budget_exceeded")
            self.rate_limit_read_count = 1
        self.send(message)

        deadline = time.monotonic() + self.timeout
        while True:
            incoming = self.receive(deadline)
            incoming_id = incoming.get("id")
            if incoming_id is not None:
                try:
                    numeric_id = exact_integer(incoming_id, "invalid_response_id")
                except CaptureError:
                    numeric_id = -1
                if numeric_id == request_id and "method" not in incoming:
                    if incoming.get("error") is not None:
                        raise CaptureError(f"{method.replace('/', '_')}_rejected")
                    if "result" not in incoming:
                        raise CaptureError("response_result_missing")
                    return incoming["result"]
                if "method" in incoming:
                    self.send(
                        {
                            "jsonrpc": "2.0",
                            "id": numeric_id,
                            "error": {"code": -32601, "message": "unsupported"},
                        }
                    )
                    continue
                raise CaptureError("unexpected_response_id")
            if "method" in incoming:
                continue
            raise CaptureError("invalid_protocol_message")


def launch_app_server(executable: Path) -> subprocess.Popen[bytes]:
    try:
        return subprocess.Popen(
            [str(executable), "app-server", "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            bufsize=0,
            start_new_session=True,
        )
    except OSError as error:
        raise CaptureError("app_server_spawn_failed") from error


def assert_safe_receipt(receipt: dict[str, Any]) -> None:
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":"))
    if any(pattern.search(encoded) for pattern in UNSAFE_TEXT_PATTERNS):
        raise CaptureError("receipt_allowlist_failed")
    forbidden_keys = {
        "accessToken",
        "accountId",
        "chatgptAccountId",
        "email",
        "prompt",
        "threadId",
        "token",
    }

    def inspect(value: Any) -> None:
        if isinstance(value, dict):
            if forbidden_keys.intersection(value):
                raise CaptureError("receipt_allowlist_failed")
            for child in value.values():
                inspect(child)
        elif isinstance(value, list):
            for child in value:
                inspect(child)

    inspect(receipt)


def capture(codex_command: str, timeout: float) -> dict[str, Any]:
    executable = resolve_executable(codex_command)
    executable_digest = sha256_file(executable)
    version = codex_version(executable, timeout)
    schema = attest_schema(executable, timeout)
    started_ns = time.time_ns()
    process = launch_app_server(executable)
    session = AppServerSession(process, timeout)
    failure_code: str | None = None
    observations: list[dict[str, Any]] = []
    limitations: list[str] = []
    app_server_user_agent: str | None = None
    try:
        initialize = session.request(
            "initialize",
            {
                "clientInfo": {
                    "name": "decodex-xy-1357-evidence",
                    "version": "1",
                },
                "capabilities": {"experimentalApi": True},
            },
        )
        if not isinstance(initialize, dict):
            raise CaptureError("initialize_result_invalid")
        app_server_user_agent = safe_text(
            initialize.get("userAgent"), "app_server_user_agent_unsafe"
        )
        session.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        rate_limits = session.request(RATE_LIMIT_METHOD)
        if not isinstance(rate_limits, dict):
            raise CaptureError("rate_limit_result_invalid")
        observations, limitations = extract_observations(rate_limits)
    except CaptureError as error:
        failure_code = error.code
    finally:
        shutdown_process(process)
    completed_ns = time.time_ns()

    if sha256_file(executable) != executable_digest:
        failure_code = "codex_executable_changed"
    verdict = (
        "insufficient_evidence"
        if failure_code is not None
        else receipt_verdict(observations, limitations)
    )
    receipt: dict[str, Any] = {
        "schema": RECEIPT_SCHEMA,
        "issue": "XY-1357",
        "verdict": verdict,
        "source": {
            "account_alias": "ambient-account-1",
            "codex_cli_version": version,
            "codex_executable_sha256": executable_digest,
            "app_server_user_agent": app_server_user_agent,
            "app_server_build_basis": "same exact executable as codex_cli_version",
            "generated_schema": schema,
        },
        "capture": {
            "started_at_utc": wall_clock_text(started_ns),
            "completed_at_utc": wall_clock_text(completed_ns),
            "transport": "JSON-RPC 2.0 newline frames over app-server stdio",
            "method": RATE_LIMIT_METHOD,
            "rate_limit_read_count": session.rate_limit_read_count,
            "request_sequence": list(REQUEST_SEQUENCE),
            "turn_start_count": 0,
            "tool_invocation_count": 0,
            "account_login_or_configuration_call_count": 0,
            "routing_call_count": 0,
            "raw_capture_boundary": (
                "bounded UTF-8 response frame decoded with exact JSON number lexemes before conversion"
            ),
            "complete_unredacted_response_retained": False,
        },
        "redaction": {
            "method": "strict constructed allowlist with opaque bucket and account aliases",
            "raw_bucket_identifiers_retained": False,
            "account_identity_retained": False,
            "credential_or_auth_values_directly_read_by_capture_script": False,
            "ambient_app_server_authentication_used": True,
            "unrelated_response_fields_retained": False,
        },
        "observations": observations,
        "limitations": limitations,
        "failure": None if failure_code is None else {"code": failure_code},
        "routing_enabled": False,
        "xy_1304_input": verdict,
    }
    assert_safe_receipt(receipt)
    return receipt


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--codex", default="codex")
    result.add_argument("--timeout-seconds", type=float, default=20.0)
    return result


def main() -> int:
    args = parser().parse_args()
    if not 1.0 <= args.timeout_seconds <= 60.0:
        print(
            json.dumps(
                {
                    "schema": RECEIPT_SCHEMA,
                    "verdict": "insufficient_evidence",
                    "failure": {"code": "invalid_timeout"},
                    "capture": {"rate_limit_read_count": 0},
                },
                sort_keys=True,
            )
        )
        return 2
    try:
        receipt = capture(args.codex, args.timeout_seconds)
    except CaptureError as error:
        receipt = {
            "schema": RECEIPT_SCHEMA,
            "issue": "XY-1357",
            "verdict": "insufficient_evidence",
            "failure": {"code": error.code},
            "capture": {"rate_limit_read_count": 0},
            "routing_enabled": False,
            "xy_1304_input": "insufficient_evidence",
        }
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0 if receipt["verdict"] == "exact_microseconds_compatible" else 2


if __name__ == "__main__":
    raise SystemExit(main())
