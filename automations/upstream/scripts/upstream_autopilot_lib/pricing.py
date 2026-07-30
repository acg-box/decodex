"""Audit the bounded official X API pricing contract."""

from __future__ import annotations

from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import time
from typing import Any, Callable

from .core import (
    X_PRICING_AUDIT_EVIDENCE_SCHEMA,
    X_PRICING_PARSER_VERSION,
    X_PRICING_RATE_KEYS,
    X_PRICING_SOURCE_URL,
    AutopilotError,
    atomic_write_json,
    validate_x_pricing_audit_evidence,
)

__all__ = [
    "PricingAuditFailure",
    "X_PRICING_COMPILED_RATES_MICROUSD",
    "X_PRICING_FAILURE_RELATIVE_PATH",
    "X_PRICING_FRESH_SECONDS",
    "X_PRICING_LOCK_RELATIVE_PATH",
    "X_PRICING_MAX_RECEIPT_BYTES",
    "X_PRICING_MAX_SOURCE_BYTES",
    "X_PRICING_MONTHLY_CAP_MICROUSD",
    "X_PRICING_RECEIPT_RELATIVE_PATH",
    "audit_x_pricing",
    "fetch_official_x_pricing",
    "parse_x_pricing_markdown",
]


X_PRICING_RECEIPT_SCHEMA = "decodex/x-pricing-audit-receipt/1"
X_PRICING_FAILURE_SCHEMA = "decodex/x-pricing-audit-failure/2"
X_PRICING_DIAGNOSTIC_SCHEMA = "decodex/x-pricing-parser-diagnostic/1"
X_PRICING_PARSER_CONTRACT = "official-credit-consumption-tables/1"
X_PRICING_RECEIPT_RELATIVE_PATH = Path(
    ".agent/automations/decodex/cache/social/x/x-pricing-receipt.json"
)
X_PRICING_FAILURE_RELATIVE_PATH = Path(
    ".agent/automations/decodex/cache/social/x/x-pricing-failure.json"
)
X_PRICING_LOCK_RELATIVE_PATH = Path(
    ".agent/automations/decodex/cache/social/x/x-pricing-audit.lock"
)
X_PRICING_MAX_SOURCE_BYTES = 1024 * 1024
X_PRICING_MAX_RECEIPT_BYTES = 16 * 1024
X_PRICING_FETCH_TIMEOUT_SECONDS = 10
X_PRICING_FRESH_SECONDS = 36 * 60 * 60
X_PRICING_MONTHLY_CAP_MICROUSD = 1_250_000
X_PRICING_COMPILED_RATES_MICROUSD = {
    "post_create": 15_000,
    "post_create_with_url": 200_000,
    "post_read": 5_000,
    "user_read": 10_000,
}

_CURL_PATH = Path("/usr/bin/curl")
_PRICE_PATTERN = re.compile(
    r"^\\?\$([0-9]+)(?:\.([0-9]{1,6}))? per (resource|request)$"
)
_TABLE_SEPARATOR_PATTERN = re.compile(r"^:?-{3,}:?$")
_TARGET_SECTION_HEADING = "## Credit consumption details"
_TARGET_SECTION_UNIT_STATEMENT = (
    "All prices are per resource fetched (reads) or per request "
    "(writes/actions)."
)
_READ_HEADING = "### Read operations"
_WRITE_HEADING = "### Write operations"
_READ_DESCRIPTION = "Charged per resource returned in the response."
_WRITE_DESCRIPTION = "Charged per request."
_TIMESTAMP_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T"
    r"[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
_READ_RATE_LABELS = {
    "Posts: Read": "post_read",
    "User: Read": "user_read",
}
_WRITE_RATE_LABELS = {
    "Post: Create": "post_create",
    "Post: Create (with URL)": "post_create_with_url",
}


class PricingAuditFailure(AutopilotError):
    """A classified pricing fetch or parser failure."""

    def __init__(self, code: str, *, raw: bytes | None = None) -> None:
        self.raw = raw
        super().__init__(code)


