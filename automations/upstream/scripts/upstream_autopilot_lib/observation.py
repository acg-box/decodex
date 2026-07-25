"""Collect bounded facts from official Codex sources and the installed CLI."""

from __future__ import annotations

from contextlib import contextmanager
from dataclasses import replace
import fcntl
import json
from pathlib import Path
import tempfile
from typing import Any, Iterator, Sequence

from .core import (
    AutopilotError,
    CODEX_VERSION_PATTERN,
    MAX_ACTIVE_SOURCE_CANDIDATES,
    MAX_GIT_TEXT_BYTES,
    MAX_SCHEMA_BYTES,
    MAX_SCHEMA_EVIDENCE_BYTES,
    MAX_SCHEMA_EVIDENCE_FILES,
    MAX_SCHEMA_FILES,
    MAX_UPSTREAM_COMMITS,
    Observation,
    REPO_ROOT,
    SHA_PATTERN,
    TAG_PATTERN,
    atomic_write_json,
    command_succeeds,
    ensure_cache_root,
    hash_file_bounded,
    is_sha256,
    resolve_executable,
    run_command,
    sha256_value,
)


def mirror_arguments(mirror: Path, *arguments: str) -> list[str]:
    return ["git", f"--git-dir={mirror}", *arguments]


def ensure_mirror(cache_root: Path, policy: dict[str, Any]) -> Path:
    root = ensure_cache_root(cache_root)
    lock_path = root / "mirror.lock"
    if lock_path.exists() and lock_path.is_symlink():
        raise AutopilotError("upstream_mirror_lock_symlink")
    try:
        with lock_path.open("a+", encoding="utf-8") as lock:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            mirror = sync_mirror(root, policy)
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
            return mirror
    except OSError as error:
        raise AutopilotError("upstream_mirror_lock_failed") from error


@contextmanager
def observation_session_lock(cache_root: Path) -> Iterator[None]:
    root = ensure_cache_root(cache_root)
    lock_path = root / "observation.lock"
    if lock_path.exists() and lock_path.is_symlink():
        raise AutopilotError("observation_lock_symlink")
    try:
        with lock_path.open("a+", encoding="utf-8") as lock:
            lock_path.chmod(0o600)
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            yield
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    except OSError as error:
        raise AutopilotError("observation_lock_failed") from error


def sync_mirror(cache_root: Path, policy: dict[str, Any]) -> Path:
    mirror_root = cache_root / "mirror"
    if mirror_root.exists() and (
        mirror_root.is_symlink() or not mirror_root.is_dir()
    ):
        raise AutopilotError("upstream_mirror_invalid")
    mirror = mirror_root / "openai-codex.git"
    if mirror.exists() and (mirror.is_symlink() or not mirror.is_dir()):
        raise AutopilotError("upstream_mirror_invalid")
    if not mirror.exists():
        mirror.parent.mkdir(parents=True, exist_ok=True)
        run_command(
            ["git", "init", "--bare", str(mirror)],
            failure_code="upstream_mirror_init_failed",
        )
        run_command(
            mirror_arguments(mirror, "remote", "add", "origin", policy["upstream_repository"]),
            failure_code="upstream_remote_add_failed",
        )
    remote = run_command(
        mirror_arguments(mirror, "remote", "get-url", "origin"),
        failure_code="upstream_remote_unavailable",
    )
    if remote != policy["upstream_repository"]:
        raise AutopilotError("upstream_remote_mismatch")
    run_command(
        mirror_arguments(
            mirror,
            "fetch",
            "--force",
            "--prune",
            "--filter=blob:none",
            "origin",
            f"+refs/heads/{policy['upstream_branch']}:refs/remotes/origin/{policy['upstream_branch']}",
            "+refs/tags/rust-v*:refs/tags/rust-v*",
        ),
        failure_code="upstream_fetch_failed",
    )
    return mirror


