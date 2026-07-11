#!/usr/bin/env python3
"""Parse the P1 Git cut and materialize bounded P2 syntax/cfg relations."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import subprocess
import tarfile
import tempfile
from pathlib import Path
from typing import Any

import verify_contract as contract


SOURCE_INVENTORY_PATH = Path("tools/lane-authority-inventory/manifests/source_inventory.json")
CANDIDATE_RECORDS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/candidate_records.json"
)
RELATION_ROOT = Path("tools/lane-authority-inventory/manifests/relations")
RUST_PRELUDE_TYPE_PATHS = {
    "Box": "alloc::boxed::Box",
    "Default": "core::default::Default",
    "Option": "core::option::Option",
    "String": "alloc::string::String",
    "Vec": "alloc::vec::Vec",
    **{
        primitive: f"core::primitive::{primitive}"
        for primitive in (
            "bool",
            "char",
            "f32",
            "f64",
            "i8",
            "i16",
            "i32",
            "i64",
            "i128",
            "isize",
            "str",
            "u8",
            "u16",
            "u32",
            "u64",
            "u128",
            "usize",
        )
    },
}


def canonical_id(domain: str, *parts: str) -> str:
    digest = hashlib.sha256()
    digest.update(domain.encode("utf-8"))
    digest.update(b"\0")
    for part in parts:
        digest.update(part.encode("utf-8"))
        digest.update(b"\0")
    return digest.hexdigest()


def relation(schema_name: str, name: str, records: list[dict[str, Any]]) -> dict[str, Any]:
    return {"records": records, "relation": name, "schema": schema_name}


def write_json(root: Path, path: Path, value: dict[str, Any]) -> None:
    destination = root / path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(contract.canonical_json(value), encoding="utf-8")


def extract_git_tree(root: Path, source_cut: str, destination: Path) -> None:
    archive_bytes = subprocess.run(
        ["git", "archive", "--format=tar", source_cut],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:") as archive:
        archive.extractall(destination, filter="data")
    for directory, directories, files in os.walk(destination, topdown=False):
        for filename in files:
            os.chmod(Path(directory, filename), 0o444)
        for name in directories:
            os.chmod(Path(directory, name), 0o555)
    os.chmod(destination, 0o555)


def config_projection(site: dict[str, Any], kind: str, platform: str) -> dict[str, Any]:
    projection_id = canonical_id(
        "decodex/lane-authority-v2-cfg-projection/1",
        site["site_id"],
        kind,
        platform,
    )
    return {
        "cfg_expression_digest": canonical_id(
            "decodex/lane-authority-v2-cfg-expression/1", platform, kind
        ),
        "disposition": "active_supported",
        "evidence_digest": canonical_id(
            "decodex/lane-authority-v2-cfg-evidence/1", site["site_id"], platform, kind
        ),
        "language": site["language"],
        "platform": platform,
        "projection_id": projection_id,
        "projection_kind": kind,
        "site_id": site["site_id"],
    }


def data_kind(category: str) -> str:
    if "sqlite" in category:
        return "sql"
    if "provider" in category:
        return "provider"
    if "filesystem" in category or "git" in category:
        return "filesystem"
    if "credential" in category or "config" in category:
        return "config"
    if "process" in category or category == "launcher":
        return "environment"
    return "serialization"


def id_set_fields(prefix: str, identifiers: set[str]) -> dict[str, Any]:
    return {
        f"{prefix}_count": len(identifiers),
        f"{prefix}_ids_digest": contract.stable_id_set_digest(
            f"decodex/lane-authority-v2-tool-receipt-{prefix}/1", identifiers
        ),
    }


def _relative_metadata_path(materialized: Path, value: str) -> str:
    try:
        return Path(value).resolve().relative_to(materialized.resolve()).as_posix()
    except ValueError as error:
        raise contract.ContractError(
            f"cargo metadata path escapes the exact source cut: {value}"
        ) from error


def normalize_cargo_metadata_targets(
    materialized: Path,
    metadata: dict[str, Any],
    source_by_path: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    targets: list[dict[str, Any]] = []
    for package in metadata.get("packages", []):
        manifest_path = _relative_metadata_path(materialized, package["manifest_path"])
        if manifest_path not in source_by_path:
            raise contract.ContractError(
                f"cargo package manifest is outside the exact source inventory: {manifest_path}"
            )
        package_targets = package.get("targets", [])
        dependency_crate_names = {
            (dependency.get("rename") or dependency["name"]).replace("-", "_")
            for dependency in package.get("dependencies", [])
        }
        library_crate_names = {
            target["name"].replace("-", "_")
            for target in package_targets
            if set(target["kind"]) & {"lib", "proc-macro"}
        }
        for target in package_targets:
            root_path = _relative_metadata_path(materialized, target["src_path"])
            root_source = source_by_path.get(root_path)
            if root_source is None or root_source["language"] != "rust":
                raise contract.ContractError(
                    f"cargo target root is not an exact-cut Rust source: {root_path}"
                )
            kinds = sorted(set(target["kind"]))
            crate_types = sorted(set(target["crate_types"]))
            target_id = canonical_id(
                "decodex/lane-authority-v2-rust-crate-target/1",
                manifest_path,
                target["name"],
                ",".join(kinds),
                root_path,
            )
            extern_crate_names = dependency_crate_names | {
                "alloc",
                "core",
                "proc_macro",
                "std",
            }
            if not (set(kinds) & {"lib", "proc-macro"}):
                extern_crate_names.update(library_crate_names)
            targets.append(
                {
                    "crate_target_id": target_id,
                    "crate_types": crate_types,
                    "edition": target["edition"],
                    "extern_crate_names": sorted(extern_crate_names),
                    "manifest_path": manifest_path,
                    "package_name": package["name"],
                    "package_version": package["version"],
                    "target_kinds": kinds,
                    "target_name": target["name"],
                    "target_root_path": root_path,
                    "target_root_source_node_id": root_source["source_node_id"],
                }
            )
    targets.sort(key=lambda target: target["crate_target_id"])
    if len({target["crate_target_id"] for target in targets}) != len(targets):
        raise contract.ContractError("cargo metadata produced duplicate crate target ids")
    return targets


def exact_cut_cargo_targets(
    materialized: Path, source_inventory: dict[str, Any]
) -> tuple[list[dict[str, Any]], str]:
    current_sources = {
        source["path"]: source
        for source in source_inventory["records"]
        if source["status"] == "current"
    }
    manifests = sorted(
        path for path in current_sources if path.endswith("Cargo.toml")
    )
    if "Cargo.toml" in manifests:
        manifests.remove("Cargo.toml")
        manifests.insert(0, "Cargo.toml")

    covered_manifests: set[str] = set()
    targets_by_id: dict[str, dict[str, Any]] = {}
    cargo_version = subprocess.run(
        ["cargo", "--version", "--verbose"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    ).stdout.strip()
    for manifest in manifests:
        if manifest in covered_manifests:
            continue
        metadata = json.loads(
            subprocess.run(
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--no-deps",
                    "--format-version",
                    "1",
                    "--manifest-path",
                    str(materialized / manifest),
                ],
                cwd=materialized,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            ).stdout
        )
        normalized = normalize_cargo_metadata_targets(
            materialized, metadata, current_sources
        )
        metadata_manifests = {target["manifest_path"] for target in normalized}
        covered_manifests.update(metadata_manifests)
        for target in normalized:
            existing = targets_by_id.get(target["crate_target_id"])
            if existing is not None and existing != target:
                raise contract.ContractError(
                    f"cargo target identity has conflicting metadata: {target['crate_target_id']}"
                )
            targets_by_id[target["crate_target_id"]] = target

    package_manifests = {
        path for path in manifests if path != "Cargo.toml"
    }
    missing_manifests = package_manifests - covered_manifests
    if missing_manifests:
        raise contract.ContractError(
            f"cargo metadata did not cover package manifests: {sorted(missing_manifests)}"
        )
    targets = sorted(targets_by_id.values(), key=lambda target: target["crate_target_id"])
    if not targets:
        raise contract.ContractError("cargo metadata produced no exact-cut targets")
    receipt_digest = hashlib.sha256(
        contract.canonical_json(
            {"cargo_version": cargo_version, "targets": targets}
        ).encode("utf-8")
    ).hexdigest()
    return targets, receipt_digest


def materialize_rust_module_scopes(
    parsed: dict[str, Any],
    cargo_targets: list[dict[str, Any]],
    cfg_projections: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    sources_by_id = {
        source["source_node_id"]: source
        for source in parsed["source_nodes"]
        if source["status"] == "current"
    }
    sources_by_path = {source["path"]: source for source in sources_by_id.values()}
    scope_facts_by_source: dict[str, dict[str, dict[str, Any]]] = {}
    for fact in parsed["rust_scope_facts"]:
        scope_facts_by_source.setdefault(fact["source_node_id"], {})[
            fact["syntax_site_id"]
        ] = fact
    declarations_by_source: dict[str, list[dict[str, Any]]] = {}
    for fact in parsed["rust_module_declaration_facts"]:
        declarations_by_source.setdefault(fact["source_node_id"], []).append(fact)

    syntax_by_id = {site["site_id"]: site for site in parsed["syntax_sites"]}
    projections_by_source: dict[str, list[str]] = {}
    for projection in cfg_projections:
        source_id = syntax_by_id[projection["site_id"]]["source_node_id"]
        projections_by_source.setdefault(source_id, []).append(
            projection["projection_id"]
        )
    for projection_ids in projections_by_source.values():
        projection_ids.sort()

    records_by_id: dict[str, dict[str, Any]] = {}
    visited_contexts: set[tuple[str, str, str]] = set()

    def scope_id(
        target_id: str, source_id: str, syntax_site_id: str, module_path: str
    ) -> str:
        return canonical_id(
            "decodex/lane-authority-v2-rust-module-scope/1",
            target_id,
            source_id,
            syntax_site_id,
            module_path,
        )

    def insert(record: dict[str, Any]) -> None:
        existing = records_by_id.get(record["scope_id"])
        if existing is not None and existing != record:
            raise contract.ContractError(
                f"Rust module scope identity has conflicting facts: {record['scope_id']}"
            )
        records_by_id[record["scope_id"]] = record

    def add_source_context(
        target: dict[str, Any],
        source: dict[str, Any],
        module_path: str,
        module_directory: Path,
        parent_scope_id: str | None,
        declaration_syntax_site_id: str | None,
        root_kind: str,
    ) -> None:
        context_key = (
            target["crate_target_id"],
            source["source_node_id"],
            module_path,
        )
        if context_key in visited_contexts:
            return
        visited_contexts.add(context_key)

        facts = scope_facts_by_source.get(source["source_node_id"], {})
        roots = [fact for fact in facts.values() if fact["scope_kind"] == "source_file"]
        if len(roots) != 1:
            raise contract.ContractError(
                f"Rust source must have one parser root scope: {source['path']}"
            )
        root_fact = roots[0]
        root_scope_id = scope_id(
            target["crate_target_id"],
            source["source_node_id"],
            root_fact["syntax_site_id"],
            module_path,
        )
        root_record = {
            "byte_end": root_fact["byte_end"],
            "byte_start": root_fact["byte_start"],
            "canonical_module_path": module_path,
            "cfg_projection_ids": projections_by_source.get(
                source["source_node_id"], []
            ),
            "crate_target_id": target["crate_target_id"],
            "declaration_syntax_site_id": declaration_syntax_site_id,
            "parent_scope_id": parent_scope_id,
            "scope_id": root_scope_id,
            "scope_kind": root_kind,
            "scope_syntax_site_id": root_fact["syntax_site_id"],
            "source_node_id": source["source_node_id"],
            "target_extern_crate_names": (
                target["extern_crate_names"] if root_kind == "crate_root" else None
            ),
            "target_manifest_path": (
                target["manifest_path"] if root_kind == "crate_root" else None
            ),
            "target_kinds": target["target_kinds"] if root_kind == "crate_root" else None,
            "target_name": target["target_name"] if root_kind == "crate_root" else None,
            "target_root_path": (
                target["target_root_path"] if root_kind == "crate_root" else None
            ),
            "target_root_source_node_id": (
                target["target_root_source_node_id"]
                if root_kind == "crate_root"
                else None
            ),
        }
        insert(root_record)

        scope_records_by_syntax = {root_fact["syntax_site_id"]: root_record}
        module_directories_by_syntax = {
            root_fact["syntax_site_id"]: module_directory
        }
        inline_declarations = {
            declaration["body_scope_syntax_site_id"]: declaration
            for declaration in declarations_by_source.get(source["source_node_id"], [])
            if declaration["body_scope_syntax_site_id"] is not None
        }
        pending = [
            fact for fact in facts.values() if fact["syntax_site_id"] != root_fact["syntax_site_id"]
        ]
        while pending:
            progressed = False
            for fact in list(pending):
                parent = scope_records_by_syntax.get(
                    fact["parent_scope_syntax_site_id"]
                )
                if parent is None:
                    continue
                canonical_path = parent["canonical_module_path"]
                scope_kind = fact["scope_kind"]
                declaration_id = None
                child_module_directory = module_directories_by_syntax[
                    fact["parent_scope_syntax_site_id"]
                ]
                if scope_kind == "inline_module":
                    declaration = inline_declarations.get(fact["syntax_site_id"])
                    if declaration is None:
                        raise contract.ContractError(
                            "inline Rust module scope has no declaration fact"
                        )
                    canonical_path = (
                        f"{canonical_path}::{declaration['module_name']}"
                    )
                    child_module_directory = (
                        child_module_directory / declaration["module_name"]
                    )
                    declaration_id = declaration["declaration_syntax_site_id"]
                child_id = scope_id(
                    target["crate_target_id"],
                    source["source_node_id"],
                    fact["syntax_site_id"],
                    canonical_path,
                )
                record = {
                    "byte_end": fact["byte_end"],
                    "byte_start": fact["byte_start"],
                    "canonical_module_path": canonical_path,
                    "cfg_projection_ids": projections_by_source.get(
                        source["source_node_id"], []
                    ),
                    "crate_target_id": target["crate_target_id"],
                    "declaration_syntax_site_id": declaration_id,
                    "parent_scope_id": parent["scope_id"],
                    "scope_id": child_id,
                    "scope_kind": scope_kind,
                    "scope_syntax_site_id": fact["syntax_site_id"],
                    "source_node_id": source["source_node_id"],
                    "target_extern_crate_names": None,
                    "target_manifest_path": None,
                    "target_kinds": None,
                    "target_name": None,
                    "target_root_path": None,
                    "target_root_source_node_id": None,
                }
                insert(record)
                scope_records_by_syntax[fact["syntax_site_id"]] = record
                module_directories_by_syntax[fact["syntax_site_id"]] = (
                    child_module_directory
                )
                pending.remove(fact)
                progressed = True
            if not progressed:
                raise contract.ContractError(
                    f"Rust scope parent chain is disconnected: {source['path']}"
                )

        for declaration in declarations_by_source.get(source["source_node_id"], []):
            if declaration["body_scope_syntax_site_id"] is not None:
                continue
            lexical_scope = scope_records_by_syntax.get(
                declaration["lexical_scope_syntax_site_id"]
            )
            if lexical_scope is None or lexical_scope["scope_kind"] == "block":
                continue
            parent_directory = module_directories_by_syntax[
                declaration["lexical_scope_syntax_site_id"]
            ]
            name = declaration["module_name"]
            candidates = [
                (parent_directory / f"{name}.rs").as_posix(),
                (parent_directory / name / "mod.rs").as_posix(),
            ]
            matches = [sources_by_path[path] for path in candidates if path in sources_by_path]
            if len(matches) != 1 or matches[0]["language"] != "rust":
                continue
            add_source_context(
                target,
                matches[0],
                f"{lexical_scope['canonical_module_path']}::{name}",
                parent_directory / name,
                lexical_scope["scope_id"],
                declaration["declaration_syntax_site_id"],
                "file_module",
            )

    for target in cargo_targets:
        root_source = sources_by_id[target["target_root_source_node_id"]]
        add_source_context(
            target,
            root_source,
            f"target::{target['crate_target_id']}",
            Path(target["target_root_path"]).parent,
            None,
            None,
            "crate_root",
        )

    return sorted(records_by_id.values(), key=lambda record: record["scope_id"])


def materialize_rust_name_bindings(
    parsed: dict[str, Any],
    rust_module_scopes: list[dict[str, Any]],
    symbol_sites: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    scopes_by_lexical_site: dict[
        tuple[str, str], list[dict[str, Any]]
    ] = {}
    module_targets: dict[tuple[str, str, str], list[str]] = {}
    for scope in rust_module_scopes:
        scopes_by_lexical_site.setdefault(
            (scope["source_node_id"], scope["scope_syntax_site_id"]), []
        ).append(scope)
        if scope["declaration_syntax_site_id"] is not None:
            module_targets.setdefault(
                (
                    scope["crate_target_id"],
                    scope["parent_scope_id"],
                    scope["declaration_syntax_site_id"],
                ),
                [],
            ).append(scope["scope_id"])

    type_symbols_by_syntax: dict[tuple[str, str], list[str]] = {}
    for symbol in symbol_sites:
        if symbol["language"] == "rust" and symbol["role"] == "declaration":
            type_symbols_by_syntax.setdefault(
                (symbol["syntax_site_id"], symbol["signature"]), []
            ).append(symbol["site_id"])

    bindings_by_id: dict[str, dict[str, Any]] = {}
    for fact in parsed["rust_name_binding_facts"]:
        lexical_scopes = scopes_by_lexical_site.get(
            (
                fact["source_node_id"],
                fact["lexical_scope_syntax_site_id"],
            ),
            [],
        )
        for lexical_scope in lexical_scopes:
            target_scope_id = None
            target_symbol_site_id = None
            resolution = "unresolved"
            reason_code = "rust_binding_path_resolution_pending"
            if fact["visibility"] == "unsupported":
                resolution = "unsupported"
                reason_code = "rust_binding_visibility_unsupported"
            elif fact["binding_kind"] == "glob":
                resolution = "unsupported"
                reason_code = "rust_binding_glob_unsupported"
            elif fact["binding_kind"] == "module":
                targets = module_targets.get(
                    (
                        lexical_scope["crate_target_id"],
                        lexical_scope["scope_id"],
                        fact["syntax_site_id"],
                    ),
                    [],
                )
                if len(targets) == 1:
                    target_scope_id = targets[0]
                    resolution = "resolved"
                    reason_code = "rust_binding_exact_module_declaration"
                elif len(targets) > 1:
                    resolution = "ambiguous"
                    reason_code = "rust_binding_ambiguous_module_declaration"
                else:
                    reason_code = "rust_binding_module_target_unresolved"
            elif fact["binding_kind"] == "type_declaration":
                targets = type_symbols_by_syntax.get(
                    (fact["syntax_site_id"], fact["local_name"]), []
                )
                if len(targets) == 1:
                    target_symbol_site_id = targets[0]
                    resolution = "resolved"
                    reason_code = "rust_binding_exact_type_declaration"
                elif len(targets) > 1:
                    resolution = "ambiguous"
                    reason_code = "rust_binding_ambiguous_type_declaration"
                else:
                    reason_code = "rust_binding_type_symbol_unresolved"

            binding_id = canonical_id(
                "decodex/lane-authority-v2-rust-name-binding/1",
                lexical_scope["crate_target_id"],
                lexical_scope["scope_id"],
                fact["syntax_site_id"],
                fact["binding_kind"],
                "type",
                fact["local_name"],
                fact["surface_target_path"] or "",
            )
            record = {
                "binding_id": binding_id,
                "binding_kind": fact["binding_kind"],
                "crate_target_id": lexical_scope["crate_target_id"],
                "local_name": fact["local_name"],
                "namespace": "type",
                "reason_code": reason_code,
                "resolution": resolution,
                "scope_id": lexical_scope["scope_id"],
                "source_node_id": fact["source_node_id"],
                "surface_target_path": fact["surface_target_path"],
                "syntax_site_id": fact["syntax_site_id"],
                "target_scope_id": target_scope_id,
                "target_symbol_site_id": target_symbol_site_id,
                "visibility": fact["visibility"],
                "visibility_path": fact["visibility_path"],
            }
            existing = bindings_by_id.get(binding_id)
            if existing is not None and existing != record:
                raise contract.ContractError(
                    f"Rust name binding identity has conflicting facts: {binding_id}"
                )
            bindings_by_id[binding_id] = record

    bindings_by_identity: dict[tuple[str, str, str], list[dict[str, Any]]] = {}
    for binding in bindings_by_id.values():
        if binding["local_name"] == "_" or binding["binding_kind"] not in {
            "module",
            "type_declaration",
        }:
            continue
        bindings_by_identity.setdefault(
            (
                binding["crate_target_id"],
                binding["scope_id"],
                binding["local_name"],
            ),
            [],
        ).append(binding)
    for bindings in bindings_by_identity.values():
        if len(bindings) <= 1:
            continue
        for binding in bindings:
            binding["resolution"] = "ambiguous"
            binding["reason_code"] = "rust_binding_same_scope_ambiguous"
            binding["target_scope_id"] = None
            binding["target_symbol_site_id"] = None

    return sorted(bindings_by_id.values(), key=lambda binding: binding["binding_id"])


def resolve_rust_binding_paths(
    rust_module_scopes: list[dict[str, Any]],
    rust_name_bindings: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    scopes = {scope["scope_id"]: scope for scope in rust_module_scopes}
    bindings = {
        binding["binding_id"]: binding for binding in rust_name_bindings
    }
    bindings_by_scope_name: dict[tuple[str, str, str], list[str]] = {}
    for binding in rust_name_bindings:
        bindings_by_scope_name.setdefault(
            (
                binding["crate_target_id"],
                binding["scope_id"],
                binding["local_name"],
            ),
            [],
        ).append(binding["binding_id"])
    roots = {
        scope["crate_target_id"]: scope["scope_id"]
        for scope in rust_module_scopes
        if scope["scope_kind"] == "crate_root"
    }
    extern_crates = {
        target_id: set(scopes[root_id]["target_extern_crate_names"])
        for target_id, root_id in roots.items()
    }

    def module_scope(scope_id: str) -> dict[str, Any]:
        scope = scopes[scope_id]
        while scope["scope_kind"] == "block":
            scope = scopes[scope["parent_scope_id"]]
        return scope

    def parent_module(scope_id: str) -> dict[str, Any] | None:
        scope = module_scope(scope_id)
        parent_id = scope["parent_scope_id"]
        if parent_id is None:
            return None
        return module_scope(parent_id)

    def visible_from(binding: dict[str, Any], accessing_scope_id: str) -> bool:
        visibility = binding["visibility"]
        if visibility in {"public", "crate"}:
            return True
        declaring_module = module_scope(binding["scope_id"])
        accessing_path = module_scope(accessing_scope_id)["canonical_module_path"]
        allowed_path = declaring_module["canonical_module_path"]
        if visibility == "super":
            parent = parent_module(declaring_module["scope_id"])
            if parent is None:
                return False
            allowed_path = parent["canonical_module_path"]
        elif visibility == "in":
            surface = binding["visibility_path"] or ""
            segments = [segment for segment in surface.split("::") if segment]
            if not segments:
                return False
            if segments[0] == "crate":
                allowed_path = scopes[roots[binding["crate_target_id"]]][
                    "canonical_module_path"
                ]
                segments = segments[1:]
            elif segments[0] == "self":
                segments = segments[1:]
            elif segments[0] == "super":
                cursor = declaring_module
                while segments and segments[0] == "super":
                    parent = parent_module(cursor["scope_id"])
                    if parent is None:
                        return False
                    cursor = parent
                    segments = segments[1:]
                allowed_path = cursor["canonical_module_path"]
            else:
                return False
            if segments:
                allowed_path = f"{allowed_path}::{'::'.join(segments)}"
        elif visibility != "private":
            return False
        return accessing_path == allowed_path or accessing_path.startswith(
            f"{allowed_path}::"
        )

    def lookup(
        target_id: str,
        scope_id: str,
        name: str,
        *,
        accessing_scope_id: str,
        lexical: bool,
        require_module: bool = False,
        excluded_binding_id: str | None = None,
    ) -> tuple[str, str | None]:
        current_id: str | None = scope_id
        while current_id is not None:
            all_candidate_ids = [
                binding_id
                for binding_id in bindings_by_scope_name.get(
                    (target_id, current_id, name), []
                )
                if binding_id != excluded_binding_id
                and bindings[binding_id]["local_name"] != "_"
            ]
            candidate_ids = [
                binding_id
                for binding_id in all_candidate_ids
                if visible_from(bindings[binding_id], accessing_scope_id)
            ]
            if all_candidate_ids and not candidate_ids:
                return "inaccessible", None
            declarations = [
                binding_id
                for binding_id in candidate_ids
                if bindings[binding_id]["binding_kind"] in {
                    "module",
                    "type_declaration",
                }
            ]
            if require_module:
                module_ids = [
                    binding_id
                    for binding_id in candidate_ids
                    if bindings[binding_id]["binding_kind"] == "module"
                ]
                if module_ids:
                    candidate_ids = module_ids
            elif declarations:
                candidate_ids = declarations
            if len(candidate_ids) > 1:
                if not declarations:
                    return "unresolved", None
                return "ambiguous", None
            if len(candidate_ids) == 1:
                candidate = bindings[candidate_ids[0]]
                if candidate["resolution"] == "ambiguous":
                    return "ambiguous", None
                return "found", candidate_ids[0]
            if not lexical:
                break
            if scopes[current_id]["scope_kind"] != "block":
                break
            current_id = scopes[current_id]["parent_scope_id"]
        return "missing", None

    cache: dict[str, dict[str, Any]] = {}

    def terminal_for_declaration(binding: dict[str, Any]) -> dict[str, Any]:
        scope = scopes[binding["scope_id"]]
        if binding["binding_kind"] == "module":
            target_scope_id = binding["target_scope_id"]
            if target_scope_id is None:
                return {"status": "unresolved", "binding_ids": [binding["binding_id"]]}
            return {
                "binding_ids": [binding["binding_id"]],
                "canonical_module_scope_id": target_scope_id,
                "canonical_path": scopes[target_scope_id]["canonical_module_path"],
                "canonical_type_definition_site_id": None,
                "status": "resolved_local_module",
            }
        target_symbol_id = binding["target_symbol_site_id"]
        if target_symbol_id is None:
            return {"status": "unresolved", "binding_ids": [binding["binding_id"]]}
        return {
            "binding_ids": [binding["binding_id"]],
            "canonical_module_scope_id": None,
            "canonical_path": f"{scope['canonical_module_path']}::{binding['local_name']}",
            "canonical_type_definition_site_id": target_symbol_id,
            "status": "resolved_local_type",
        }

    def follow(binding_id: str, stack: tuple[str, ...]) -> dict[str, Any]:
        if binding_id in cache:
            return cache[binding_id]
        if binding_id in stack:
            return {"status": "cycle", "binding_ids": [binding_id]}
        binding = bindings[binding_id]
        if binding["resolution"] == "ambiguous":
            return {"status": "ambiguous", "binding_ids": [binding_id]}
        if binding["visibility"] == "unsupported" or binding["binding_kind"] == "glob":
            return {"status": "unsupported", "binding_ids": [binding_id]}
        if binding["binding_kind"] in {"module", "type_declaration"}:
            result = terminal_for_declaration(binding)
            cache[binding_id] = result
            return result
        result = resolve_surface(binding, stack + (binding_id,))
        if result["status"] != "cycle":
            cache[binding_id] = result
        return result

    def resolve_surface(binding: dict[str, Any], stack: tuple[str, ...]) -> dict[str, Any]:
        surface = binding["surface_target_path"] or ""
        segments = [segment for segment in surface.split("::") if segment]
        if not segments:
            return {"status": "unsupported", "binding_ids": [binding["binding_id"]]}
        target_id = binding["crate_target_id"]
        current_scope = module_scope(binding["scope_id"])
        index = 0
        terminal: dict[str, Any] | None = None
        path_binding_ids = [binding["binding_id"]]
        if segments[0] == "crate":
            current_scope = scopes[roots[target_id]]
            index = 1
        elif segments[0] == "self":
            index = 1
        elif segments[0] == "super":
            while index < len(segments) and segments[index] == "super":
                parent = parent_module(current_scope["scope_id"])
                if parent is None:
                    return {"status": "unsupported", "binding_ids": [binding["binding_id"]]}
                current_scope = parent
                index += 1
        else:
            state, found = lookup(
                target_id,
                binding["scope_id"],
                segments[0],
                accessing_scope_id=binding["scope_id"],
                lexical=True,
                require_module=len(segments) > 1,
                excluded_binding_id=binding["binding_id"],
            )
            if state == "ambiguous":
                return {"status": "ambiguous", "binding_ids": path_binding_ids}
            if state == "unresolved":
                return {"status": "unresolved", "binding_ids": path_binding_ids}
            if state == "inaccessible":
                return {"status": "inaccessible", "binding_ids": path_binding_ids}
            if state == "missing":
                if segments[0] not in extern_crates[target_id]:
                    return {
                        "status": "unresolved",
                        "binding_ids": [binding["binding_id"]],
                    }
                return {
                    "binding_ids": [binding["binding_id"]],
                    "canonical_module_scope_id": None,
                    "canonical_path": surface,
                    "canonical_type_definition_site_id": None,
                    "status": "external",
                }
            terminal = follow(found, stack)
            path_binding_ids.extend(terminal["binding_ids"])
            index = 1

        while index < len(segments):
            if terminal is not None:
                if terminal["status"] == "external":
                    return {
                        **terminal,
                        "binding_ids": path_binding_ids,
                        "canonical_path": (
                            f"{terminal['canonical_path']}::"
                            f"{'::'.join(segments[index:])}"
                        ),
                    }
                if terminal["status"] != "resolved_local_module":
                    return {**terminal, "binding_ids": path_binding_ids}
                current_scope = scopes[terminal["canonical_module_scope_id"]]
            state, found = lookup(
                target_id,
                current_scope["scope_id"],
                segments[index],
                accessing_scope_id=binding["scope_id"],
                lexical=False,
                require_module=index < len(segments) - 1,
            )
            if state == "ambiguous":
                return {"status": "ambiguous", "binding_ids": path_binding_ids}
            if state == "unresolved":
                return {"status": "unresolved", "binding_ids": path_binding_ids}
            if state == "inaccessible":
                return {"status": "inaccessible", "binding_ids": path_binding_ids}
            if state == "missing":
                return {"status": "unresolved", "binding_ids": path_binding_ids}
            terminal = follow(found, stack)
            path_binding_ids.extend(terminal["binding_ids"])
            index += 1
        if terminal is None:
            return {
                "binding_ids": path_binding_ids,
                "canonical_module_scope_id": current_scope["scope_id"],
                "canonical_path": current_scope["canonical_module_path"],
                "canonical_type_definition_site_id": None,
                "status": "resolved_local_module",
            }
        return {**terminal, "binding_ids": path_binding_ids}

    resolutions: list[dict[str, Any]] = []
    reason_by_status = {
        "ambiguous": "rust_path_ambiguous_binding",
        "cycle": "rust_path_reexport_cycle",
        "external": "rust_path_external_crate",
        "inaccessible": "rust_path_visibility_denied",
        "resolved_local_module": "rust_path_unique_local_module",
        "resolved_local_type": "rust_path_unique_local_type",
        "unresolved": "rust_path_target_unresolved",
        "unsupported": "rust_path_unsupported_construct",
    }
    for binding in rust_name_bindings:
        if binding["binding_kind"] not in {"use", "reexport", "glob"}:
            continue
        result = follow(binding["binding_id"], ())
        status = result["status"]
        resolution_id = canonical_id(
            "decodex/lane-authority-v2-rust-path-resolution/1",
            binding["binding_id"],
            binding["scope_id"],
            binding["namespace"],
            binding["surface_target_path"] or "",
            status,
            result.get("canonical_path") or "",
        )
        record = {
            "binding_ids": result["binding_ids"],
            "canonical_module_scope_id": result.get("canonical_module_scope_id"),
            "canonical_path": result.get("canonical_path"),
            "canonical_type_definition_site_id": result.get(
                "canonical_type_definition_site_id"
            ),
            "crate_target_id": binding["crate_target_id"],
            "lexical_scope_id": binding["scope_id"],
            "namespace": binding["namespace"],
            "purpose": "binding_target",
            "reason_code": reason_by_status[status],
            "resolution_id": resolution_id,
            "source_binding_id": binding["binding_id"],
            "source_symbol_site_id": None,
            "status": status,
            "surface_path": binding["surface_target_path"],
        }
        record["resolution_digest"] = hashlib.sha256(
            contract.canonical_json(record).encode("utf-8")
        ).hexdigest()
        resolutions.append(record)
        if status in {"resolved_local_module", "resolved_local_type"}:
            binding["resolution"] = "resolved"
            binding["reason_code"] = "rust_binding_exact_path_resolution"
            binding["target_scope_id"] = result.get("canonical_module_scope_id")
            binding["target_symbol_site_id"] = result.get(
                "canonical_type_definition_site_id"
            )
        elif status == "external":
            binding["resolution"] = "external"
            binding["reason_code"] = "rust_binding_external_path"
        elif status in {"ambiguous", "cycle"}:
            binding["resolution"] = "ambiguous"
            binding["reason_code"] = "rust_binding_path_ambiguous"
        elif status == "unsupported":
            binding["resolution"] = "unsupported"
            binding["reason_code"] = "rust_binding_path_unsupported"
        elif status == "inaccessible":
            binding["resolution"] = "unresolved"
            binding["reason_code"] = "rust_binding_path_inaccessible"
        else:
            binding["resolution"] = "unresolved"
            binding["reason_code"] = "rust_binding_path_target_unresolved"
        if binding["resolution"] != "resolved":
            binding["target_scope_id"] = None
            binding["target_symbol_site_id"] = None

    resolutions.sort(key=lambda resolution: resolution["resolution_id"])
    return resolutions


def materialize_rust_receiver_type_resolutions(
    syntax_sites: list[dict[str, Any]],
    symbol_sites: list[dict[str, Any]],
    rust_module_scopes: list[dict[str, Any]],
    rust_name_bindings: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    syntax_by_id = {site["site_id"]: site for site in syntax_sites}
    scopes_by_source: dict[str, list[dict[str, Any]]] = {}
    for scope in rust_module_scopes:
        scopes_by_source.setdefault(scope["source_node_id"], []).append(scope)

    synthetic_by_id: dict[str, dict[str, Any]] = {}
    receiver_by_binding_id: dict[str, dict[str, Any]] = {}
    for symbol in symbol_sites:
        surface = symbol["receiver_type_signature"]
        if symbol["language"] != "rust" or symbol["role"] != "call_target" or surface is None:
            continue
        query_path = RUST_PRELUDE_TYPE_PATHS.get(surface, surface)
        if symbol["receiver_type_kind"] == "implicit_self":
            if symbol["owner_signature"] is None:
                raise contract.ContractError(
                    f"Rust Self receiver has no enclosing owner: {symbol['site_id']}"
                )
            query_path = symbol["owner_signature"]
        syntax = syntax_by_id[symbol["syntax_site_id"]]
        candidates = [
            scope
            for scope in scopes_by_source.get(syntax["source_node_id"], [])
            if scope["byte_start"] <= syntax["byte_start"]
            and syntax["byte_end"] <= scope["byte_end"]
        ]
        by_target: dict[str, list[dict[str, Any]]] = {}
        for scope in candidates:
            by_target.setdefault(scope["crate_target_id"], []).append(scope)
        if not by_target:
            raise contract.ContractError(
                f"Rust receiver has no Cargo-target lexical scope: {symbol['site_id']}"
            )
        for target_id, target_scopes in by_target.items():
            target_scopes.sort(
                key=lambda scope: (
                    scope["byte_end"] - scope["byte_start"],
                    -scope["byte_start"],
                    scope["scope_id"],
                )
            )
            lexical_scope = target_scopes[0]
            best_identity = (
                lexical_scope["byte_end"] - lexical_scope["byte_start"],
                lexical_scope["byte_start"],
            )
            if (
                sum(
                    (
                        scope["byte_end"] - scope["byte_start"],
                        scope["byte_start"],
                    )
                    == best_identity
                    for scope in target_scopes
                )
                != 1
            ):
                raise contract.ContractError(
                    f"Rust receiver lexical scope is ambiguous: {symbol['site_id']}"
                )
            query_binding_id = canonical_id(
                "decodex/lane-authority-v2-rust-receiver-type-query/1",
                symbol["site_id"],
                target_id,
                lexical_scope["scope_id"],
                surface,
                query_path,
            )
            query = {
                "binding_id": query_binding_id,
                "binding_kind": "use",
                "crate_target_id": target_id,
                "local_name": f"__receiver_type_{symbol['site_id']}",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": lexical_scope["scope_id"],
                "source_node_id": syntax["source_node_id"],
                "surface_target_path": query_path,
                "syntax_site_id": symbol["syntax_site_id"],
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
                "visibility_path": None,
            }
            synthetic_by_id[query_binding_id] = query
            receiver_by_binding_id[query_binding_id] = symbol

    working_bindings = [dict(binding) for binding in rust_name_bindings]
    working_bindings.extend(dict(binding) for binding in synthetic_by_id.values())
    path_resolutions = resolve_rust_binding_paths(
        rust_module_scopes, working_bindings
    )
    path_by_source = {
        resolution["source_binding_id"]: resolution
        for resolution in path_resolutions
        if resolution["source_binding_id"] in synthetic_by_id
    }
    if set(path_by_source) != set(synthetic_by_id):
        raise contract.ContractError("Rust receiver type path coverage is incomplete")

    records: list[dict[str, Any]] = []
    for query_binding_id, query in synthetic_by_id.items():
        path = path_by_source[query_binding_id]
        symbol = receiver_by_binding_id[query_binding_id]
        status = path["status"]
        reason_code = path["reason_code"]
        if symbol["receiver_type_kind"] == "generic_parameter":
            if status != "unresolved":
                raise contract.ContractError(
                    f"Rust generic receiver unexpectedly resolved: {symbol['site_id']}"
                )
            status = "generic_parameter"
            reason_code = "rust_receiver_generic_parameter"
        resolution_id = canonical_id(
            "decodex/lane-authority-v2-rust-receiver-type-resolution/1",
            symbol["site_id"],
            query["crate_target_id"],
            query["scope_id"],
            symbol["receiver_type_signature"],
            query["surface_target_path"],
            path["status"],
            status,
            path.get("canonical_path") or "",
        )
        record = {
            "binding_ids": path["binding_ids"][1:],
            "canonical_module_scope_id": path.get("canonical_module_scope_id"),
            "canonical_path": path.get("canonical_path"),
            "canonical_type_definition_site_id": path.get(
                "canonical_type_definition_site_id"
            ),
            "crate_target_id": query["crate_target_id"],
            "lexical_scope_id": query["scope_id"],
            "namespace": "type",
            "path_status": path["status"],
            "purpose": "receiver_type",
            "query_path": query["surface_target_path"],
            "query_binding_id": query_binding_id,
            "reason_code": reason_code,
            "receiver_type_evidence": symbol["receiver_type_evidence"],
            "receiver_type_kind": symbol["receiver_type_kind"],
            "resolution_id": resolution_id,
            "source_symbol_site_id": symbol["site_id"],
            "source_syntax_site_id": symbol["syntax_site_id"],
            "status": status,
            "surface_path": symbol["receiver_type_signature"],
        }
        record["resolution_digest"] = hashlib.sha256(
            contract.canonical_json(record).encode("utf-8")
        ).hexdigest()
        records.append(record)
    records.sort(key=lambda record: record["resolution_id"])
    return records


def materialize_rust_method_owner_resolutions(
    syntax_sites: list[dict[str, Any]],
    symbol_sites: list[dict[str, Any]],
    rust_module_scopes: list[dict[str, Any]],
    rust_name_bindings: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    syntax_by_id = {site["site_id"]: site for site in syntax_sites}
    scopes_by_source: dict[str, list[dict[str, Any]]] = {}
    for scope in rust_module_scopes:
        scopes_by_source.setdefault(scope["source_node_id"], []).append(scope)

    synthetic_by_id: dict[str, dict[str, Any]] = {}
    owner_by_binding_id: dict[str, dict[str, Any]] = {}
    for symbol in symbol_sites:
        surface = symbol["owner_signature"]
        if symbol["language"] != "rust" or symbol["role"] != "declaration" or surface is None:
            continue
        query_path = RUST_PRELUDE_TYPE_PATHS.get(surface, surface)
        syntax = syntax_by_id[symbol["syntax_site_id"]]
        candidates = [
            scope
            for scope in scopes_by_source.get(syntax["source_node_id"], [])
            if scope["byte_start"] <= syntax["byte_start"]
            and syntax["byte_end"] <= scope["byte_end"]
        ]
        by_target: dict[str, list[dict[str, Any]]] = {}
        for scope in candidates:
            by_target.setdefault(scope["crate_target_id"], []).append(scope)
        if not by_target:
            raise contract.ContractError(
                f"Rust method owner has no Cargo-target lexical scope: {symbol['site_id']}"
            )
        for target_id, target_scopes in by_target.items():
            target_scopes.sort(
                key=lambda scope: (
                    scope["byte_end"] - scope["byte_start"],
                    -scope["byte_start"],
                    scope["scope_id"],
                )
            )
            lexical_scope = target_scopes[0]
            best_identity = (
                lexical_scope["byte_end"] - lexical_scope["byte_start"],
                lexical_scope["byte_start"],
            )
            if (
                sum(
                    (
                        scope["byte_end"] - scope["byte_start"],
                        scope["byte_start"],
                    )
                    == best_identity
                    for scope in target_scopes
                )
                != 1
            ):
                raise contract.ContractError(
                    f"Rust method owner lexical scope is ambiguous: {symbol['site_id']}"
                )
            query_binding_id = canonical_id(
                "decodex/lane-authority-v2-rust-method-owner-query/1",
                symbol["site_id"],
                target_id,
                lexical_scope["scope_id"],
                surface,
                query_path,
            )
            query = {
                "binding_id": query_binding_id,
                "binding_kind": "use",
                "crate_target_id": target_id,
                "local_name": f"__method_owner_{symbol['site_id']}",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": lexical_scope["scope_id"],
                "source_node_id": syntax["source_node_id"],
                "surface_target_path": query_path,
                "syntax_site_id": symbol["syntax_site_id"],
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
                "visibility_path": None,
            }
            synthetic_by_id[query_binding_id] = query
            owner_by_binding_id[query_binding_id] = symbol

    working_bindings = [dict(binding) for binding in rust_name_bindings]
    working_bindings.extend(dict(binding) for binding in synthetic_by_id.values())
    path_resolutions = resolve_rust_binding_paths(rust_module_scopes, working_bindings)
    path_by_source = {
        resolution["source_binding_id"]: resolution
        for resolution in path_resolutions
        if resolution["source_binding_id"] in synthetic_by_id
    }
    if set(path_by_source) != set(synthetic_by_id):
        raise contract.ContractError("Rust method owner path coverage is incomplete")

    records: list[dict[str, Any]] = []
    for query_binding_id, query in synthetic_by_id.items():
        path = path_by_source[query_binding_id]
        symbol = owner_by_binding_id[query_binding_id]
        resolution_id = canonical_id(
            "decodex/lane-authority-v2-rust-method-owner-resolution/1",
            symbol["site_id"],
            query["crate_target_id"],
            query["scope_id"],
            symbol["owner_signature"],
            query["surface_target_path"],
            path["status"],
            path.get("canonical_path") or "",
        )
        record = {
            "binding_ids": path["binding_ids"][1:],
            "canonical_module_scope_id": path.get("canonical_module_scope_id"),
            "canonical_path": path.get("canonical_path"),
            "canonical_type_definition_site_id": path.get(
                "canonical_type_definition_site_id"
            ),
            "crate_target_id": query["crate_target_id"],
            "lexical_scope_id": query["scope_id"],
            "namespace": "type",
            "purpose": "method_owner",
            "query_binding_id": query_binding_id,
            "query_path": query["surface_target_path"],
            "reason_code": path["reason_code"],
            "resolution_id": resolution_id,
            "source_symbol_site_id": symbol["site_id"],
            "source_syntax_site_id": symbol["syntax_site_id"],
            "status": path["status"],
            "surface_path": symbol["owner_signature"],
        }
        record["resolution_digest"] = hashlib.sha256(
            contract.canonical_json(record).encode("utf-8")
        ).hexdigest()
        records.append(record)
    records.sort(key=lambda record: record["resolution_id"])
    return records


def materialize_rust_qualified_owner_resolutions(
    syntax_sites: list[dict[str, Any]],
    symbol_sites: list[dict[str, Any]],
    rust_module_scopes: list[dict[str, Any]],
    rust_name_bindings: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    syntax_by_id = {site["site_id"]: site for site in syntax_sites}
    scopes_by_source: dict[str, list[dict[str, Any]]] = {}
    for scope in rust_module_scopes:
        scopes_by_source.setdefault(scope["source_node_id"], []).append(scope)

    synthetic_by_id: dict[str, dict[str, Any]] = {}
    owner_by_binding_id: dict[str, dict[str, Any]] = {}
    for symbol in symbol_sites:
        surface = symbol["qualified_owner_signature"]
        if symbol["language"] != "rust" or symbol["role"] != "call_target" or surface is None:
            continue
        query_path = RUST_PRELUDE_TYPE_PATHS.get(surface, surface)
        if symbol["qualified_owner_kind"] == "implicit_self":
            if symbol["owner_signature"] is None:
                raise contract.ContractError(
                    f"Rust Self qualified call has no enclosing owner: {symbol['site_id']}"
                )
            query_path = symbol["owner_signature"]
        syntax = syntax_by_id[symbol["syntax_site_id"]]
        candidates = [
            scope
            for scope in scopes_by_source.get(syntax["source_node_id"], [])
            if scope["byte_start"] <= syntax["byte_start"]
            and syntax["byte_end"] <= scope["byte_end"]
        ]
        by_target: dict[str, list[dict[str, Any]]] = {}
        for scope in candidates:
            by_target.setdefault(scope["crate_target_id"], []).append(scope)
        if not by_target:
            raise contract.ContractError(
                f"Rust qualified call has no Cargo-target lexical scope: {symbol['site_id']}"
            )
        for target_id, target_scopes in by_target.items():
            target_scopes.sort(
                key=lambda scope: (
                    scope["byte_end"] - scope["byte_start"],
                    -scope["byte_start"],
                    scope["scope_id"],
                )
            )
            lexical_scope = target_scopes[0]
            best_identity = (
                lexical_scope["byte_end"] - lexical_scope["byte_start"],
                lexical_scope["byte_start"],
            )
            if (
                sum(
                    (
                        scope["byte_end"] - scope["byte_start"],
                        scope["byte_start"],
                    )
                    == best_identity
                    for scope in target_scopes
                )
                != 1
            ):
                raise contract.ContractError(
                    f"Rust qualified owner lexical scope is ambiguous: {symbol['site_id']}"
                )
            query_binding_id = canonical_id(
                "decodex/lane-authority-v2-rust-qualified-owner-query/1",
                symbol["site_id"],
                target_id,
                lexical_scope["scope_id"],
                surface,
                query_path,
            )
            query = {
                "binding_id": query_binding_id,
                "binding_kind": "use",
                "crate_target_id": target_id,
                "local_name": f"__qualified_owner_{symbol['site_id']}",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": lexical_scope["scope_id"],
                "source_node_id": syntax["source_node_id"],
                "surface_target_path": query_path,
                "syntax_site_id": symbol["syntax_site_id"],
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
                "visibility_path": None,
            }
            synthetic_by_id[query_binding_id] = query
            owner_by_binding_id[query_binding_id] = symbol

    working_bindings = [dict(binding) for binding in rust_name_bindings]
    working_bindings.extend(dict(binding) for binding in synthetic_by_id.values())
    path_resolutions = resolve_rust_binding_paths(rust_module_scopes, working_bindings)
    path_by_source = {
        resolution["source_binding_id"]: resolution
        for resolution in path_resolutions
        if resolution["source_binding_id"] in synthetic_by_id
    }
    if set(path_by_source) != set(synthetic_by_id):
        raise contract.ContractError("Rust qualified owner path coverage is incomplete")

    records: list[dict[str, Any]] = []
    for query_binding_id, query in synthetic_by_id.items():
        path = path_by_source[query_binding_id]
        symbol = owner_by_binding_id[query_binding_id]
        status = path["status"]
        reason_code = path["reason_code"]
        if symbol["qualified_owner_kind"] == "generic_parameter":
            if status != "unresolved":
                raise contract.ContractError(
                    f"Rust generic qualified owner unexpectedly resolved: {symbol['site_id']}"
                )
            status = "generic_parameter"
            reason_code = "rust_qualified_owner_generic_parameter"
        resolution_id = canonical_id(
            "decodex/lane-authority-v2-rust-qualified-owner-resolution/1",
            symbol["site_id"],
            query["crate_target_id"],
            query["scope_id"],
            symbol["qualified_owner_signature"],
            query["surface_target_path"],
            path["status"],
            status,
            path.get("canonical_path") or "",
        )
        record = {
            "binding_ids": path["binding_ids"][1:],
            "canonical_module_scope_id": path.get("canonical_module_scope_id"),
            "canonical_path": path.get("canonical_path"),
            "canonical_type_definition_site_id": path.get(
                "canonical_type_definition_site_id"
            ),
            "crate_target_id": query["crate_target_id"],
            "lexical_scope_id": query["scope_id"],
            "namespace": "type",
            "path_status": path["status"],
            "purpose": "qualified_call_owner",
            "qualified_owner_evidence": symbol["qualified_owner_evidence"],
            "qualified_owner_kind": symbol["qualified_owner_kind"],
            "query_binding_id": query_binding_id,
            "query_path": query["surface_target_path"],
            "reason_code": reason_code,
            "resolution_id": resolution_id,
            "source_symbol_site_id": symbol["site_id"],
            "source_syntax_site_id": symbol["syntax_site_id"],
            "status": status,
            "surface_path": symbol["qualified_owner_signature"],
        }
        record["resolution_digest"] = hashlib.sha256(
            contract.canonical_json(record).encode("utf-8")
        ).hexdigest()
        records.append(record)
    records.sort(key=lambda record: record["resolution_id"])
    return records


def resolve_swift_recovery(
    materialized: Path, parsed: dict[str, Any]
) -> tuple[str | None, set[str]]:
    recovery_sources = {
        source["source_node_id"]: source
        for source in parsed["source_nodes"]
        if source["language"] == "swift" and source["parser_error_count"] > 0
    }
    if not recovery_sources:
        return None, set()

    try:
        version = subprocess.run(
            ["swiftc", "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        ).stdout.strip()
    except FileNotFoundError as error:
        raise contract.ContractError(
            "Swift recovery requires the native swiftc parser"
        ) from error

    for source in recovery_sources.values():
        source_path = materialized / source["path"]
        subprocess.run(
            ["swiftc", "-frontend", "-parse", str(source_path)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        source["parser_error_count"] = 0

    version_digest = hashlib.sha256(version.encode("utf-8")).hexdigest()
    return version_digest, set(recovery_sources)


def materialize(root: Path) -> dict[str, Any]:
    source_inventory = contract.load_json(root, SOURCE_INVENTORY_PATH)
    analysis_cut = contract.load_json(root, contract.ANALYSIS_CUT_PATH)
    source_cut = analysis_cut["source_cut_commit"]
    if source_inventory["source_cut_commit"] != source_cut:
        raise contract.ContractError("P2 source inventory disagrees with the analysis cut")

    with tempfile.TemporaryDirectory(prefix="decodex-c1i-p2-") as temporary:
        materialized = Path(temporary, "source")
        materialized.mkdir()
        extract_git_tree(root, source_cut, materialized)
        cargo_targets, cargo_metadata_digest = exact_cut_cargo_targets(
            materialized, source_inventory
        )
        parser_output = Path(temporary, "parser-output.json")
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--locked",
                "--manifest-path",
                str(root / "tools/lane-authority-inventory/Cargo.toml"),
                "--",
                str(materialized),
                str(root / SOURCE_INVENTORY_PATH),
                str(root / CANDIDATE_RECORDS_PATH),
                str(parser_output),
            ],
            cwd=root,
            check=True,
        )
        parsed = json.loads(parser_output.read_text(encoding="utf-8"))
        swift_parser_digest, swift_recovery_source_ids = resolve_swift_recovery(
            materialized, parsed
        )

    language_by_source = {
        source["source_node_id"]: source["language"] for source in parsed["source_nodes"]
    }
    for site in parsed["syntax_sites"]:
        site["language"] = language_by_source[site["source_node_id"]]
    cfg_projections: list[dict[str, Any]] = []
    for site in parsed["syntax_sites"]:
        if not site["is_parser_root"]:
            continue
        cfg_projections.append(config_projection(site, "config", "common"))
        cfg_projections.append(config_projection(site, "target", "common"))
        cfg_projections.append(config_projection(site, "config", "linux"))
        cfg_projections.append(config_projection(site, "config", "macos"))
    for site in parsed["syntax_sites"]:
        del site["language"]
    cfg_projections.sort(key=lambda projection: projection["projection_id"])
    rust_module_scopes = materialize_rust_module_scopes(
        parsed, cargo_targets, cfg_projections
    )

    syntax_by_id = {site["site_id"]: site for site in parsed["syntax_sites"]}
    candidate_manifest = contract.load_json(root, CANDIDATE_RECORDS_PATH)
    candidate_by_id = {
        candidate["candidate_id"]: candidate for candidate in candidate_manifest["records"]
    }
    data_sites: list[dict[str, Any]] = []
    dataflow_edges: list[dict[str, Any]] = []
    for candidate_edge in parsed["candidate_site_edges"]:
        candidate = candidate_by_id[candidate_edge["candidate_id"]]
        data_site_id = canonical_id(
            "decodex/lane-authority-v2-data-site/1", candidate["candidate_id"]
        )
        data_sites.append(
            {
                "data_kind": data_kind(candidate["candidate_category"]),
                "site_id": data_site_id,
                "syntax_site_id": candidate_edge["site_id"],
            }
        )
        dataflow_edges.append(
            {
                "edge_id": canonical_id(
                    "decodex/lane-authority-v2-dataflow-edge/1",
                    candidate_edge["site_id"],
                    data_site_id,
                ),
                "from_site_id": candidate_edge["site_id"],
                "to_site_id": data_site_id,
            }
        )
    data_sites.sort(key=lambda site: site["site_id"])
    dataflow_edges.sort(key=lambda edge: edge["edge_id"])

    symbol_sites_by_id: dict[str, dict[str, Any]] = {}
    for fact in parsed["semantic_symbol_facts"]:
        symbol_site_id = canonical_id(
            "decodex/lane-authority-v2-symbol-site/1",
            fact["syntax_site_id"],
            fact["role"],
            fact["signature_digest"],
        )
        declaration = fact["role"] == "declaration"
        symbol_sites_by_id[symbol_site_id] = {
            "definition_site_ids": [],
            "external": False if declaration else None,
            "language": fact["language"],
            "owner_signature": fact["owner_signature"],
            "qualified_owner_evidence": fact["qualified_owner_evidence"],
            "qualified_owner_kind": fact["qualified_owner_kind"],
            "qualified_owner_signature": fact["qualified_owner_signature"],
            "receiver_type_evidence": fact["receiver_type_evidence"],
            "receiver_type_kind": fact["receiver_type_kind"],
            "receiver_type_signature": fact["receiver_type_signature"],
            "resolution": "declaration" if declaration else "unresolved",
            "resolution_hint": fact["resolution_hint"],
            "role": fact["role"],
            "signature": fact["signature"],
            "signature_digest": fact["signature_digest"],
            "site_id": symbol_site_id,
            "syntax_site_id": fact["syntax_site_id"],
        }
    declarations_by_signature: dict[tuple[str, str], list[str]] = {}
    declarations_by_owner: dict[tuple[str, str, str], list[str]] = {}
    for site in symbol_sites_by_id.values():
        if site["resolution"] == "declaration":
            declarations_by_signature.setdefault(
                (site["language"], site["signature"]), []
            ).append(site["site_id"])
            if site["owner_signature"] is not None:
                declarations_by_owner.setdefault(
                    (site["language"], site["owner_signature"], site["signature"]),
                    [],
                ).append(site["site_id"])
    call_edges: list[dict[str, Any]] = []
    for site in symbol_sites_by_id.values():
        if site["role"] != "call_target":
            continue
        if site["language"] == "rust":
            continue
        definitions: list[str] = []
        if site["resolution_hint"] == "exact":
            definitions = declarations_by_signature.get(
                (site["language"], site["signature"]), []
            )
        elif site["resolution_hint"] == "qualified":
            separator = "::" if "::" in site["signature"] else "."
            owner, separator_found, name = site["signature"].rpartition(separator)
            if separator_found:
                if owner in {"self", "Self"}:
                    owner = site["owner_signature"] or ""
                definitions = declarations_by_owner.get(
                    (site["language"], owner, name), []
                )
        if len(definitions) != 1:
            continue
        definition_site_id = definitions[0]
        site["definition_site_ids"] = [definition_site_id]
        site["external"] = False
        site["resolution"] = "local"
        call_edges.append(
            {
                "edge_id": canonical_id(
                    "decodex/lane-authority-v2-call-edge/1",
                    site["site_id"],
                    definition_site_id,
                ),
                "from_site_id": site["site_id"],
                "to_site_id": definition_site_id,
            }
        )
    symbol_sites = sorted(symbol_sites_by_id.values(), key=lambda site: site["site_id"])
    rust_name_bindings = materialize_rust_name_bindings(
        parsed, rust_module_scopes, symbol_sites
    )
    rust_path_resolutions = resolve_rust_binding_paths(
        rust_module_scopes, rust_name_bindings
    )
    rust_receiver_type_resolutions = materialize_rust_receiver_type_resolutions(
        parsed["syntax_sites"],
        symbol_sites,
        rust_module_scopes,
        rust_name_bindings,
    )
    rust_method_owner_resolutions = materialize_rust_method_owner_resolutions(
        parsed["syntax_sites"],
        symbol_sites,
        rust_module_scopes,
        rust_name_bindings,
    )
    rust_qualified_owner_resolutions = materialize_rust_qualified_owner_resolutions(
        parsed["syntax_sites"],
        symbol_sites,
        rust_module_scopes,
        rust_name_bindings,
    )
    declarations_by_canonical_owner: dict[tuple[str, str, str], list[str]] = {}
    for resolution in rust_method_owner_resolutions:
        owner_site_id = resolution["canonical_type_definition_site_id"]
        if resolution["status"] != "resolved_local_type" or owner_site_id is None:
            continue
        declaration = symbol_sites_by_id[resolution["source_symbol_site_id"]]
        declarations_by_canonical_owner.setdefault(
            (resolution["crate_target_id"], owner_site_id, declaration["signature"]), []
        ).append(declaration["site_id"])
    for resolution in rust_receiver_type_resolutions:
        owner_site_id = resolution["canonical_type_definition_site_id"]
        if resolution["status"] != "resolved_local_type" or owner_site_id is None:
            continue
        call = symbol_sites_by_id[resolution["source_symbol_site_id"]]
        method_name = call["signature"].rsplit("::", 1)[-1]
        definitions = declarations_by_canonical_owner.get(
            (resolution["crate_target_id"], owner_site_id, method_name), []
        )
        if len(definitions) != 1:
            continue
        definition_site_id = definitions[0]
        if call["definition_site_ids"] and call["definition_site_ids"] != [definition_site_id]:
            raise contract.ContractError(
                f"Rust receiver call has conflicting canonical targets: {call['site_id']}"
            )
        call["definition_site_ids"] = [definition_site_id]
        call["external"] = False
        call["resolution"] = "local"
        call_edges.append(
            {
                "edge_id": canonical_id(
                    "decodex/lane-authority-v2-call-edge/1",
                    call["site_id"],
                    definition_site_id,
                ),
                "from_site_id": call["site_id"],
                "to_site_id": definition_site_id,
            }
        )
    call_edges.sort(key=lambda edge: edge["edge_id"])

    current_sources = [
        source for source in parsed["source_nodes"] if source["status"] == "current"
    ]
    source_by_id = {source["source_node_id"]: source for source in current_sources}
    candidate_ids_by_source: dict[str, set[str]] = {
        source_id: set() for source_id in source_by_id
    }
    for candidate in candidate_manifest["records"]:
        candidate_ids_by_source[candidate["source_node_id"]].add(candidate["candidate_id"])
    syntax_ids_by_source: dict[str, set[str]] = {source_id: set() for source_id in source_by_id}
    for site in parsed["syntax_sites"]:
        syntax_ids_by_source[site["source_node_id"]].add(site["site_id"])
    call_ids_by_source: dict[str, set[str]] = {source_id: set() for source_id in source_by_id}
    for edge in call_edges:
        symbol = symbol_sites_by_id[edge["from_site_id"]]
        source_id = syntax_by_id[symbol["syntax_site_id"]]["source_node_id"]
        call_ids_by_source[source_id].add(edge["edge_id"])
    dataflow_ids_by_source: dict[str, set[str]] = {
        source_id: set() for source_id in source_by_id
    }
    for edge in dataflow_edges:
        source_id = syntax_by_id[edge["from_site_id"]]["source_node_id"]
        dataflow_ids_by_source[source_id].add(edge["edge_id"])
    projections_by_language_platform: dict[tuple[str, str], list[str]] = {}
    for projection in cfg_projections:
        if projection["projection_kind"] != "config":
            continue
        projections_by_language_platform.setdefault(
            (projection["language"], projection["platform"]), []
        ).append(projection["projection_id"])
    cargo_lock_digest = contract.sha256_path(
        root, Path("tools/lane-authority-inventory/Cargo.lock")
    )
    tool_receipts: list[dict[str, Any]] = []
    for language in sorted(contract.EXPECTED_LANGUAGES):
        source_ids = {
            source["source_node_id"]
            for source in current_sources
            if source["language"] == language
        }
        for role, platform in (
            ("parser", "common"),
            ("platform_slice", "linux"),
            ("platform_slice", "macos"),
        ):
            receipt_id = f"tool:{language}:{role}:{platform}"
            syntax_ids = set().union(
                *(syntax_ids_by_source[source_id] for source_id in source_ids)
            )
            candidate_ids = set().union(
                *(candidate_ids_by_source[source_id] for source_id in source_ids)
            )
            call_ids = set().union(*(call_ids_by_source[source_id] for source_id in source_ids))
            dataflow_ids = set().union(
                *(dataflow_ids_by_source[source_id] for source_id in source_ids)
            )
            tool_receipts.append(
                {
                    **id_set_fields("expected_source_node", source_ids),
                    **id_set_fields("completed_source_node", source_ids),
                    **id_set_fields("syntax_site", syntax_ids),
                    **id_set_fields("candidate_record", candidate_ids),
                    **id_set_fields("call_edge", call_ids),
                    **id_set_fields("dataflow_edge", dataflow_ids),
                    "completed": True,
                    "completed_source_node_ids": sorted(source_ids),
                    "config_projection_ids": sorted(
                        projections_by_language_platform[(language, platform)]
                    ),
                    "language": language,
                    "platform": platform,
                    "receipt_id": receipt_id,
                    "receipt_role": role,
                    "rejection_reason_codes": [],
                    "tool": (
                        "tree-sitter-swift+swiftc-parse"
                        if language == "swift" and swift_parser_digest is not None
                        else f"tree-sitter-{language}"
                    ),
                    "tool_identity_digest": canonical_id(
                        "decodex/lane-authority-v2-tool-identity/1",
                        cargo_lock_digest,
                        language,
                        role,
                        platform,
                        swift_parser_digest or "no-native-recovery",
                        contract.stable_id_set_digest(
                            "decodex/lane-authority-v2-swift-recovery-sources/1",
                            swift_recovery_source_ids,
                        ),
                        (
                            cargo_metadata_digest
                            if language == "rust"
                            else "not-rust-cargo-metadata"
                        ),
                    ),
                    "unresolved_count": 0,
                }
            )
    tool_receipts.sort(key=lambda receipt: receipt["receipt_id"])

    write_json(
        root,
        RELATION_ROOT / "source_nodes.json",
        relation(
            "decodex/lane-authority-v2-c1i-source-nodes/1",
            "source_nodes",
            parsed["source_nodes"],
        ),
    )
    for name, schema_name, records in (
        ("data_sites", "decodex/lane-authority-v2-c1i-data-sites/1", data_sites),
        ("call_edges", "decodex/lane-authority-v2-c1i-call-edges/1", call_edges),
        (
            "dataflow_edges",
            "decodex/lane-authority-v2-c1i-dataflow-edges/1",
            dataflow_edges,
        ),
        (
            "symbol_sites",
            "decodex/lane-authority-v2-c1i-symbol-sites/1",
            symbol_sites,
        ),
        (
            "rust_module_scopes",
            "decodex/lane-authority-v2-c1i-rust-module-scopes/1",
            rust_module_scopes,
        ),
        (
            "rust_name_bindings",
            "decodex/lane-authority-v2-c1i-rust-name-bindings/1",
            rust_name_bindings,
        ),
        (
            "rust_path_resolutions",
            "decodex/lane-authority-v2-c1i-rust-path-resolutions/1",
            rust_path_resolutions,
        ),
        (
            "rust_receiver_type_resolutions",
            "decodex/lane-authority-v2-c1i-rust-receiver-type-resolutions/1",
            rust_receiver_type_resolutions,
        ),
        (
            "rust_method_owner_resolutions",
            "decodex/lane-authority-v2-c1i-rust-method-owner-resolutions/1",
            rust_method_owner_resolutions,
        ),
        (
            "rust_qualified_owner_resolutions",
            "decodex/lane-authority-v2-c1i-rust-qualified-owner-resolutions/1",
            rust_qualified_owner_resolutions,
        ),
        (
            "supporting_inputs",
            "decodex/lane-authority-v2-c1i-supporting-inputs/1",
            [],
        ),
        (
            "toolchain_receipts",
            "decodex/lane-authority-v2-c1i-toolchain-receipts/1",
            tool_receipts,
        ),
    ):
        write_json(root, RELATION_ROOT / f"{name}.json", relation(schema_name, name, records))
    write_json(
        root,
        RELATION_ROOT / "syntax_sites.json",
        relation(
            "decodex/lane-authority-v2-c1i-syntax-sites/1",
            "syntax_sites",
            parsed["syntax_sites"],
        ),
    )
    write_json(
        root,
        RELATION_ROOT / "candidate_site_edges.json",
        relation(
            "decodex/lane-authority-v2-c1i-candidate-site-edges/1",
            "candidate_site_edges",
            parsed["candidate_site_edges"],
        ),
    )
    write_json(
        root,
        RELATION_ROOT / "cfg_projections.json",
        relation(
            "decodex/lane-authority-v2-c1i-cfg-projections/1",
            "cfg_projections",
            cfg_projections,
        ),
    )
    parser_errors = sum(source["parser_error_count"] for source in parsed["source_nodes"])
    symbol_fact_counts = {
        f"{role}_{hint}_symbol_facts": sum(
            1
            for fact in parsed["semantic_symbol_facts"]
            if fact["role"] == role and fact["resolution_hint"] == hint
        )
        for role in ("call_target", "declaration")
        for hint in ("dynamic", "exact", "qualified")
    }
    return {
        **symbol_fact_counts,
        "candidate_site_edges": len(parsed["candidate_site_edges"]),
        "cargo_targets": len(cargo_targets),
        "cfg_projections": len(cfg_projections),
        "call_edges": len(call_edges),
        "data_sites": len(data_sites),
        "dataflow_edges": len(dataflow_edges),
        "parser_errors": parser_errors,
        "rust_module_scopes": len(rust_module_scopes),
        "rust_name_bindings": len(rust_name_bindings),
        "rust_path_resolutions": len(rust_path_resolutions),
        "rust_receiver_type_resolutions": len(rust_receiver_type_resolutions),
        "rust_method_owner_resolutions": len(rust_method_owner_resolutions),
        "rust_qualified_owner_resolutions": len(rust_qualified_owner_resolutions),
        "source_cut_commit": source_cut,
        "source_nodes": len(parsed["source_nodes"]),
        "symbol_sites": len(symbol_sites),
        "syntax_sites": len(parsed["syntax_sites"]),
        "toolchain_receipts": len(tool_receipts),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.parse_args()
    root = Path(contract.run_git(Path.cwd(), "rev-parse", "--show-toplevel"))
    print(contract.canonical_json(materialize(root)), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
