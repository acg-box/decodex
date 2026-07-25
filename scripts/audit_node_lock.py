#!/usr/bin/env python3
"""Validate the pinned Node runtime and resolved site dependency provenance."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "site"
REQUIRED_NODE = (22, 12, 0)
EXPECTED_PACKAGE_MANAGER = "npm@11.17.0"
EXPECTED_INSTALL_SCRIPT_PACKAGES = {
    "node_modules/esbuild": {
        "name": "esbuild",
        "version": "0.28.1",
        "integrity": (
            "sha512-HrJrvZv5ayxBzPfwphOoNzkzOIIlifzk0KJrGK2c8R4+"
            "LKpMtpYLQeUdjnwjWv/LZlkH2laZk+4w78pi99D4Vw=="
        ),
    },
    "node_modules/fsevents": {
        "name": "fsevents",
        "version": "2.3.3",
        "integrity": (
            "sha512-5xoDfX+fL7faATnagmWPpbFtwh/R77WmMMqqHGS65C3vv"
            "B0YHrgF+B1YmZ3441tMj5n63k0212XNoJwzlhffQw=="
        ),
    },
}
EXPECTED_INSTALL_METADATA_SHA256 = {
    "node_modules/esbuild": (
        "03dfffc6e78a07dc579b606e9ee98d00fe9f435c0067d504d2f4e770809aa744"
    ),
    "node_modules/fsevents": (
        "92061b4377f5827b78dbbc00fa890d3ec41cfae88f0a323d565cedf9cd991716"
    ),
}
EXPECTED_NATIVE_PACKAGE_SET_SHA256 = (
    "ccb68edecddfb92b32be2e1a8cdf848f7200fcaf7598de7837bc8f3cc2caf951"
)
REGISTRY_PREFIX = "https://registry.npmjs.org/"
INTEGRITY_PATTERN = re.compile(r"sha512-[A-Za-z0-9+/]+={0,2}")
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_PACKAGE_MANIFEST_BYTES = 2 * 1024 * 1024
LIFECYCLE_SCRIPTS = ("preinstall", "install", "postinstall")
EXPECTED_ROOT_SCRIPTS = {
    "build": "astro build",
    "check": "astro check",
    "dev": "astro dev",
    "preview": "astro preview",
    "start": "astro dev",
}
ROOT_PACKAGE_KEYS = {
    "dependencies",
    "engines",
    "name",
    "overrides",
    "packageManager",
    "private",
    "scripts",
    "type",
    "version",
}


class AuditError(RuntimeError):
    """A bounded Node provenance audit failure."""

    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


def fail(code: str) -> None:
    print(json.dumps({"status": "failed", "error_code": code}, sort_keys=True))
    raise SystemExit(1)


def sha256_value(value: Any) -> str:
    encoded = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load_json(
    path: Path,
    *,
    unavailable_code: str = "node_manifest_unavailable",
    invalid_code: str = "node_manifest_invalid",
    maximum_bytes: int = MAX_JSON_BYTES,
) -> dict[str, Any]:
    try:
        if (
            path.is_symlink()
            or not path.is_file()
            or path.stat().st_size > maximum_bytes
        ):
            raise AuditError(unavailable_code)
        value = json.loads(path.read_text(encoding="utf-8"))
    except AuditError:
        raise
    except (OSError, json.JSONDecodeError) as error:
        raise AuditError(unavailable_code) from error
    if not isinstance(value, dict):
        raise AuditError(invalid_code)
    return value


def node_version() -> tuple[int, int, int]:
    try:
        completed = subprocess.run(
            ["node", "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AuditError("node_runtime_unavailable") from error
    match = re.fullmatch(r"v([0-9]+)\.([0-9]+)\.([0-9]+)\s*", completed.stdout)
    if match is None:
        raise AuditError("node_runtime_version_invalid")
    return tuple(int(value) for value in match.groups())


def npm_version() -> str:
    try:
        completed = subprocess.run(
            ["npm", "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AuditError("npm_runtime_unavailable") from error
    version = completed.stdout.strip()
    if version != EXPECTED_PACKAGE_MANAGER.partition("@")[2]:
        raise AuditError("npm_runtime_version_mismatch")
    return version


def package_name_from_lock_path(path: str) -> str:
    parts = Path(path).parts
    if (
        not parts
        or Path(path).is_absolute()
        or ".." in parts
        or "node_modules" not in parts
    ):
        raise AuditError("node_lock_package_invalid")
    index = len(parts) - 1 - tuple(reversed(parts)).index("node_modules")
    package_parts = parts[index + 1 :]
    if len(package_parts) == 1 and not package_parts[0].startswith("@"):
        return package_parts[0]
    if (
        len(package_parts) == 2
        and package_parts[0].startswith("@")
        and len(package_parts[0]) > 1
        and not package_parts[1].startswith("@")
    ):
        return "/".join(package_parts)
    raise AuditError("node_lock_package_invalid")


def package_scripts(
    manifest: dict[str, Any],
    *,
    invalid_code: str = "node_installed_package_scripts_invalid",
) -> dict[str, str]:
    scripts = manifest.get("scripts", {})
    if scripts is None:
        scripts = {}
    if (
        not isinstance(scripts, dict)
        or len(scripts) > 128
        or any(
            not isinstance(key, str)
            or not key
            or len(key) > 128
            or not isinstance(value, str)
            or len(value) > 8192
            or "\x00" in value
            for key, value in scripts.items()
        )
    ):
        raise AuditError(invalid_code)
    return dict(sorted(scripts.items()))


def dependency_map(value: Any) -> dict[str, str]:
    if (
        not isinstance(value, dict)
        or len(value) > 512
        or any(
            not isinstance(name, str)
            or not name
            or len(name) > 256
            or not isinstance(requirement, str)
            or not requirement
            or len(requirement) > 512
            or "\x00" in requirement
            for name, requirement in value.items()
        )
    ):
        raise AuditError("node_root_dependency_contract_invalid")
    return dict(sorted(value.items()))


def validate_root_contract(
    package: dict[str, Any],
    lock_root: dict[str, Any],
    nvmrc: str,
) -> None:
    if (
        set(package) != ROOT_PACKAGE_KEYS
        or package.get("name") != "decodex-site"
        or package.get("version") != "0.0.0"
        or package.get("private") is not True
        or package.get("type") != "module"
        or package.get("packageManager") != EXPECTED_PACKAGE_MANAGER
        or package.get("engines") != {"node": ">=22.12.0"}
        or lock_root.get("name") != package["name"]
        or lock_root.get("version") != package["version"]
        or lock_root.get("engines") != package["engines"]
        or nvmrc != "22.12.0\n"
    ):
        raise AuditError("node_toolchain_contract_invalid")
    if package_scripts(
        package,
        invalid_code="node_root_scripts_invalid",
    ) != EXPECTED_ROOT_SCRIPTS:
        raise AuditError("node_root_scripts_changed")
    if dependency_map(package["dependencies"]) != dependency_map(
        lock_root.get("dependencies")
    ):
        raise AuditError("node_root_dependency_contract_invalid")
    dependency_map(package["overrides"])


def installed_package_metadata(
    site: Path,
    lock_path: str,
    lock_value: dict[str, Any],
) -> dict[str, Any] | None:
    package_root = site / lock_path
    if not package_root.exists():
        if lock_value.get("optional") is True:
            return None
        raise AuditError("node_installed_package_missing")
    if package_root.is_symlink() or not package_root.is_dir():
        raise AuditError("node_installed_package_invalid")
    try:
        package_root.resolve().relative_to((site / "node_modules").resolve())
    except (OSError, ValueError) as error:
        raise AuditError("node_installed_package_invalid") from error
    manifest = load_json(
        package_root / "package.json",
        unavailable_code="node_installed_package_invalid",
        invalid_code="node_installed_package_invalid",
        maximum_bytes=MAX_PACKAGE_MANIFEST_BYTES,
    )
    expected_name = package_name_from_lock_path(lock_path)
    if (
        manifest.get("name") != expected_name
        or manifest.get("version") != lock_value.get("version")
        or manifest.get("os") != lock_value.get("os")
        or manifest.get("cpu") != lock_value.get("cpu")
    ):
        raise AuditError("node_installed_package_identity_invalid")
    scripts = package_scripts(manifest)
    lifecycle = {
        name: scripts[name] for name in LIFECYCLE_SCRIPTS if name in scripts
    }
    if lifecycle and lock_value.get("hasInstallScript") is not True:
        raise AuditError("node_install_script_metadata_mismatch")
    install_metadata = {
        "name": expected_name,
        "version": manifest["version"],
        "scripts": scripts,
        "gypfile": manifest.get("gypfile"),
        "os": manifest.get("os"),
        "cpu": manifest.get("cpu"),
    }
    return {
        "name": expected_name,
        "version": manifest["version"],
        "install_metadata_sha256": sha256_value(install_metadata),
    }


def audit_package_graph(
    site: Path,
    packages: dict[str, Any],
    *,
    inspect_installed: bool = True,
) -> dict[str, Any]:
    install_script_packages: dict[str, dict[str, str]] = {}
    installed_script_metadata: dict[str, str] = {}
    native_packages: list[dict[str, Any]] = []
    installed_packages = 0
    optional_packages_absent = 0
    audited_packages = 0
    for path, value in packages.items():
        if path == "":
            continue
        if (
            not isinstance(path, str)
            or not isinstance(value, dict)
            or value.get("link") is True
            or not path.startswith("node_modules/")
        ):
            raise AuditError("node_lock_package_invalid")
        name = package_name_from_lock_path(path)
        version = value.get("version")
        resolved = value.get("resolved")
        integrity = value.get("integrity")
        if (
            not isinstance(version, str)
            or not version
            or not isinstance(resolved, str)
            or not resolved.startswith(REGISTRY_PREFIX)
            or not isinstance(integrity, str)
            or INTEGRITY_PATTERN.fullmatch(integrity) is None
        ):
            raise AuditError("node_lock_provenance_invalid")
        audited_packages += 1
        if value.get("hasInstallScript") is True:
            install_script_packages[path] = {
                "name": name,
                "version": version,
                "integrity": integrity,
            }
        if value.get("os") is not None or value.get("cpu") is not None:
            native_packages.append(
                {
                    "path": path,
                    "name": name,
                    "version": version,
                    "resolved": resolved,
                    "integrity": integrity,
                    "os": value.get("os"),
                    "cpu": value.get("cpu"),
                }
            )
        if not inspect_installed:
            continue
        installed = installed_package_metadata(site, path, value)
        if installed is None:
            optional_packages_absent += 1
            continue
        installed_packages += 1
        if value.get("hasInstallScript") is True:
            installed_script_metadata[path] = installed[
                "install_metadata_sha256"
            ]
    native_packages.sort(key=lambda item: item["path"])
    return {
        "audited_packages": audited_packages,
        "installed_packages": installed_packages,
        "optional_packages_absent": optional_packages_absent,
        "install_script_packages": install_script_packages,
        "installed_script_metadata": installed_script_metadata,
        "native_or_platform_packages": len(native_packages),
        "native_package_set_sha256": sha256_value(native_packages),
    }


def audit_site(
    site: Path = SITE,
    *,
    inspect_installed: bool = True,
) -> dict[str, Any]:
    if site.is_symlink() or not site.is_dir():
        raise AuditError("node_site_root_invalid")
    package = load_json(site / "package.json")
    lock = load_json(site / "package-lock.json")
    try:
        nvmrc_path = site / ".nvmrc"
        if nvmrc_path.is_symlink():
            raise OSError
        nvmrc = nvmrc_path.read_text(encoding="utf-8")
    except OSError as error:
        raise AuditError("node_toolchain_contract_invalid") from error
    packages = lock.get("packages")
    if lock.get("lockfileVersion") != 3 or not isinstance(packages, dict):
        raise AuditError("node_lock_shape_invalid")
    root = packages.get("")
    if not isinstance(root, dict):
        raise AuditError("node_lock_root_missing")
    validate_root_contract(package, root, nvmrc)
    graph = audit_package_graph(
        site,
        packages,
        inspect_installed=inspect_installed,
    )
    if graph["install_script_packages"] != EXPECTED_INSTALL_SCRIPT_PACKAGES:
        raise AuditError("node_install_script_set_changed")
    if inspect_installed:
        for path, digest in graph["installed_script_metadata"].items():
            if EXPECTED_INSTALL_METADATA_SHA256.get(path) != digest:
                raise AuditError("node_install_script_metadata_changed")
    if graph["native_package_set_sha256"] != EXPECTED_NATIVE_PACKAGE_SET_SHA256:
        raise AuditError("node_native_package_set_changed")
    return graph


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--runtime-only", action="store_true")
    scope.add_argument("--lock-only", action="store_true")
    parser.add_argument("--site", type=Path, default=SITE)
    args = parser.parse_args(argv)
    try:
        runtime = node_version()
        if runtime < REQUIRED_NODE:
            raise AuditError("node_runtime_too_old")
        npm_runtime = npm_version()
        graph = (
            {}
            if args.runtime_only
            else audit_site(
                args.site.resolve(),
                inspect_installed=not args.lock_only,
            )
        )
    except AuditError as error:
        fail(error.code)
    print(
        json.dumps(
            {
                "status": "pass",
                "node_version": ".".join(str(value) for value in runtime),
                "package_manager": f"npm@{npm_runtime}",
                "scope": (
                    "runtime"
                    if args.runtime_only
                    else "lock_graph"
                    if args.lock_only
                    else "dependency_graph"
                ),
                **graph,
                **(
                    {}
                    if args.runtime_only
                    else {
                        "registry": "registry.npmjs.org",
                        "integrity": "sha512",
                    }
                ),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