def parse_release_tags(tags: Sequence[str]) -> tuple[str | None, str | None]:
    stable: list[tuple[tuple[int, int, int], str]] = []
    prerelease: list[tuple[tuple[int, int, int, int, int], str]] = []
    label_order = {"alpha": 0, "beta": 1, "rc": 2}
    for tag in tags:
        match = TAG_PATTERN.fullmatch(tag)
        if match is None:
            continue
        base = tuple(int(match.group(name)) for name in ("major", "minor", "patch"))
        label = match.group("label")
        if label is None:
            stable.append((base, tag))
            continue
        prerelease.append(
            (
                (*base, label_order[label], int(match.group("number"))),
                tag,
            )
        )
    return (
        max(stable)[1] if stable else None,
        max(prerelease)[1] if prerelease else None,
    )


def tag_target(mirror: Path, tag: str | None) -> str | None:
    if tag is None:
        return None
    target = run_command(
        mirror_arguments(mirror, "rev-parse", f"{tag}^{{commit}}"),
        failure_code="upstream_tag_target_unavailable",
    )
    if SHA_PATTERN.fullmatch(target) is None:
        raise AutopilotError("upstream_tag_target_invalid")
    return target


def upstream_source_observation(
    mirror: Path,
    policy: dict[str, Any],
) -> tuple[str, str | None, str | None, str | None, str | None]:
    head = run_command(
        mirror_arguments(mirror, "rev-parse", f"refs/remotes/origin/{policy['upstream_branch']}"),
        failure_code="upstream_head_unavailable",
    )
    if not SHA_PATTERN.fullmatch(head):
        raise AutopilotError("upstream_head_invalid")
    tag_output = run_command(
        mirror_arguments(
            mirror,
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/tags/rust-v*",
        ),
        failure_code="upstream_tags_unavailable",
    )
    stable, prerelease = parse_release_tags(tag_output.splitlines())
    return (
        head,
        stable,
        tag_target(mirror, stable),
        prerelease,
        tag_target(mirror, prerelease),
    )


def extract_methods(schema: dict[str, Any]) -> set[str]:
    methods: set[str] = set()
    for variant in schema.get("oneOf", []):
        values = variant.get("properties", {}).get("method", {}).get("enum", [])
        methods.update(value for value in values if isinstance(value, str))
    return methods


