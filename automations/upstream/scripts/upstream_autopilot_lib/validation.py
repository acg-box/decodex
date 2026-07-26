"""Run and validate exact repository-bound automation verification profiles."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import tomllib
from typing import Any

from .core import (
    FULL_GATE_EXACT_PATHS,
    FULL_GATE_PREFIXES,
    FULL_VALIDATION_PROFILE,
    REQUIRED_VALIDATION_PROFILES,
    REASON_PATTERN,
    SHA_PATTERN,
    TRUSTED_SYSTEM_EXECUTABLE_ROOTS,
    TRUSTED_SYSTEM_TOOL_DIRECTORIES,
    VALIDATION_AUTHORITY_PATHS,
    VALIDATION_AUTHORITY_PREFIXES,
    VALIDATION_PROFILE_COMMANDS,
    AutopilotError,
    command_succeeds,
    hash_file_bounded,
    has_exact_keys,
    is_sha256,
    real_home_directory,
    run_command,
    sha256_value,
    target_origin_urls,
    trusted_executable,
    utc_now,
)


PROFILE_RESULT_KEYS = {
    "name",
    "command_sha256",
    "environment_sha256",
    "exit_code",
    "output_sha256",
    "toolchain_evidence",
}
VALIDATION_RECEIPT_KEYS = {
    "role",
    "base_head",
    "repository_head",
    "repository_tree",
    "changed_path_count",
    "changed_paths_sha256",
    "requires_full_gate",
    "validation_authority",
    "profiles",
    "completed_at",
}
VALIDATION_AUTHORITY_KEYS = {
    "repository_head",
    "repository_tree",
    "closure_sha256",
}
VALIDATION_TOOL_NAMES = (
    "cargo",
    "cargo-make",
    "cargo-nextest",
    "cp",
    "git",
    "node",
    "npm",
    "python3",
    "rustup",
    "sandbox-exec",
    "taplo",
)
TRUSTED_TOOL_DISCOVERY_PATHS = tuple(
    str(path) for path in TRUSTED_SYSTEM_TOOL_DIRECTORIES
)
FULL_XCODE_EVIDENCE_KEYS = {
    "developer_dir_sha256",
    "metal_sha256",
    "xcode_select_sha256",
    "xcode_version_sha256",
    "xcodebuild_sha256",
    "xcrun_sha256",
}
TOOLCHAIN_EVIDENCE_KEYS = {
    "full_xcode",
    "sandbox",
    "validation_tools",
}
SANDBOX_EVIDENCE_KEYS = {
    "dependency_preparation_sha256",
    "nightly_cargo_fmt_sha256",
    "nightly_rustfmt_sha256",
    "profile_sha256",
    "sandbox_executable_sha256",
    "stable_cargo_sha256",
}
FORMATTER_TOOLCHAIN = "nightly-2026-07-16"
TRUSTED_TOOLCHAIN_EXECUTABLES = {"cargo", "cargo-fmt", "rustfmt"}
SANDBOX_PROFILE_TASKS = {
    "focused_tests": "test-automations",
    "cargo_make_check_upstream_automation": (
        "check-upstream-automation-sandboxed"
    ),
    FULL_VALIDATION_PROFILE: "check-sandboxed",
}
MAX_CARGO_LOCK_BYTES = 16 * 1024 * 1024
MAX_CARGO_PACKAGES = 4096
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
GIT_SOURCE_PATTERN = re.compile(
    r"^git\+(https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:\.git)?)"
    r"\?rev=([0-9a-f]{40})#([0-9a-f]{40})$"
)
MINIMUM_VALIDATION_PYTHON = (3, 11)


def repository_identity(worktree: Path) -> tuple[str, str]:
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="validation_head_unavailable",
    )
    tree = run_command(
        ["git", "rev-parse", "HEAD^{tree}"],
        cwd=worktree,
        failure_code="validation_tree_unavailable",
    )
    status = run_command(
        ["git", "status", "--porcelain=v1"],
        cwd=worktree,
        failure_code="validation_status_unavailable",
    )
    if (
        SHA_PATTERN.fullmatch(head) is None
        or SHA_PATTERN.fullmatch(tree) is None
        or status
    ):
        raise AutopilotError("validation_worktree_not_clean")
    return head, tree


def validation_authority_identity(repo_root: Path) -> dict[str, str]:
    head, tree = repository_identity(repo_root)
    closure = run_command(
        [
            "git",
            "ls-tree",
            "-r",
            "--full-tree",
            "HEAD",
            "--",
            *sorted(VALIDATION_AUTHORITY_PATHS),
            *VALIDATION_AUTHORITY_PREFIXES,
        ],
        cwd=repo_root,
        failure_code="validation_authority_unavailable",
    )
    if not closure:
        raise AutopilotError("validation_authority_unavailable")
    return {
        "repository_head": head,
        "repository_tree": tree,
        "closure_sha256": sha256_value(
            {
                "paths": [
                    *sorted(VALIDATION_AUTHORITY_PATHS),
                    *VALIDATION_AUTHORITY_PREFIXES,
                ],
                "git_tree_entries": closure,
            }
        ),
    }


def changed_paths_between(
    worktree: Path,
    *,
    base_head: str,
    head: str,
) -> tuple[str, ...]:
    if (
        SHA_PATTERN.fullmatch(base_head) is None
        or SHA_PATTERN.fullmatch(head) is None
        or not command_succeeds(
            ["git", "merge-base", "--is-ancestor", base_head, head],
            cwd=worktree,
            failure_code="validation_base_unavailable",
        )
    ):
        raise AutopilotError("validation_base_invalid")
    output = run_command(
        [
            "git",
            "diff",
            "--name-only",
            "--diff-filter=ACDMRTUXB",
            f"{base_head}..{head}",
            "--",
        ],
        cwd=worktree,
        failure_code="validation_diff_unavailable",
        max_output_bytes=8 * 1024 * 1024,
    )
    paths = tuple(line for line in output.splitlines() if line)
    if (
        len(paths) > 4096
        or len(set(paths)) != len(paths)
        or any(
            Path(path).is_absolute()
            or ".." in Path(path).parts
            or len(path) > 512
            or "\r" in path
            for path in paths
        )
    ):
        raise AutopilotError("validation_diff_invalid")
    return paths


def path_requires_full_gate(path: str) -> bool:
    value = Path(path)
    return (
        path in FULL_GATE_EXACT_PATHS
        or any(path.startswith(prefix) for prefix in FULL_GATE_PREFIXES)
        or value.name in {"Cargo.toml", "build.rs"}
        or value.suffix in {".entitlements", ".swift", ".xcconfig", ".xcodeproj"}
        or ".xcodeproj" in value.parts
    )


def classify_validation_scope(
    changed_paths: tuple[str, ...],
    *,
    candidate_kind: str,
) -> dict[str, Any]:
    authority_paths = tuple(
        path
        for path in changed_paths
        if path in VALIDATION_AUTHORITY_PATHS
        or any(path.startswith(prefix) for prefix in VALIDATION_AUTHORITY_PREFIXES)
    )
    if authority_paths and candidate_kind != "automation_repair":
        raise AutopilotError("validation_authority_change_not_repair")
    requires_full_gate = bool(authority_paths) or any(
        path_requires_full_gate(path) for path in changed_paths
    )
    return {
        "changed_path_count": len(changed_paths),
        "changed_paths_sha256": sha256_value(list(changed_paths)),
        "requires_full_gate": requires_full_gate,
    }


def required_profile_names(requires_full_gate: bool) -> tuple[str, ...]:
    if requires_full_gate:
        return (*REQUIRED_VALIDATION_PROFILES, FULL_VALIDATION_PROFILE)
    return REQUIRED_VALIDATION_PROFILES


def trusted_profile_command(
    repo_root: Path,
    name: str,
    *,
    cargo_executable: Path | None = None,
) -> list[str]:
    command = VALIDATION_PROFILE_COMMANDS.get(name)
    if command is None or command[:2] != ["cargo", "make"] or len(command) != 3:
        raise AutopilotError("validation_profile_command_invalid")
    sandbox_task = SANDBOX_PROFILE_TASKS.get(name)
    if sandbox_task is None:
        raise AutopilotError("validation_profile_command_invalid")
    return [
        str(cargo_executable or "cargo"),
        "make",
        "--makefile",
        str((repo_root / "Makefile.toml").resolve()),
        sandbox_task,
    ]


def full_xcode_environment() -> tuple[dict[str, str], dict[str, str]]:
    xcode_select = Path("/usr/bin/xcode-select")
    xcrun = Path("/usr/bin/xcrun")
    if (
        xcode_select.is_symlink()
        or not xcode_select.is_file()
        or xcrun.is_symlink()
        or not xcrun.is_file()
    ):
        raise AutopilotError("full_xcode_unavailable")
    candidates: list[Path] = []
    configured = os.environ.get("DEVELOPER_DIR", "").strip()
    if configured:
        candidates.append(Path(configured))
    selected = run_command(
        [str(xcode_select), "-p"],
        failure_code="xcode_select_unavailable",
        allow_failure=True,
    )
    if selected:
        candidates.append(Path(selected))
    try:
        candidates.extend(
            sorted(
                (
                    app / "Contents/Developer"
                    for app in Path("/Applications").glob("Xcode*.app")
                ),
                key=str,
            )[:16]
        )
    except OSError:
        pass

    seen: set[Path] = set()
    for candidate in candidates:
        try:
            resolved = candidate.expanduser().resolve(strict=True)
        except OSError:
            continue
        if resolved in seen or not resolved.is_dir():
            continue
        seen.add(resolved)
        xcodebuild = resolved / "usr/bin/xcodebuild"
        if xcodebuild.is_symlink() or not xcodebuild.is_file():
            continue
        environment = {"DEVELOPER_DIR": str(resolved)}
        metal = run_command(
            [str(xcrun), "--find", "metal"],
            environment=environment,
            failure_code="full_xcode_unavailable",
            allow_failure=True,
        )
        version = run_command(
            [str(xcodebuild), "-version"],
            environment=environment,
            failure_code="full_xcode_unavailable",
            allow_failure=True,
        )
        if not metal or not version:
            continue
        try:
            metal_path = Path(metal).resolve(strict=True)
            evidence = {
                "developer_dir_sha256": sha256_value(str(resolved)),
                "xcode_select_sha256": hash_file_bounded(xcode_select),
                "xcode_version_sha256": sha256_value(version),
                "xcodebuild_sha256": hash_file_bounded(xcodebuild),
                "xcrun_sha256": hash_file_bounded(xcrun),
                "metal_sha256": hash_file_bounded(metal_path),
            }
        except (OSError, AutopilotError):
            continue
        return environment, evidence
    raise AutopilotError("full_xcode_unavailable")


def validation_tools(
    repo_root: Path,
) -> tuple[dict[str, Path], dict[str, str]]:
    paths: dict[str, Path] = {}
    discovery_path = os.pathsep.join(TRUSTED_TOOL_DISCOVERY_PATHS)
    for name in VALIDATION_TOOL_NAMES:
        if name == "python3":
            paths[name] = trusted_validation_python(repo_root)
            continue
        located = shutil.which(name, path=discovery_path)
        if located is None:
            raise AutopilotError("validation_tool_unavailable")
        path = Path(located).absolute()
        if path.is_symlink():
            target = Path(os.readlink(path))
            if not target.is_absolute():
                target = path.parent / target
            if (
                len(target.parts) >= 3
                and target.parts[:3] == ("/", "nix", "store")
                and target.name == name
            ):
                path = target
        try:
            resolved = path.resolve(strict=True)
        except OSError as error:
            raise AutopilotError("validation_tool_unavailable") from error
        if path.name != name:
            raise AutopilotError("validation_tool_name_mismatch")
        if (
            _path_is_within(path, repo_root)
            or _path_is_within(resolved, repo_root)
            or ".worktrees" in path.parts
            or ".worktrees" in resolved.parts
        ):
            raise AutopilotError("validation_tool_inside_candidate")
        paths[name] = path
    paths["cargo"] = trusted_toolchain_cargo(
        repo_root,
        paths["rustup"],
        repository_toolchain_channel(repo_root),
    )
    environment_path = os.pathsep.join(
        _validation_path_entries(paths)
    )
    for name, expected in paths.items():
        selected = shutil.which(name, path=environment_path)
        if selected is None or Path(selected).absolute() != expected:
            raise AutopilotError("validation_tool_path_conflict")
    return paths, validation_tool_evidence(paths)


def trusted_validation_python(repo_root: Path) -> Path:
    if not MINIMUM_VALIDATION_PYTHON <= sys.version_info[:2] < (4, 0):
        raise AutopilotError("validation_python_runtime_unsupported")
    runtime = Path(sys.executable).absolute()
    candidate = runtime.parent / "python3"
    try:
        candidate_metadata = candidate.lstat()
        parent_metadata = candidate.parent.stat()
        resolved = candidate.resolve(strict=True)
        resolved_metadata = resolved.stat()
        runtime_resolved = runtime.resolve(strict=True)
    except OSError as error:
        raise AutopilotError("validation_python_runtime_unavailable") from error
    if (
        candidate.name != "python3"
        or resolved != runtime_resolved
        or not resolved.is_file()
        or candidate_metadata.st_uid != 0
        or parent_metadata.st_uid != 0
        or resolved_metadata.st_uid != 0
        or candidate_metadata.st_mode & 0o022
        or parent_metadata.st_mode & 0o022
        or resolved_metadata.st_mode & 0o022
        or not any(
            _path_is_within(candidate, root)
            and _path_is_within(resolved, root)
            for root in TRUSTED_SYSTEM_EXECUTABLE_ROOTS
        )
        or _path_is_within(candidate, repo_root)
        or _path_is_within(resolved, repo_root)
        or ".worktrees" in candidate.parts
        or ".worktrees" in resolved.parts
    ):
        raise AutopilotError("validation_python_runtime_untrusted")
    hash_file_bounded(resolved)
    return candidate


def _path_is_within(path: Path, root: Path) -> bool:
    try:
        path.absolute().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def _validation_path_entries(tools: dict[str, Path]) -> list[str]:
    paths = [
        *(path.parent for path in tools.values()),
        Path("/usr/bin"),
        Path("/bin"),
        Path("/usr/sbin"),
        Path("/sbin"),
    ]
    unique: list[str] = []
    for path in paths:
        value = str(path)
        if value not in unique:
            unique.append(value)
    return unique


def trusted_rustup_home() -> Path:
    home = real_home_directory().resolve()
    configured = home / ".rustup"
    try:
        resolved = configured.resolve(strict=True)
        stat = resolved.stat()
    except OSError as error:
        raise AutopilotError("rustup_toolchain_unavailable") from error
    if (
        not _path_is_within(resolved, home)
        or not resolved.is_dir()
        or stat.st_uid != os.getuid()
        or stat.st_mode & 0o022
        or not (resolved / "toolchains").is_dir()
    ):
        raise AutopilotError("rustup_toolchain_unavailable")
    return resolved


def repository_toolchain_channel(repo_root: Path) -> str:
    source = repo_root / "rust-toolchain.toml"
    path = source.resolve()
    if (
        not _path_is_within(path, repo_root)
        or source.is_symlink()
        or not path.is_file()
        or path.stat().st_size > 16 * 1024
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AutopilotError("rust_toolchain_policy_invalid") from error
    toolchain = value.get("toolchain")
    channel = toolchain.get("channel") if isinstance(toolchain, dict) else None
    if (
        not isinstance(channel, str)
        or re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", channel) is None
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    return channel


def trusted_toolchain_cargo(
    repo_root: Path,
    rustup: Path,
    toolchain: str,
) -> Path:
    return trusted_toolchain_executable(
        repo_root,
        rustup,
        toolchain,
        "cargo",
    )


def trusted_toolchain_executable(
    repo_root: Path,
    rustup: Path,
    toolchain: str,
    executable: str,
) -> Path:
    if (
        _path_is_within(rustup, repo_root)
        or executable not in TRUSTED_TOOLCHAIN_EXECUTABLES
        or re.fullmatch(
            r"(?:[0-9]+\.[0-9]+\.[0-9]+|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})",
            toolchain,
        )
        is None
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    output = run_command(
        [
            str(rustup),
            "which",
            executable,
            "--toolchain",
            toolchain,
        ],
        failure_code="rust_toolchain_unavailable",
    )
    try:
        path = Path(output).resolve(strict=True)
        metadata = path.stat()
        path.relative_to(trusted_rustup_home() / "toolchains")
    except (OSError, ValueError) as error:
        raise AutopilotError("rust_toolchain_unavailable") from error
    if (
        path.name != executable
        or not path.is_file()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o022
    ):
        raise AutopilotError("rust_toolchain_unavailable")
    hash_file_bounded(path)
    return path


def trusted_formatter_tools(
    repo_root: Path,
    rustup: Path,
) -> dict[str, Path]:
    return {
        executable: trusted_toolchain_executable(
            repo_root,
            rustup,
            FORMATTER_TOOLCHAIN,
            executable,
        )
        for executable in ("cargo-fmt", "rustfmt")
    }


def validation_tool_evidence(tools: dict[str, Path]) -> dict[str, str]:
    if set(tools) != set(VALIDATION_TOOL_NAMES):
        raise AutopilotError("validation_tool_set_invalid")
    evidence: dict[str, str] = {}
    for name, path in tools.items():
        try:
            resolved = path.resolve(strict=True)
            link_target = os.readlink(path) if path.is_symlink() else None
        except OSError as error:
            raise AutopilotError("validation_tool_unavailable") from error
        evidence[name] = sha256_value(
            {
                "execution_path": str(path),
                "link_target": link_target,
                "resolved_path": str(resolved),
                "resolved_sha256": hash_file_bounded(resolved),
            }
        )
    return evidence


def sanitized_validation_environment(
    temporary_home: Path,
    tools: dict[str, Path],
    *,
    cargo_home: Path | None = None,
    offline: bool = False,
    overrides: dict[str, str] | None = None,
) -> dict[str, str]:
    rustup_home = trusted_rustup_home()
    npm_global_config = temporary_home / "npm-globalrc"
    npm_project_config = temporary_home / "npm-projectrc"
    npm_user_config = temporary_home / "npm-userrc"
    environment = {
        "ASTRO_TELEMETRY_DISABLED": "1",
        "CARGO_HOME": str(cargo_home or temporary_home / "cargo-home"),
        "CARGO_BUILD_RUSTC_WRAPPER": "",
        "CARGO_NET_GIT_FETCH_WITH_CLI": "false",
        "CARGO_TARGET_DIR": str(temporary_home / "cargo-target"),
        "CARGO_TERM_COLOR": "never",
        "CI": "1",
        "DECODEX_CANDIDATE_SANDBOX": "1",
        "GCM_INTERACTIVE": "never",
        "GH_PROMPT_DISABLED": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "HOME": str(temporary_home),
        "LANG": os.environ.get("LANG", "C.UTF-8"),
        "NPM_CONFIG_CACHE": str(temporary_home / "npm-cache"),
        "NPM_CONFIG_GLOBALCONFIG": str(npm_global_config),
        "NPM_CONFIG_IGNORE_SCRIPTS": "true",
        "NPM_CONFIG_PROJECTCONFIG": str(npm_project_config),
        "NPM_CONFIG_REGISTRY": "https://registry.npmjs.org/",
        "NPM_CONFIG_USERCONFIG": str(npm_user_config),
        "PATH": os.pathsep.join(_validation_path_entries(tools)),
        "PYTHONDONTWRITEBYTECODE": "1",
        "RUSTUP_HOME": str(rustup_home),
        "RUSTC_WRAPPER": "",
        "TMPDIR": str(temporary_home),
        "XDG_CACHE_HOME": str(temporary_home / "xdg-cache"),
        "XDG_CONFIG_HOME": str(temporary_home / "xdg-config"),
    }
    if offline:
        environment["CARGO_NET_OFFLINE"] = "true"
        environment["NPM_CONFIG_OFFLINE"] = "true"
    for name in ("NIX_SSL_CERT_FILE", "SSL_CERT_FILE", "SSL_CERT_DIR"):
        value = os.environ.get(name)
        if value:
            environment[name] = value
    locale = os.environ.get("LC_ALL")
    if locale:
        environment["LC_ALL"] = locale
    environment.update(overrides or {})
    return environment


def initialize_validation_home(temporary_home: Path) -> None:
    for name in ("npm-globalrc", "npm-projectrc", "npm-userrc"):
        path = temporary_home / name
        try:
            with path.open("x", encoding="utf-8") as handle:
                handle.write("")
                handle.flush()
                os.fsync(handle.fileno())
            os.chmod(path, 0o400)
        except OSError as error:
            raise AutopilotError("validation_home_initialization_failed") from error


def validate_validation_receipt(
    receipt: Any,
    *,
    role: str | None = None,
    expected_base_head: str | None = None,
    expected_head: str | None = None,
    expected_tree: str | None = None,
) -> None:
    if not has_exact_keys(receipt, VALIDATION_RECEIPT_KEYS):
        raise AutopilotError("validation_receipt_invalid")
    if receipt.get("role") not in {"maintainer", "reviewer"}:
        raise AutopilotError("validation_receipt_invalid")
    if role is not None and receipt["role"] != role:
        raise AutopilotError("validation_receipt_role_mismatch")
    if (
        SHA_PATTERN.fullmatch(str(receipt.get("base_head", ""))) is None
        or SHA_PATTERN.fullmatch(str(receipt.get("repository_head", "")))
        is None
        or SHA_PATTERN.fullmatch(str(receipt.get("repository_tree", ""))) is None
        or not isinstance(receipt.get("changed_path_count"), int)
        or not 0 <= receipt["changed_path_count"] <= 4096
        or not is_sha256(receipt.get("changed_paths_sha256"))
        or not isinstance(receipt.get("requires_full_gate"), bool)
        or not isinstance(receipt.get("completed_at"), int)
    ):
        raise AutopilotError("validation_receipt_invalid")
    if expected_base_head is not None and receipt["base_head"] != expected_base_head:
        raise AutopilotError("validation_receipt_base_mismatch")
    if expected_head is not None and receipt["repository_head"] != expected_head:
        raise AutopilotError("validation_receipt_head_mismatch")
    if expected_tree is not None and receipt["repository_tree"] != expected_tree:
        raise AutopilotError("validation_receipt_tree_mismatch")
    authority = receipt.get("validation_authority")
    if (
        not has_exact_keys(authority, VALIDATION_AUTHORITY_KEYS)
        or SHA_PATTERN.fullmatch(str(authority.get("repository_head", ""))) is None
        or SHA_PATTERN.fullmatch(str(authority.get("repository_tree", ""))) is None
        or not is_sha256(authority.get("closure_sha256"))
        or (
            (receipt["base_head"] == receipt["repository_head"])
            != (receipt["changed_path_count"] == 0)
        )
        or (
            receipt["changed_path_count"] == 0
            and receipt["changed_paths_sha256"] != sha256_value([])
        )
    ):
        raise AutopilotError("validation_receipt_invalid")
    expected_names = required_profile_names(receipt["requires_full_gate"])
    profiles = receipt.get("profiles")
    if not isinstance(profiles, list) or len(profiles) != len(expected_names):
        raise AutopilotError("validation_receipt_invalid")
    names: list[str] = []
    for profile in profiles:
        if (
            not has_exact_keys(profile, PROFILE_RESULT_KEYS)
            or REASON_PATTERN.fullmatch(str(profile.get("name", ""))) is None
            or not is_sha256(profile.get("command_sha256"))
            or not is_sha256(profile.get("environment_sha256"))
            or profile.get("exit_code") != 0
            or not is_sha256(profile.get("output_sha256"))
        ):
            raise AutopilotError("validation_receipt_invalid")
        names.append(profile["name"])
        expected_command = VALIDATION_PROFILE_COMMANDS.get(profile["name"])
        if (
            expected_command is None
            or profile["command_sha256"] != sha256_value(expected_command)
        ):
            raise AutopilotError("validation_receipt_command_mismatch")
        toolchain = profile.get("toolchain_evidence")
        if (
            not has_exact_keys(toolchain, TOOLCHAIN_EVIDENCE_KEYS)
            or not has_exact_keys(
                toolchain.get("validation_tools"),
                set(VALIDATION_TOOL_NAMES),
            )
            or any(
                not is_sha256(value)
                for value in toolchain["validation_tools"].values()
            )
        ):
            raise AutopilotError("validation_receipt_toolchain_invalid")
        full_xcode = toolchain.get("full_xcode")
        if profile["name"] == FULL_VALIDATION_PROFILE:
            if (
                not has_exact_keys(
                    full_xcode,
                    FULL_XCODE_EVIDENCE_KEYS,
                )
                or any(not is_sha256(value) for value in full_xcode.values())
            ):
                raise AutopilotError(
                    "validation_receipt_toolchain_invalid"
                )
        elif full_xcode is not None:
            raise AutopilotError("validation_receipt_toolchain_invalid")
        sandbox = toolchain.get("sandbox")
        if (
            not has_exact_keys(sandbox, SANDBOX_EVIDENCE_KEYS)
            or any(not is_sha256(value) for value in sandbox.values())
        ):
            raise AutopilotError("validation_receipt_toolchain_invalid")
    if names != list(expected_names):
        raise AutopilotError("validation_receipt_invalid")


def validate_receipt_against_policy(
    receipt: dict[str, Any],
    policy: dict[str, Any],
    *,
    role: str,
    expected_base_head: str,
    expected_head: str,
    expected_tree: str,
) -> None:
    validate_validation_receipt(
        receipt,
        role=role,
        expected_base_head=expected_base_head,
        expected_head=expected_head,
        expected_tree=expected_tree,
    )
    for profile in receipt["profiles"]:
        command = policy["validation_profiles"][profile["name"]]
        if (
            command != VALIDATION_PROFILE_COMMANDS[profile["name"]]
            or profile["command_sha256"] != sha256_value(command)
        ):
            raise AutopilotError("validation_receipt_command_mismatch")


def validation_receipt_is_current(
    receipt: dict[str, Any],
    *,
    current_main_head: str,
    current_authority: dict[str, str],
) -> bool:
    validate_validation_receipt(receipt)
    if SHA_PATTERN.fullmatch(current_main_head) is None:
        raise AutopilotError("current_main_head_invalid")
    if (
        not has_exact_keys(current_authority, VALIDATION_AUTHORITY_KEYS)
        or any(
            SHA_PATTERN.fullmatch(str(current_authority.get(key, "")))
            is None
            for key in ("repository_head", "repository_tree")
        )
        or not is_sha256(current_authority.get("closure_sha256"))
    ):
        raise AutopilotError("validation_authority_invalid")
    return (
        receipt["base_head"] == current_main_head
        and receipt["validation_authority"] == current_authority
    )


def _load_cargo_lock(path: Path) -> dict[str, Any]:
    if (
        path.is_symlink()
        or not path.is_file()
        or path.stat().st_size > MAX_CARGO_LOCK_BYTES
    ):
        raise AutopilotError("cargo_lock_unavailable")
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AutopilotError("cargo_lock_unavailable") from error
    packages = value.get("package")
    if (
        value.get("version") != 4
        or not isinstance(packages, list)
        or not 1 <= len(packages) <= MAX_CARGO_PACKAGES
    ):
        raise AutopilotError("cargo_lock_invalid")
    return value


def cargo_lock_provenance(repo_root: Path, worktree: Path) -> dict[str, Any]:
    trusted = _load_cargo_lock(repo_root / "Cargo.lock")
    candidate = _load_cargo_lock(worktree / "Cargo.lock")

    def inspect(value: dict[str, Any]) -> tuple[set[str], list[dict[str, Any]]]:
        git_repositories: set[str] = set()
        identities: list[dict[str, Any]] = []
        for package in value["package"]:
            if (
                not isinstance(package, dict)
                or not isinstance(package.get("name"), str)
                or not 1 <= len(package["name"]) <= 256
                or not isinstance(package.get("version"), str)
                or not 1 <= len(package["version"]) <= 128
            ):
                raise AutopilotError("cargo_lock_invalid")
            source = package.get("source")
            checksum = package.get("checksum")
            if source is None:
                if checksum is not None:
                    raise AutopilotError("cargo_lock_provenance_invalid")
                continue
            if source == CRATES_IO_SOURCE:
                if (
                    not isinstance(checksum, str)
                    or re.fullmatch(r"[0-9a-f]{64}", checksum) is None
                ):
                    raise AutopilotError("cargo_lock_provenance_invalid")
            else:
                match = GIT_SOURCE_PATTERN.fullmatch(str(source))
                if (
                    match is None
                    or match.group(2) != match.group(3)
                    or checksum is not None
                ):
                    raise AutopilotError("cargo_lock_provenance_invalid")
                git_repositories.add(match.group(1))
            identities.append(
                {
                    "name": package["name"],
                    "version": package["version"],
                    "source": source,
                    "checksum": checksum,
                }
            )
        identities.sort(
            key=lambda item: (
                item["name"],
                item["version"],
                item["source"],
            )
        )
        return git_repositories, identities

    trusted_git, _trusted_identities = inspect(trusted)
    candidate_git, candidate_identities = inspect(candidate)
    if not candidate_git.issubset(trusted_git):
        raise AutopilotError("cargo_lock_git_source_not_approved")
    return {
        "package_count": len(candidate_identities),
        "git_repositories": sorted(candidate_git),
        "graph_sha256": sha256_value(candidate_identities),
    }


def prepare_dependency_cache(
    repo_root: Path,
    worktree: Path,
    temporary_home: Path,
    tools: dict[str, Path],
    trusted_audit: Path,
) -> tuple[dict[str, str], dict[str, Any]]:
    cargo_home = temporary_home / "cargo-home"
    cargo_home.mkdir(mode=0o700)
    source_cargo_home = real_home_directory() / ".cargo"
    for name in ("registry", "git"):
        source = source_cargo_home / name
        destination = cargo_home / name
        if source.is_symlink() or not source.is_dir():
            raise AutopilotError("cargo_cache_source_invalid")
        run_command(
            [
                str(tools["cp"]),
                "-ac",
                str(source),
                str(destination),
            ],
            environment=sanitized_validation_environment(
                temporary_home,
                tools,
                cargo_home=cargo_home,
            ),
            inherit_environment=False,
            failure_code="cargo_cache_clone_failed",
            timeout_seconds=900,
        )
    environment = sanitized_validation_environment(
        temporary_home,
        tools,
        cargo_home=cargo_home,
    )
    cargo_provenance = cargo_lock_provenance(repo_root, worktree)
    cargo_fetch = run_command(
        [
            str(tools["cargo"]),
            "--config",
            "net.git-fetch-with-cli=false",
            "--config",
            'registry.global-credential-providers=["cargo:token"]',
            "fetch",
            "--locked",
            "--manifest-path",
            str((worktree / "Cargo.toml").resolve()),
        ],
        cwd=worktree,
        environment=environment,
        inherit_environment=False,
        failure_code="cargo_dependency_fetch_failed",
        timeout_seconds=1800,
    )
    npm_install = run_command(
        [
            str(tools["npm"]),
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
        ],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_install_failed",
        timeout_seconds=900,
    )
    npm_advisories = run_command(
        [str(tools["npm"]), "audit", "--audit-level=high"],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_advisory_failed",
        timeout_seconds=900,
    )
    npm_signatures = run_command(
        [str(tools["npm"]), "audit", "signatures"],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_signature_failed",
        timeout_seconds=900,
    )
    node_provenance = run_command(
        [
            str(tools["python3"]),
            str(trusted_audit),
            "--site",
            str((worktree / "site").resolve()),
        ],
        cwd=worktree,
        environment=environment,
        inherit_environment=False,
        failure_code="validation_node_provenance_failed",
    )
    evidence = {
        "cargo_lock_provenance": cargo_provenance,
        "cargo_fetch_sha256": sha256_value(cargo_fetch),
        "npm_install_sha256": sha256_value(npm_install),
        "npm_advisories_sha256": sha256_value(npm_advisories),
        "npm_signatures_sha256": sha256_value(npm_signatures),
        "node_provenance_sha256": sha256_value(node_provenance),
    }
    return (
        sanitized_validation_environment(
            temporary_home,
            tools,
            cargo_home=cargo_home,
            offline=True,
        ),
        evidence,
    )


def validation_sandbox_profile(
    repo_root: Path,
    worktree: Path,
    temporary_home: Path,
) -> str:
    root = repo_root.resolve()
    candidate = worktree.resolve()
    home = real_home_directory()
    rustup_home = trusted_rustup_home()
    trusted_makefile = root / "Makefile.toml"
    if (
        not candidate.is_dir()
        or not root.is_dir()
        or not temporary_home.is_dir()
        or not rustup_home.is_dir()
        or trusted_makefile.is_symlink()
        or not trusted_makefile.is_file()
    ):
        raise AutopilotError("validation_sandbox_path_invalid")

    def literal(path: Path) -> str:
        return json.dumps(str(path.resolve(strict=False)))

    readable = (
        candidate,
        root / ".git",
        rustup_home / "toolchains",
        rustup_home / "settings.toml",
        temporary_home,
    )
    writable = (
        temporary_home,
        candidate / "site/.astro",
        candidate / "site/dist",
        candidate / "site/node_modules/.astro",
        candidate / "site/node_modules/.cache",
        candidate / "site/node_modules/.vite",
    )
    lines = [
        "(version 1)",
        "(deny default)",
        "(allow process*)",
        "(allow sysctl-read)",
        "(allow mach-lookup)",
        f"(allow network* (subpath {literal(temporary_home)}))",
        (
            "(deny mach-lookup "
            '(global-name "com.apple.securityd") '
            '(global-name "com.apple.securityd.system") '
            '(global-name "com.apple.securityd.xpc") '
            '(global-name "com.apple.securityd.general") '
            '(global-name "com.apple.securityd.sos") '
            '(global-name "com.apple.securityd.ckks") '
            '(global-name "com.apple.pboard") '
            '(global-name "com.apple.cfprefsd.agent") '
            '(global-name "com.apple.cfprefsd.daemon") '
            '(global-name "com.apple.security.XPCKeychainSandboxCheck"))'
        ),
    ]
    sensitive_read_roots = (
        home.parent,
        temporary_home.parent,
        Path("/Volumes"),
        Path("/Network"),
        Path("/cores"),
        Path("/private/tmp"),
        Path("/tmp"),
        Path("/private/var/tmp"),
        Path("/var/tmp"),
        Path("/private/var/root"),
        Path("/Library/Keychains"),
        Path("/private/var/db/dslocal"),
    )
    exclusions = " ".join(
        f"(require-not (subpath {literal(path)}))"
        for path in sensitive_read_roots
    )
    lines.append(f"(allow file-read* (require-all {exclusions}))")
    lines.append("(allow file-read-metadata)")
    lines.extend(
        f"(allow file-read* (subpath {literal(path)}))"
        for path in readable
        if path.exists()
    )
    lines.append(
        f"(allow file-read* (literal {literal(trusted_makefile)}))"
    )
    lines.extend(
        f"(allow file-write* (subpath {literal(path)}))" for path in writable
    )
    lines.append('(allow file-write* (literal "/dev/null"))')
    cargo_home = temporary_home / "cargo-home"
    for path in (
        cargo_home / "git/checkouts",
        cargo_home / "git/db",
        cargo_home / "registry/cache",
        cargo_home / "registry/src",
    ):
        lines.append(f"(deny file-write* (subpath {literal(path)}))")
    return "\n".join(lines) + "\n"


def sandbox_evidence(
    profile: str,
    dependency_preparation: dict[str, Any],
    tools: dict[str, Path],
    formatter_tools: dict[str, Path],
) -> dict[str, str]:
    if set(formatter_tools) != {"cargo-fmt", "rustfmt"}:
        raise AutopilotError("validation_formatter_tool_set_invalid")
    sandbox_path = tools["sandbox-exec"].resolve(strict=True)
    return {
        "dependency_preparation_sha256": sha256_value(
            dependency_preparation
        ),
        "nightly_cargo_fmt_sha256": hash_file_bounded(
            formatter_tools["cargo-fmt"].resolve(strict=True)
        ),
        "nightly_rustfmt_sha256": hash_file_bounded(
            formatter_tools["rustfmt"].resolve(strict=True)
        ),
        "profile_sha256": sha256_value(profile),
        "sandbox_executable_sha256": hash_file_bounded(sandbox_path),
        "stable_cargo_sha256": hash_file_bounded(
            tools["cargo"].resolve(strict=True)
        ),
    }


def run_validation_profiles(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    role: str,
    candidate_kind: str,
    base_head: str,
    expected_head: str | None = None,
) -> dict[str, Any]:
    if role not in {"maintainer", "reviewer"}:
        raise AutopilotError("validation_receipt_role_mismatch")
    head, tree = repository_identity(worktree)
    if expected_head is not None and head != expected_head:
        raise AutopilotError("validation_receipt_head_mismatch")
    authority = validation_authority_identity(repo_root)
    changed_paths = changed_paths_between(
        worktree,
        base_head=base_head,
        head=head,
    )
    scope = classify_validation_scope(
        changed_paths,
        candidate_kind=candidate_kind,
    )
    results: list[dict[str, Any]] = []
    profile_names = required_profile_names(scope["requires_full_gate"])
    trusted_audit = (repo_root / "scripts/audit_node_lock.py").resolve()
    tool_paths, tool_evidence = validation_tools(repo_root)
    formatter_tools = trusted_formatter_tools(
        repo_root,
        tool_paths["rustup"],
    )
    formatter_environment = {
        "CARGO": str(tool_paths["cargo"]),
        "DECODEX_TRUSTED_NIGHTLY_CARGO_FMT": str(
            formatter_tools["cargo-fmt"]
        ),
        "RUSTFMT": str(formatter_tools["rustfmt"]),
    }
    with tempfile.TemporaryDirectory(
        prefix="decodex-upstream-validation-"
    ) as temporary:
        temporary_home = Path(temporary).resolve()
        initialize_validation_home(temporary_home)
        acquisition_environment = sanitized_validation_environment(
            temporary_home,
            tool_paths,
            cargo_home=temporary_home / "cargo-home",
        )
        runtime_preflight = run_command(
            [
                str(tool_paths["python3"]),
                str(trusted_audit),
                "--runtime-only",
            ],
            cwd=worktree,
            environment=acquisition_environment,
            inherit_environment=False,
            failure_code="validation_node_runtime_failed",
        )
        trusted_lock_provenance = run_command(
            [
                str(tool_paths["python3"]),
                str(trusted_audit),
                "--lock-only",
                "--site",
                str((worktree / "site").resolve()),
            ],
            cwd=worktree,
            environment=acquisition_environment,
            inherit_environment=False,
            failure_code="validation_node_lock_failed",
        )
        profile_environment, dependency_preparation = (
            prepare_dependency_cache(
                repo_root,
                worktree,
                temporary_home,
                tool_paths,
                trusted_audit,
            )
        )
        profile_environment.update(formatter_environment)
        profile = validation_sandbox_profile(
            repo_root,
            worktree,
            temporary_home,
        )
        profile_path = temporary_home / "validation.sb"
        try:
            with profile_path.open("x", encoding="utf-8") as handle:
                handle.write(profile)
                handle.flush()
                os.fsync(handle.fileno())
            os.chmod(profile_path, 0o400)
        except OSError as error:
            raise AutopilotError("validation_sandbox_profile_failed") from error
        current_sandbox_evidence = sandbox_evidence(
            profile,
            dependency_preparation,
            tool_paths,
            formatter_tools,
        )
        if validation_tool_evidence(tool_paths) != tool_evidence:
            raise AutopilotError("validation_tool_changed")
        for name in profile_names:
            command = policy["validation_profiles"][name]
            full_xcode_evidence = None
            current_environment = profile_environment
            if name == FULL_VALIDATION_PROFILE:
                xcode_environment, full_xcode_evidence = (
                    full_xcode_environment()
                )
                current_environment = sanitized_validation_environment(
                    temporary_home,
                    tool_paths,
                    cargo_home=temporary_home / "cargo-home",
                    offline=True,
                    overrides={
                        **xcode_environment,
                        **formatter_environment,
                    },
                )
            profile_command = trusted_profile_command(
                repo_root,
                name,
                cargo_executable=tool_paths["cargo"],
            )
            output = run_command(
                [
                    str(tool_paths["sandbox-exec"]),
                    "-f",
                    str(profile_path),
                    *profile_command,
                ],
                cwd=worktree,
                environment=current_environment,
                inherit_environment=False,
                failure_code=f"validation_profile_{name}_failed",
                timeout_seconds=3600,
            )
            current_head, current_tree = repository_identity(worktree)
            if current_head != head or current_tree != tree:
                raise AutopilotError("validation_repository_changed")
            if validation_authority_identity(repo_root) != authority:
                raise AutopilotError("validation_authority_changed")
            if validation_tool_evidence(tool_paths) != tool_evidence:
                raise AutopilotError("validation_tool_changed")
            trusted_node_provenance = run_command(
                [
                    str(tool_paths["python3"]),
                    str(trusted_audit),
                    "--site",
                    str((worktree / "site").resolve()),
                ],
                cwd=worktree,
                environment=acquisition_environment,
                inherit_environment=False,
                failure_code="validation_node_provenance_failed",
            )
            if validation_authority_identity(repo_root) != authority:
                raise AutopilotError("validation_authority_changed")
            if validation_tool_evidence(tool_paths) != tool_evidence:
                raise AutopilotError("validation_tool_changed")
            if (
                sandbox_evidence(
                    profile,
                    dependency_preparation,
                    tool_paths,
                    formatter_tools,
                )
                != current_sandbox_evidence
            ):
                raise AutopilotError("validation_sandbox_changed")
            toolchain_evidence = {
                "validation_tools": tool_evidence,
                "full_xcode": full_xcode_evidence,
                "sandbox": current_sandbox_evidence,
            }
            results.append(
                {
                    "name": name,
                    "command_sha256": sha256_value(command),
                    "environment_sha256": sha256_value(
                        current_environment
                    ),
                    "exit_code": 0,
                    "output_sha256": sha256_value(
                        {
                            "command": command,
                            "head": head,
                            "tree": tree,
                            "base_head": base_head,
                            "validation_authority": authority,
                            "runtime_preflight": runtime_preflight,
                            "trusted_lock_provenance": (
                                trusted_lock_provenance
                            ),
                            "dependency_preparation": (
                                dependency_preparation
                            ),
                            "trusted_node_provenance": (
                                trusted_node_provenance
                            ),
                            "sandbox_profile_sha256": (
                                current_sandbox_evidence["profile_sha256"]
                            ),
                            "toolchain_evidence": toolchain_evidence,
                            "stdout": output,
                        }
                    ),
                    "toolchain_evidence": toolchain_evidence,
                }
            )
    receipt = {
        "role": role,
        "base_head": base_head,
        "repository_head": head,
        "repository_tree": tree,
        **scope,
        "validation_authority": authority,
        "profiles": results,
        "completed_at": utc_now(),
    }
    validate_receipt_against_policy(
        receipt,
        policy,
        role=role,
        expected_base_head=base_head,
        expected_head=head,
        expected_tree=tree,
    )
    return receipt


def assert_candidate_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    branch: str,
    head_sha: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("candidate_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="candidate_worktree_unavailable",
        )
    ).resolve()
    if root != resolved:
        raise AutopilotError("candidate_worktree_root_mismatch")
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    if common_dir.resolve() != (repo_root / ".git").resolve():
        raise AutopilotError("candidate_worktree_repository_mismatch")
    current_branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    current_head, tree = repository_identity(resolved)
    if current_branch != branch or current_head != head_sha:
        raise AutopilotError("candidate_worktree_identity_mismatch")
    target_origin_urls(resolved, policy["target_repository"])
    return tree


def assert_detached_review_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    head_sha: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("review_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="review_worktree_unavailable",
        )
    ).resolve()
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="review_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="review_worktree_unavailable",
    )
    current_head, tree = repository_identity(resolved)
    if (
        root != resolved
        or common_dir.resolve() != (repo_root / ".git").resolve()
        or branch
        or current_head != head_sha
        or not command_succeeds(
            [
                "git",
                "merge-base",
                "--is-ancestor",
                head_sha,
                f"refs/remotes/origin/{policy['target_branch']}",
            ],
            cwd=repo_root,
            failure_code="review_head_containment_unavailable",
        )
    ):
        raise AutopilotError("review_worktree_identity_mismatch")
    target_origin_urls(resolved, policy["target_repository"])
    return tree


def assert_candidate_commit_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    branch: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("candidate_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="candidate_worktree_unavailable",
        )
    ).resolve()
    current_branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    if (
        root != resolved
        or current_branch != branch
        or SHA_PATTERN.fullmatch(head) is None
        or common_dir.resolve() != (repo_root / ".git").resolve()
    ):
        raise AutopilotError("candidate_worktree_identity_mismatch")
    if not command_succeeds(
        ["git", "diff", "--quiet"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    ):
        raise AutopilotError("candidate_unstaged_changes")
    untracked = run_command(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    )
    if untracked:
        raise AutopilotError("candidate_untracked_changes")
    if command_succeeds(
        ["git", "diff", "--cached", "--quiet"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    ):
        raise AutopilotError("candidate_staged_change_missing")
    target_origin_urls(resolved, policy["target_repository"])
    return head


def referenced_schema_evidence(state: dict[str, Any]) -> set[str]:
    values: set[str] = set()
    local_build = state.get("local_build")
    if isinstance(local_build, dict):
        for key in (
            "stable_schema_evidence_sha256",
            "experimental_schema_evidence_sha256",
        ):
            value = local_build.get(key)
            if is_sha256(value):
                values.add(value)
    for candidate in state.get("candidates", []):
        evidence = candidate.get("schema_evidence")
        if not isinstance(evidence, dict):
            continue
        for value in evidence.values():
            if is_sha256(value):
                values.add(value)
    return values