def fetch_official_x_pricing() -> bytes:
    """Fetch the pinned Markdown document within one total 10-second deadline."""

    deadline = time.monotonic() + X_PRICING_FETCH_TIMEOUT_SECONDS
    executable = _trusted_curl_path()
    arguments = [
        executable,
        "--disable",
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-redirs",
        "0",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        str(X_PRICING_FETCH_TIMEOUT_SECONDS),
        "--max-time",
        str(X_PRICING_FETCH_TIMEOUT_SECONDS),
        "--max-filesize",
        str(X_PRICING_MAX_SOURCE_BYTES),
        "--header",
        "Accept: text/markdown",
        "--user-agent",
        "decodex-x-pricing-audit/1",
        X_PRICING_SOURCE_URL,
    ]
    try:
        process = subprocess.Popen(
            arguments,
            cwd="/",
            env={
                "LANG": "C",
                "LC_ALL": "C",
                "PATH": "/usr/bin:/bin",
            },
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
            start_new_session=True,
        )
    except OSError as error:
        raise PricingAuditFailure("x_pricing_network_unavailable") from error
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        _kill_curl(process)
        raise PricingAuditFailure("x_pricing_deadline_exceeded")
    try:
        stdout, _stderr = process.communicate(timeout=remaining)
    except subprocess.TimeoutExpired as error:
        _kill_curl(process)
        raise PricingAuditFailure("x_pricing_deadline_exceeded") from error
    if process.returncode == 47:
        raise PricingAuditFailure("x_pricing_redirect_rejected")
    if process.returncode == 63:
        raise PricingAuditFailure("x_pricing_source_oversize")
    if process.returncode == 28:
        raise PricingAuditFailure("x_pricing_deadline_exceeded")
    if process.returncode in {35, 51, 58, 59, 60, 77, 80, 82, 83, 90, 91}:
        raise PricingAuditFailure("x_pricing_tls_invalid")
    if process.returncode != 0:
        raise PricingAuditFailure("x_pricing_network_unavailable")
    if not isinstance(stdout, bytes):
        raise PricingAuditFailure("x_pricing_source_read_invalid")
    if len(stdout) > X_PRICING_MAX_SOURCE_BYTES:
        raise PricingAuditFailure("x_pricing_source_oversize")
    if not stdout:
        raise PricingAuditFailure("x_pricing_source_empty")
    return stdout


def _trusted_curl_path() -> str:
    try:
        metadata = _CURL_PATH.lstat()
    except OSError as error:
        raise PricingAuditFailure("x_pricing_fetch_runtime_invalid") from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != 0
        or metadata.st_nlink != 1
        or metadata.st_mode & 0o022
        or not metadata.st_mode & stat.S_IXUSR
    ):
        raise PricingAuditFailure("x_pricing_fetch_runtime_invalid")
    return str(_CURL_PATH)