def schema_snapshot(
    codex: str,
    *,
    experimental: bool,
    required_requests: Sequence[str],
    required_notifications: Sequence[str],
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="decodex-upstream-schema-") as directory:
        output = Path(directory)
        command = [codex, "app-server", "generate-json-schema"]
        if experimental:
            command.append("--experimental")
        command.extend(["--out", str(output)])
        run_command(command, failure_code="codex_schema_generation_failed")
        files = sorted(output.rglob("*.json"))
        if not files or len(files) > MAX_SCHEMA_FILES:
            raise AutopilotError("codex_schema_file_budget")
        total_bytes = 0
        digests: dict[str, str] = {}
        schemas: dict[str, Any] = {}
        for file in files:
            if file.is_symlink() or not file.is_file():
                raise AutopilotError("codex_schema_file_invalid")
            total_bytes += file.stat().st_size
            if total_bytes > MAX_SCHEMA_BYTES:
                raise AutopilotError("codex_schema_byte_budget")
            try:
                value = json.loads(file.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as error:
                raise AutopilotError("codex_schema_json_invalid") from error
            relative = file.relative_to(output).as_posix()
            digests[relative] = sha256_value(value)
            schemas[relative] = value
        request_schema = schemas.get("ClientRequest.json")
        notification_schema = schemas.get("ServerNotification.json")
        if not isinstance(request_schema, dict) or not isinstance(
            notification_schema,
            dict,
        ):
            raise AutopilotError("codex_schema_root_missing")
        requests = extract_methods(request_schema)
        notifications = extract_methods(notification_schema)
        core_names = (
            "ClientRequest.json",
            "ServerNotification.json",
            "codex_app_server_protocol.v2.schemas.json",
        )
        if any(name not in digests for name in core_names):
            raise AutopilotError("codex_schema_root_missing")
        return {
            "fingerprint": sha256_value(digests),
            "core_digests": {name: digests[name] for name in core_names},
            "file_digests": digests,
            "core_schemas": {name: schemas[name] for name in core_names},
            "request_method_count": len(requests),
            "notification_method_count": len(notifications),
            "missing_request_methods": sorted(set(required_requests) - requests),
            "missing_notification_methods": sorted(
                set(required_notifications) - notifications
            ),
        }


def upstream_schema_snapshot(
    mirror: Path,
    reference: str,
    *,
    required_requests: Sequence[str],
    required_notifications: Sequence[str],
) -> dict[str, Any]:
    root = "codex-rs/app-server-protocol/schema/json"
    values: dict[str, Any] = {}
    for name in (
        "ClientRequest.json",
        "ServerNotification.json",
        "codex_app_server_protocol.v2.schemas.json",
    ):
        output = run_command(
            mirror_arguments(mirror, "show", f"{reference}:{root}/{name}"),
            failure_code="upstream_schema_unavailable",
            max_output_bytes=MAX_SCHEMA_BYTES,
        )
        if len(output.encode("utf-8")) > MAX_SCHEMA_BYTES:
            raise AutopilotError("upstream_schema_byte_budget")
        try:
            values[name] = json.loads(output)
        except json.JSONDecodeError as error:
            raise AutopilotError("upstream_schema_invalid") from error
    requests = extract_methods(values["ClientRequest.json"])
    notifications = extract_methods(values["ServerNotification.json"])
    digests = {name: sha256_value(value) for name, value in values.items()}
    return {
        "fingerprint": sha256_value(digests),
        "core_digests": digests,
        "missing_request_methods": sorted(set(required_requests) - requests),
        "missing_notification_methods": sorted(
            set(required_notifications) - notifications
        ),
    }


def persist_schema_evidence(
    cache_root: Path,
    *,
    codex_version: str,
    executable_sha256: str,
    experimental: bool,
    snapshot: dict[str, Any],
    retained_evidence: set[str] | None = None,
) -> str:
    evidence = {
        "schema": "decodex/codex-installed-schema-evidence/1",
        "codex_version": codex_version,
        "executable_sha256": executable_sha256,
        "experimental": experimental,
        "schema_fingerprint": snapshot["fingerprint"],
        "file_digests": snapshot["file_digests"],
        "core_schemas": snapshot["core_schemas"],
        "request_method_count": snapshot["request_method_count"],
        "notification_method_count": snapshot["notification_method_count"],
        "missing_request_methods": snapshot["missing_request_methods"],
        "missing_notification_methods": snapshot["missing_notification_methods"],
    }
    serialized = (
        json.dumps(evidence, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")
    if len(serialized) > MAX_SCHEMA_BYTES:
        raise AutopilotError("schema_evidence_byte_budget")
    evidence_sha256 = sha256_value(evidence)
    evidence_root = ensure_cache_root(cache_root) / "schema-evidence"
    if evidence_root.exists() and (
        evidence_root.is_symlink() or not evidence_root.is_dir()
    ):
        raise AutopilotError("schema_evidence_root_invalid")
    evidence_root.mkdir(parents=True, exist_ok=True)
    evidence_root.chmod(0o700)
    lock_path = ensure_cache_root(cache_root) / "schema-evidence.lock"
    if lock_path.exists() and lock_path.is_symlink():
        raise AutopilotError("schema_evidence_lock_symlink")
    retained = set(retained_evidence or ())
    retained.add(evidence_sha256)
    try:
        with lock_path.open("a+", encoding="utf-8") as lock:
            lock_path.chmod(0o600)
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            path = evidence_root / f"{evidence_sha256}.json"
            evidence_files = sorted(evidence_root.glob("*.json"))
            if any(
                file.is_symlink()
                or not file.is_file()
                or not is_sha256(file.stem)
                for file in evidence_files
            ):
                raise AutopilotError("schema_evidence_invalid")
            current_bytes = sum(file.stat().st_size for file in evidence_files)
            added_files = 0 if path.exists() else 1
            added_bytes = 0 if path.exists() else len(serialized)
            for removable in [
                file for file in evidence_files if file.stem not in retained
            ]:
                if (
                    len(evidence_files) + added_files <= MAX_SCHEMA_EVIDENCE_FILES
                    and current_bytes + added_bytes <= MAX_SCHEMA_EVIDENCE_BYTES
                ):
                    break
                size = removable.stat().st_size
                removable.unlink()
                evidence_files.remove(removable)
                current_bytes -= size
            if (
                len(evidence_files) + added_files > MAX_SCHEMA_EVIDENCE_FILES
                or current_bytes + added_bytes > MAX_SCHEMA_EVIDENCE_BYTES
            ):
                raise AutopilotError("schema_evidence_capacity")
            if path.exists():
                if path.is_symlink() or path.stat().st_size > MAX_SCHEMA_BYTES:
                    raise AutopilotError("schema_evidence_invalid")
                try:
                    existing = json.loads(path.read_text(encoding="utf-8"))
                except (OSError, json.JSONDecodeError) as error:
                    raise AutopilotError("schema_evidence_invalid") from error
                if sha256_value(existing) != evidence_sha256 or existing != evidence:
                    raise AutopilotError("schema_evidence_mismatch")
            else:
                atomic_write_json(path, evidence)
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    except OSError as error:
        raise AutopilotError("schema_evidence_lock_failed") from error
    return evidence_sha256


def reference_contract_missing(
    snapshot: dict[str, Any] | None,
    *,
    lane: str,
) -> tuple[str, ...]:
    if snapshot is None:
        return ()
    return tuple(
        sorted(
            {
                *(
                    f"{lane}_request:{value}"
                    for value in snapshot["missing_request_methods"]
                ),
                *(
                    f"{lane}_notification:{value}"
                    for value in snapshot["missing_notification_methods"]
                ),
            }
        )
    )


def observation_for_upstream_reference(
    observation: Observation,
    mirror: Path,
    reference: str,
    policy: dict[str, Any],
) -> Observation:
    snapshot = upstream_schema_snapshot(
        mirror,
        reference,
        required_requests=policy["required_stable_request_methods"],
        required_notifications=policy["required_notification_methods"],
    )
    return replace(
        observation,
        upstream_main_schema_fingerprint=snapshot["fingerprint"],
        upstream_main_contract_missing=reference_contract_missing(
            snapshot,
            lane="main",
        ),
    )


def collect_observation(
    cache_root: Path,
    policy: dict[str, Any],
    codex: str,
    *,
    retained_evidence: set[str] | None = None,
) -> tuple[Observation, Path]:
    mirror = ensure_mirror(cache_root, policy)
    (
        head,
        stable_tag,
        stable_tag_sha,
        prerelease_tag,
        prerelease_tag_sha,
    ) = upstream_source_observation(mirror, policy)
    executable, executable_sha256 = resolve_executable(codex)
    executable_text = str(executable)
    version = run_command(
        [executable_text, "--version"],
        failure_code="codex_version_unavailable",
    )
    if CODEX_VERSION_PATTERN.fullmatch(version) is None:
        raise AutopilotError("codex_version_invalid")
    if hash_file_bounded(executable) != executable_sha256:
        raise AutopilotError("codex_executable_changed")
    stable_schema = schema_snapshot(
        executable_text,
        experimental=False,
        required_requests=policy["required_stable_request_methods"],
        required_notifications=policy["required_notification_methods"],
    )
    if hash_file_bounded(executable) != executable_sha256:
        raise AutopilotError("codex_executable_changed")
    experimental_schema = schema_snapshot(
        executable_text,
        experimental=True,
        required_requests=policy["required_experimental_request_methods"],
        required_notifications=policy["required_notification_methods"],
    )
    if hash_file_bounded(executable) != executable_sha256:
        raise AutopilotError("codex_executable_changed")
    stable_evidence = persist_schema_evidence(
        cache_root,
        codex_version=version,
        executable_sha256=executable_sha256,
        experimental=False,
        snapshot=stable_schema,
        retained_evidence=retained_evidence,
    )
    retained_for_experimental = set(retained_evidence or ())
    retained_for_experimental.add(stable_evidence)
    experimental_evidence = persist_schema_evidence(
        cache_root,
        codex_version=version,
        executable_sha256=executable_sha256,
        experimental=True,
        snapshot=experimental_schema,
        retained_evidence=retained_for_experimental,
    )
    upstream_main_schema = upstream_schema_snapshot(
        mirror,
        head,
        required_requests=policy["required_stable_request_methods"],
        required_notifications=policy["required_notification_methods"],
    )
    stable_release_schema = (
        upstream_schema_snapshot(
            mirror,
            stable_tag_sha,
            required_requests=policy["required_stable_request_methods"],
            required_notifications=policy["required_notification_methods"],
        )
        if stable_tag_sha is not None
        else None
    )
    prerelease_schema = (
        upstream_schema_snapshot(
            mirror,
            prerelease_tag_sha,
            required_requests=policy["required_stable_request_methods"],
            required_notifications=policy["required_notification_methods"],
        )
        if prerelease_tag_sha is not None
        else None
    )
    marker_path = REPO_ROOT / policy["accepted_schema_marker_path"]
    if marker_path.is_symlink() or not marker_path.is_file():
        raise AutopilotError("accepted_schema_marker_invalid")
    try:
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
        accepted_digests = marker["canonical_sha256"]
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
        raise AutopilotError("accepted_schema_marker_invalid") from error
    if not isinstance(accepted_digests, dict):
        raise AutopilotError("accepted_schema_marker_invalid")
    repository_contract_drift = tuple(
        sorted(
            name
            for name, digest in experimental_schema["core_digests"].items()
            if accepted_digests.get(name) != digest
        )
    )
    observation = Observation(
        upstream_head_sha=head,
        stable_tag=stable_tag,
        stable_tag_sha=stable_tag_sha,
        prerelease_tag=prerelease_tag,
        prerelease_tag_sha=prerelease_tag_sha,
        codex_version=version,
        codex_executable_sha256=executable_sha256,
        policy_fingerprint=sha256_value(policy),
        accepted_marker_fingerprint=sha256_value(marker),
        stable_schema_fingerprint=stable_schema["fingerprint"],
        experimental_schema_fingerprint=experimental_schema["fingerprint"],
        stable_schema_evidence_sha256=stable_evidence,
        experimental_schema_evidence_sha256=experimental_evidence,
        upstream_main_schema_fingerprint=upstream_main_schema["fingerprint"],
        stable_release_schema_fingerprint=(
            stable_release_schema["fingerprint"]
            if stable_release_schema is not None
            else None
        ),
        prerelease_schema_fingerprint=(
            prerelease_schema["fingerprint"] if prerelease_schema is not None else None
        ),
        stable_missing_request_methods=tuple(stable_schema["missing_request_methods"]),
        stable_missing_notification_methods=tuple(
            stable_schema["missing_notification_methods"]
        ),
        experimental_missing_request_methods=tuple(
            experimental_schema["missing_request_methods"]
        ),
        experimental_missing_notification_methods=tuple(
            experimental_schema["missing_notification_methods"]
        ),
        repository_contract_drift=repository_contract_drift,
        upstream_main_contract_missing=reference_contract_missing(
            upstream_main_schema,
            lane="main",
        ),
        stable_release_contract_missing=reference_contract_missing(
            stable_release_schema,
            lane="stable",
        ),
        prerelease_contract_missing=reference_contract_missing(
            prerelease_schema,
            lane="prerelease",
        ),
    )
    return observation, mirror


def prepare_observation_plan(
    state: dict[str, Any],
    policy: dict[str, Any],
    observation: Observation,
    mirror: Path,
) -> tuple[list[str], dict[str, Observation], dict[str, dict[str, Any]]]:
    queued_head = state["source"]["queued_head_sha"]
    if queued_head is None or queued_head == observation.upstream_head_sha:
        return [], {}, {}
    commits = upstream_commits(mirror, queued_head, observation.upstream_head_sha)
    batch_size = int(policy["max_batch_commits"])
    active_source_candidates = sum(
        candidate.get("source_sequence") is not None
        and candidate["status"] not in {"landed", "no_change", "rejected"}
        for candidate in state["candidates"]
    )
    available_batches = max(
        0,
        MAX_ACTIVE_SOURCE_CANDIDATES - active_source_candidates,
    )
    references: dict[str, Observation] = {}
    summaries: dict[str, dict[str, Any]] = {}
    lower = queued_head
    for offset in range(
        0,
        min(len(commits), available_batches * batch_size),
        batch_size,
    ):
        upper = commits[min(offset + batch_size, len(commits)) - 1]
        references[upper] = (
            observation
            if upper == observation.upstream_head_sha
            else observation_for_upstream_reference(
                observation,
                mirror,
                upper,
                policy,
            )
        )
        summaries[f"{lower}:{upper}"] = changed_path_summary(
            mirror,
            lower,
            upper,
            policy["relevant_path_prefixes"],
        )
        lower = upper
    return commits, references, summaries


def upstream_commits(mirror: Path, previous: str, current: str) -> list[str]:
    if not SHA_PATTERN.fullmatch(previous) or not SHA_PATTERN.fullmatch(current):
        raise AutopilotError("upstream_range_invalid")
    if previous != current and not command_succeeds(
        mirror_arguments(mirror, "merge-base", "--is-ancestor", previous, current),
        failure_code="upstream_ancestry_unavailable",
    ):
        raise AutopilotError("upstream_history_rewritten")
    count_text = run_command(
        mirror_arguments(
            mirror,
            "rev-list",
            "--count",
            "--first-parent",
            f"{previous}..{current}",
        ),
        failure_code="upstream_range_unavailable",
    )
    try:
        count = int(count_text)
    except ValueError as error:
        raise AutopilotError("upstream_range_count_invalid") from error
    if count > MAX_UPSTREAM_COMMITS:
        raise AutopilotError("upstream_range_budget")
    output = run_command(
        mirror_arguments(
            mirror,
            "rev-list",
            "--reverse",
            "--first-parent",
            f"{previous}..{current}",
        ),
        failure_code="upstream_range_unavailable",
        max_output_bytes=MAX_GIT_TEXT_BYTES,
    )
    commits = [value for value in output.splitlines() if value]
    if any(not SHA_PATTERN.fullmatch(value) for value in commits):
        raise AutopilotError("upstream_commit_invalid")
    if previous != current and (not commits or commits[-1] != current):
        raise AutopilotError("upstream_range_incomplete")
    return commits


def changed_path_summary(
    mirror: Path,
    previous: str,
    current: str,
    relevant_prefixes: Sequence[str],
) -> dict[str, Any]:
    output = run_command(
        mirror_arguments(
            mirror,
            "diff",
            "--name-only",
            "--no-renames",
            previous,
            current,
        ),
        failure_code="upstream_diff_unavailable",
        max_output_bytes=MAX_GIT_TEXT_BYTES,
    )
    paths = [value for value in output.splitlines() if value]
    if len(output.encode("utf-8")) > MAX_GIT_TEXT_BYTES or len(paths) > 100_000:
        raise AutopilotError("upstream_diff_budget")
    relevant = [
        value for value in paths if any(value.startswith(prefix) for prefix in relevant_prefixes)
    ]
    affected_prefixes = sorted(
        prefix
        for prefix in relevant_prefixes
        if any(value.startswith(prefix) for value in relevant)
    )
    return {
        "changed_path_count": len(paths),
        "relevant_path_count": len(relevant),
        "affected_trusted_prefixes": affected_prefixes,
    }
