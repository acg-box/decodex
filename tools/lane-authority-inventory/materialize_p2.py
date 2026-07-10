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

    write_json(
        root,
        RELATION_ROOT / "source_nodes.json",
        relation(
            "decodex/lane-authority-v2-c1i-source-nodes/1",
            "source_nodes",
            parsed["source_nodes"],
        ),
    )
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
        "parser_errors": parser_errors,
        "source_cut_commit": source_cut,
        "source_nodes": len(parsed["source_nodes"]),
        "syntax_sites": len(parsed["syntax_sites"]),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.parse_args()
    root = Path(contract.run_git(Path.cwd(), "rev-parse", "--show-toplevel"))
    print(contract.canonical_json(materialize(root)), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