def _kill_curl(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except (OSError, ProcessLookupError):
        try:
            process.kill()
        except OSError:
            pass
    try:
        process.communicate()
    except OSError:
        pass


def parse_x_pricing_markdown(raw: bytes) -> dict[str, int]:
    """Parse the exact official operation tables into integer micro-USD."""

    if not raw or len(raw) > X_PRICING_MAX_SOURCE_BYTES:
        raise PricingAuditFailure(
            "x_pricing_source_oversize"
            if raw
            else "x_pricing_source_empty",
            raw=raw,
        )
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise PricingAuditFailure(
            "x_pricing_source_encoding_invalid",
            raw=raw,
        ) from error

    visible, _fence_count = _visible_markdown_lines(text)
    target_indices = [
        index
        for index, line in enumerate(visible)
        if line is not None and line.strip() == _TARGET_SECTION_HEADING
    ]
    if not target_indices:
        raise PricingAuditFailure("x_pricing_target_section_missing", raw=raw)
    if len(target_indices) != 1:
        raise PricingAuditFailure("x_pricing_target_section_duplicate", raw=raw)
    section_start = target_indices[0] + 1
    section_end = _next_h2_index(visible, section_start)
    section = visible[section_start:section_end]
    if any(line is None for line in section):
        raise PricingAuditFailure("x_pricing_target_section_fenced", raw=raw)
    section_lines = [line for line in section if line is not None]
    unit_statements = [
        line
        for line in section_lines
        if line.strip() == _TARGET_SECTION_UNIT_STATEMENT
    ]
    if len(unit_statements) != 1:
        raise PricingAuditFailure(
            "x_pricing_section_unit_statement_invalid",
            raw=raw,
        )

    read_indices = _exact_line_indices(section_lines, _READ_HEADING)
    write_indices = _exact_line_indices(section_lines, _WRITE_HEADING)
    if len(read_indices) != 1 or len(write_indices) != 1:
        raise PricingAuditFailure(
            "x_pricing_operation_sections_invalid",
            raw=raw,
        )
    read_index = read_indices[0]
    write_index = write_indices[0]
    h3_indices = [
        index
        for index, line in enumerate(section_lines)
        if _is_h3(line.strip())
    ]
    try:
        read_position = h3_indices.index(read_index)
    except ValueError as error:
        raise PricingAuditFailure(
            "x_pricing_operation_sections_invalid",
            raw=raw,
        ) from error
    if (
        write_index <= read_index
        or read_position + 1 >= len(h3_indices)
        or h3_indices[read_position + 1] != write_index
    ):
        raise PricingAuditFailure(
            "x_pricing_operation_sections_not_adjacent",
            raw=raw,
        )
    next_write_h3 = next(
        (index for index in h3_indices if index > write_index),
        len(section_lines),
    )
    read_rates = _parse_operation_table(
        section_lines[read_index + 1 : write_index],
        description=_READ_DESCRIPTION,
        header=("Resource", "Unit cost"),
        labels=_READ_RATE_LABELS,
        unit="resource",
        raw=raw,
    )
    write_rates = _parse_operation_table(
        section_lines[write_index + 1 : next_write_h3],
        description=_WRITE_DESCRIPTION,
        header=("Action", "Unit cost"),
        labels=_WRITE_RATE_LABELS,
        unit="request",
        raw=raw,
    )
    _require_unique_target_labels(section_lines, raw=raw)
    rates = {**read_rates, **write_rates}
    if set(rates) != X_PRICING_RATE_KEYS:
        raise PricingAuditFailure("x_pricing_rows_missing", raw=raw)
    return {key: rates[key] for key in sorted(rates)}


def _visible_markdown_lines(text: str) -> tuple[list[str | None], int]:
    visible: list[str | None] = []
    fence_character: str | None = None
    fence_length = 0
    fence_count = 0
    for line in text.splitlines():
        stripped = line.lstrip()
        match = re.match(r"^(`{3,}|~{3,})", stripped)
        if match is not None:
            marker = match.group(1)
            if fence_character is None:
                fence_character = marker[0]
                fence_length = len(marker)
                fence_count += 1
                visible.append(None)
                continue
            if marker[0] == fence_character and len(marker) >= fence_length:
                fence_character = None
                fence_length = 0
                visible.append(None)
                continue
        visible.append(None if fence_character is not None else line)
    return visible, fence_count


def _next_h2_index(lines: list[str | None], start: int) -> int:
    return next(
        (
            index
            for index in range(start, len(lines))
            if lines[index] is not None and _is_h2(lines[index].strip())
        ),
        len(lines),
    )


def _is_h2(value: str) -> bool:
    return re.match(r"^##(?!#)(?:\s|$)", value) is not None


def _is_h3(value: str) -> bool:
    return re.match(r"^###(?!#)(?:\s|$)", value) is not None


def _exact_line_indices(lines: list[str], expected: str) -> list[int]:
    return [
        index
        for index, line in enumerate(lines)
        if line.strip() == expected
    ]


def _parse_operation_table(
    lines: list[str],
    *,
    description: str,
    header: tuple[str, str],
    labels: dict[str, str],
    unit: str,
    raw: bytes,
) -> dict[str, int]:
    if sum(line.strip() == description for line in lines) != 1:
        raise PricingAuditFailure(
            "x_pricing_operation_unit_statement_invalid",
            raw=raw,
        )
    blocks = _table_blocks(lines)
    if len(blocks) != 1:
        raise PricingAuditFailure(
            "x_pricing_operation_table_count_invalid",
            raw=raw,
        )
    rows = [_table_cells(line) for line in blocks[0]]
    if (
        len(rows) < 3
        or rows[0] != list(header)
        or len(rows[1]) != 2
        or any(
            _TABLE_SEPARATOR_PATTERN.fullmatch(cell) is None
            for cell in rows[1]
        )
    ):
        raise PricingAuditFailure(
            "x_pricing_operation_table_header_invalid",
            raw=raw,
        )
    rates: dict[str, int] = {}
    for cells in rows[2:]:
        if len(cells) != 2 or not cells[0] or not cells[1]:
            raise PricingAuditFailure(
                "x_pricing_operation_row_invalid",
                raw=raw,
            )
        label = _plain_markdown_cell(cells[0])
        key = labels.get(label)
        if key is not None:
            if cells[0] != f"**{label}**":
                raise PricingAuditFailure(
                    "x_pricing_operation_label_markup_invalid",
                    raw=raw,
                )
            if key in rates:
                raise PricingAuditFailure(
                    "x_pricing_row_duplicate",
                    raw=raw,
                )
            rates[key] = _parse_micro_usd(
                cells[1],
                expected_unit=unit,
                raw=raw,
            )
            continue
        _parse_micro_usd(cells[1], expected_unit=unit, raw=raw)
    if set(rates) != set(labels.values()):
        raise PricingAuditFailure("x_pricing_rows_missing", raw=raw)
    return rates


def _table_blocks(lines: list[str]) -> list[list[str]]:
    blocks: list[list[str]] = []
    current: list[str] = []
    for line in lines:
        if _is_table_line(line):
            current.append(line)
            continue
        if current:
            blocks.append(current)
            current = []
    if current:
        blocks.append(current)
    return blocks


def _is_table_line(line: str) -> bool:
    stripped = line.strip()
    return stripped.startswith("|") and stripped.endswith("|")


def _table_cells(line: str) -> list[str]:
    stripped = line.strip()
    if not _is_table_line(stripped):
        return []
    return [cell.strip() for cell in stripped[1:-1].split("|")]


def _require_unique_target_labels(lines: list[str], *, raw: bytes) -> None:
    expected = {*_READ_RATE_LABELS, *_WRITE_RATE_LABELS}
    counts = {label: 0 for label in expected}
    for line in lines:
        if not _is_table_line(line):
            continue
        for cell in _table_cells(line):
            label = _plain_markdown_cell(cell)
            if label in counts:
                counts[label] += 1
    if any(count != 1 for count in counts.values()):
        raise PricingAuditFailure("x_pricing_row_duplicate", raw=raw)


def _plain_markdown_cell(value: str) -> str:
    if value.startswith("**") and value.endswith("**"):
        return value[2:-2].strip()
    return value


def _parse_micro_usd(
    value: str,
    *,
    expected_unit: str,
    raw: bytes,
) -> int:
    match = _PRICE_PATTERN.fullmatch(value)
    if match is None:
        raise PricingAuditFailure("x_pricing_value_ambiguous", raw=raw)
    whole, fractional, unit = match.groups()
    if unit != expected_unit:
        raise PricingAuditFailure("x_pricing_value_unit_invalid", raw=raw)
    amount = int(whole) * 1_000_000
    amount += int((fractional or "").ljust(6, "0") or "0")
    if amount <= 0 or amount > 10_000_000:
        raise PricingAuditFailure("x_pricing_value_out_of_range", raw=raw)
    return amount


def audit_x_pricing(
    repo_root: Path,
    *,
    now: int,
    fetcher: Callable[[], bytes] | None = None,
) -> dict[str, Any]:
    """Fetch, parse, persist, and classify one pricing observation."""

    pricing_root = _ensure_private_pricing_root(repo_root)
    with _pricing_lock(pricing_root):
        previous = _try_load_valid_receipt(
            repo_root / X_PRICING_RECEIPT_RELATIVE_PATH
        )
        if (
            previous is not None
            and _parse_timestamp(previous["fetched_at"]) > now
        ):
            raise AutopilotError("x_pricing_clock_regression")
        source_fetcher = fetch_official_x_pricing if fetcher is None else fetcher
        try:
            raw = source_fetcher()
        except PricingAuditFailure as error:
            return _network_result(previous, now=now, error_code=error.code)
        except Exception as error:
            raise AutopilotError("x_pricing_fetcher_invalid") from error
        if not isinstance(raw, bytes):
            raise AutopilotError("x_pricing_fetcher_invalid")

        raw_sha256 = hashlib.sha256(raw).hexdigest()
        fetched_at = _format_timestamp(now)
        try:
            rates = parse_x_pricing_markdown(raw)
        except PricingAuditFailure as error:
            diagnostic = _parser_diagnostic(
                raw,
                error_code=error.code,
            )
            failure = _failure_receipt(
                fetched_at=fetched_at,
                raw_sha256=raw_sha256,
                error_code=error.code,
                diagnostic=diagnostic,
            )
            failure_path = repo_root / X_PRICING_FAILURE_RELATIVE_PATH
            _atomic_private_json(failure_path, failure)
            failure_sha256 = _serialized_sha256(failure)
            evidence = _drift_evidence(
                status="parse_failed",
                fetched_at=fetched_at,
                raw_sha256=raw_sha256,
                receipt_sha256=failure_sha256,
                rates=None,
                error_code=error.code,
            )
            return {
                "status": "parse_failed",
                "source_url": X_PRICING_SOURCE_URL,
                "parser_version": X_PRICING_PARSER_VERSION,
                "fetched_at": fetched_at,
                "raw_sha256": raw_sha256,
                "rates_microusd": None,
                "receipt": _failure_projection(failure),
                "drift_evidence": evidence,
                "error_code": error.code,
            }

        receipt = _success_receipt(
            fetched_at=fetched_at,
            raw_sha256=raw_sha256,
            rates=rates,
        )
        receipt_path = repo_root / X_PRICING_RECEIPT_RELATIVE_PATH
        _atomic_private_json(receipt_path, receipt)
        receipt_sha256 = _serialized_sha256(receipt)
        _remove_private_file(
            repo_root / X_PRICING_FAILURE_RELATIVE_PATH
        )
        contract_matches = rates == X_PRICING_COMPILED_RATES_MICROUSD
        evidence = None
        if not contract_matches:
            evidence = _drift_evidence(
                status="contract_drift",
                fetched_at=fetched_at,
                raw_sha256=raw_sha256,
                receipt_sha256=receipt_sha256,
                rates=rates,
                error_code=None,
            )
        return {
            "status": "current" if contract_matches else "contract_drift",
            "source_url": X_PRICING_SOURCE_URL,
            "parser_version": X_PRICING_PARSER_VERSION,
            "fetched_at": fetched_at,
            "raw_sha256": raw_sha256,
            "rates_microusd": rates,
            "receipt": _receipt_projection(receipt, now=now),
            "drift_evidence": evidence,
            "error_code": None,
        }


def _success_receipt(
    *,
    fetched_at: str,
    raw_sha256: str,
    rates: dict[str, int],
) -> dict[str, Any]:
    receipt: dict[str, Any] = {
        "schema": X_PRICING_RECEIPT_SCHEMA,
        "parser_version": X_PRICING_PARSER_VERSION,
        "source_url": X_PRICING_SOURCE_URL,
        "fetched_at": fetched_at,
        "raw_sha256": raw_sha256,
        "rates_microusd": {
            key: rates[key] for key in sorted(X_PRICING_RATE_KEYS)
        },
    }
    receipt["integrity_sha256"] = _receipt_integrity(receipt)
    return receipt


def _failure_receipt(
    *,
    fetched_at: str,
    raw_sha256: str,
    error_code: str,
    diagnostic: dict[str, Any],
) -> dict[str, Any]:
    receipt: dict[str, Any] = {
        "schema": X_PRICING_FAILURE_SCHEMA,
        "parser_version": X_PRICING_PARSER_VERSION,
        "source_url": X_PRICING_SOURCE_URL,
        "fetched_at": fetched_at,
        "raw_sha256": raw_sha256,
        "error_code": error_code,
        "diagnostic": diagnostic,
        "diagnostic_sha256": _canonical_json_sha256(diagnostic),
    }
    receipt["integrity_sha256"] = _failure_integrity(receipt)
    if len(_serialized_json_bytes(receipt)) > X_PRICING_MAX_RECEIPT_BYTES:
        raise AutopilotError("x_pricing_diagnostic_oversize")
    return receipt


def _receipt_integrity(receipt: dict[str, Any]) -> str:
    rates = receipt["rates_microusd"]
    material = "\n".join(
        [
            f"schema={receipt['schema']}",
            f"parser_version={receipt['parser_version']}",
            f"source_url={receipt['source_url']}",
            f"fetched_at={receipt['fetched_at']}",
            f"raw_sha256={receipt['raw_sha256']}",
            f"post_create={rates['post_create']}",
            f"post_create_with_url={rates['post_create_with_url']}",
            f"post_read={rates['post_read']}",
            f"user_read={rates['user_read']}",
        ]
    )
    return hashlib.sha256(material.encode("ascii")).hexdigest()


def _failure_integrity(receipt: dict[str, Any]) -> str:
    material = "\n".join(
        [
            f"schema={receipt['schema']}",
            f"parser_version={receipt['parser_version']}",
            f"source_url={receipt['source_url']}",
            f"fetched_at={receipt['fetched_at']}",
            f"raw_sha256={receipt['raw_sha256']}",
            f"error_code={receipt['error_code']}",
            f"diagnostic_sha256={receipt['diagnostic_sha256']}",
        ]
    )
    return hashlib.sha256(material.encode("ascii")).hexdigest()


def _parser_diagnostic(
    raw: bytes,
    *,
    error_code: str,
) -> dict[str, Any]:
    text = raw.decode("utf-8", errors="replace")
    visible, fence_count = _visible_markdown_lines(text)
    target_indices = [
        index
        for index, line in enumerate(visible)
        if line is not None and line.strip() == _TARGET_SECTION_HEADING
    ]
    target_section: list[str] = []
    if target_indices:
        start = target_indices[0] + 1
        end = _next_h2_index(visible, start)
        target_section = [
            line for line in visible[start:end] if line is not None
        ]
    diagnostic = {
        "schema": X_PRICING_DIAGNOSTIC_SCHEMA,
        "parser_contract": X_PRICING_PARSER_CONTRACT,
        "error_code": error_code,
        "raw_sha256": hashlib.sha256(raw).hexdigest(),
        "source_bytes": len(raw),
        "source_lines": len(text.splitlines()),
        "code_fence_count": fence_count,
        "target_section_count": len(target_indices),
        "target_section_lines": len(target_section),
        "target_section_sha256": (
            hashlib.sha256(
                "\n".join(target_section).encode("utf-8")
            ).hexdigest()
            if target_section
            else None
        ),
        "tables": _diagnostic_table_summaries(visible),
    }
    if len(_canonical_json_bytes(diagnostic)) > 12 * 1024:
        raise AutopilotError("x_pricing_diagnostic_oversize")
    return diagnostic


def _diagnostic_table_summaries(
    lines: list[str | None],
) -> list[dict[str, Any]]:
    tables: list[tuple[str, str, list[str]]] = []
    current_h2 = ""
    current_h3 = ""
    block: list[str] = []

    def finish() -> None:
        nonlocal block
        if block and any(
            "$" in line or "Unit cost" in line for line in block
        ):
            tables.append((current_h2, current_h3, block))
        block = []

    for line in lines:
        if line is None:
            finish()
            continue
        stripped = line.strip()
        if _is_h2(stripped):
            finish()
            current_h2 = stripped
            current_h3 = ""
            continue
        if _is_h3(stripped):
            finish()
            current_h3 = stripped
            continue
        if _is_table_line(line):
            block.append(line)
        else:
            finish()
    finish()

    summaries: list[dict[str, Any]] = []
    for h2, h3, table in tables[:4]:
        rows = [_table_cells(line) for line in table]
        data_rows = rows[2:] if len(rows) >= 2 else []
        selected = _diagnostic_sample_rows(data_rows)
        summaries.append(
            {
                "nearest_h2": _diagnostic_text(h2),
                "nearest_h3": _diagnostic_text(h3),
                "header_cells": [
                    _diagnostic_text(cell)
                    for cell in (rows[0][:4] if rows else [])
                ],
                "header_sha256": hashlib.sha256(
                    (table[0] if table else "").encode("utf-8")
                ).hexdigest(),
                "row_count": len(data_rows),
                "rows_sha256": hashlib.sha256(
                    "\n".join(table[2:]).encode("utf-8")
                ).hexdigest(),
                "sample_rows": [
                    {
                        "cells": [
                            _diagnostic_text(cell) for cell in row[:2]
                        ],
                        "row_sha256": hashlib.sha256(
                            "|".join(row).encode("utf-8")
                        ).hexdigest(),
                    }
                    for row in selected
                ],
                "truncated": len(selected) < len(data_rows),
            }
        )
    return summaries


def _diagnostic_sample_rows(rows: list[list[str]]) -> list[list[str]]:
    selected: list[list[str]] = []
    priority_terms = ("read", "create", "content", "post", "user")
    for row in rows:
        folded = " ".join(row).casefold()
        if any(term in folded for term in priority_terms):
            selected.append(row)
        if len(selected) == 8:
            return selected
    for row in rows:
        if row not in selected:
            selected.append(row)
        if len(selected) == 8:
            break
    return selected


def _diagnostic_text(value: str) -> str:
    collapsed = " ".join(value.split())
    return collapsed.encode("ascii", errors="replace").decode("ascii")[:64]


def _canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("ascii")


def _canonical_json_sha256(value: Any) -> str:
    return hashlib.sha256(_canonical_json_bytes(value)).hexdigest()


def _drift_evidence(
    *,
    status: str,
    fetched_at: str,
    raw_sha256: str,
    receipt_sha256: str,
    rates: dict[str, int] | None,
    error_code: str | None,
) -> dict[str, Any]:
    evidence = {
        "schema": X_PRICING_AUDIT_EVIDENCE_SCHEMA,
        "status": status,
        "source_url": X_PRICING_SOURCE_URL,
        "parser_version": X_PRICING_PARSER_VERSION,
        "fetched_at": fetched_at,
        "raw_sha256": raw_sha256,
        "receipt_sha256": receipt_sha256,
        "rates_microusd": rates,
        "error_code": error_code,
    }
    validate_x_pricing_audit_evidence(evidence)
    return evidence


def _network_result(
    previous: dict[str, Any] | None,
    *,
    now: int,
    error_code: str,
) -> dict[str, Any]:
    projection = _receipt_projection(previous, now=now)
    return {
        "status": (
            "network_deferred"
            if projection["status"] == "current"
            else "blocked"
        ),
        "source_url": X_PRICING_SOURCE_URL,
        "parser_version": X_PRICING_PARSER_VERSION,
        "fetched_at": None,
        "raw_sha256": None,
        "rates_microusd": None,
        "receipt": projection,
        "drift_evidence": None,
        "error_code": error_code,
    }


def _receipt_projection(
    receipt: dict[str, Any] | None,
    *,
    now: int,
) -> dict[str, Any]:
    if receipt is None:
        return {
            "status": "missing",
            "source_url": X_PRICING_SOURCE_URL,
            "parser_version": X_PRICING_PARSER_VERSION,
            "fetched_at": None,
            "raw_sha256": None,
            "rates_microusd": None,
        }
    return {
        "status": _receipt_freshness(receipt, now=now),
        "source_url": receipt["source_url"],
        "parser_version": receipt["parser_version"],
        "fetched_at": receipt["fetched_at"],
        "raw_sha256": receipt["raw_sha256"],
        "rates_microusd": receipt["rates_microusd"],
    }


def _failure_projection(receipt: dict[str, Any]) -> dict[str, Any]:
    return {
        "status": "parse_failed",
        "source_url": receipt["source_url"],
        "parser_version": receipt["parser_version"],
        "fetched_at": receipt["fetched_at"],
        "raw_sha256": receipt["raw_sha256"],
        "rates_microusd": None,
    }


def _receipt_freshness(receipt: dict[str, Any], *, now: int) -> str:
    fetched_at = _parse_timestamp(receipt["fetched_at"])
    if fetched_at > now:
        return "future"
    if now - fetched_at > X_PRICING_FRESH_SECONDS:
        return "stale"
    if receipt["rates_microusd"] != X_PRICING_COMPILED_RATES_MICROUSD:
        return "contract_drift"
    return "current"


def _try_load_valid_receipt(path: Path) -> dict[str, Any] | None:
    try:
        receipt, _digest = _load_private_json(path)
        _validate_success_receipt(receipt)
        return receipt
    except (FileNotFoundError, AutopilotError):
        return None


def _validate_success_receipt(receipt: Any) -> None:
    if (
        not isinstance(receipt, dict)
        or set(receipt)
        != {
            "schema",
            "parser_version",
            "source_url",
            "fetched_at",
            "raw_sha256",
            "rates_microusd",
            "integrity_sha256",
        }
        or receipt["schema"] != X_PRICING_RECEIPT_SCHEMA
        or receipt["parser_version"] != X_PRICING_PARSER_VERSION
        or receipt["source_url"] != X_PRICING_SOURCE_URL
        or _TIMESTAMP_PATTERN.fullmatch(str(receipt["fetched_at"]))
        is None
        or re.fullmatch(r"[0-9a-f]{64}", str(receipt["raw_sha256"]))
        is None
        or not isinstance(receipt["rates_microusd"], dict)
        or set(receipt["rates_microusd"]) != X_PRICING_RATE_KEYS
        or any(
            type(receipt["rates_microusd"][key]) is not int
            or not 0 < receipt["rates_microusd"][key] <= 10_000_000
            for key in X_PRICING_RATE_KEYS
        )
        or receipt["integrity_sha256"] != _receipt_integrity(receipt)
    ):
        raise AutopilotError("x_pricing_receipt_invalid")
    _parse_timestamp(receipt["fetched_at"])


def _format_timestamp(value: int) -> str:
    return datetime.fromtimestamp(
        value,
        tz=timezone.utc,
    ).strftime("%Y-%m-%dT%H:%M:%SZ")


def _parse_timestamp(value: str) -> int:
    if _TIMESTAMP_PATTERN.fullmatch(value) is None:
        raise AutopilotError("x_pricing_receipt_invalid")
    try:
        parsed = datetime.strptime(
            value,
            "%Y-%m-%dT%H:%M:%SZ",
        ).replace(tzinfo=timezone.utc)
    except ValueError as error:
        raise AutopilotError("x_pricing_receipt_invalid") from error
    return int(parsed.timestamp())


def _ensure_private_pricing_root(repo_root: Path) -> Path:
    root = repo_root.resolve()
    current = root
    for component in (
        ".agent",
        "automations",
        "decodex",
        "cache",
        "social",
        "x",
    ):
        current = current / component
        try:
            current.mkdir(mode=0o700)
        except FileExistsError:
            pass
        try:
            metadata = current.lstat()
        except OSError as error:
            raise AutopilotError("x_pricing_private_root_invalid") from error
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or stat.S_ISLNK(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o700
        ):
            raise AutopilotError("x_pricing_private_root_invalid")
    return current


class _PricingLock:
    def __init__(self, descriptor: int) -> None:
        self.descriptor = descriptor

    def __enter__(self) -> None:
        try:
            fcntl.flock(self.descriptor, fcntl.LOCK_EX)
        except OSError as error:
            raise AutopilotError("x_pricing_lock_failed") from error

    def __exit__(self, *_args: Any) -> None:
        try:
            fcntl.flock(self.descriptor, fcntl.LOCK_UN)
        finally:
            os.close(self.descriptor)


def _pricing_lock(pricing_root: Path) -> _PricingLock:
    flags = os.O_RDWR | os.O_CREAT
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(
            pricing_root / X_PRICING_LOCK_RELATIVE_PATH.name,
            flags,
            0o600,
        )
        metadata = os.fstat(descriptor)
    except OSError as error:
        raise AutopilotError("x_pricing_lock_failed") from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
    ):
        os.close(descriptor)
        raise AutopilotError("x_pricing_lock_failed")
    try:
        os.fchmod(descriptor, 0o600)
        metadata = os.fstat(descriptor)
    except OSError as error:
        os.close(descriptor)
        raise AutopilotError("x_pricing_lock_failed") from error
    if stat.S_IMODE(metadata.st_mode) != 0o600:
        os.close(descriptor)
        raise AutopilotError("x_pricing_lock_failed")
    return _PricingLock(descriptor)


