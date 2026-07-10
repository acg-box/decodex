#!/usr/bin/env python3
"""Materialize the immutable C1I source cut and replay frozen C0 candidates."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import types
from pathlib import Path
from typing import Any

import verify_contract as contract


BASELINE = "d57553bc1bcdceebe1d0c7ec5ad5dc492b695348"
C0_GENERATOR_COMMIT = "17f50311af30331061a5355ac81bab4e30c0c68f"
SOURCE_INVENTORY_PATH = Path(
    "tools/lane-authority-inventory/manifests/source_inventory.json"
)
ANALYSIS_CUT_PATH = Path("tools/lane-authority-inventory/manifests/analysis_cut.json")
CANDIDATE_RECORDS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/candidate_records.json"
)
TOOL_EXACT_PATHS = {
    "scripts/verify_lane_authority_v2_c1i_contract.sh",
    "scripts/verify_lane_authority_v2_gates.sh",
    "tests/scripts/test_lane_authority_v2_c1i_contract.py",
}
TOOL_PREFIX = "tools/lane-authority-inventory/"
LANGUAGE_BY_SUFFIX = {
    ".py": "python",
    ".rs": "rust",
    ".sh": "shell",
    ".bash": "shell",
    ".zsh": "shell",
    ".swift": "swift",
    ".toml": "toml",
    ".yml": "yaml",
    ".yaml": "yaml",
}


def git_bytes(root: Path, commit: str, path: str) -> bytes:
    return subprocess.run(
        ["git", "show", f"{commit}:{path}"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout


def source_id(path: str, digest: str, *, deleted: bool = False) -> str:
    disposition = "deleted" if deleted else "current"
    return hashlib.sha256(
        f"decodex/lane-authority-v2-source-node/1\0{disposition}\0{path}\0{digest}".encode(
            "utf-8"
        )
    ).hexdigest()


def is_test_path(path: str) -> bool:
    parts = Path(path).parts
    return "tests" in parts or "fixtures" in parts or Path(path).name.startswith("test_")


def is_tool_path(path: str) -> bool:
    return path.startswith(TOOL_PREFIX) or path in TOOL_EXACT_PATHS


def language_for(path: str) -> str:
    return LANGUAGE_BY_SUFFIX[Path(path).suffix]


def load_baseline_scanner(root: Path) -> dict[str, Any]:
    path = "scripts/lane_authority_v2_baseline.py"
    module_name = "lane_authority_v2_frozen_baseline"
    module = types.ModuleType(module_name)
    module.__file__ = path
    sys.modules[module_name] = module
    source = git_bytes(root, C0_GENERATOR_COMMIT, path)
    exec(compile(source, path, "exec"), module.__dict__)
    return module.__dict__


def build_source_records(root: Path, source_cut: str) -> tuple[list[dict[str, Any]], dict[str, bytes]]:
    baseline_bytes = contract.git_source_bytes(root, BASELINE)
    base = contract.run_git(root, "rev-parse", "origin/main")
    base_bytes = contract.git_source_bytes(root, base)
    cut_bytes = contract.git_source_bytes(root, source_cut)
    records: list[dict[str, Any]] = []

    for path, content in sorted(cut_bytes.items()):
        digest = hashlib.sha256(content).hexdigest()
        predecessor_content = base_bytes.get(path) or baseline_bytes.get(path)
        predecessor_digest = (
            hashlib.sha256(predecessor_content).hexdigest()
            if predecessor_content is not None
            else None
        )
        tool = is_tool_path(path)
        if tool:
            provenance = "tool"
            scope = "tool"
        else:
            scope = "test" if is_test_path(path) else "production"
            baseline_content = baseline_bytes.get(path)
            base_content = base_bytes.get(path)
            if baseline_content == content:
                provenance = "c0"
            elif base_content == content:
                provenance = "post_c0_base"
            else:
                provenance = "c1i_head"
        records.append(
            {
                "byte_length": len(content),
                "content_digest": digest,
                "language": language_for(path),
                "path": path,
                "predecessor_source_node_id": (
                    source_id(path, predecessor_digest)
                    if predecessor_digest is not None and predecessor_digest != digest
                    else None
                ),
                "provenance": provenance,
                "scope": scope,
                "source_node_id": source_id(path, digest),
                "status": "current",
            }
        )

    predecessor_universe = {**baseline_bytes, **base_bytes}
    for path in sorted(set(predecessor_universe) - set(cut_bytes)):
        content = predecessor_universe[path]
        digest = hashlib.sha256(content).hexdigest()
        records.append(
            {
                "byte_length": len(content),
                "content_digest": digest,
                "language": language_for(path),
                "path": path,
                "predecessor_source_node_id": source_id(path, digest),
                "provenance": "deleted_tombstone",
                "scope": "test" if is_test_path(path) else "production",
                "source_node_id": source_id(path, digest, deleted=True),
                "status": "deleted",
            }
        )
    records.sort(key=lambda record: record["source_node_id"])
    return records, baseline_bytes


def replay_candidates(
    root: Path,
    source_records: list[dict[str, Any]],
    baseline_bytes: dict[str, bytes],
) -> list[dict[str, Any]]:
    scanner = load_baseline_scanner(root)
    patterns = {pattern.category: pattern.expression for pattern in scanner["PATTERNS"]}
    patterns["launcher"] = scanner["LAUNCH_PATTERN"]
    observations = contract.expected_c0_candidate_observations(root)
    source_by_path = {record["path"]: record for record in source_records}
    candidates: dict[tuple[str, str, int, str], dict[str, Any]] = {}

    for observation_id, observation in sorted(observations.items()):
        path = observation["path"]
        expression = patterns[observation["category"]]
        text = baseline_bytes[path].decode("utf-8", errors="replace")
        hits = [
            (line_number, hashlib.sha256(line.encode("utf-8")).hexdigest())
            for line_number, line in enumerate(text.splitlines(), 1)
            if expression.search(line)
        ]
        if (
            len(hits) != observation["candidate_line_count"]
            or hits[0][0] != observation["first_line"]
            or contract.c0_candidate_digest(hits) != observation["candidate_digest"]
        ):
            raise contract.ContractError(f"frozen C0 candidate replay drifted: {observation_id}")
        for line_number, line_digest in hits:
            key = (path, observation["category"], line_number, line_digest)
            candidate = candidates.setdefault(
                key,
                {
                    "candidate_category": observation["category"],
                    "candidate_digest": "0" * 64,
                    "candidate_id": "0" * 64,
                    "c0_observation_ids": [],
                    "c0_origin_artifacts": [],
                    "line_digest": line_digest,
                    "line_number": line_number,
                    "provenance": "c0_replay",
                    "source_node_id": source_by_path[path]["source_node_id"],
                },
            )
            candidate["c0_observation_ids"].append(observation_id)
            candidate["c0_origin_artifacts"].append(observation["origin"])

    result: list[dict[str, Any]] = []
    for candidate in candidates.values():
        candidate["c0_observation_ids"] = sorted(set(candidate["c0_observation_ids"]))
        candidate["c0_origin_artifacts"] = sorted(set(candidate["c0_origin_artifacts"]))
        candidate["candidate_id"] = contract.canonical_candidate_id(candidate)
        candidate["candidate_digest"] = contract.candidate_record_digest(candidate)
        result.append(candidate)
    return sorted(result, key=lambda candidate: candidate["candidate_id"])


def write_json(root: Path, path: Path, value: dict[str, Any]) -> None:
    destination = root / path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(contract.canonical_json(value), encoding="utf-8")


def materialize(root: Path, source_cut: str) -> dict[str, Any]:
    source_cut = contract.run_git(root, "rev-parse", source_cut)
    base = contract.run_git(root, "rev-parse", "origin/main")
    subprocess.run(["git", "merge-base", "--is-ancestor", base, source_cut], cwd=root, check=True)
    records, baseline_bytes = build_source_records(root, source_cut)
    partitions = contract.source_partition_digests(records)
    candidates = replay_candidates(root, records, baseline_bytes)
    source_cut_bytes = contract.git_source_bytes(root, source_cut)
    cut_digests = {
        path: hashlib.sha256(content).hexdigest() for path, content in source_cut_bytes.items()
    }
    analysis_cut = {
        "analysis_input_tree_digest": contract.canonical_source_tree_digest(cut_digests),
        "analysis_source_node_count": sum(contract.source_partition(record) == "analysis" for record in records),
        "analysis_source_nodes_digest": partitions["analysis"],
        "c0_artifact_sha256": {
            "launcher_inventory": contract.sha256_path(root, contract.LAUNCHER_PATH),
            "legacy_authority_inventory": contract.sha256_path(root, contract.LEGACY_PATH),
            "mutation_registry": contract.sha256_path(root, contract.MUTATION_PATH),
            "scenario_manifest": contract.sha256_path(
                root,
                Path("apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json"),
            ),
        },
        "c0_baseline_commit": BASELINE,
        "c0_baseline_tree_oid": contract.run_git(root, "rev-parse", f"{BASELINE}^{{tree}}"),
        "c0_candidate_anchors": {"launcher": 203, "legacy": 40854, "mutation": 39516},
        "c0_source_tree_digest": "d55e72c9b4a522f7cec8af4afa8c968c6fd3749139d045d178160b6287f00507",
        "deleted_tombstone_count": sum(contract.source_partition(record) == "deleted_tombstone" for record in records),
        "deleted_tombstones_digest": partitions["deleted_tombstone"],
        "output_binding_policy": "outputs_excluded_from_input_preimage_and_bound_by_artifact_digest_ledger_and_exact_head_gate",
        "post_c0_added_count": contract.post_c0_delta(root, BASELINE, source_cut)[0],
        "post_c0_delta_digest": contract.post_c0_delta(root, BASELINE, source_cut)[2],
        "post_c0_modified_count": contract.post_c0_delta(root, BASELINE, source_cut)[1],
        "pr_base_commit": base,
        "pr_base_tree_oid": contract.run_git(root, "rev-parse", f"{base}^{{tree}}"),
        "repository_key": "github.com/hack-ink/decodex",
        "schema": "decodex/lane-authority-v2-c1i-analysis-cut/1",
        "source_cut_commit": source_cut,
        "source_cut_tree_oid": contract.run_git(root, "rev-parse", f"{source_cut}^{{tree}}"),
        "tool_source_node_count": sum(contract.source_partition(record) == "tool" for record in records),
        "tool_source_nodes_digest": partitions["tool"],
    }
    write_json(
        root,
        SOURCE_INVENTORY_PATH,
        {
            "records": records,
            "schema": "decodex/lane-authority-v2-c1i-source-inventory/1",
            "source_cut_commit": source_cut,
            "source_cut_tree_oid": analysis_cut["source_cut_tree_oid"],
        },
    )
    write_json(root, ANALYSIS_CUT_PATH, analysis_cut)
    write_json(
        root,
        CANDIDATE_RECORDS_PATH,
        {
            "records": candidates,
            "relation": "candidate_records",
            "schema": "decodex/lane-authority-v2-c1i-candidate-records/1",
        },
    )
    return {
        "analysis_sources": analysis_cut["analysis_source_node_count"],
        "candidate_records": len(candidates),
        "deleted_tombstones": analysis_cut["deleted_tombstone_count"],
        "source_cut_commit": source_cut,
        "tool_sources": analysis_cut["tool_source_node_count"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-cut", default="HEAD")
    args = parser.parse_args()
    root = Path(contract.run_git(Path.cwd(), "rev-parse", "--show-toplevel"))
    print(contract.canonical_json(materialize(root, args.source_cut)), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
