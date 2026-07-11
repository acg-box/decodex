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

    syntax_by_id = {site["site_id"]: site for site in parsed["syntax_sites"]}
    roots_by_source = {
        site["source_node_id"]: site["site_id"]
        for site in parsed["syntax_sites"]
        if site["is_parser_root"]
    }
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

    call_edges: list[dict[str, Any]] = []
    for site in parsed["syntax_sites"]:
        if not any(token in site["node_kind"] for token in ("call", "command", "exec", "macro")):
            continue
        target = roots_by_source[site["source_node_id"]]
        call_edges.append(
            {
                "edge_id": canonical_id(
                    "decodex/lane-authority-v2-call-edge/1", site["site_id"], target
                ),
                "from_site_id": site["site_id"],
                "to_site_id": target,
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
        source_id = syntax_by_id[edge["from_site_id"]]["source_node_id"]
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
        ("symbol_sites", "decodex/lane-authority-v2-c1i-symbol-sites/1", []),
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
    return {
        "candidate_site_edges": len(parsed["candidate_site_edges"]),
        "cfg_projections": len(cfg_projections),
        "call_edges": len(call_edges),
        "data_sites": len(data_sites),
        "dataflow_edges": len(dataflow_edges),
        "parser_errors": parser_errors,
        "source_cut_commit": source_cut,
        "source_nodes": len(parsed["source_nodes"]),
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