def _atomic_private_json(path: Path, value: dict[str, Any]) -> None:
    _validate_existing_private_target(path)
    atomic_write_json(path, value)
    loaded, digest = _load_private_json(path)
    if loaded != value or digest != _serialized_sha256(value):
        raise AutopilotError("x_pricing_receipt_write_failed")


def _validate_existing_private_target(path: Path) -> None:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return
    except OSError as error:
        raise AutopilotError("x_pricing_receipt_invalid") from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
        or stat.S_IMODE(metadata.st_mode) != 0o600
    ):
        raise AutopilotError("x_pricing_receipt_invalid")


def _load_private_json(path: Path) -> tuple[dict[str, Any], str]:
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags)
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or stat.S_IMODE(before.st_mode) != 0o600
            or before.st_size <= 0
            or before.st_size > X_PRICING_MAX_RECEIPT_BYTES
        ):
            raise AutopilotError("x_pricing_receipt_invalid")
        with os.fdopen(descriptor, "rb", closefd=False) as handle:
            raw = handle.read(X_PRICING_MAX_RECEIPT_BYTES + 1)
        after = os.fstat(descriptor)
        if (
            len(raw) > X_PRICING_MAX_RECEIPT_BYTES
            or (
                before.st_dev,
                before.st_ino,
                before.st_size,
                before.st_mtime_ns,
            )
            != (
                after.st_dev,
                after.st_ino,
                after.st_size,
                after.st_mtime_ns,
            )
        ):
            raise AutopilotError("x_pricing_receipt_invalid")
    finally:
        os.close(descriptor)
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AutopilotError("x_pricing_receipt_invalid") from error
    if not isinstance(value, dict):
        raise AutopilotError("x_pricing_receipt_invalid")
    return value, hashlib.sha256(raw).hexdigest()


def _serialized_sha256(value: dict[str, Any]) -> str:
    return hashlib.sha256(_serialized_json_bytes(value)).hexdigest()


def _serialized_json_bytes(value: dict[str, Any]) -> bytes:
    return (
        json.dumps(value, indent=2, sort_keys=True).encode("utf-8")
        + b"\n"
    )


def _remove_private_file(path: Path) -> None:
    try:
        _validate_existing_private_target(path)
        path.unlink()
    except FileNotFoundError:
        return
    except OSError as error:
        raise AutopilotError("x_pricing_receipt_write_failed") from error
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
