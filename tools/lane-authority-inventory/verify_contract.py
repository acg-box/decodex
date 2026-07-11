#!/usr/bin/env python3
"""Verify the Lane Authority v2 C1I P0 contract and negative readiness gate."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import subprocess
import sys
import tarfile
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError, ValidationError


CHECKPOINT_PATH = Path("tools/lane-authority-inventory/contracts/p0_checkpoint.json")
CATALOG_PATH = Path("tools/lane-authority-inventory/manifests/authority_surface_catalog.json")
AUTHORITY_SYMBOL_POLICY_PATH = Path(
    "tools/lane-authority-inventory/catalog/authority_symbol_policy.json"
)
EXTERNAL_SYMBOL_POLICY_PATH = Path(
    "tools/lane-authority-inventory/catalog/external_symbol_policy.json"
)
DATAFLOW_PATH = Path("tools/lane-authority-inventory/contracts/dataflow_contract.json")
OUTPUT_POLICY_PATH = Path(
    "tools/lane-authority-inventory/contracts/output_artifact_policy.json"
)
CFG_COVERAGE_PATH = Path("tools/lane-authority-inventory/manifests/cfg_coverage.json")
DATAFLOW_PROOFS_PATH = Path("tools/lane-authority-inventory/manifests/dataflow_proofs.json")
ANALYSIS_CUT_PATH = Path("tools/lane-authority-inventory/manifests/analysis_cut.json")
SOURCE_INVENTORY_PATH = Path("tools/lane-authority-inventory/manifests/source_inventory.json")
CANDIDATE_RECORDS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/candidate_records.json"
)
SOURCE_NODES_PATH = Path("tools/lane-authority-inventory/manifests/relations/source_nodes.json")
SYNTAX_SITES_PATH = Path("tools/lane-authority-inventory/manifests/relations/syntax_sites.json")
CFG_PROJECTIONS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/cfg_projections.json"
)
CANDIDATE_SITE_EDGES_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/candidate_site_edges.json"
)
CATALOG_DISPOSITIONS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/catalog_entry_dispositions.json"
)
REVIEW_RECEIPT_PATH = Path(
    "tools/lane-authority-inventory/reviews/c1i_integrated_review.json"
)
EXECUTABLE_CONTRACT_PATHS = (
    Path("scripts/verify_lane_authority_v2_c1i_contract.sh"),
    Path("scripts/verify_lane_authority_v2_gates.sh"),
    Path("tools/lane-authority-inventory/Cargo.lock"),
    Path("tools/lane-authority-inventory/Cargo.toml"),
    Path("tools/lane-authority-inventory/requirements.lock"),
    Path("tools/lane-authority-inventory/requirements.txt"),
    Path("tools/lane-authority-inventory/run_locked_python.sh"),
    Path("tools/lane-authority-inventory/materialize_p1.py"),
    Path("tools/lane-authority-inventory/materialize_p2.py"),
    Path("tools/lane-authority-inventory/materialize_p3.py"),
    Path("tools/lane-authority-inventory/verify_contract.py"),
)
REASON_CODES_PATH = Path(
    "tools/lane-authority-inventory/contracts/rejection_reason_codes.json"
)
INCOMPLETE_FIXTURE_PATH = Path(
    "tools/lane-authority-inventory/fixtures/c1i_incomplete.json"
)
SCHEMA_PATHS = (
    Path("tools/lane-authority-inventory/contracts/analysis_cut.schema.json"),
    Path("tools/lane-authority-inventory/contracts/authority_symbol_policy.schema.json"),
    Path("tools/lane-authority-inventory/contracts/authority_surface_catalog.schema.json"),
    Path("tools/lane-authority-inventory/contracts/external_symbol_policy.schema.json"),
    Path("tools/lane-authority-inventory/contracts/dataflow_contract.schema.json"),
    Path("tools/lane-authority-inventory/contracts/cfg_coverage.schema.json"),
    Path("tools/lane-authority-inventory/contracts/dataflow_proofs.schema.json"),
    Path("tools/lane-authority-inventory/contracts/inventory_composition.schema.json"),
    Path("tools/lane-authority-inventory/contracts/output_artifact_manifest.schema.json"),
    Path("tools/lane-authority-inventory/contracts/p0_checkpoint.schema.json"),
    Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json"),
    Path("tools/lane-authority-inventory/contracts/review_receipt.schema.json"),
    Path("tools/lane-authority-inventory/contracts/source_inventory.schema.json"),
    Path("tools/lane-authority-inventory/contracts/rejection_report.schema.json"),
)
LAUNCHER_PATH = Path(
    "apps/decodex/src/bootstrap/tests/fixtures/lane_authority_v2/launcher_inventory.json"
)
LEGACY_PATH = Path(
    "apps/decodex/src/state/tests/fixtures/lane_authority_v2/legacy_authority_inventory.json"
)
MUTATION_PATH = Path(
    "apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json"
)
ALLOWED_EXACT_PATHS = {
    "openwiki/evidence/lane-authority-v2-checkpoints.md",
    "openwiki/quickstart.md",
    "openwiki/specs/lane-authority-v2-effects.md",
    "openwiki/specs/lane-authority-v2-gates.md",
    "openwiki/specs/lane-authority-v2-inventory.md",
    "scripts/verify_lane_authority_v2_c1i_contract.sh",
    "scripts/verify_lane_authority_v2_gates.sh",
    "tests/scripts/test_lane_authority_v2_c1i_contract.py",
    "tests/scripts/test_lane_authority_v2_c1i_materialize_p2.py",
}
ALLOWED_PREFIXES = ("tools/lane-authority-inventory/",)
EXPECTED_LANGUAGES = {"python", "rust", "shell", "swift", "toml", "yaml"}
SOURCE_SUFFIXES = {".bash", ".py", ".rs", ".sh", ".swift", ".toml", ".yaml", ".yml", ".zsh"}
PARSER_ROOT_KINDS = {
    "python": "module",
    "rust": "source_file",
    "shell": "program",
    "swift": "source_file",
    "toml": "document",
    "yaml": "stream",
}
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
EXPECTED_LATTICE = [
    "Bottom",
    "Constant",
    "FiniteSet",
    "Structured",
    "AuthorityRoot",
    "Top",
]
EXPECTED_RELATIONS = {
    "call_edges",
    "candidate_adjudications",
    "candidate_records",
    "candidate_site_edges",
    "cfg_projections",
    "data_sites",
    "dataflow_edges",
    "catalog_entry_dispositions",
    "site_classifications",
    "source_nodes",
    "rust_module_scopes",
    "rust_name_bindings",
    "rust_path_resolutions",
    "rust_receiver_type_resolutions",
    "rust_method_owner_resolutions",
    "rust_qualified_owner_resolutions",
    "supporting_inputs",
    "symbol_sites",
    "syntax_sites",
    "toolchain_receipts",
}
RELATION_DEFINITIONS = {
    "call_edges": "edge",
    "candidate_adjudications": "candidate_adjudication",
    "candidate_records": "candidate_record",
    "candidate_site_edges": "candidate_site_edge",
    "catalog_entry_dispositions": "catalog_entry_disposition",
    "cfg_projections": "cfg_projection",
    "data_sites": "data_site",
    "dataflow_edges": "edge",
    "site_classifications": "site_classification",
    "source_nodes": "source_node",
    "rust_module_scopes": "rust_module_scope",
    "rust_name_bindings": "rust_name_binding",
    "rust_path_resolutions": "rust_path_resolution",
    "rust_receiver_type_resolutions": "rust_receiver_type_resolution",
    "rust_method_owner_resolutions": "rust_method_owner_resolution",
    "rust_qualified_owner_resolutions": "rust_qualified_owner_resolution",
    "supporting_inputs": "supporting_input",
    "symbol_sites": "symbol_site",
    "syntax_sites": "syntax_site",
    "toolchain_receipts": "toolchain_receipt",
}
NONEMPTY_RELATIONS = {
    "call_edges",
    "candidate_records",
    "dataflow_edges",
    "rust_module_scopes",
    "rust_name_bindings",
    "rust_path_resolutions",
    "rust_receiver_type_resolutions",
    "rust_method_owner_resolutions",
    "rust_qualified_owner_resolutions",
    "syntax_sites",
}
EXPECTED_TRANSFER_RULES = {
    "bind_or_assign_exact",
    "branch_or_match_finite_union",
    "constant_construct",
    "constant_format_interpolation",
    "finite_enum_or_collection_construct",
    "path_join_or_push_finite",
    "reviewed_serialization_mapping",
    "typed_field_or_wrapper_projection",
}
EXPECTED_CROSS_RELATION_INVARIANTS = [
    "every_source_node_or_tombstone_is_present_exactly_once",
    "source_node_partitions_equal_analysis_cut_current_tombstone_and_tool_counts",
    "source_node_partition_digests_equal_the_exact_analysis_cut",
    "every_source_node_syntax_count_digest_and_parser_receipt_match",
    "every_current_source_has_exactly_one_clean_parser_root",
    "candidate_call_and_dataflow_relations_are_nonempty",
    "candidate_records_have_canonical_unique_source_category_line_identity",
    "candidate_records_replay_every_frozen_c0_observation_count_line_and_digest",
    "every_candidate_record_has_exactly_one_adjudication",
    "every_candidate_site_edge_references_existing_candidate_and_site",
    "every_candidate_adjudication_category_matches_its_candidate",
    "every_candidate_adjudication_related_site_set_equals_its_edge_site_set",
    "every_covered_candidate_has_at_least_one_related_site",
    "every_noncovered_candidate_has_no_related_sites",
    "every_site_has_exactly_one_site_classification",
    "every_site_classification_scope_matches_its_source",
    "every_site_classification_projection_resolves_to_its_site_and_kind",
    "every_site_classification_target_and_config_platform_match",
    "every_cfg_projection_references_an_existing_site",
    "every_cfg_projection_language_matches_its_source",
    "every_syntax_site_has_at_least_one_cfg_projection",
    "every_external_symbol_has_exactly_one_disposition",
    "every_catalog_entry_has_exact_consumers_or_one_reviewed_absent_receipt",
    "toolchain_receipts_equal_the_unique_catalog_matrix_and_cover_every_language",
    "toolchain_projection_language_and_platform_match_the_receipt",
    "tool_receipt_expected_completed_and_output_sets_match_relations",
    "each_language_has_one_common_parser_with_analysis_cut_derived_expected_sources",
    "every_call_and_dataflow_edge_has_existing_endpoints",
    "no_accepted_relation_contains_unknown_or_unresolved_state",
    "catalog_used_symbol_and_semantic_digests_match_canonical_relations",
    "composition_and_analysis_cut_artifact_digests_are_recomputed",
    "analysis_cut_source_universe_is_reconstructed_from_git_objects",
    "post_source_cut_commits_change_only_closed_output_policy_paths",
    "cfg_coverage_and_dataflow_proof_artifacts_are_schema_valid_and_digest_bound",
    "each_relation_digest_is_composed_exactly_once",
]
EXPECTED_OUTPUT_ARTIFACTS = {
    "openwiki/evidence/lane-authority-v2-checkpoints.md": ("ledger", "exact_head_gate", False),
    "tools/lane-authority-inventory/manifests/analysis_cut.json": ("analysis_cut", "artifact_sha256", True),
    "tools/lane-authority-inventory/manifests/authority_surface_catalog.json": ("catalog_projection", "artifact_sha256", True),
    "tools/lane-authority-inventory/manifests/cfg_coverage.json": ("cfg_coverage", "artifact_sha256", True),
    "tools/lane-authority-inventory/manifests/dataflow_proofs.json": ("dataflow_proof", "artifact_sha256", True),
    "tools/lane-authority-inventory/manifests/inventory_composition.json": ("composition", "artifact_sha256", True),
    "tools/lane-authority-inventory/manifests/source_inventory.json": ("source_inventory", "artifact_sha256", True),
    "tools/lane-authority-inventory/rejections/latest.json": ("rejection_report", "diagnostic_only", False),
    "tools/lane-authority-inventory/reviews/c1i_integrated_review.json": ("review_receipt", "exact_head_gate", False),
    **{
        f"tools/lane-authority-inventory/manifests/relations/{relation}.json": (
            "tool_receipt" if relation == "toolchain_receipts" else "relation",
            "composition_relation_receipt",
            True,
        )
        for relation in EXPECTED_RELATIONS
    },
}


class ContractError(RuntimeError):
    """Raised when a P0 contract invariant is not satisfied."""


def run_git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return result.stdout.strip()


def git_source_bytes(root: Path, commit: str) -> dict[str, bytes]:
    result = subprocess.run(
        ["git", "archive", "--format=tar", commit],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    )
    sources: dict[str, bytes] = {}
    with tarfile.open(fileobj=io.BytesIO(result.stdout), mode="r:") as archive:
        for member in archive.getmembers():
            if not member.isfile() or Path(member.name).suffix not in SOURCE_SUFFIXES:
                continue
            extracted = archive.extractfile(member)
            if extracted is None:
                raise ContractError(f"cannot read Git object from archive: {member.name}")
            sources[member.name] = extracted.read()
    return sources


def canonical_source_tree_digest(content_digests: dict[str, str]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-analysis-source-tree/1\0")
    for path, content_digest in sorted(content_digests.items()):
        path_bytes = path.encode("utf-8")
        digest.update(len(path_bytes).to_bytes(4, "big"))
        digest.update(path_bytes)
        digest.update(bytes.fromhex(content_digest))
    return digest.hexdigest()


def c0_source_tree_digest(content_digests: dict[str, str]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-source-tree/1\0")
    for path, content_digest in sorted(content_digests.items()):
        path_bytes = path.encode("utf-8")
        digest.update(len(path_bytes).to_bytes(4, "big"))
        digest.update(path_bytes)
        digest.update(bytes.fromhex(content_digest))
    return digest.hexdigest()


def post_c0_delta(root: Path, baseline: str, source_cut: str) -> tuple[int, int, str]:
    output = subprocess.run(
        ["git", "diff", "--name-status", "--no-renames", "-z", baseline, source_cut, "--"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout.split(b"\0")
    entries: list[dict[str, str]] = []
    added = 0
    modified = 0
    for index in range(0, len(output) - 1, 2):
        status = output[index].decode("ascii")
        path = output[index + 1].decode("utf-8")
        if Path(path).suffix not in SOURCE_SUFFIXES:
            continue
        if status == "A":
            added += 1
        elif status == "M":
            modified += 1
        entries.append({"path": path, "status": status})
    digest = hashlib.sha256(
        (
            "decodex/lane-authority-v2-post-c0-delta/1\0"
            + canonical_json(sorted(entries, key=lambda entry: (entry["path"], entry["status"])))
        ).encode("utf-8")
    ).hexdigest()
    return added, modified, digest


def load_json(root: Path, path: Path) -> dict[str, Any]:
    def closed_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise ContractError(f"{path} contains duplicate JSON key: {key}")
            value[key] = item
        return value

    value = json.loads(
        (root / path).read_text(encoding="utf-8"), object_pairs_hook=closed_object
    )
    if not isinstance(value, dict):
        raise ContractError(f"{path} must contain a JSON object")
    return value


def sha256_path(root: Path, path: Path) -> str:
    return hashlib.sha256((root / path).read_bytes()).hexdigest()


def canonical_json(value: object) -> str:
    return json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n"


def stable_id_set_digest(domain: str, identifiers: set[str]) -> str:
    digest = hashlib.sha256()
    digest.update(domain.encode("utf-8"))
    digest.update(b"\0")
    for identifier in sorted(identifiers):
        digest.update(identifier.encode("utf-8"))
        digest.update(b"\0")
    return digest.hexdigest()


def stable_parts_id(domain: str, *parts: str) -> str:
    digest = hashlib.sha256()
    digest.update(domain.encode("utf-8"))
    digest.update(b"\0")
    for part in parts:
        digest.update(part.encode("utf-8"))
        digest.update(b"\0")
    return digest.hexdigest()


def dataflow_fixed_point_digest(proof: dict[str, Any]) -> str:
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-dataflow-fixed-point/1\n"
            + canonical_json({**proof, "fixed_point_digest": "0" * 64})
        ).encode("utf-8")
    ).hexdigest()


def validate_dataflow_proof_path(
    proof: dict[str, Any],
    *,
    call_edges: dict[str, dict[str, Any]],
    dataflow_edges: dict[str, dict[str, Any]],
    site_ids: set[str],
    sink_ids: set[str],
    syntax_ids: set[str],
    syntax_sources: dict[str, str],
    derived_syntax: dict[str, str],
    projections: dict[str, dict[str, Any]],
) -> None:
    selected_call_ids = set(proof["call_edge_ids"])
    selected_dataflow_ids = set(proof["dataflow_edge_ids"])
    if not selected_call_ids.issubset(call_edges):
        raise ContractError("dataflow proof references a missing call edge")
    if not selected_dataflow_ids.issubset(dataflow_edges):
        raise ContractError("dataflow proof references a missing dataflow edge")
    source_site_ids = set(proof["source_site_ids"])
    if not source_site_ids.issubset(site_ids) or proof["sink_site_id"] not in sink_ids:
        raise ContractError("dataflow proof references a missing source or authority sink")
    projection_ids = set(proof["config_projection_ids"])
    if not projection_ids.issubset(projections):
        raise ContractError("dataflow proof references a missing config projection")

    selected_edges = [
        *(call_edges[edge_id] for edge_id in selected_call_ids),
        *(dataflow_edges[edge_id] for edge_id in selected_dataflow_ids),
    ]
    if not selected_edges:
        raise ContractError("dataflow proof contains no graph path evidence")
    graph: dict[str, set[str]] = {}
    reverse_graph: dict[str, set[str]] = {}
    for edge in selected_edges:
        graph.setdefault(edge["from_site_id"], set()).add(edge["to_site_id"])
        reverse_graph.setdefault(edge["to_site_id"], set()).add(edge["from_site_id"])

    def reachable(starts: set[str], edges: dict[str, set[str]]) -> set[str]:
        seen = set(starts)
        pending = list(starts)
        while pending:
            current = pending.pop()
            for target in edges.get(current, set()):
                if target not in seen:
                    seen.add(target)
                    pending.append(target)
        return seen

    forward = reachable(source_site_ids, graph)
    reverse = reachable({proof["sink_site_id"]}, reverse_graph)
    if proof["sink_site_id"] not in forward:
        raise ContractError("dataflow proof sources do not reach its sink")
    if any(
        edge["from_site_id"] not in forward or edge["to_site_id"] not in reverse
        for edge in selected_edges
    ):
        raise ContractError("dataflow proof contains an edge outside every source-to-sink path")
    path_syntax_ids = {
        site_id if site_id in syntax_ids else derived_syntax[site_id]
        for site_id in forward.intersection(reverse)
    }
    path_source_ids = {syntax_sources[site_id] for site_id in path_syntax_ids}
    if any(
        projections[projection_id]["projection_kind"] != "config"
        or syntax_sources[projections[projection_id]["site_id"]] not in path_source_ids
        for projection_id in projection_ids
    ):
        raise ContractError("dataflow proof config projection is outside its proven path")
    if proof["fixed_point_digest"] != dataflow_fixed_point_digest(proof):
        raise ContractError("dataflow proof fixed-point digest drifted")


def c0_candidate_digest(records: list[tuple[int, str]]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-candidates/1\0")
    for line_number, line_digest in sorted(records):
        digest.update(line_number.to_bytes(4, "big"))
        digest.update(bytes.fromhex(line_digest))
    return digest.hexdigest()


def candidate_record_digest(candidate: dict[str, Any]) -> str:
    preimage = {
        key: value
        for key, value in candidate.items()
        if key != "candidate_digest"
    }
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-candidate-record/1\0"
            + canonical_json(preimage)
        ).encode("utf-8")
    ).hexdigest()


def canonical_candidate_id(candidate: dict[str, Any]) -> str:
    identity = {
        "candidate_category": candidate["candidate_category"],
        "line_digest": candidate["line_digest"],
        "line_number": candidate["line_number"],
        "source_node_id": candidate["source_node_id"],
    }
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-candidate-id/1\0"
            + canonical_json(identity)
        ).encode("utf-8")
    ).hexdigest()


def expected_c0_candidate_observations(root: Path) -> dict[str, dict[str, Any]]:
    observations: dict[str, dict[str, Any]] = {}

    def add(
        origin: str,
        observation_key: str,
        path: str,
        category: str,
        count: int,
        first_line: int,
        digest: str,
    ) -> None:
        observation_id = f"{origin}:{observation_key}"
        if observation_id in observations:
            raise ContractError(f"duplicate C0 candidate observation: {observation_id}")
        observations[observation_id] = {
            "candidate_digest": digest,
            "candidate_line_count": count,
            "category": category,
            "first_line": first_line,
            "origin": origin,
            "path": path,
        }

    launcher = load_json(root, LAUNCHER_PATH)
    for entry in launcher["entries"]:
        add(
            "launcher",
            entry["source_node_id"],
            entry["path"],
            "launcher",
            int(entry["candidate_line_count"]),
            int(entry["first_line"]),
            entry["candidate_digest"],
        )
    for origin, path, records_key in (
        ("legacy", LEGACY_PATH, "nodes"),
        ("mutation", MUTATION_PATH, "entries"),
    ):
        manifest = load_json(root, path)
        for entry in manifest[records_key]:
            for classification in entry["classifications"]:
                add(
                    origin,
                    classification[4],
                    entry["path"],
                    classification[0],
                    int(classification[1]),
                    int(classification[2]),
                    classification[3],
                )
    return observations


def catalog_semantic_digest(catalog: dict[str, Any]) -> str:
    preimage = json.loads(json.dumps(catalog))
    preimage["catalog_semantic_digest"] = None
    preimage["used_external_symbol_set_digest"] = None
    for section in (
        "dynamic_capability_roots",
        "executable_declarative_paths",
        "external_symbols",
        "local_closure_boundaries",
        "persistent_data_roots",
        "provider_and_config_roots",
        "reviewed_non_authority_external_symbols",
    ):
        for entry in preimage[section]:
            entry["consumer_ids"] = []
            entry["used_site_set_digest"] = "0" * 64
    for entry in preimage["toolchain_matrix"]:
        entry["config_projection_ids"] = []
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-catalog-semantic/1\0"
            + canonical_json(preimage)
        ).encode("utf-8")
    ).hexdigest()


def composition_semantic_digest(composition: dict[str, Any]) -> str:
    preimage = json.loads(json.dumps(composition))
    preimage["composition_digest"] = "0" * 64
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-inventory-composition/1\0"
            + canonical_json(preimage)
        ).encode("utf-8")
    ).hexdigest()


def toolchain_matrix_digest(catalog: dict[str, Any]) -> str:
    matrix = sorted(
        catalog["toolchain_matrix"],
        key=lambda entry: (
            entry["language"],
            entry["platform"],
            entry["receipt_role"],
            entry["tool"],
        ),
    )
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-toolchain-matrix/1\0"
            + canonical_json(matrix)
        ).encode("utf-8")
    ).hexdigest()


def self_bound_artifact_digest(value: dict[str, Any], field: str, domain: str) -> str:
    preimage = json.loads(json.dumps(value))
    preimage[field] = "0" * 64
    return hashlib.sha256(
        (domain + "\0" + canonical_json(preimage)).encode("utf-8")
    ).hexdigest()


def _reject_unresolved_reason_codes(value: object) -> None:
    forbidden = ("unknown", "unresolved", "pending", "todo", "missing", "error")
    if isinstance(value, dict):
        for key, item in value.items():
            if key.endswith("reason_code") and isinstance(item, str):
                if any(token in item.lower() for token in forbidden):
                    raise ContractError(f"accepted record contains unresolved reason code: {item}")
            _reject_unresolved_reason_codes(item)
    elif isinstance(value, list):
        for item in value:
            _reject_unresolved_reason_codes(item)


def source_partition(source: dict[str, Any]) -> str:
    provenance = source["provenance"]
    status = source["status"]
    scope = source["scope"]
    predecessor = source["predecessor_source_node_id"]
    if provenance == "deleted_tombstone":
        if status != "deleted" or scope == "tool" or predecessor is None:
            raise ContractError("deleted source node has an invalid tombstone disposition")
        return "deleted_tombstone"
    if provenance == "tool":
        if status != "current" or scope != "tool":
            raise ContractError("tool source node has an invalid tool disposition")
        return "tool"
    if status != "current" or scope == "tool":
        raise ContractError("analysis source node has an invalid current disposition")
    return "analysis"


def source_partition_digests(sources: list[dict[str, Any]]) -> dict[str, str]:
    partitions: dict[str, list[dict[str, Any]]] = {
        "analysis": [],
        "deleted_tombstone": [],
        "tool": [],
    }
    for source in sources:
        identity = {
            key: source[key]
            for key in (
                "byte_length",
                "content_digest",
                "language",
                "path",
                "predecessor_source_node_id",
                "provenance",
                "scope",
                "source_node_id",
                "status",
            )
        }
        partitions[source_partition(source)].append(identity)
    return {
        partition: hashlib.sha256(
            (
                f"decodex/lane-authority-v2-source-partition/{partition}/1\0"
                + canonical_json(sorted(records, key=lambda record: record["source_node_id"]))
            ).encode("utf-8")
        ).hexdigest()
        for partition, records in partitions.items()
    }


def validate_candidate_replay_records(
    candidates: list[dict[str, Any]],
    *,
    source_paths: dict[str, str],
    expected_observations: dict[str, dict[str, Any]],
) -> None:
    indexed = _unique_index(candidates, "candidate_id", "candidate_records")
    if not indexed:
        raise ContractError("candidate relation must not be empty")
    identities: set[tuple[str, str, int, str]] = set()
    by_observation = {observation_id: [] for observation_id in expected_observations}
    for candidate in indexed.values():
        if candidate["source_node_id"] not in source_paths:
            raise ContractError("candidate references a missing source node")
        if candidate["candidate_digest"] != candidate_record_digest(candidate):
            raise ContractError("candidate record digest disagrees with its canonical fields")
        if candidate["candidate_id"] != canonical_candidate_id(candidate):
            raise ContractError("candidate id is not canonical for its source/category/line")
        identity = (
            candidate["source_node_id"],
            candidate["candidate_category"],
            candidate["line_number"],
            candidate["line_digest"],
        )
        if identity in identities:
            raise ContractError("candidate source/category/line identity is duplicated")
        identities.add(identity)
        if candidate["provenance"] != "c0_replay":
            raise ContractError("P1 candidate replay contains a post-C0 candidate")
        observed_origins: set[str] = set()
        for observation_id in candidate["c0_observation_ids"]:
            observation = expected_observations.get(observation_id)
            if observation is None:
                raise ContractError("candidate references an unknown C0 observation")
            if candidate["candidate_category"] != observation["category"]:
                raise ContractError("candidate category disagrees with its C0 observation")
            if source_paths[candidate["source_node_id"]] != observation["path"]:
                raise ContractError("candidate source path disagrees with its C0 observation")
            observed_origins.add(observation["origin"])
            by_observation[observation_id].append(candidate)
        if set(candidate["c0_origin_artifacts"]) != observed_origins:
            raise ContractError("candidate origin set disagrees with its C0 observations")
    for observation_id, observation in expected_observations.items():
        observed = by_observation[observation_id]
        if len(observed) != observation["candidate_line_count"]:
            raise ContractError("C0 candidate observation count disagrees with replay")
        line_records = [
            (candidate["line_number"], candidate["line_digest"])
            for candidate in observed
        ]
        if min(line for line, _ in line_records) != observation["first_line"]:
            raise ContractError("C0 candidate observation first line disagrees with replay")
        if c0_candidate_digest(line_records) != observation["candidate_digest"]:
            raise ContractError("C0 candidate observation digest disagrees with replay")
def validate_json_schema_documents(root: Path) -> None:
    ids: set[str] = set()
    for path in SCHEMA_PATHS:
        schema = load_json(root, path)
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise ContractError(f"{path} must use JSON Schema draft 2020-12")
        schema_id = schema.get("$id")
        if not isinstance(schema_id, str) or not schema_id:
            raise ContractError(f"{path} must have a non-empty $id")
        if schema_id in ids:
            raise ContractError(f"duplicate JSON Schema $id: {schema_id}")
        ids.add(schema_id)
        if schema.get("type") != "object" or schema.get("additionalProperties") is not False:
            raise ContractError(f"{path} must define a closed object")
        try:
            Draft202012Validator.check_schema(schema)
        except SchemaError as error:
            raise ContractError(f"invalid JSON Schema {path}: {error.message}") from error


def validate_instance(root: Path, instance_path: Path, schema_path: Path) -> None:
    instance = load_json(root, instance_path)
    schema = load_json(root, schema_path)
    try:
        Draft202012Validator(schema).validate(instance)
    except ValidationError as error:
        location = "/".join(str(part) for part in error.absolute_path) or "<root>"
        raise ContractError(
            f"{instance_path} violates {schema_path} at {location}: {error.message}"
        ) from error


def validate_typed_relation_manifest(
    manifest: dict[str, Any], relation_name: str, relation_schema: dict[str, Any]
) -> None:
    if set(manifest) != {"schema", "relation", "records"}:
        raise ContractError(f"relation manifest fields drifted: {relation_name}")
    expected_schema = f"decodex/lane-authority-v2-c1i-{relation_name.replace('_', '-')}/1"
    if manifest["schema"] != expected_schema or manifest["relation"] != relation_name:
        raise ContractError(f"relation manifest identity drifted: {relation_name}")
    records = manifest["records"]
    if not isinstance(records, list) or (relation_name in NONEMPTY_RELATIONS and not records):
        raise ContractError(f"relation manifest cardinality drifted: {relation_name}")
    validator = Draft202012Validator(
        {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": relation_schema["$defs"],
            "$ref": f"#/$defs/{RELATION_DEFINITIONS[relation_name]}",
        }
    )
    for index, record in enumerate(records):
        try:
            validator.validate(record)
        except ValidationError as error:
            raise ContractError(
                f"relation {relation_name} record {index} violates its schema: {error.message}"
            ) from error


def validate_rejection_contract(root: Path) -> None:
    reason_registry = load_json(root, REASON_CODES_PATH)
    fixture = load_json(root, INCOMPLETE_FIXTURE_PATH)
    report_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/rejection_report.schema.json")
    )
    reason_codes = reason_registry.get("reason_codes")
    if not isinstance(reason_codes, list) or not reason_codes:
        raise ContractError("rejection reason registry must be non-empty")
    if len(set(reason_codes)) != len(reason_codes) or reason_codes != sorted(reason_codes):
        raise ContractError("rejection reason codes must be unique and sorted")
    try:
        schema_codes = report_schema["properties"]["rejections"]["items"]["properties"][
            "reason_code"
        ]["enum"]
    except (KeyError, TypeError) as error:
        raise ContractError("rejection report schema lacks the reason-code enum") from error
    if schema_codes != reason_codes:
        raise ContractError("rejection report schema and reason registry disagree")
    if fixture != {
        "expected_exit_code": 1,
        "expected_reason_code": "c1i_phase_incomplete",
        "phase": "P0",
        "schema": "decodex/lane-authority-v2-c1i-readiness-negative/1",
        "status": "C1I_INCOMPLETE",
    }:
        raise ContractError("P0 negative readiness fixture drifted")


def validate_dataflow_contract(value: dict[str, Any], root: Path | None = None) -> None:
    expected_keys = {
        "accepted_proof_receipt_fields",
        "allowed_transfer_rules",
        "analysis",
        "limits",
        "schema",
        "sink_semantic_kinds",
        "top_reason_codes",
        "top_transition",
        "value_lattice",
    }
    if set(value) != expected_keys:
        raise ContractError("dataflow contract fields do not match its closed schema")
    if value.get("schema") != "decodex/lane-authority-v2-dataflow-contract/1":
        raise ContractError("unexpected dataflow contract schema")
    if value.get("value_lattice") != EXPECTED_LATTICE:
        raise ContractError("dataflow lattice must be the frozen Bottom-to-Top order")
    schema_root = root or Path(__file__).resolve().parents[2]
    schema = load_json(
        schema_root,
        Path("tools/lane-authority-inventory/contracts/dataflow_contract.schema.json"),
    )
    properties = schema["properties"]
    for field in (
        "value_lattice",
        "limits",
        "analysis",
        "allowed_transfer_rules",
        "top_reason_codes",
        "top_transition",
        "sink_semantic_kinds",
        "accepted_proof_receipt_fields",
    ):
        if value.get(field) != properties[field]["const"]:
            raise ContractError(f"dataflow {field} drifted from its frozen schema")
    rules = value.get("allowed_transfer_rules")
    if not isinstance(rules, list) or not rules:
        raise ContractError("dataflow contract must define transfer rules")
    rule_ids = [rule.get("id") for rule in rules if isinstance(rule, dict)]
    if (
        len(rule_ids) != len(rules)
        or len(set(rule_ids)) != len(rule_ids)
        or set(rule_ids) != EXPECTED_TRANSFER_RULES
    ):
        raise ContractError("dataflow transfer rule closure drifted")
    if any("Top" in rule.get("inputs", []) for rule in rules):
        raise ContractError("Top must be absorbing")
    top_codes = value.get("top_reason_codes")
    if not isinstance(top_codes, list) or "unresolved_call" not in top_codes:
        raise ContractError("dataflow contract must make unresolved calls Top")
    receipt_fields = value.get("accepted_proof_receipt_fields")
    if not isinstance(receipt_fields, list) or "fixed_point_digest" not in receipt_fields:
        raise ContractError("dataflow proof receipts must bind the fixed-point digest")


def validate_catalog_p0(value: dict[str, Any]) -> None:
    expected_keys = {
        "catalog_semantic_digest",
        "catalog_status",
        "dynamic_capability_roots",
        "executable_declarative_paths",
        "external_symbols",
        "languages",
        "local_closure_boundaries",
        "persistent_data_roots",
        "provider_and_config_roots",
        "review_gate",
        "reviewed_non_authority_external_symbols",
        "schema",
        "toolchain_matrix",
        "used_external_symbol_set_digest",
    }
    if set(value) != expected_keys:
        raise ContractError("authority catalog fields do not match its closed schema")
    if value.get("schema") != "decodex/lane-authority-v2-authority-surface-catalog/1":
        raise ContractError("unexpected authority catalog schema")
    if value.get("catalog_status") != "p0_schema_only_incomplete":
        raise ContractError("P0 catalog must remain explicitly incomplete")
    if value.get("catalog_semantic_digest") is not None:
        raise ContractError("P0 catalog must not claim a semantic digest")
    if value.get("used_external_symbol_set_digest") is not None:
        raise ContractError("P0 catalog must not claim a used-symbol digest")
    if value.get("languages") != ["python", "rust", "shell", "swift", "toml", "yaml"]:
        raise ContractError("P0 catalog must enumerate every supported source language")
    sections = (
        "dynamic_capability_roots",
        "executable_declarative_paths",
        "external_symbols",
        "local_closure_boundaries",
        "persistent_data_roots",
        "provider_and_config_roots",
        "reviewed_non_authority_external_symbols",
        "toolchain_matrix",
    )
    for section in sections:
        if value.get(section) != []:
            raise ContractError(f"P0 catalog section {section} must be empty until P3")
    review_gate = value.get("review_gate")
    expected_review_gate = {
        "architecture_review_complete": True,
        "p0_p4_machine_validation_only": True,
        "p5_integrated_digest_requires_fresh_review": True,
        "semantic_change_invalidates_ready_review": True,
    }
    if review_gate != expected_review_gate:
        raise ContractError("catalog review invalidation rules must all be enabled")


def external_symbol_policy_semantic_digest(value: dict[str, Any]) -> str:
    semantic_value = {
        "entries": value["entries"],
        "policy_status": value["policy_status"],
        "schema": value["schema"],
    }
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-external-symbol-policy/1\n"
            + canonical_json(semantic_value)
        ).encode("utf-8")
    ).hexdigest()


def authority_symbol_policy_semantic_digest(value: dict[str, Any]) -> str:
    semantic_value = {
        "entries": value["entries"],
        "policy_status": value["policy_status"],
        "schema": value["schema"],
    }
    return hashlib.sha256(
        (
            "decodex/lane-authority-v2-authority-symbol-policy/1\n"
            + canonical_json(semantic_value)
        ).encode("utf-8")
    ).hexdigest()


def validate_authority_symbol_policy(value: dict[str, Any]) -> None:
    expected_keys = {"entries", "policy_semantic_digest", "policy_status", "schema"}
    if set(value) != expected_keys:
        raise ContractError("authority symbol policy fields do not match its closed schema")
    if value.get("schema") != "decodex/lane-authority-v2-authority-symbol-policy/1":
        raise ContractError("unexpected authority symbol policy schema")
    if value.get("policy_status") != "p3_machine_validated_review_pending":
        raise ContractError("authority symbol policy must remain review-pending before P5")
    entries = value.get("entries")
    if not isinstance(entries, list) or not entries:
        raise ContractError("authority symbol policy must be non-empty")
    identities = [(entry["language"], entry["signature"]) for entry in entries]
    if identities != sorted(identities):
        raise ContractError("authority symbol policy entries must be language/signature sorted")
    if len(set(identities)) != len(identities):
        raise ContractError("authority symbol policy contains a duplicate language/signature")
    ids = [entry["id"] for entry in entries]
    if len(set(ids)) != len(ids):
        raise ContractError("authority symbol policy contains a duplicate id")
    if any("?" in entry["signature"] or "*" in entry["signature"] for entry in entries):
        raise ContractError("authority symbol policy forbids wildcard signatures")
    if value["policy_semantic_digest"] != authority_symbol_policy_semantic_digest(value):
        raise ContractError("authority symbol policy semantic digest disagrees")


def validate_external_symbol_policy(value: dict[str, Any]) -> None:
    expected_keys = {"entries", "policy_semantic_digest", "policy_status", "schema"}
    if set(value) != expected_keys:
        raise ContractError("external symbol policy fields do not match its closed schema")
    if value.get("schema") != "decodex/lane-authority-v2-external-symbol-policy/1":
        raise ContractError("unexpected external symbol policy schema")
    if value.get("policy_status") != "p3_machine_validated_review_pending":
        raise ContractError("external symbol policy must remain review-pending before P5")
    entries = value.get("entries")
    if not isinstance(entries, list) or not entries:
        raise ContractError("external symbol policy must be non-empty")
    identities = [(entry["language"], entry["signature"]) for entry in entries]
    if identities != sorted(identities):
        raise ContractError("external symbol policy entries must be language/signature sorted")
    if len(set(identities)) != len(identities):
        raise ContractError("external symbol policy contains a duplicate language/signature")
    ids = [entry["id"] for entry in entries]
    if len(set(ids)) != len(ids):
        raise ContractError("external symbol policy contains a duplicate id")
    if any("?" in entry["signature"] or "*" in entry["signature"] for entry in entries):
        raise ContractError("external symbol policy forbids wildcard signatures")
    allowed_capabilities = {
        "assertion",
        "in_memory_construction",
        "presentation",
        "pure_data",
    }
    if any(entry["capability_class"] not in allowed_capabilities for entry in entries):
        raise ContractError("external symbol policy contains an authority-capable class")
    if value["policy_semantic_digest"] != external_symbol_policy_semantic_digest(value):
        raise ContractError("external symbol policy semantic digest disagrees")


def policy_catalog_entry(
    policy_entry: dict[str, Any], consumer_ids: set[str]
) -> dict[str, Any]:
    return {
        "authority_relevance": "reviewed_non_authority",
        "consumer_ids": sorted(consumer_ids),
        "entry_kind": "external_symbol",
        "id": policy_entry["id"],
        "language": policy_entry["language"],
        "match_mode": "exact_symbol",
        "owner": None,
        "ownership": "not_applicable",
        "reason_code": policy_entry["reason_code"],
        "removal_checkpoint": None,
        "replacement_kind": None,
        "semantic_kinds": policy_entry["semantic_kinds"],
        "signature": policy_entry["signature"],
        "used_site_set_digest": stable_id_set_digest(
            "decodex/lane-authority-v2-catalog-entry-used-sites/1", consumer_ids
        ),
    }


def authority_policy_catalog_entry(
    policy_entry: dict[str, Any], consumer_ids: set[str]
) -> dict[str, Any]:
    return {
        "authority_relevance": "authority_surface",
        "consumer_ids": sorted(consumer_ids),
        "entry_kind": "external_symbol",
        "id": policy_entry["id"],
        "language": policy_entry["language"],
        "match_mode": "exact_symbol",
        "owner": policy_entry["owner"],
        "ownership": policy_entry["ownership"],
        "reason_code": policy_entry["reason_code"],
        "removal_checkpoint": policy_entry["removal_checkpoint"],
        "replacement_kind": policy_entry["replacement_kind"],
        "semantic_kinds": policy_entry["semantic_kinds"],
        "signature": policy_entry["signature"],
        "used_site_set_digest": stable_id_set_digest(
            "decodex/lane-authority-v2-catalog-entry-used-sites/1", consumer_ids
        ),
    }


def validate_catalog_p3_policy_projection(
    catalog: dict[str, Any],
    policy: dict[str, Any],
    authority_policy: dict[str, Any],
    *,
    allow_pending_authority_projection: bool = False,
) -> None:
    if catalog.get("catalog_status") != "p3_machine_validated_incomplete":
        raise ContractError("P3 policy catalog must remain explicitly incomplete")
    empty_sections = (
        "dynamic_capability_roots",
        "executable_declarative_paths",
        "local_closure_boundaries",
        "persistent_data_roots",
        "provider_and_config_roots",
        "toolchain_matrix",
    )
    for section in empty_sections:
        if catalog.get(section) != []:
            raise ContractError(f"P3 policy cut cannot populate {section}")
    sections = (
        (
            "reviewed_non_authority_external_symbols",
            policy,
            policy_catalog_entry,
            "non-authority",
        ),
        ("external_symbols", authority_policy, authority_policy_catalog_entry, "authority"),
    )
    all_policy_ids: set[str] = set()
    all_policy_identities: set[tuple[str, str]] = set()
    for section, section_policy, builder, label in sections:
        entries = catalog.get(section)
        if (
            allow_pending_authority_projection
            and label == "authority"
        ):
            continue
        if not isinstance(entries, list) or not entries:
            raise ContractError(f"P3 catalog must contain {label} policy entries")
        policy_by_id = {entry["id"]: entry for entry in section_policy["entries"]}
        catalog_by_id = _unique_index(entries, "id", f"P3 {label} policy catalog")
        if set(catalog_by_id) != set(policy_by_id):
            raise ContractError(f"P3 catalog entries do not equal the {label} policy")
        if all_policy_ids & set(policy_by_id):
            raise ContractError("P3 authority policies contain duplicate ids")
        all_policy_ids.update(policy_by_id)
        identities = {
            (entry["language"], entry["signature"])
            for entry in section_policy["entries"]
        }
        if all_policy_identities & identities:
            raise ContractError("P3 authority policies overlap by language/signature")
        all_policy_identities.update(identities)
        for entry_id, catalog_entry in catalog_by_id.items():
            expected = builder(
                policy_by_id[entry_id], set(catalog_entry["consumer_ids"])
            )
            if catalog_entry != expected:
                raise ContractError(f"P3 {label} catalog fields drifted from policy")
            if not catalog_entry["consumer_ids"]:
                raise ContractError(f"P3 {label} policy entry has no exact source consumer")
    if catalog["catalog_semantic_digest"] != catalog_semantic_digest(catalog):
        raise ContractError("P3 catalog semantic digest disagrees")


def validate_checkpoint_p0(value: dict[str, Any], root: Path | None = None) -> None:
    expected_keys = {
        "advancement_state",
        "provisional_analysis_cut_anchor",
        "candidate_anchors",
        "catalog_status",
        "review_cadence_policy",
        "migration_state",
        "phase",
        "plan_review",
        "readiness_expectation",
        "schema",
        "unresolved_state",
    }
    if set(value) != expected_keys:
        raise ContractError("P0 checkpoint fields do not match its closed schema")
    schema_root = root or Path(__file__).resolve().parents[2]
    schema = load_json(
        schema_root,
        Path("tools/lane-authority-inventory/contracts/p0_checkpoint.schema.json"),
    )
    schema_fields = set(schema.get("properties", {}))
    if schema_fields != expected_keys or set(schema.get("required", [])) != expected_keys:
        raise ContractError("P0 checkpoint schema does not close over every checkpoint field")
    plan_review = value.get("plan_review", {})
    if plan_review.get("verdict") != "APPROVE" or plan_review.get("material_findings") != 0:
        raise ContractError("P0 checkpoint plan review receipt drifted")
    if plan_review.get("scope") != "plan_contract_only":
        raise ContractError("P0 plan review overstates its scope")
    if plan_review.get("future_independent_review_gates") != [
        "P5_exact_head_ready",
        "C7_exact_head_land",
    ]:
        raise ContractError("future independent review gates drifted")
    if value.get("review_cadence_policy") != {
        "architecture_review": "complete_at_c0",
        "integrated_ready_review": "p5_exact_head",
        "intermediate_phases": "machine_validation_only",
        "lane_v2_land_review": "c7_exact_head",
    }:
        raise ContractError("C1I review cadence policy drifted")
    if value.get("readiness_expectation") != {
        "exit_code": 1,
        "reason_code": "c1i_phase_incomplete",
    }:
        raise ContractError("P0 readiness expectation drifted")


def validate_composition_schema(root: Path) -> None:
    schema = load_json(
        root,
        Path("tools/lane-authority-inventory/contracts/inventory_composition.schema.json"),
    )
    relations = schema["properties"]["relations"]
    if set(relations["required"]) != EXPECTED_RELATIONS:
        raise ContractError("required inventory relation closure drifted")
    if set(relations["properties"]) != EXPECTED_RELATIONS:
        raise ContractError("inventory relation property closure drifted")
    for definition in ("candidate_adjudication", "site_classification", "cfg_projection"):
        if definition not in schema.get("$defs", {}):
            raise ContractError(f"inventory composition lacks {definition}")
    if (
        schema["properties"]["cross_relation_invariants"].get("const")
        != EXPECTED_CROSS_RELATION_INVARIANTS
    ):
        raise ContractError("cross-relation integrity obligations drifted")
    for relation, receipt_schema in relations["properties"].items():
        constraints = receipt_schema.get("allOf", [{}, {}])[1].get("properties", {})
        expected_path = f"tools/lane-authority-inventory/manifests/relations/{relation}.json"
        if constraints.get("path", {}).get("const") != expected_path:
            raise ContractError(f"relation receipt path is not bound: {relation}")
        expected_schema = f"decodex/lane-authority-v2-c1i-{relation.replace('_', '-')}/1"
        if constraints.get("schema", {}).get("const") != expected_schema:
            raise ContractError(f"relation receipt schema is not bound: {relation}")
    relation_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json")
    )
    typed_relations = {
        branch["properties"]["relation"]["const"]
        for branch in relation_schema.get("oneOf", [])
    }
    if typed_relations != EXPECTED_RELATIONS:
        raise ContractError("typed relation manifest does not close over every relation")
    for branch in relation_schema["oneOf"]:
        records = branch["properties"].get("records", {})
        if "$ref" not in records.get("items", {}):
            raise ContractError("relation manifest branch lacks a typed record schema")


def validate_output_policy(root: Path) -> None:
    validate_instance(
        root,
        OUTPUT_POLICY_PATH,
        Path(
            "tools/lane-authority-inventory/contracts/output_artifact_manifest.schema.json"
        ),
    )
    policy = load_json(root, OUTPUT_POLICY_PATH)
    artifacts = policy["artifacts"]
    paths = [artifact["path"] for artifact in artifacts]
    if len(paths) != len(set(paths)):
        raise ContractError("output artifact policy contains duplicate paths")
    actual = {
        artifact["path"]: (
            artifact["role"],
            artifact["binding"],
            artifact["accepted_authority"],
        )
        for artifact in artifacts
    }
    if actual != EXPECTED_OUTPUT_ARTIFACTS:
        raise ContractError("output artifact path/role/binding/authority closure drifted")
    relation_paths = {
        artifact["path"]
        for artifact in artifacts
        if artifact["binding"] == "composition_relation_receipt"
    }
    expected_relation_paths = {
        f"tools/lane-authority-inventory/manifests/relations/{relation}.json"
        for relation in EXPECTED_RELATIONS
    }
    if relation_paths != expected_relation_paths:
        raise ContractError("output artifact policy does not close over every relation")
    required_roles = {
        "analysis_cut",
        "cfg_coverage",
        "composition",
        "dataflow_proof",
        "ledger",
        "rejection_report",
        "review_receipt",
    }
    if not required_roles.issubset({artifact["role"] for artifact in artifacts}):
        raise ContractError("output artifact policy lacks a required output role")


def _unique_index(records: list[dict[str, Any]], field: str, relation: str) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for record in records:
        identifier = record[field]
        if identifier in result:
            raise ContractError(f"{relation} contains duplicate {field}: {identifier}")
        result[identifier] = record
    return result


def replay_rust_type_path_resolution(
    resolution: dict[str, Any],
    scopes: dict[str, dict[str, Any]],
    bindings: dict[str, dict[str, Any]],
    replay_index: dict[str, Any] | None = None,
) -> None:
    chain = resolution["binding_ids"]
    cursor = 0
    if replay_index is None:
        bindings_by_scope_name: dict[tuple[str, str, str], list[str]] = {}
        for binding in bindings.values():
            bindings_by_scope_name.setdefault(
                (
                    binding["crate_target_id"],
                    binding["scope_id"],
                    binding["local_name"],
                ),
                [],
            ).append(binding["binding_id"])
        replay_index = {
            "bindings_by_scope_name": bindings_by_scope_name,
            "roots": {
                scope["crate_target_id"]: scope["scope_id"]
                for scope in scopes.values()
                if scope["scope_kind"] == "crate_root"
            },
        }
    roots = replay_index["roots"]
    bindings_by_scope_name = replay_index["bindings_by_scope_name"]

    def module_scope(scope_id: str) -> dict[str, Any]:
        scope = scopes[scope_id]
        while scope["scope_kind"] == "block":
            scope = scopes[scope["parent_scope_id"]]
        return scope

    def parent_module(scope_id: str) -> dict[str, Any] | None:
        scope = module_scope(scope_id)
        if scope["parent_scope_id"] is None:
            return None
        return module_scope(scope["parent_scope_id"])

    def visible_from(binding: dict[str, Any], accessing_scope_id: str) -> bool:
        visibility = binding["visibility"]
        if visibility in {"public", "crate"}:
            return True
        declaring = module_scope(binding["scope_id"])
        accessing_path = module_scope(accessing_scope_id)["canonical_module_path"]
        allowed_path = declaring["canonical_module_path"]
        if visibility == "super":
            parent = parent_module(declaring["scope_id"])
            if parent is None:
                return False
            allowed_path = parent["canonical_module_path"]
        elif visibility == "in":
            segments = [
                segment
                for segment in (binding["visibility_path"] or "").split("::")
                if segment
            ]
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
                current = declaring
                while segments and segments[0] == "super":
                    parent = parent_module(current["scope_id"])
                    if parent is None:
                        return False
                    current = parent
                    segments = segments[1:]
                allowed_path = current["canonical_module_path"]
            else:
                return False
            if segments:
                allowed_path = f"{allowed_path}::{'::'.join(segments)}"
        elif visibility != "private":
            return False
        return accessing_path == allowed_path or accessing_path.startswith(
            f"{allowed_path}::"
        )

    def select(
        target_id: str,
        scope_id: str,
        accessing_scope_id: str,
        name: str,
        *,
        lexical: bool,
        require_module: bool,
        excluded_binding_id: str | None = None,
    ) -> tuple[str, str | None]:
        current_id: str | None = scope_id
        while current_id is not None:
            all_candidates = [
                binding_id
                for binding_id in bindings_by_scope_name.get(
                    (target_id, current_id, name), []
                )
                if binding_id != excluded_binding_id
                and bindings[binding_id]["local_name"] != "_"
            ]
            candidates = [
                binding_id
                for binding_id in all_candidates
                if visible_from(bindings[binding_id], accessing_scope_id)
            ]
            if all_candidates and not candidates:
                return "inaccessible", None
            declarations = [
                binding_id
                for binding_id in candidates
                if bindings[binding_id]["binding_kind"]
                in {"module", "type_declaration"}
            ]
            if require_module:
                modules = [
                    binding_id
                    for binding_id in candidates
                    if bindings[binding_id]["binding_kind"] == "module"
                ]
                if modules:
                    candidates = modules
            elif declarations:
                candidates = declarations
            if len(candidates) > 1:
                return ("ambiguous" if declarations else "unresolved"), None
            if len(candidates) == 1:
                candidate = bindings[candidates[0]]
                if (
                    candidate["binding_kind"] in {"module", "type_declaration"}
                    and candidate["resolution"] == "ambiguous"
                ):
                    return "ambiguous", None
                return "found", candidates[0]
            if not lexical or scopes[current_id]["scope_kind"] != "block":
                break
            current_id = scopes[current_id]["parent_scope_id"]
        return "missing", None

    def consume(binding_id: str, stack: tuple[str, ...]) -> dict[str, Any]:
        nonlocal cursor
        if cursor >= len(chain) or chain[cursor] != binding_id:
            raise ContractError("P2 Rust path resolution binding chain is not replayable")
        cursor += 1
        if binding_id in stack:
            return {"status": "cycle"}
        binding = bindings[binding_id]
        if binding["visibility"] == "unsupported" or binding["binding_kind"] == "glob":
            return {"status": "unsupported"}
        if binding["binding_kind"] == "module":
            target_scope_id = binding["target_scope_id"]
            if target_scope_id is None:
                return {"status": binding["resolution"]}
            return {
                "canonical_module_scope_id": target_scope_id,
                "canonical_path": scopes[target_scope_id]["canonical_module_path"],
                "canonical_type_definition_site_id": None,
                "status": "resolved_local_module",
            }
        if binding["binding_kind"] == "type_declaration":
            target_symbol_id = binding["target_symbol_site_id"]
            if target_symbol_id is None:
                return {"status": binding["resolution"]}
            return {
                "canonical_module_scope_id": None,
                "canonical_path": (
                    f"{module_scope(binding['scope_id'])['canonical_module_path']}"
                    f"::{binding['local_name']}"
                ),
                "canonical_type_definition_site_id": target_symbol_id,
                "status": "resolved_local_type",
            }

        surface = binding["surface_target_path"] or ""
        segments = [segment for segment in surface.split("::") if segment]
        if not segments:
            return {"status": "unsupported"}
        target_id = binding["crate_target_id"]
        current_scope = module_scope(binding["scope_id"])
        index = 0
        terminal: dict[str, Any] | None = None
        if segments[0] == "crate":
            current_scope = scopes[roots[target_id]]
            index = 1
        elif segments[0] == "self":
            index = 1
        elif segments[0] == "super":
            while index < len(segments) and segments[index] == "super":
                parent = parent_module(current_scope["scope_id"])
                if parent is None:
                    return {"status": "unsupported"}
                current_scope = parent
                index += 1
        else:
            state, found = select(
                target_id,
                binding["scope_id"],
                binding["scope_id"],
                segments[0],
                lexical=True,
                require_module=len(segments) > 1,
                excluded_binding_id=binding_id,
            )
            if state == "missing":
                root = scopes[roots[target_id]]
                if segments[0] in root["target_extern_crate_names"]:
                    return {
                        "canonical_module_scope_id": None,
                        "canonical_path": surface,
                        "canonical_type_definition_site_id": None,
                        "status": "external",
                    }
                return {"status": "unresolved"}
            if state != "found":
                return {"status": state}
            terminal = consume(found, stack + (binding_id,))
            index = 1

        while index < len(segments):
            if terminal is not None:
                if terminal["status"] == "external":
                    return {
                        **terminal,
                        "canonical_path": (
                            f"{terminal['canonical_path']}::"
                            f"{'::'.join(segments[index:])}"
                        ),
                    }
                if terminal["status"] != "resolved_local_module":
                    return terminal
                current_scope = scopes[terminal["canonical_module_scope_id"]]
            state, found = select(
                target_id,
                current_scope["scope_id"],
                binding["scope_id"],
                segments[index],
                lexical=False,
                require_module=index < len(segments) - 1,
            )
            if state != "found":
                return {"status": "unresolved" if state == "missing" else state}
            terminal = consume(found, stack + (binding_id,))
            index += 1
        if terminal is not None:
            return terminal
        return {
            "canonical_module_scope_id": current_scope["scope_id"],
            "canonical_path": current_scope["canonical_module_path"],
            "canonical_type_definition_site_id": None,
            "status": "resolved_local_module",
        }

    replayed = consume(resolution["source_binding_id"], ())
    if cursor != len(chain):
        raise ContractError("P2 Rust path resolution binding chain has trailing hops")
    for field in (
        "canonical_module_scope_id",
        "canonical_path",
        "canonical_type_definition_site_id",
        "status",
    ):
        if resolution[field] != replayed.get(field):
            raise ContractError("P2 Rust path resolution replay disagrees")


def validate_cross_relation_records(
    records: dict[str, list[dict[str, Any]]],
    *,
    expected_source_partitions: dict[str, int],
    expected_source_partition_digests: dict[str, str],
    expected_candidate_observations: dict[str, dict[str, Any]],
    catalog: dict[str, Any],
) -> None:
    if set(records) != EXPECTED_RELATIONS:
        raise ContractError("relation record set is not closed")
    _reject_unresolved_reason_codes(records)
    sources = _unique_index(records["source_nodes"], "source_node_id", "source_nodes")
    expected_partition_keys = {"analysis", "deleted_tombstone", "tool"}
    if (
        set(expected_source_partitions) != expected_partition_keys
        or expected_source_partitions["analysis"] <= 0
        or expected_source_partitions["deleted_tombstone"] < 0
        or expected_source_partitions["tool"] <= 0
    ):
        raise ContractError("expected source partitions are not a complete analysis cut")
    actual_source_partitions = {key: 0 for key in expected_partition_keys}
    for source in sources.values():
        actual_source_partitions[source_partition(source)] += 1
    if actual_source_partitions != expected_source_partitions:
        raise ContractError("source node partitions disagree with the exact analysis cut")
    if set(expected_source_partition_digests) != expected_partition_keys:
        raise ContractError("expected source partition digests are incomplete")
    if source_partition_digests(list(sources.values())) != expected_source_partition_digests:
        raise ContractError("source node partition digests disagree with the exact analysis cut")
    syntax = _unique_index(records["syntax_sites"], "site_id", "syntax_sites")
    symbols = _unique_index(records["symbol_sites"], "site_id", "symbol_sites")
    data = _unique_index(records["data_sites"], "site_id", "data_sites")
    all_sites = {**syntax, **symbols, **data}
    if len(all_sites) != len(syntax) + len(symbols) + len(data):
        raise ContractError("site ids must be globally unique")
    syntax_ids_by_source = {source_id: set() for source_id in sources}
    parser_roots_by_source = {source_id: [] for source_id in sources}
    for site in syntax.values():
        if site["source_node_id"] not in sources:
            raise ContractError("syntax site references a missing source node")
        if site["byte_end"] < site["byte_start"]:
            raise ContractError("syntax site byte range is reversed")
        if site["byte_end"] > sources[site["source_node_id"]]["byte_length"]:
            raise ContractError("syntax site byte range exceeds source bytes")
        if site["recovery_state"] != "clean":
            raise ContractError("accepted syntax relation contains ERROR or MISSING recovery")
        syntax_ids_by_source[site["source_node_id"]].add(site["site_id"])
        if site["is_parser_root"]:
            parser_roots_by_source[site["source_node_id"]].append(site)
    for source_id, source in sources.items():
        syntax_ids = syntax_ids_by_source[source_id]
        if source["syntax_site_count"] != len(syntax_ids):
            raise ContractError("source syntax site count disagrees with syntax relation")
        if source["parser_node_count"] < source["syntax_site_count"]:
            raise ContractError("source materialized more syntax sites than parser nodes")
        if source["status"] == "current" and source["parser_error_count"] != 0:
            raise ContractError("accepted source contains parser recovery nodes")
        if source["syntax_site_ids_digest"] != stable_id_set_digest(
            "decodex/lane-authority-v2-source-syntax-sites/1", syntax_ids
        ):
            raise ContractError("source syntax site digest disagrees with syntax relation")
        if source["status"] == "deleted":
            if source["parser_receipt_id"] is not None or source["zero_syntax_reason_code"] != "deleted_tombstone":
                raise ContractError("deleted source has invalid syntax coverage evidence")
        elif source["parser_receipt_id"] is None:
            raise ContractError("current source lacks a parser receipt")
        elif not syntax_ids or source["zero_syntax_reason_code"] is not None:
            raise ContractError("current source must publish at least its root syntax node")
        elif (
            len(parser_roots_by_source[source_id]) != 1
            or parser_roots_by_source[source_id][0]["byte_start"] != 0
            or parser_roots_by_source[source_id][0]["byte_end"] != source["byte_length"]
        ):
            raise ContractError("current source must publish exactly one full-byte parser root")
        elif parser_roots_by_source[source_id][0]["node_kind"] != PARSER_ROOT_KINDS[source["language"]]:
            raise ContractError("parser root kind disagrees with source language")
    for site in [*symbols.values(), *data.values()]:
        if site["syntax_site_id"] not in syntax:
            raise ContractError("derived site references a missing syntax site")
    for site in symbols.values():
        source = sources[syntax[site["syntax_site_id"]]["source_node_id"]]
        if site["language"] != source["language"]:
            raise ContractError("symbol language disagrees with its source")
    if any(site["resolution"] == "unresolved" for site in symbols.values()):
        raise ContractError("accepted symbol relation contains unresolved targets")
    for site in symbols.values():
        for definition_site_id in site["definition_site_ids"]:
            definition = symbols.get(definition_site_id)
            if definition is None or definition["resolution"] != "declaration":
                raise ContractError("local symbol target references a missing declaration")

    projection_kinds_by_source: dict[str, set[str]] = {}
    projections = _unique_index(
        records["cfg_projections"], "projection_id", "cfg_projections"
    )
    for projection in records["cfg_projections"]:
        site_id = projection["site_id"]
        if site_id not in syntax:
            raise ContractError("cfg projection references a missing syntax site")
        source = sources[syntax[site_id]["source_node_id"]]
        if projection["language"] != source["language"]:
            raise ContractError("cfg projection language disagrees with its source")
        projection_kinds_by_source.setdefault(source["source_node_id"], set()).add(
            projection["projection_kind"]
        )
    current_source_ids = {
        source_id for source_id, source in sources.items() if source["status"] == "current"
    }
    if any(
        projection_kinds_by_source.get(source_id, set()) != {"config", "target"}
        for source_id in current_source_ids
    ):
        raise ContractError("every current source must have config and target cfg projections")

    classifications = _unique_index(
        records["site_classifications"], "site_id", "site_classifications"
    )
    if set(classifications) != set(all_sites):
        raise ContractError("every site must have exactly one site classification")
    for classification in classifications.values():
        site_id = classification["site_id"]
        syntax_site_id = site_id if site_id in syntax else all_sites[site_id]["syntax_site_id"]
        source = sources[syntax[syntax_site_id]["source_node_id"]]
        if classification["scope"] != source["scope"]:
            raise ContractError("site classification scope disagrees with its source")
        resolved_classification_projections: dict[str, dict[str, Any]] = {}
        for field, kind in (("config_projection", "config"), ("target_projection", "target")):
            projection = projections.get(classification[field])
            if projection is None:
                raise ContractError(f"site classification {field} is unresolved")
            projection_source_id = syntax[projection["site_id"]]["source_node_id"]
            classified_source_id = syntax[syntax_site_id]["source_node_id"]
            if projection_source_id != classified_source_id or projection["projection_kind"] != kind:
                raise ContractError(f"site classification {field} belongs to another site or kind")
            resolved_classification_projections[kind] = projection
        if (
            resolved_classification_projections["config"]["platform"]
            != resolved_classification_projections["target"]["platform"]
        ):
            raise ContractError("site classification target/config platforms disagree")

    candidates = _unique_index(
        records["candidate_records"], "candidate_id", "candidate_records"
    )
    if not candidates:
        raise ContractError("candidate relation must not be empty")
    candidate_identities: set[tuple[str, str, int, str]] = set()
    candidates_by_observation = {
        observation_id: [] for observation_id in expected_candidate_observations
    }
    for candidate in candidates.values():
        if candidate["source_node_id"] not in sources:
            raise ContractError("candidate references a missing source node")
        if candidate["candidate_digest"] != candidate_record_digest(candidate):
            raise ContractError("candidate record digest disagrees with its canonical fields")
        if candidate["candidate_id"] != canonical_candidate_id(candidate):
            raise ContractError("candidate id is not canonical for its source/category/line")
        identity = (
            candidate["source_node_id"],
            candidate["candidate_category"],
            candidate["line_number"],
            candidate["line_digest"],
        )
        if identity in candidate_identities:
            raise ContractError("candidate source/category/line identity is duplicated")
        candidate_identities.add(identity)
        observation_ids = candidate["c0_observation_ids"]
        origins = candidate["c0_origin_artifacts"]
        if candidate["provenance"] == "c0_replay" and not observation_ids:
            raise ContractError("C0 candidate lacks an observation")
        if candidate["provenance"] == "post_c0" and (observation_ids or origins):
            raise ContractError("post-C0 candidate claims C0 evidence")
        observed_origins: set[str] = set()
        for observation_id in observation_ids:
            observation = expected_candidate_observations.get(observation_id)
            if observation is None:
                raise ContractError("candidate references an unknown C0 observation")
            source = sources[candidate["source_node_id"]]
            if source["path"] != observation["path"]:
                raise ContractError("candidate source path disagrees with its C0 observation")
            if candidate["candidate_category"] != observation["category"]:
                raise ContractError("candidate category disagrees with its C0 observation")
            observed_origins.add(observation["origin"])
            candidates_by_observation[observation_id].append(candidate)
        if set(origins) != observed_origins:
            raise ContractError("candidate origin set disagrees with its C0 observations")
    for observation_id, observation in expected_candidate_observations.items():
        observed = candidates_by_observation[observation_id]
        if len(observed) != observation["candidate_line_count"]:
            raise ContractError("C0 candidate observation count disagrees with replay")
        line_records = [
            (candidate["line_number"], candidate["line_digest"])
            for candidate in observed
        ]
        if min(line for line, _ in line_records) != observation["first_line"]:
            raise ContractError("C0 candidate observation first line disagrees with replay")
        if c0_candidate_digest(line_records) != observation["candidate_digest"]:
            raise ContractError("C0 candidate observation digest disagrees with replay")
    adjudications = _unique_index(
        records["candidate_adjudications"], "candidate_id", "candidate_adjudications"
    )
    if set(adjudications) != set(candidates):
        raise ContractError("every candidate must have exactly one adjudication")
    edge_sites: dict[str, set[str]] = {candidate_id: set() for candidate_id in candidates}
    for edge in records["candidate_site_edges"]:
        candidate_id = edge["candidate_id"]
        site_id = edge["site_id"]
        if candidate_id not in candidates or site_id not in all_sites:
            raise ContractError("candidate-site edge has a missing endpoint")
        if site_id in edge_sites[candidate_id]:
            raise ContractError("candidate-site edge is duplicated")
        edge_sites[candidate_id].add(site_id)
    for candidate_id, adjudication in adjudications.items():
        if adjudication["candidate_category"] != candidates[candidate_id]["candidate_category"]:
            raise ContractError("candidate adjudication category disagrees with its candidate")
        related = set(adjudication["related_site_ids"])
        if related != edge_sites[candidate_id]:
            raise ContractError("candidate adjudication related sites disagree with edge set")
        covered = adjudication["disposition"] == "covered_by_sites"
        if covered != bool(related):
            raise ContractError("candidate adjudication disposition cardinality is invalid")

    for relation in ("call_edges", "dataflow_edges"):
        if not records[relation]:
            raise ContractError(f"{relation} must not be empty")
        _unique_index(records[relation], "edge_id", relation)
        for edge in records[relation]:
            if edge["from_site_id"] not in all_sites or edge["to_site_id"] not in all_sites:
                raise ContractError(f"{relation} contains a missing endpoint")
    call_targets: dict[str, set[str]] = {site_id: set() for site_id in symbols}
    for edge in records["call_edges"]:
        if edge["from_site_id"] not in symbols or edge["to_site_id"] not in symbols:
            raise ContractError("accepted call edge must connect symbol sites")
        call_targets[edge["from_site_id"]].add(edge["to_site_id"])
    for site_id, site in symbols.items():
        expected = set(site["definition_site_ids"])
        if call_targets[site_id] != expected:
            raise ContractError("symbol definition set disagrees with call edges")

    external_symbols = {
        site_id for site_id, site in symbols.items() if site["resolution"] == "external"
    }
    dispositions = _unique_index(
        records["catalog_entry_dispositions"],
        "disposition_id",
        "catalog_entry_dispositions",
    )
    catalog_sections = (
        "dynamic_capability_roots",
        "executable_declarative_paths",
        "external_symbols",
        "local_closure_boundaries",
        "persistent_data_roots",
        "provider_and_config_roots",
        "reviewed_non_authority_external_symbols",
    )
    catalog_entries: dict[str, dict[str, Any]] = {}
    for section in catalog_sections:
        for entry in catalog[section]:
            if entry["id"] in catalog_entries:
                raise ContractError(f"authority catalog contains duplicate id: {entry['id']}")
            catalog_entries[entry["id"]] = entry
    used_sites_by_catalog_entry = {entry_id: set() for entry_id in catalog_entries}
    absent_receipts_by_catalog_entry = {entry_id: 0 for entry_id in catalog_entries}
    external_site_dispositions: dict[str, int] = {site_id: 0 for site_id in external_symbols}
    for disposition in dispositions.values():
        entry = catalog_entries.get(disposition["catalog_entry_id"])
        if entry is None:
            raise ContractError("catalog disposition references a missing catalog entry")
        site_id = disposition["site_id"]
        if disposition["disposition"] == "reviewed_absent":
            if site_id is not None:
                raise ContractError("reviewed-absent catalog disposition has a site")
            absent_receipts_by_catalog_entry[entry["id"]] += 1
            continue
        if site_id not in all_sites:
            raise ContractError("catalog disposition references a missing site")
        allowed_sites = {
            "external_symbol": external_symbols,
            "dynamic_capability_root": set(all_sites),
            "executable_declarative_path": set(syntax) | set(data),
            "local_closure_boundary": set(syntax) | set(symbols),
            "persistent_data_root": set(data),
            "provider_or_config_root": set(data),
        }[entry["entry_kind"]]
        if site_id not in allowed_sites:
            raise ContractError("catalog disposition site kind disagrees with its entry kind")
        if site_id in used_sites_by_catalog_entry[entry["id"]]:
            raise ContractError("catalog entry contains a duplicate consumer disposition")
        used_sites_by_catalog_entry[entry["id"]].add(site_id)
        syntax_site_id = site_id if site_id in syntax else all_sites[site_id]["syntax_site_id"]
        source = sources[syntax[syntax_site_id]["source_node_id"]]
        if entry["language"] != source["language"]:
            raise ContractError("catalog disposition language disagrees with its source")
        if entry["entry_kind"] == "external_symbol":
            expected_signature_digest = hashlib.sha256(
                entry["signature"].encode("utf-8")
            ).hexdigest()
            if expected_signature_digest != symbols[site_id]["signature_digest"]:
                raise ContractError("external symbol signature disagrees with its catalog entry")
            external_site_dispositions[site_id] += 1
    if any(count != 1 for count in external_site_dispositions.values()):
        raise ContractError("every external symbol must have exactly one catalog disposition")
    for entry_id, entry in catalog_entries.items():
        used_sites = used_sites_by_catalog_entry[entry_id]
        absent_count = absent_receipts_by_catalog_entry[entry_id]
        if used_sites and absent_count:
            raise ContractError("catalog entry has both consumers and an absent receipt")
        if not used_sites and absent_count != 1:
            raise ContractError("unused catalog entry lacks exactly one reviewed-absent receipt")
        if set(entry["consumer_ids"]) != used_sites:
            raise ContractError("catalog consumer set disagrees with dispositions")
        expected_used_site_digest = stable_id_set_digest(
            "decodex/lane-authority-v2-catalog-entry-used-sites/1", used_sites
        )
        if entry["used_site_set_digest"] != expected_used_site_digest:
            raise ContractError("catalog used-site digest disagrees with dispositions")
    expected_used_external_digest = stable_id_set_digest(
        "decodex/lane-authority-v2-used-external-symbol-sites/1", external_symbols
    )
    if catalog["used_external_symbol_set_digest"] != expected_used_external_digest:
        raise ContractError("catalog used external symbol set digest disagrees with dispositions")
    if catalog["catalog_semantic_digest"] != catalog_semantic_digest(catalog):
        raise ContractError("catalog semantic digest disagrees with canonical catalog bytes")
    tool_receipts = _unique_index(
        records["toolchain_receipts"], "receipt_id", "toolchain_receipts"
    )
    actual_toolchains = [
        (
            receipt["language"],
            receipt["platform"],
            receipt["receipt_role"],
            receipt["tool"],
            receipt["tool_identity_digest"],
            tuple(sorted(receipt["config_projection_ids"])),
        )
        for receipt in tool_receipts.values()
    ]
    expected_toolchains = [
        (
            entry["language"],
            entry["platform"],
            entry["receipt_role"],
            entry["tool"],
            entry["tool_identity_digest"],
            tuple(sorted(entry["config_projection_ids"])),
        )
        for entry in catalog["toolchain_matrix"]
    ]
    if len(actual_toolchains) != len(set(actual_toolchains)):
        raise ContractError("toolchain receipts contain duplicate semantic identities")
    if len(expected_toolchains) != len(set(expected_toolchains)):
        raise ContractError("approved catalog matrix contains duplicate semantic identities")
    if sorted(actual_toolchains) != sorted(expected_toolchains):
        raise ContractError("toolchain receipts disagree with the approved catalog matrix")
    if {receipt["language"] for receipt in tool_receipts.values()} != EXPECTED_LANGUAGES:
        raise ContractError("toolchain receipts do not cover every supported language")
    if any(
        projection_id not in projections
        or projections[projection_id]["projection_kind"] != "config"
        for receipt in tool_receipts.values()
        for projection_id in receipt["config_projection_ids"]
    ):
        raise ContractError("toolchain receipt references a missing config projection")
    for receipt in tool_receipts.values():
        has_exact_platform_projection = False
        for projection_id in receipt["config_projection_ids"]:
            projection = projections[projection_id]
            allowed_platforms = {"common"}
            if receipt["platform"] != "common":
                allowed_platforms.add(receipt["platform"])
            if projection["language"] != receipt["language"]:
                raise ContractError("toolchain projection language disagrees with its receipt")
            if projection["platform"] not in allowed_platforms:
                raise ContractError("toolchain projection platform disagrees with its receipt")
            has_exact_platform_projection |= projection["platform"] == receipt["platform"]
        if receipt["receipt_role"] == "platform_slice" and not has_exact_platform_projection:
            raise ContractError("platform-slice receipt lacks an exact-platform projection")
    parser_receipt_by_language: dict[str, dict[str, Any]] = {}
    platform_slice_receipts = {"linux": [], "macos": []}
    for receipt in tool_receipts.values():
        if receipt["receipt_role"] == "platform_slice":
            if receipt["platform"] not in platform_slice_receipts:
                raise ContractError("platform-slice receipt must be linux or macos")
            platform_slice_receipts[receipt["platform"]].append(receipt)
            continue
        if receipt["platform"] != "common":
            raise ContractError("source parser receipt must use the common platform")
        if receipt["language"] in parser_receipt_by_language:
            raise ContractError("each language must have exactly one common parser receipt")
        parser_receipt_by_language[receipt["language"]] = receipt
    if set(parser_receipt_by_language) != EXPECTED_LANGUAGES:
        raise ContractError("common parser receipts do not cover every language")
    if any(not receipts for receipts in platform_slice_receipts.values()):
        raise ContractError("linux and macos platform-slice receipts are both required")
    for platform, receipts in platform_slice_receipts.items():
        expected_projection_ids = {
            projection_id
            for projection_id, projection in projections.items()
            if projection["platform"] == platform
        }
        completed_projection_ids = {
            projection_id
            for receipt in receipts
            for projection_id in receipt["config_projection_ids"]
            if projections[projection_id]["platform"] == platform
        }
        if not expected_projection_ids or completed_projection_ids != expected_projection_ids:
            raise ContractError(f"{platform} platform-slice receipts are incomplete")
    for source in sources.values():
        receipt_id = source["parser_receipt_id"]
        if receipt_id is None:
            continue
        expected_receipt = parser_receipt_by_language[source["language"]]
        if receipt_id != expected_receipt["receipt_id"]:
            raise ContractError("source is not assigned to its language's canonical parser")
    def source_id_for_site(site_id: str) -> str:
        syntax_site_id = site_id if site_id in syntax else all_sites[site_id]["syntax_site_id"]
        return syntax[syntax_site_id]["source_node_id"]

    for receipt_id, receipt in tool_receipts.items():
        if receipt["receipt_role"] == "parser":
            expected_source_ids = {
                source_id
                for source_id, source in sources.items()
                if source["status"] == "current" and source["language"] == receipt["language"]
            }
        else:
            expected_source_ids = {
                syntax[projections[projection_id]["site_id"]]["source_node_id"]
                for projection_id in receipt["config_projection_ids"]
            }
        completed_source_ids = set(receipt["completed_source_node_ids"])
        if completed_source_ids != expected_source_ids:
            raise ContractError("tool receipt completed source set disagrees with expected inputs")
        if any(
            source_id not in sources
            or sources[source_id]["status"] != "current"
            or sources[source_id]["language"] != receipt["language"]
            for source_id in completed_source_ids
        ):
            raise ContractError("tool receipt completed source has wrong identity or language")
        syntax_ids = {
            site_id
            for site_id, site in syntax.items()
            if site["source_node_id"] in completed_source_ids
        }
        candidate_ids = {
            candidate_id
            for candidate_id, candidate in candidates.items()
            if candidate["source_node_id"] in completed_source_ids
        }
        call_edge_ids = {
            edge["edge_id"]
            for edge in records["call_edges"]
            if source_id_for_site(edge["from_site_id"]) in completed_source_ids
        }
        dataflow_edge_ids = {
            edge["edge_id"]
            for edge in records["dataflow_edges"]
            if source_id_for_site(edge["from_site_id"]) in completed_source_ids
        }
        observed_sets = {
            "expected_source_node": expected_source_ids,
            "completed_source_node": completed_source_ids,
            "syntax_site": syntax_ids,
            "candidate_record": candidate_ids,
            "call_edge": call_edge_ids,
            "dataflow_edge": dataflow_edge_ids,
        }
        for field_prefix, identifiers in observed_sets.items():
            if receipt[f"{field_prefix}_count"] != len(identifiers):
                raise ContractError(f"tool receipt {field_prefix} count disagrees with outputs")
            expected_digest = stable_id_set_digest(
                f"decodex/lane-authority-v2-tool-receipt-{field_prefix}/1",
                identifiers,
            )
            if receipt[f"{field_prefix}_ids_digest"] != expected_digest:
                raise ContractError(f"tool receipt {field_prefix} digest disagrees with outputs")
        if receipt["unresolved_count"] != 0 or receipt["rejection_reason_codes"]:
            raise ContractError("accepted tool receipt contains unresolved or rejected work")

    graph: dict[str, set[str]] = {site_id: set() for site_id in all_sites}
    for edge in [*records["call_edges"], *records["dataflow_edges"]]:
        graph[edge["from_site_id"]].add(edge["to_site_id"])

    def reachable(start_ids: set[str]) -> set[str]:
        seen = set(start_ids)
        pending = list(start_ids)
        while pending:
            current = pending.pop()
            for target in graph[current]:
                if target not in seen:
                    seen.add(target)
                    pending.append(target)
        return seen

    for supporting_input in records["supporting_inputs"]:
        consumers = set(supporting_input["consumer_site_ids"])
        if not consumers.issubset(all_sites):
            raise ContractError("supporting input references a missing consumer site")
        projection_ids = set(supporting_input["config_projection_ids"])
        if not projection_ids.issubset(projections):
            raise ContractError("supporting input references a missing config projection")
        if any(projections[projection_id]["projection_kind"] != "config" for projection_id in projection_ids):
            raise ContractError("supporting input references a non-config projection")
        producer_receipt = tool_receipts.get(supporting_input["producer_receipt_id"])
        if producer_receipt is None:
            raise ContractError("supporting input references a missing producer receipt")
        source_id = supporting_input["materialized_source_node_id"]
        if supporting_input["authority_capability"] != "authority_capable":
            continue
        source = sources.get(source_id)
        if source is None or source["status"] != "current":
            raise ContractError("authority-capable supporting input lacks a current materialized source")
        if (
            source["path"] != supporting_input["path"]
            or source["content_digest"] != supporting_input["content_digest"]
            or source["scope"] != supporting_input["scope"]
        ):
            raise ContractError("authority-capable supporting input identity disagrees with its source")
        if source_id not in producer_receipt["completed_source_node_ids"]:
            raise ContractError("authority-capable supporting input was not completed by its producer")
        if any(
            syntax[projections[projection_id]["site_id"]]["source_node_id"] != source_id
            for projection_id in projection_ids
        ):
            raise ContractError("authority-capable supporting input cfg belongs to another source")
        source_sites = {
            site_id
            for site_id in all_sites
            if source_id_for_site(site_id) == source_id
        }
        if not consumers.issubset(reachable(source_sites)):
            raise ContractError("authority-capable supporting input consumers lack a graph path")


def validate_analysis_cut_against_git(
    root: Path,
    analysis_cut: dict[str, Any],
    source_records: list[dict[str, Any]],
) -> None:
    baseline = analysis_cut["c0_baseline_commit"]
    base = analysis_cut["pr_base_commit"]
    source_cut = analysis_cut["source_cut_commit"]
    if run_git(root, "rev-parse", "origin/main") != base:
        raise ContractError("analysis-cut PR base is not canonical origin/main")
    for commit_field, tree_field in (
        ("c0_baseline_commit", "c0_baseline_tree_oid"),
        ("pr_base_commit", "pr_base_tree_oid"),
        ("source_cut_commit", "source_cut_tree_oid"),
    ):
        if run_git(root, "rev-parse", f"{analysis_cut[commit_field]}^{{tree}}") != analysis_cut[tree_field]:
            raise ContractError(f"analysis-cut {tree_field} disagrees with Git objects")
    subprocess.run(["git", "merge-base", "--is-ancestor", base, source_cut], cwd=root, check=True)
    subprocess.run(["git", "merge-base", "--is-ancestor", source_cut, "HEAD"], cwd=root, check=True)
    output_paths = {
        artifact["path"] for artifact in load_json(root, OUTPUT_POLICY_PATH)["artifacts"]
    }
    post_cut_paths = set(
        run_git(root, "diff", "--name-only", source_cut, "HEAD", "--").splitlines()
    )
    if not post_cut_paths.issubset(output_paths):
        raise ContractError("commits after the analysis source cut changed non-output paths")

    baseline_bytes = git_source_bytes(root, baseline)
    source_cut_bytes = git_source_bytes(root, source_cut)
    baseline_digests = {
        path: hashlib.sha256(content).hexdigest() for path, content in baseline_bytes.items()
    }
    source_cut_digests = {
        path: hashlib.sha256(content).hexdigest() for path, content in source_cut_bytes.items()
    }
    if c0_source_tree_digest(baseline_digests) != analysis_cut["c0_source_tree_digest"]:
        raise ContractError("analysis-cut C0 source tree digest disagrees with Git objects")
    if canonical_source_tree_digest(source_cut_digests) != analysis_cut["analysis_input_tree_digest"]:
        raise ContractError("analysis input tree digest disagrees with source-cut Git objects")

    current_records = [record for record in source_records if record["status"] == "current"]
    tombstones = [record for record in source_records if record["status"] == "deleted"]
    records_by_path: dict[str, dict[str, Any]] = {}
    for record in current_records:
        if record["path"] in records_by_path:
            raise ContractError("source relation contains duplicate current paths")
        records_by_path[record["path"]] = record
    if set(records_by_path) != set(source_cut_digests):
        raise ContractError("source relation path universe disagrees with source-cut Git objects")
    for path, content_digest in source_cut_digests.items():
        if records_by_path[path]["content_digest"] != content_digest:
            raise ContractError("source relation content digest disagrees with Git objects")
        if records_by_path[path]["byte_length"] != len(source_cut_bytes[path]):
            raise ContractError("source relation byte length disagrees with Git objects")
    expected_tombstone_paths = set(baseline_digests) - set(source_cut_digests)
    if {record["path"] for record in tombstones} != expected_tombstone_paths:
        raise ContractError("deleted tombstones disagree with baseline/source-cut Git objects")
    for record in tombstones:
        if record["content_digest"] != baseline_digests[record["path"]]:
            raise ContractError("tombstone content digest disagrees with baseline Git objects")
        if record["byte_length"] != len(baseline_bytes[record["path"]]):
            raise ContractError("tombstone byte length disagrees with baseline Git objects")

    analysis_count = sum(source_partition(record) == "analysis" for record in source_records)
    tool_count = sum(source_partition(record) == "tool" for record in source_records)
    if analysis_count != analysis_cut["analysis_source_node_count"]:
        raise ContractError("analysis source count disagrees with reconstructed source universe")
    if tool_count != analysis_cut["tool_source_node_count"]:
        raise ContractError("tool source count disagrees with reconstructed source universe")
    if len(tombstones) != analysis_cut["deleted_tombstone_count"]:
        raise ContractError("tombstone count disagrees with reconstructed source universe")

    added, modified, delta_digest = post_c0_delta(root, baseline, source_cut)
    if added != analysis_cut["post_c0_added_count"]:
        raise ContractError("post-C0 added count disagrees with Git diff")
    if modified != analysis_cut["post_c0_modified_count"]:
        raise ContractError("post-C0 modified count disagrees with Git diff")
    if delta_digest != analysis_cut["post_c0_delta_digest"]:
        raise ContractError("post-C0 delta digest disagrees with Git diff")

    expected_artifacts = {
        "launcher_inventory": sha256_path(root, LAUNCHER_PATH),
        "legacy_authority_inventory": sha256_path(root, LEGACY_PATH),
        "mutation_registry": sha256_path(root, MUTATION_PATH),
        "scenario_manifest": sha256_path(
            root,
            Path("apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json"),
        ),
    }
    if analysis_cut["c0_artifact_sha256"] != expected_artifacts:
        raise ContractError("analysis-cut C0 artifact hashes drifted")


def validate_proof_artifacts(
    root: Path,
    composition: dict[str, Any],
    analysis_cut_digest: str,
    records: dict[str, list[dict[str, Any]]],
    catalog: dict[str, Any],
) -> None:
    cfg = load_json(root, CFG_COVERAGE_PATH)
    dataflow = load_json(root, DATAFLOW_PROOFS_PATH)
    for value, schema_path, label in (
        (
            cfg,
            Path("tools/lane-authority-inventory/contracts/cfg_coverage.schema.json"),
            "cfg coverage",
        ),
        (
            dataflow,
            Path("tools/lane-authority-inventory/contracts/dataflow_proofs.schema.json"),
            "dataflow proofs",
        ),
    ):
        try:
            Draft202012Validator(load_json(root, schema_path)).validate(value)
        except ValidationError as error:
            raise ContractError(f"{label} violates its schema: {error.message}") from error

    syntax_ids = {record["site_id"] for record in records["syntax_sites"]}
    syntax_sources = {
        record["site_id"]: record["source_node_id"] for record in records["syntax_sites"]
    }
    syntax_source_ids = {
        record["site_id"]: record["source_node_id"] for record in records["syntax_sites"]
    }
    projected_source_ids = {
        syntax_source_ids[record["site_id"]] for record in records["cfg_projections"]
    }
    covered_ids = {
        site_id
        for site_id, source_id in syntax_source_ids.items()
        if source_id in projected_source_ids
    }
    if cfg["analysis_cut_digest"] != analysis_cut_digest:
        raise ContractError("cfg coverage analysis-cut digest drifted")
    if cfg["cfg_relation_digest"] != composition["relations"]["cfg_projections"]["digest"]:
        raise ContractError("cfg coverage relation digest drifted")
    for prefix, identifiers in (("syntax_site", syntax_ids), ("covered_syntax_site", covered_ids)):
        if cfg[f"{prefix}_count"] != len(identifiers):
            raise ContractError(f"cfg coverage {prefix} count drifted")
        if cfg[f"{prefix}_ids_digest"] != stable_id_set_digest(
            f"decodex/lane-authority-v2-cfg-{prefix}s/1", identifiers
        ):
            raise ContractError(f"cfg coverage {prefix} digest drifted")
    if covered_ids != syntax_ids:
        raise ContractError("cfg coverage does not cover every syntax site")
    for platform in ("common", "linux", "macos"):
        platform_ids = {
            projection["projection_id"]
            for projection in records["cfg_projections"]
            if projection["platform"] == platform
        }
        if platform != "common" and not platform_ids:
            raise ContractError(f"cfg {platform} platform slice is empty")
        if cfg["platform_slice_digests"][platform] != stable_id_set_digest(
            f"decodex/lane-authority-v2-cfg-platform-{platform}/1", platform_ids
        ):
            raise ContractError("cfg platform slice digest drifted")
    if cfg["cfg_coverage_digest"] != self_bound_artifact_digest(
        cfg, "cfg_coverage_digest", "decodex/lane-authority-v2-cfg-coverage/1"
    ):
        raise ContractError("cfg coverage artifact digest drifted")
    if composition["cfg_coverage_digest"] != sha256_path(root, CFG_COVERAGE_PATH):
        raise ContractError("composition cfg coverage digest drifted")

    call_edges = {record["edge_id"]: record for record in records["call_edges"]}
    dataflow_edges = {record["edge_id"]: record for record in records["dataflow_edges"]}
    classifications = {
        record["site_id"]: record for record in records["site_classifications"]
    }
    sink_kinds = set(load_json(root, DATAFLOW_PATH)["sink_semantic_kinds"])
    sink_ids = {
        site_id
        for site_id, classification in classifications.items()
        if sink_kinds.intersection(classification["semantic_kinds"])
    }
    catalog_ids = {
        entry["id"]
        for section in (
            "dynamic_capability_roots",
            "executable_declarative_paths",
            "external_symbols",
            "local_closure_boundaries",
            "persistent_data_roots",
            "provider_and_config_roots",
            "reviewed_non_authority_external_symbols",
        )
        for entry in catalog[section]
    }
    site_ids = {record["site_id"] for relation in ("syntax_sites", "symbol_sites", "data_sites") for record in records[relation]}
    syntax_ids = {record["site_id"] for record in records["syntax_sites"]}
    derived_syntax = {
        record["site_id"]: record["syntax_site_id"]
        for relation in ("symbol_sites", "data_sites")
        for record in records[relation]
    }
    projections = {
        record["projection_id"]: record for record in records["cfg_projections"]
    }
    tool_receipt_ids = {record["receipt_id"] for record in records["toolchain_receipts"]}
    dataflow_contract = load_json(root, DATAFLOW_PATH)
    allowed_transfer_ids = {rule["id"] for rule in dataflow_contract["allowed_transfer_rules"]}
    accepted_fields = set(dataflow_contract["accepted_proof_receipt_fields"])
    proof_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/dataflow_proofs.schema.json")
    )["$defs"]["proof"]
    if set(proof_schema["required"]) != accepted_fields or set(proof_schema["properties"]) != accepted_fields:
        raise ContractError("dataflow proof schema disagrees with accepted receipt fields")

    proof_by_sink: dict[str, dict[str, Any]] = {}
    proof_ids: set[str] = set()
    for proof in dataflow["proofs"]:
        if proof["proof_id"] in proof_ids or proof["sink_site_id"] in proof_by_sink:
            raise ContractError("dataflow proof id or sink is duplicated")
        proof_ids.add(proof["proof_id"])
        proof_by_sink[proof["sink_site_id"]] = proof
        if not set(proof["catalog_entry_ids"]).issubset(catalog_ids):
            raise ContractError("dataflow proof references a missing catalog entry")
        if not set(proof["transfer_rule_ids"]).issubset(allowed_transfer_ids):
            raise ContractError("dataflow proof references an unapproved transfer rule")
        if not set(proof["tool_receipt_ids"]).issubset(tool_receipt_ids):
            raise ContractError("dataflow proof references a missing tool receipt")
        validate_dataflow_proof_path(
            proof,
            call_edges=call_edges,
            dataflow_edges=dataflow_edges,
            site_ids=site_ids,
            sink_ids=sink_ids,
            syntax_ids=syntax_ids,
            syntax_sources=syntax_sources,
            derived_syntax=derived_syntax,
            projections=projections,
        )
    if set(proof_by_sink) != sink_ids or dataflow["sink_count"] != len(sink_ids):
        raise ContractError("dataflow proofs do not cover every authority sink")
    if dataflow["analysis_cut_digest"] != analysis_cut_digest:
        raise ContractError("dataflow proof analysis-cut digest drifted")
    if dataflow["dataflow_contract_digest"] != sha256_path(root, DATAFLOW_PATH):
        raise ContractError("dataflow proof contract digest drifted")
    if dataflow["call_relation_digest"] != composition["relations"]["call_edges"]["digest"]:
        raise ContractError("dataflow proof call relation digest drifted")
    if dataflow["dataflow_relation_digest"] != composition["relations"]["dataflow_edges"]["digest"]:
        raise ContractError("dataflow proof dataflow relation digest drifted")
    if dataflow["fixed_point_digest"] != stable_id_set_digest(
        "decodex/lane-authority-v2-dataflow-fixed-points/1",
        {proof["fixed_point_digest"] for proof in dataflow["proofs"]},
    ):
        raise ContractError("dataflow fixed-point aggregate digest drifted")
    if dataflow["dataflow_proofs_digest"] != self_bound_artifact_digest(
        dataflow,
        "dataflow_proofs_digest",
        "decodex/lane-authority-v2-dataflow-proofs/1",
    ):
        raise ContractError("dataflow proof artifact digest drifted")
    if composition["dataflow_proofs_digest"] != sha256_path(root, DATAFLOW_PROOFS_PATH):
        raise ContractError("composition dataflow proof digest drifted")


def validate_relation_manifests(
    root: Path,
    composition: dict[str, Any],
    analysis_cut: dict[str, Any],
    catalog: dict[str, Any],
) -> None:
    for value, schema_path, label in (
        (
            analysis_cut,
            Path("tools/lane-authority-inventory/contracts/analysis_cut.schema.json"),
            "analysis cut",
        ),
        (
            catalog,
            Path(
                "tools/lane-authority-inventory/contracts/authority_surface_catalog.schema.json"
            ),
            "catalog",
        ),
        (
            composition,
            Path(
                "tools/lane-authority-inventory/contracts/inventory_composition.schema.json"
            ),
            "composition",
        ),
    ):
        try:
            Draft202012Validator(load_json(root, schema_path)).validate(value)
        except ValidationError as error:
            raise ContractError(f"{label} violates its schema: {error.message}") from error
    if composition["status"] != "accepted" or composition["unresolved_count"] != 0:
        raise ContractError("inventory composition is not accepted with zero unresolved state")
    if catalog["catalog_status"] != "p3_populated_approved":
        raise ContractError("accepted composition requires an approved populated catalog")
    if composition["composition_digest"] != composition_semantic_digest(composition):
        raise ContractError("composition digest disagrees with canonical composition bytes")
    analysis_cut_digest = hashlib.sha256(canonical_json(analysis_cut).encode("utf-8")).hexdigest()
    if composition["analysis_cut_digest"] != analysis_cut_digest:
        raise ContractError("composition analysis-cut digest disagrees with canonical bytes")
    if composition["dataflow_contract_digest"] != sha256_path(root, DATAFLOW_PATH):
        raise ContractError("composition dataflow-contract digest drifted")
    semantic_digest = catalog_semantic_digest(catalog)
    if catalog.get("catalog_semantic_digest") != semantic_digest:
        raise ContractError("catalog semantic digest is invalid")
    if composition["catalog_semantic_digest"] != semantic_digest:
        raise ContractError("composition catalog digest disagrees with the catalog")
    relation_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json")
    )
    records: dict[str, list[dict[str, Any]]] = {}
    for relation, receipt in composition["relations"].items():
        path = Path(receipt["path"])
        manifest = load_json(root, path)
        validate_typed_relation_manifest(manifest, relation, relation_schema)
        if manifest["schema"] != receipt["schema"]:
            raise ContractError(f"relation receipt identity mismatch: {relation}")
        if len(manifest["records"]) != receipt["count"]:
            raise ContractError(f"relation receipt count mismatch: {relation}")
        if sha256_path(root, path) != receipt["digest"]:
            raise ContractError(f"relation receipt digest mismatch: {relation}")
        records[relation] = manifest["records"]
    validate_analysis_cut_against_git(root, analysis_cut, records["source_nodes"])
    expected_candidate_observations = expected_c0_candidate_observations(root)
    observed_anchor_counts = {"launcher": 0, "legacy": 0, "mutation": 0}
    for observation in expected_candidate_observations.values():
        observed_anchor_counts[observation["origin"]] += observation["candidate_line_count"]
    if observed_anchor_counts != analysis_cut["c0_candidate_anchors"]:
        raise ContractError("analysis-cut C0 candidate anchors disagree with frozen artifacts")
    validate_cross_relation_records(
        records,
        expected_source_partitions={
            "analysis": analysis_cut["analysis_source_node_count"],
            "deleted_tombstone": analysis_cut["deleted_tombstone_count"],
            "tool": analysis_cut["tool_source_node_count"],
        },
        expected_source_partition_digests={
            "analysis": analysis_cut["analysis_source_nodes_digest"],
            "deleted_tombstone": analysis_cut["deleted_tombstones_digest"],
            "tool": analysis_cut["tool_source_nodes_digest"],
        },
        expected_candidate_observations=expected_candidate_observations,
        catalog=catalog,
    )
    validate_proof_artifacts(root, composition, analysis_cut_digest, records, catalog)


def review_scope_digest(root: Path, base: str) -> str:
    paths = sorted(
        path for path in set(changed_paths(root, base)) if path != str(REVIEW_RECEIPT_PATH)
    )
    digest = hashlib.sha256()
    digest.update(b"decodex/lane-authority-v2-c1i-integrated-review-preimage/1\0")
    digest.update(base.encode("ascii"))
    digest.update(b"\0")
    digest.update(run_git(root, "rev-parse", f"{base}^{{tree}}").encode("ascii"))
    digest.update(b"\0")
    for path in paths:
        digest.update(path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(hashlib.sha256((root / path).read_bytes()).digest())
        digest.update(b"\0")
    return digest.hexdigest()


def validate_review_receipt(root: Path, base: str, base_tree: str) -> None:
    if not (root / REVIEW_RECEIPT_PATH).is_file():
        raise ContractError("fresh C1I integrated review receipt is missing")
    validate_instance(
        root,
        REVIEW_RECEIPT_PATH,
        Path("tools/lane-authority-inventory/contracts/review_receipt.schema.json"),
    )
    receipt = load_json(root, REVIEW_RECEIPT_PATH)
    if receipt["reviewed_base_commit"] != base or receipt["reviewed_base_tree_oid"] != base_tree:
        raise ContractError("C1I integrated review receipt base binding drifted")
    if receipt["reviewed_input_digest"] != review_scope_digest(root, base):
        raise ContractError("C1I integrated review receipt preimage digest drifted")


def validate_rejection_report(root: Path, report: dict[str, Any]) -> None:
    expected_keys = {
        "advancement_state",
        "analysis_input_digest",
        "contract_digests",
        "counts",
        "phase",
        "rejections",
        "schema",
        "status",
    }
    if set(report) != expected_keys:
        raise ContractError("rejection report fields do not match its closed schema")
    if report["advancement_state"] != "C1I_INCOMPLETE" or report["status"] != "rejected":
        raise ContractError("P0 rejection report must remain non-accepting")
    reason_codes = set(load_json(root, REASON_CODES_PATH)["reason_codes"])
    for rejection in report["rejections"]:
        if rejection.get("reason_code") not in reason_codes:
            raise ContractError("rejection report contains an unregistered reason code")
        expected_rejection_keys = {
            "actual_digest",
            "candidate_ids",
            "expected_digest",
            "reason_code",
            "site_ids",
            "tool_receipt_ids",
        }
        if set(rejection) != expected_rejection_keys:
            raise ContractError("rejection entry fields do not match its closed schema")


def candidate_counts(root: Path) -> dict[str, int]:
    launcher = load_json(root, LAUNCHER_PATH)
    legacy = load_json(root, LEGACY_PATH)
    mutation = load_json(root, MUTATION_PATH)
    return {
        "c0_source_files": len(legacy["source_files"]),
        "launcher_candidate_line_hits": sum(
            int(entry["candidate_line_count"]) for entry in launcher["entries"]
        ),
        "legacy_candidate_line_hits": sum(
            int(classification[1])
            for node in legacy["nodes"]
            for classification in node["classifications"]
        ),
        "mutation_candidate_line_hits": sum(
            int(classification[1])
            for entry in mutation["entries"]
            for classification in entry["classifications"]
        ),
    }


def unexpected_changed_paths(paths: list[str]) -> list[str]:
    return sorted(
        path
        for path in set(paths)
        if path not in ALLOWED_EXACT_PATHS
        and not any(path.startswith(prefix) for prefix in ALLOWED_PREFIXES)
    )


def changed_paths(root: Path, base: str) -> list[str]:
    changed = run_git(root, "diff", "--name-only", base, "--").splitlines()
    untracked = run_git(root, "ls-files", "--others", "--exclude-standard").splitlines()
    return [path for path in changed + untracked if path]


def validate_origin_main_anchor(root: Path, base: str) -> None:
    if run_git(root, "rev-parse", "origin/main") != base:
        raise ContractError("provisional PR base no longer matches canonical origin/main")


def contract_digests(root: Path) -> dict[str, str]:
    paths = (
        CHECKPOINT_PATH,
        CATALOG_PATH,
        AUTHORITY_SYMBOL_POLICY_PATH,
        EXTERNAL_SYMBOL_POLICY_PATH,
        DATAFLOW_PATH,
        OUTPUT_POLICY_PATH,
        REASON_CODES_PATH,
        INCOMPLETE_FIXTURE_PATH,
        *SCHEMA_PATHS,
        *EXECUTABLE_CONTRACT_PATHS,
    )
    return {str(path): sha256_path(root, path) for path in paths}


def verify_p0(
    root: Path,
    *,
    require_review: bool = False,
    allow_later_catalog: bool = False,
    allow_pending_authority_projection: bool = False,
) -> dict[str, Any]:
    checkpoint = load_json(root, CHECKPOINT_PATH)
    catalog = load_json(root, CATALOG_PATH)
    authority_symbol_policy = load_json(root, AUTHORITY_SYMBOL_POLICY_PATH)
    external_symbol_policy = load_json(root, EXTERNAL_SYMBOL_POLICY_PATH)
    dataflow = load_json(root, DATAFLOW_PATH)
    validate_json_schema_documents(root)
    validate_instance(
        root,
        CHECKPOINT_PATH,
        Path("tools/lane-authority-inventory/contracts/p0_checkpoint.schema.json"),
    )
    validate_instance(
        root,
        CATALOG_PATH,
        Path(
            "tools/lane-authority-inventory/contracts/authority_surface_catalog.schema.json"
        ),
    )
    validate_instance(
        root,
        AUTHORITY_SYMBOL_POLICY_PATH,
        Path(
            "tools/lane-authority-inventory/contracts/authority_symbol_policy.schema.json"
        ),
    )
    validate_instance(
        root,
        EXTERNAL_SYMBOL_POLICY_PATH,
        Path(
            "tools/lane-authority-inventory/contracts/external_symbol_policy.schema.json"
        ),
    )
    validate_instance(
        root,
        DATAFLOW_PATH,
        Path("tools/lane-authority-inventory/contracts/dataflow_contract.schema.json"),
    )
    validate_rejection_contract(root)
    validate_authority_symbol_policy(authority_symbol_policy)
    validate_external_symbol_policy(external_symbol_policy)
    if allow_later_catalog and catalog.get("catalog_status") == "p3_machine_validated_incomplete":
        validate_catalog_p3_policy_projection(
            catalog,
            external_symbol_policy,
            authority_symbol_policy,
            allow_pending_authority_projection=allow_pending_authority_projection,
        )
    else:
        validate_catalog_p0(catalog)
    validate_dataflow_contract(dataflow, root)
    validate_checkpoint_p0(checkpoint, root)
    validate_composition_schema(root)
    validate_output_policy(root)

    if checkpoint.get("schema") != "decodex/lane-authority-v2-c1i-checkpoint/1":
        raise ContractError("unexpected P0 checkpoint schema")
    if checkpoint.get("phase") != "P0":
        raise ContractError("P0 verifier received a non-P0 checkpoint")
    if checkpoint.get("advancement_state") != "C1I_INCOMPLETE":
        raise ContractError("P0 must not claim C1I readiness")
    if not allow_later_catalog and checkpoint.get("catalog_status") != catalog.get(
        "catalog_status"
    ):
        raise ContractError("checkpoint and catalog status disagree")
    if checkpoint.get("migration_state") != "not_started":
        raise ContractError("C1I must not start runtime migration")

    cut = checkpoint.get("provisional_analysis_cut_anchor")
    if not isinstance(cut, dict):
        raise ContractError("P0 checkpoint lacks its provisional analysis-cut anchor")
    baseline = str(cut["c0_baseline_commit"])
    base = str(cut["provisional_pr_base_commit"])
    if run_git(root, "rev-parse", f"{baseline}^{{tree}}") != cut["c0_baseline_tree_oid"]:
        raise ContractError("C0 baseline tree OID drifted")
    if run_git(root, "rev-parse", f"{base}^{{tree}}") != cut["provisional_pr_base_tree_oid"]:
        raise ContractError("provisional PR base tree OID drifted")
    validate_origin_main_anchor(root, base)
    subprocess.run(["git", "merge-base", "--is-ancestor", base, "HEAD"], cwd=root, check=True)

    for relative_path, expected in cut["c0_artifacts"].items():
        actual = sha256_path(root, Path(relative_path))
        if actual != expected:
            raise ContractError(f"C0 artifact drifted: {relative_path}")

    counts = candidate_counts(root)
    if counts != checkpoint.get("candidate_anchors"):
        raise ContractError(f"candidate anchors drifted: {counts}")
    unexpected = unexpected_changed_paths(changed_paths(root, base))
    if unexpected:
        rendered = "\n".join(f"  {path}" for path in unexpected)
        raise ContractError(f"P0 changed production or unapproved paths:\n{rendered}")
    if require_review:
        validate_review_receipt(root, base, str(cut["provisional_pr_base_tree_oid"]))

    return {
        "advancement_state": "C1I_INCOMPLETE",
        "candidate_anchors": counts,
        "catalog_status": catalog["catalog_status"],
        "contract_digests": contract_digests(root),
        "phase": "P0",
        "schema": "decodex/lane-authority-v2-c1i-contract-check/1",
    }


def verify_p1(
    root: Path, *, allow_pending_authority_projection: bool = False
) -> dict[str, Any]:
    p0 = verify_p0(
        root,
        require_review=False,
        allow_later_catalog=True,
        allow_pending_authority_projection=allow_pending_authority_projection,
    )
    analysis_cut = load_json(root, ANALYSIS_CUT_PATH)
    source_inventory = load_json(root, SOURCE_INVENTORY_PATH)
    candidate_manifest = load_json(root, CANDIDATE_RECORDS_PATH)
    for value, schema_path, label in (
        (
            analysis_cut,
            Path("tools/lane-authority-inventory/contracts/analysis_cut.schema.json"),
            "analysis cut",
        ),
        (
            source_inventory,
            Path("tools/lane-authority-inventory/contracts/source_inventory.schema.json"),
            "source inventory",
        ),
    ):
        try:
            Draft202012Validator(load_json(root, schema_path)).validate(value)
        except ValidationError as error:
            raise ContractError(f"P1 {label} violates its schema: {error.message}") from error
    validate_typed_relation_manifest(
        candidate_manifest,
        "candidate_records",
        load_json(
            root,
            Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json"),
        ),
    )
    records = source_inventory["records"]
    sources = _unique_index(records, "source_node_id", "source inventory")
    if source_inventory["source_cut_commit"] != analysis_cut["source_cut_commit"]:
        raise ContractError("source inventory and analysis cut commit disagree")
    if source_inventory["source_cut_tree_oid"] != analysis_cut["source_cut_tree_oid"]:
        raise ContractError("source inventory and analysis cut tree disagree")
    validate_analysis_cut_against_git(root, analysis_cut, records)
    partitions = source_partition_digests(records)
    expected_counts = {
        "analysis": analysis_cut["analysis_source_node_count"],
        "deleted_tombstone": analysis_cut["deleted_tombstone_count"],
        "tool": analysis_cut["tool_source_node_count"],
    }
    actual_counts = {key: 0 for key in expected_counts}
    for source in records:
        actual_counts[source_partition(source)] += 1
    if actual_counts != expected_counts:
        raise ContractError("P1 source partition counts disagree with the analysis cut")
    expected_digests = {
        "analysis": analysis_cut["analysis_source_nodes_digest"],
        "deleted_tombstone": analysis_cut["deleted_tombstones_digest"],
        "tool": analysis_cut["tool_source_nodes_digest"],
    }
    if partitions != expected_digests:
        raise ContractError("P1 source partition digests disagree with the analysis cut")
    validate_candidate_replay_records(
        candidate_manifest["records"],
        source_paths={source_id: source["path"] for source_id, source in sources.items()},
        expected_observations=expected_c0_candidate_observations(root),
    )
    return {
        **p0,
        "analysis_cut_digest": hashlib.sha256(
            canonical_json(analysis_cut).encode("utf-8")
        ).hexdigest(),
        "analysis_source_count": actual_counts["analysis"],
        "candidate_record_count": len(candidate_manifest["records"]),
        "deleted_tombstone_count": actual_counts["deleted_tombstone"],
        "phase": "P1",
        "source_cut_commit": analysis_cut["source_cut_commit"],
        "tool_source_count": actual_counts["tool"],
    }


def verify_p2(
    root: Path, *, allow_pending_authority_projection: bool = False
) -> dict[str, Any]:
    p1 = verify_p1(
        root, allow_pending_authority_projection=allow_pending_authority_projection
    )
    relation_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json")
    )
    manifests = {
        "source_nodes": load_json(root, SOURCE_NODES_PATH),
        "syntax_sites": load_json(root, SYNTAX_SITES_PATH),
        "cfg_projections": load_json(root, CFG_PROJECTIONS_PATH),
        "candidate_site_edges": load_json(root, CANDIDATE_SITE_EDGES_PATH),
        **{
            relation_name: load_json(
                root,
                Path(
                    f"tools/lane-authority-inventory/manifests/relations/{relation_name}.json"
                ),
            )
            for relation_name in (
                "call_edges",
                "data_sites",
                "dataflow_edges",
                "rust_module_scopes",
                "rust_name_bindings",
                "rust_path_resolutions",
                "rust_receiver_type_resolutions",
                "rust_method_owner_resolutions",
                "rust_qualified_owner_resolutions",
                "supporting_inputs",
                "symbol_sites",
                "toolchain_receipts",
            )
        },
    }
    for relation_name, manifest in manifests.items():
        validate_typed_relation_manifest(manifest, relation_name, relation_schema)

    source_inventory = load_json(root, SOURCE_INVENTORY_PATH)
    identity_fields = {
        "byte_length",
        "content_digest",
        "language",
        "path",
        "predecessor_source_node_id",
        "provenance",
        "scope",
        "source_node_id",
        "status",
    }
    expected_sources = {
        record["source_node_id"]: record for record in source_inventory["records"]
    }
    sources = _unique_index(
        manifests["source_nodes"]["records"], "source_node_id", "P2 source nodes"
    )
    if set(sources) != set(expected_sources):
        raise ContractError("P2 source nodes do not equal the immutable source inventory")
    for source_id, source in sources.items():
        identity = {key: source[key] for key in identity_fields}
        if identity != expected_sources[source_id]:
            raise ContractError("P2 enriched source identity drifted from P1")
        if source["parser_node_count"] < source["syntax_site_count"]:
            raise ContractError("P2 source has fewer parser nodes than materialized sites")

    syntax = _unique_index(
        manifests["syntax_sites"]["records"], "site_id", "P2 syntax sites"
    )
    sites_by_source = {source_id: set() for source_id in sources}
    roots_by_source = {source_id: [] for source_id in sources}
    for site in syntax.values():
        source_id = site["source_node_id"]
        if source_id not in sources:
            raise ContractError("P2 syntax site references a missing source")
        if site["byte_end"] > sources[source_id]["byte_length"]:
            raise ContractError("P2 syntax site exceeds source bytes")
        sites_by_source[source_id].add(site["site_id"])
        if site["is_parser_root"]:
            roots_by_source[source_id].append(site)
    for source_id, source in sources.items():
        site_ids = sites_by_source[source_id]
        if source["syntax_site_count"] != len(site_ids):
            raise ContractError("P2 source syntax count disagrees with materialized sites")
        if source["syntax_site_ids_digest"] != stable_id_set_digest(
            "decodex/lane-authority-v2-source-syntax-sites/1", site_ids
        ):
            raise ContractError("P2 source syntax digest disagrees with materialized sites")
        if source["status"] == "deleted":
            if site_ids or roots_by_source[source_id]:
                raise ContractError("P2 tombstone contains syntax sites")
            continue
        roots = roots_by_source[source_id]
        if (
            len(roots) != 1
            or roots[0]["byte_start"] != 0
            or roots[0]["byte_end"] != source["byte_length"]
            or roots[0]["node_kind"] != PARSER_ROOT_KINDS[source["language"]]
        ):
            raise ContractError("P2 source lacks one full-byte language parser root")

    candidates = load_json(root, CANDIDATE_RECORDS_PATH)["records"]
    candidate_ids = {candidate["candidate_id"] for candidate in candidates}
    candidate_edges = manifests["candidate_site_edges"]["records"]
    edge_candidates = [edge["candidate_id"] for edge in candidate_edges]
    if set(edge_candidates) != candidate_ids or len(edge_candidates) != len(candidate_ids):
        raise ContractError("P2 must map every candidate to exactly one syntax site")
    if any(edge["site_id"] not in syntax for edge in candidate_edges):
        raise ContractError("P2 candidate-site edge references a missing syntax site")

    projections = _unique_index(
        manifests["cfg_projections"]["records"], "projection_id", "P2 cfg projections"
    )
    coverage = {source_id: set() for source_id in sources if sources[source_id]["status"] == "current"}
    for projection in projections.values():
        site = syntax.get(projection["site_id"])
        if site is None or not site["is_parser_root"]:
            raise ContractError("P2 source projection must bind a parser root")
        source = sources[site["source_node_id"]]
        if projection["language"] != source["language"]:
            raise ContractError("P2 projection language disagrees with its source")
        coverage[source["source_node_id"]].add(
            (projection["projection_kind"], projection["platform"])
        )
    required_coverage = {
        ("config", "common"),
        ("target", "common"),
        ("config", "linux"),
        ("config", "macos"),
    }
    if any(value != required_coverage for value in coverage.values()):
        raise ContractError("P2 source cfg/target platform coverage is incomplete")

    projection_ids_by_source = {source_id: set() for source_id in coverage}
    for projection_id, projection in projections.items():
        source_id = syntax[projection["site_id"]]["source_node_id"]
        projection_ids_by_source[source_id].add(projection_id)
    rust_scopes = _unique_index(
        manifests["rust_module_scopes"]["records"],
        "scope_id",
        "P2 Rust module scopes",
    )
    roots_by_target: dict[str, list[dict[str, Any]]] = {}
    for scope in rust_scopes.values():
        source = sources.get(scope["source_node_id"])
        site = syntax.get(scope["scope_syntax_site_id"])
        if (
            source is None
            or source["language"] != "rust"
            or site is None
            or site["source_node_id"] != source["source_node_id"]
            or site["byte_start"] != scope["byte_start"]
            or site["byte_end"] != scope["byte_end"]
        ):
            raise ContractError("P2 Rust module scope lacks exact source/syntax evidence")
        if set(scope["cfg_projection_ids"]) != projection_ids_by_source[
            source["source_node_id"]
        ]:
            raise ContractError("P2 Rust module scope cfg projection set drifted")
        expected_scope_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-module-scope/1",
            scope["crate_target_id"],
            scope["source_node_id"],
            scope["scope_syntax_site_id"],
            scope["canonical_module_path"],
        )
        if scope["scope_id"] != expected_scope_id:
            raise ContractError("P2 Rust module scope id drifted")
        if scope["scope_kind"] == "crate_root":
            roots_by_target.setdefault(scope["crate_target_id"], []).append(scope)
            expected_target_id = stable_parts_id(
                "decodex/lane-authority-v2-rust-crate-target/1",
                scope["target_manifest_path"],
                scope["target_name"],
                ",".join(scope["target_kinds"]),
                scope["target_root_path"],
            )
            if (
                scope["crate_target_id"] != expected_target_id
                or scope["canonical_module_path"]
                != f"target::{scope['crate_target_id']}"
                or scope["source_node_id"] != scope["target_root_source_node_id"]
                or source["path"] != scope["target_root_path"]
            ):
                raise ContractError("P2 Rust crate-root target identity drifted")
            if (
                scope["target_extern_crate_names"]
                != sorted(set(scope["target_extern_crate_names"]))
                or not {"alloc", "core", "proc_macro", "std"}.issubset(
                    scope["target_extern_crate_names"]
                )
            ):
                raise ContractError("P2 Rust extern-crate attestation drifted")
        else:
            parent = rust_scopes.get(scope["parent_scope_id"])
            if parent is None or parent["crate_target_id"] != scope["crate_target_id"]:
                raise ContractError("P2 Rust module scope parent is missing or cross-target")
            if scope["scope_kind"] == "block":
                if scope["canonical_module_path"] != parent["canonical_module_path"]:
                    raise ContractError("P2 Rust block changed canonical module identity")
            elif not scope["canonical_module_path"].startswith(
                f"{parent['canonical_module_path']}::"
            ):
                raise ContractError("P2 Rust child module escaped its parent path")
            declaration = syntax.get(scope["declaration_syntax_site_id"])
            if scope["scope_kind"] != "block" and (
                declaration is None
                or declaration["source_node_id"] != parent["source_node_id"]
                or declaration["node_kind"] != "mod_item"
            ):
                raise ContractError("P2 Rust module scope lacks its parent mod declaration")
            if parent["source_node_id"] == scope["source_node_id"] and not (
                parent["byte_start"] <= scope["byte_start"]
                and scope["byte_end"] <= parent["byte_end"]
            ):
                raise ContractError("P2 Rust lexical scope exceeds its parent byte range")

    if not roots_by_target or any(len(roots) != 1 for roots in roots_by_target.values()):
        raise ContractError("P2 Rust module graph lacks one root per Cargo target")
    for scope in rust_scopes.values():
        seen: set[str] = set()
        current = scope
        while current["parent_scope_id"] is not None:
            if current["scope_id"] in seen:
                raise ContractError("P2 Rust module scope parent graph contains a cycle")
            seen.add(current["scope_id"])
            current = rust_scopes[current["parent_scope_id"]]
        if current["scope_kind"] != "crate_root":
            raise ContractError("P2 Rust module scope does not reach a Cargo target root")

    data_sites = _unique_index(
        manifests["data_sites"]["records"], "site_id", "P2 data sites"
    )
    if any(site["syntax_site_id"] not in syntax for site in data_sites.values()):
        raise ContractError("P2 data site references a missing syntax site")
    symbol_sites = _unique_index(
        manifests["symbol_sites"]["records"], "site_id", "P2 symbol sites"
    )
    if any(site["syntax_site_id"] not in syntax for site in symbol_sites.values()):
        raise ContractError("P2 symbol site references a missing syntax site")
    if any(
        site["language"]
        != sources[syntax[site["syntax_site_id"]]["source_node_id"]]["language"]
        for site in symbol_sites.values()
    ):
        raise ContractError("P2 symbol language disagrees with its source")
    for site in symbol_sites.values():
        if site["signature_digest"] != hashlib.sha256(
            site["signature"].encode("utf-8")
        ).hexdigest():
            raise ContractError("P2 symbol signature digest disagrees")
        receiver_type = site["receiver_type_signature"]
        receiver_evidence = site["receiver_type_evidence"]
        receiver_kind = site["receiver_type_kind"]
        if (
            (receiver_type is None) != (receiver_evidence is None)
            or (receiver_type is None) != (receiver_kind is None)
        ):
            raise ContractError("P2 receiver type evidence is incomplete")
        if receiver_type is not None and (
            site["language"] != "rust"
            or site["role"] != "call_target"
            or site["resolution_hint"] != "qualified"
            or not site["signature"].startswith(f"{receiver_type}::")
        ):
            raise ContractError("P2 receiver type proof disagrees with its symbol")
        qualified_owner = site["qualified_owner_signature"]
        qualified_evidence = site["qualified_owner_evidence"]
        qualified_kind = site["qualified_owner_kind"]
        if (
            (qualified_owner is None) != (qualified_evidence is None)
            or (qualified_owner is None) != (qualified_kind is None)
        ):
            raise ContractError("P2 qualified owner evidence is incomplete")
        if qualified_owner is not None and (
            site["language"] != "rust"
            or site["role"] != "call_target"
            or site["resolution_hint"] != "qualified"
            or not site["signature"].startswith(f"{qualified_owner}::")
        ):
            raise ContractError("P2 qualified owner proof disagrees with its symbol")

    rust_bindings = _unique_index(
        manifests["rust_name_bindings"]["records"],
        "binding_id",
        "P2 Rust name bindings",
    )
    bindings_by_identity: dict[tuple[str, str, str], list[dict[str, Any]]] = {}
    for binding in rust_bindings.values():
        scope = rust_scopes.get(binding["scope_id"])
        site = syntax.get(binding["syntax_site_id"])
        if (
            scope is None
            or scope["crate_target_id"] != binding["crate_target_id"]
            or scope["source_node_id"] != binding["source_node_id"]
            or site is None
            or site["source_node_id"] != binding["source_node_id"]
        ):
            raise ContractError("P2 Rust name binding lacks target/scope/syntax evidence")
        expected_binding_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-name-binding/1",
            binding["crate_target_id"],
            binding["scope_id"],
            binding["syntax_site_id"],
            binding["binding_kind"],
            binding["namespace"],
            binding["local_name"],
            binding["surface_target_path"] or "",
        )
        if binding["binding_id"] != expected_binding_id:
            raise ContractError("P2 Rust name binding id drifted")
        if binding["target_scope_id"] is not None:
            target_scope = rust_scopes.get(binding["target_scope_id"])
            if target_scope is None or target_scope["crate_target_id"] != binding[
                "crate_target_id"
            ]:
                raise ContractError("P2 Rust module binding target drifted")
            if binding["binding_kind"] == "module" and (
                target_scope["parent_scope_id"] != binding["scope_id"]
                or target_scope["declaration_syntax_site_id"]
                != binding["syntax_site_id"]
            ):
                raise ContractError("P2 Rust module declaration target drifted")
        if binding["target_symbol_site_id"] is not None:
            target_symbol = symbol_sites.get(binding["target_symbol_site_id"])
            if (
                target_symbol is None
                or target_symbol["language"] != "rust"
                or target_symbol["role"] != "declaration"
            ):
                raise ContractError("P2 Rust type binding target drifted")
            if binding["binding_kind"] == "type_declaration" and (
                target_symbol["syntax_site_id"] != binding["syntax_site_id"]
                or target_symbol["signature"] != binding["local_name"]
            ):
                raise ContractError("P2 Rust type declaration target drifted")
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
        if len(bindings) > 1 and any(
            binding["resolution"] != "ambiguous"
            or binding["reason_code"] != "rust_binding_same_scope_ambiguous"
            for binding in bindings
        ):
            raise ContractError("P2 Rust same-scope binding ambiguity was accepted")

    replay_bindings_by_scope_name: dict[tuple[str, str, str], list[str]] = {}
    for binding in rust_bindings.values():
        replay_bindings_by_scope_name.setdefault(
            (
                binding["crate_target_id"],
                binding["scope_id"],
                binding["local_name"],
            ),
            [],
        ).append(binding["binding_id"])
    rust_path_replay_index = {
        "bindings_by_scope_name": replay_bindings_by_scope_name,
        "roots": {
            scope["crate_target_id"]: scope["scope_id"]
            for scope in rust_scopes.values()
            if scope["scope_kind"] == "crate_root"
        },
    }

    rust_resolutions = _unique_index(
        manifests["rust_path_resolutions"]["records"],
        "resolution_id",
        "P2 Rust path resolutions",
    )
    resolutions_by_binding: dict[str, list[dict[str, Any]]] = {}
    for resolution in rust_resolutions.values():
        source_binding = rust_bindings.get(resolution["source_binding_id"])
        lexical_scope = rust_scopes.get(resolution["lexical_scope_id"])
        if (
            source_binding is None
            or source_binding["binding_kind"] not in {"use", "reexport", "glob"}
            or lexical_scope is None
            or resolution["crate_target_id"] != source_binding["crate_target_id"]
            or resolution["lexical_scope_id"] != source_binding["scope_id"]
            or resolution["namespace"] != source_binding["namespace"]
            or resolution["surface_path"] != source_binding["surface_target_path"]
            or resolution["binding_ids"][0] != source_binding["binding_id"]
            or any(binding_id not in rust_bindings for binding_id in resolution["binding_ids"])
        ):
            raise ContractError("P2 Rust path resolution source chain drifted")
        expected_resolution_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-path-resolution/1",
            source_binding["binding_id"],
            source_binding["scope_id"],
            resolution["namespace"],
            source_binding["surface_target_path"],
            resolution["status"],
            resolution["canonical_path"] or "",
        )
        if resolution["resolution_id"] != expected_resolution_id:
            raise ContractError("P2 Rust path resolution id drifted")
        digest_payload = {
            key: value
            for key, value in resolution.items()
            if key != "resolution_digest"
        }
        if resolution["resolution_digest"] != hashlib.sha256(
            canonical_json(digest_payload).encode("utf-8")
        ).hexdigest():
            raise ContractError("P2 Rust path resolution digest drifted")
        replay_rust_type_path_resolution(
            resolution,
            rust_scopes,
            rust_bindings,
            rust_path_replay_index,
        )
        terminal_binding = rust_bindings[resolution["binding_ids"][-1]]
        if resolution["status"] == "resolved_local_module":
            target = rust_scopes.get(resolution["canonical_module_scope_id"])
            if (
                target is None
                or target["crate_target_id"] != resolution["crate_target_id"]
                or target["canonical_module_path"] != resolution["canonical_path"]
                or source_binding["resolution"] != "resolved"
                or source_binding["target_scope_id"] != target["scope_id"]
                or terminal_binding["target_scope_id"] != target["scope_id"]
            ):
                raise ContractError("P2 Rust local module path resolution drifted")
        elif resolution["status"] == "resolved_local_type":
            target = symbol_sites.get(
                resolution["canonical_type_definition_site_id"]
            )
            if (
                target is None
                or target["language"] != "rust"
                or target["role"] != "declaration"
                or source_binding["resolution"] != "resolved"
                or source_binding["target_symbol_site_id"] != target["site_id"]
                or terminal_binding["target_symbol_site_id"] != target["site_id"]
            ):
                raise ContractError("P2 Rust local type path resolution drifted")
        elif resolution["status"] == "external":
            if source_binding["resolution"] != "external":
                raise ContractError("P2 Rust external path resolution drifted")
        elif source_binding["resolution"] == "resolved":
            raise ContractError("P2 unresolved Rust path retained a resolved binding")
        resolutions_by_binding.setdefault(source_binding["binding_id"], []).append(
            resolution
        )
    expected_resolution_bindings = {
        binding_id
        for binding_id, binding in rust_bindings.items()
        if binding["binding_kind"] in {"use", "reexport", "glob"}
    }
    if set(resolutions_by_binding) != expected_resolution_bindings or any(
        len(resolutions) != 1 for resolutions in resolutions_by_binding.values()
    ):
        raise ContractError("P2 Rust path resolution coverage is incomplete")

    receiver_resolutions = _unique_index(
        manifests["rust_receiver_type_resolutions"]["records"],
        "resolution_id",
        "P2 Rust receiver type resolutions",
    )
    receiver_resolutions_by_symbol: dict[str, list[dict[str, Any]]] = {}
    reason_by_status = {
        "ambiguous": "rust_path_ambiguous_binding",
        "cycle": "rust_path_reexport_cycle",
        "external": "rust_path_external_crate",
        "generic_parameter": "rust_receiver_generic_parameter",
        "inaccessible": "rust_path_visibility_denied",
        "resolved_local_module": "rust_path_unique_local_module",
        "resolved_local_type": "rust_path_unique_local_type",
        "unresolved": "rust_path_target_unresolved",
        "unsupported": "rust_path_unsupported_construct",
    }
    scopes_by_source: dict[str, list[dict[str, Any]]] = {}
    for scope in rust_scopes.values():
        scopes_by_source.setdefault(scope["source_node_id"], []).append(scope)
    for resolution in receiver_resolutions.values():
        symbol = symbol_sites.get(resolution["source_symbol_site_id"])
        syntax_site = syntax.get(resolution["source_syntax_site_id"])
        lexical_scope = rust_scopes.get(resolution["lexical_scope_id"])
        expected_query_path = RUST_PRELUDE_TYPE_PATHS.get(
            symbol["receiver_type_signature"] if symbol is not None else "",
            symbol["receiver_type_signature"] if symbol is not None else "",
        )
        if symbol is not None and symbol["receiver_type_kind"] == "implicit_self":
            expected_query_path = symbol["owner_signature"] or ""
        expected_status = resolution["path_status"]
        if symbol is not None and symbol["receiver_type_kind"] == "generic_parameter":
            if resolution["path_status"] != "unresolved":
                raise ContractError("P2 Rust generic receiver path unexpectedly resolved")
            expected_status = "generic_parameter"
        if (
            symbol is None
            or symbol["language"] != "rust"
            or symbol["role"] != "call_target"
            or symbol["receiver_type_signature"] is None
            or syntax_site is None
            or symbol["syntax_site_id"] != syntax_site["site_id"]
            or lexical_scope is None
            or lexical_scope["crate_target_id"] != resolution["crate_target_id"]
            or lexical_scope["source_node_id"] != syntax_site["source_node_id"]
            or resolution["surface_path"] != symbol["receiver_type_signature"]
            or resolution["query_path"] != expected_query_path
            or resolution["receiver_type_evidence"] != symbol["receiver_type_evidence"]
            or resolution["receiver_type_kind"] != symbol["receiver_type_kind"]
            or resolution["status"] != expected_status
            or resolution["reason_code"] != reason_by_status[resolution["status"]]
            or any(binding_id not in rust_bindings for binding_id in resolution["binding_ids"])
        ):
            raise ContractError("P2 Rust receiver type source evidence drifted")
        scope_candidates = [
            scope
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["crate_target_id"] == resolution["crate_target_id"]
            and scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        ]
        scope_candidates.sort(
            key=lambda scope: (
                scope["byte_end"] - scope["byte_start"],
                -scope["byte_start"],
                scope["scope_id"],
            )
        )
        if (
            not scope_candidates
            or scope_candidates[0]["scope_id"] != lexical_scope["scope_id"]
        ):
            raise ContractError("P2 Rust receiver lexical scope drifted")
        expected_query_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-receiver-type-query/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
        )
        expected_resolution_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-receiver-type-resolution/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
            resolution["path_status"],
            resolution["status"],
            resolution["canonical_path"] or "",
        )
        if (
            resolution["query_binding_id"] != expected_query_id
            or resolution["resolution_id"] != expected_resolution_id
        ):
            raise ContractError("P2 Rust receiver type identity drifted")
        digest_payload = {
            key: value
            for key, value in resolution.items()
            if key != "resolution_digest"
        }
        if resolution["resolution_digest"] != hashlib.sha256(
            canonical_json(digest_payload).encode("utf-8")
        ).hexdigest():
            raise ContractError("P2 Rust receiver type digest drifted")

        query = {
            "binding_id": expected_query_id,
            "binding_kind": "use",
            "crate_target_id": resolution["crate_target_id"],
            "local_name": f"__receiver_type_{symbol['site_id']}",
            "namespace": "type",
            "reason_code": "rust_binding_path_resolution_pending",
            "resolution": "unresolved",
            "scope_id": lexical_scope["scope_id"],
            "source_node_id": syntax_site["source_node_id"],
            "surface_target_path": resolution["query_path"],
            "syntax_site_id": symbol["syntax_site_id"],
            "target_scope_id": None,
            "target_symbol_site_id": None,
            "visibility": "private",
            "visibility_path": None,
        }
        replay_record = {
            **resolution,
            "binding_ids": [expected_query_id, *resolution["binding_ids"]],
            "source_binding_id": expected_query_id,
            "status": resolution["path_status"],
        }
        rust_bindings[expected_query_id] = query
        replay_key = (
            query["crate_target_id"],
            query["scope_id"],
            query["local_name"],
        )
        rust_path_replay_index["bindings_by_scope_name"][replay_key] = [
            expected_query_id
        ]
        try:
            replay_rust_type_path_resolution(
                replay_record,
                rust_scopes,
                rust_bindings,
                rust_path_replay_index,
            )
        finally:
            del rust_bindings[expected_query_id]
            del rust_path_replay_index["bindings_by_scope_name"][replay_key]
        receiver_resolutions_by_symbol.setdefault(symbol["site_id"], []).append(
            resolution
        )
    expected_receiver_symbols = {
        site_id
        for site_id, site in symbol_sites.items()
        if site["language"] == "rust"
        and site["role"] == "call_target"
        and site["receiver_type_signature"] is not None
    }
    if set(receiver_resolutions_by_symbol) != expected_receiver_symbols:
        raise ContractError("P2 Rust receiver type coverage is incomplete")
    for symbol_id in expected_receiver_symbols:
        symbol = symbol_sites[symbol_id]
        syntax_site = syntax[symbol["syntax_site_id"]]
        expected_targets = {
            scope["crate_target_id"]
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        }
        actual_targets = {
            resolution["crate_target_id"]
            for resolution in receiver_resolutions_by_symbol[symbol_id]
        }
        if actual_targets != expected_targets or len(
            receiver_resolutions_by_symbol[symbol_id]
        ) != len(expected_targets):
            raise ContractError("P2 Rust receiver target coverage is incomplete")

    owner_resolutions = _unique_index(
        manifests["rust_method_owner_resolutions"]["records"],
        "resolution_id",
        "P2 Rust method owner resolutions",
    )
    owner_resolutions_by_symbol: dict[str, list[dict[str, Any]]] = {}
    for resolution in owner_resolutions.values():
        symbol = symbol_sites.get(resolution["source_symbol_site_id"])
        syntax_site = syntax.get(resolution["source_syntax_site_id"])
        lexical_scope = rust_scopes.get(resolution["lexical_scope_id"])
        expected_query_path = RUST_PRELUDE_TYPE_PATHS.get(
            symbol["owner_signature"] if symbol is not None else "",
            symbol["owner_signature"] if symbol is not None else "",
        )
        if (
            symbol is None
            or symbol["language"] != "rust"
            or symbol["role"] != "declaration"
            or symbol["owner_signature"] is None
            or syntax_site is None
            or symbol["syntax_site_id"] != syntax_site["site_id"]
            or lexical_scope is None
            or lexical_scope["crate_target_id"] != resolution["crate_target_id"]
            or lexical_scope["source_node_id"] != syntax_site["source_node_id"]
            or resolution["surface_path"] != symbol["owner_signature"]
            or resolution["query_path"] != expected_query_path
            or resolution["reason_code"] != reason_by_status[resolution["status"]]
            or any(binding_id not in rust_bindings for binding_id in resolution["binding_ids"])
        ):
            raise ContractError("P2 Rust method owner source evidence drifted")
        scope_candidates = [
            scope
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["crate_target_id"] == resolution["crate_target_id"]
            and scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        ]
        scope_candidates.sort(
            key=lambda scope: (
                scope["byte_end"] - scope["byte_start"],
                -scope["byte_start"],
                scope["scope_id"],
            )
        )
        if not scope_candidates or scope_candidates[0]["scope_id"] != lexical_scope["scope_id"]:
            raise ContractError("P2 Rust method owner lexical scope drifted")
        expected_query_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-method-owner-query/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
        )
        expected_resolution_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-method-owner-resolution/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
            resolution["status"],
            resolution["canonical_path"] or "",
        )
        if (
            resolution["query_binding_id"] != expected_query_id
            or resolution["resolution_id"] != expected_resolution_id
        ):
            raise ContractError("P2 Rust method owner identity drifted")
        digest_payload = {
            key: value for key, value in resolution.items() if key != "resolution_digest"
        }
        if resolution["resolution_digest"] != hashlib.sha256(
            canonical_json(digest_payload).encode("utf-8")
        ).hexdigest():
            raise ContractError("P2 Rust method owner digest drifted")
        query = {
            "binding_id": expected_query_id,
            "binding_kind": "use",
            "crate_target_id": resolution["crate_target_id"],
            "local_name": f"__method_owner_{symbol['site_id']}",
            "namespace": "type",
            "reason_code": "rust_binding_path_resolution_pending",
            "resolution": "unresolved",
            "scope_id": lexical_scope["scope_id"],
            "source_node_id": syntax_site["source_node_id"],
            "surface_target_path": resolution["query_path"],
            "syntax_site_id": symbol["syntax_site_id"],
            "target_scope_id": None,
            "target_symbol_site_id": None,
            "visibility": "private",
            "visibility_path": None,
        }
        replay_record = {
            **resolution,
            "binding_ids": [expected_query_id, *resolution["binding_ids"]],
            "source_binding_id": expected_query_id,
        }
        rust_bindings[expected_query_id] = query
        replay_key = (query["crate_target_id"], query["scope_id"], query["local_name"])
        rust_path_replay_index["bindings_by_scope_name"][replay_key] = [expected_query_id]
        try:
            replay_rust_type_path_resolution(
                replay_record, rust_scopes, rust_bindings, rust_path_replay_index
            )
        finally:
            del rust_bindings[expected_query_id]
            del rust_path_replay_index["bindings_by_scope_name"][replay_key]
        owner_resolutions_by_symbol.setdefault(symbol["site_id"], []).append(resolution)
    expected_owner_symbols = {
        site_id
        for site_id, site in symbol_sites.items()
        if site["language"] == "rust"
        and site["role"] == "declaration"
        and site["owner_signature"] is not None
    }
    if set(owner_resolutions_by_symbol) != expected_owner_symbols:
        raise ContractError("P2 Rust method owner coverage is incomplete")
    for symbol_id in expected_owner_symbols:
        symbol = symbol_sites[symbol_id]
        syntax_site = syntax[symbol["syntax_site_id"]]
        expected_targets = {
            scope["crate_target_id"]
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        }
        actual_targets = {
            resolution["crate_target_id"]
            for resolution in owner_resolutions_by_symbol[symbol_id]
        }
        if actual_targets != expected_targets or len(owner_resolutions_by_symbol[symbol_id]) != len(
            expected_targets
        ):
            raise ContractError("P2 Rust method owner target coverage is incomplete")

    qualified_resolutions = _unique_index(
        manifests["rust_qualified_owner_resolutions"]["records"],
        "resolution_id",
        "P2 Rust qualified owner resolutions",
    )
    qualified_resolutions_by_symbol: dict[str, list[dict[str, Any]]] = {}
    qualified_reason_by_status = {
        **reason_by_status,
        "generic_parameter": "rust_qualified_owner_generic_parameter",
    }
    for resolution in qualified_resolutions.values():
        symbol = symbol_sites.get(resolution["source_symbol_site_id"])
        syntax_site = syntax.get(resolution["source_syntax_site_id"])
        lexical_scope = rust_scopes.get(resolution["lexical_scope_id"])
        expected_query_path = RUST_PRELUDE_TYPE_PATHS.get(
            symbol["qualified_owner_signature"] if symbol is not None else "",
            symbol["qualified_owner_signature"] if symbol is not None else "",
        )
        if symbol is not None and symbol["qualified_owner_kind"] == "implicit_self":
            expected_query_path = symbol["owner_signature"] or ""
        expected_status = resolution["path_status"]
        if symbol is not None and symbol["qualified_owner_kind"] == "generic_parameter":
            if resolution["path_status"] != "unresolved":
                raise ContractError("P2 Rust generic qualified owner unexpectedly resolved")
            expected_status = "generic_parameter"
        if (
            symbol is None
            or symbol["language"] != "rust"
            or symbol["role"] != "call_target"
            or symbol["qualified_owner_signature"] is None
            or syntax_site is None
            or symbol["syntax_site_id"] != syntax_site["site_id"]
            or lexical_scope is None
            or lexical_scope["crate_target_id"] != resolution["crate_target_id"]
            or lexical_scope["source_node_id"] != syntax_site["source_node_id"]
            or resolution["surface_path"] != symbol["qualified_owner_signature"]
            or resolution["query_path"] != expected_query_path
            or resolution["qualified_owner_evidence"] != symbol["qualified_owner_evidence"]
            or resolution["qualified_owner_kind"] != symbol["qualified_owner_kind"]
            or resolution["status"] != expected_status
            or resolution["reason_code"] != qualified_reason_by_status[resolution["status"]]
            or any(binding_id not in rust_bindings for binding_id in resolution["binding_ids"])
        ):
            raise ContractError("P2 Rust qualified owner source evidence drifted")
        scope_candidates = [
            scope
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["crate_target_id"] == resolution["crate_target_id"]
            and scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        ]
        scope_candidates.sort(
            key=lambda scope: (
                scope["byte_end"] - scope["byte_start"],
                -scope["byte_start"],
                scope["scope_id"],
            )
        )
        if not scope_candidates or scope_candidates[0]["scope_id"] != lexical_scope["scope_id"]:
            raise ContractError("P2 Rust qualified owner lexical scope drifted")
        expected_query_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-qualified-owner-query/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
        )
        expected_resolution_id = stable_parts_id(
            "decodex/lane-authority-v2-rust-qualified-owner-resolution/1",
            symbol["site_id"],
            resolution["crate_target_id"],
            lexical_scope["scope_id"],
            resolution["surface_path"],
            resolution["query_path"],
            resolution["path_status"],
            resolution["status"],
            resolution["canonical_path"] or "",
        )
        if (
            resolution["query_binding_id"] != expected_query_id
            or resolution["resolution_id"] != expected_resolution_id
        ):
            raise ContractError("P2 Rust qualified owner identity drifted")
        digest_payload = {
            key: value for key, value in resolution.items() if key != "resolution_digest"
        }
        if resolution["resolution_digest"] != hashlib.sha256(
            canonical_json(digest_payload).encode("utf-8")
        ).hexdigest():
            raise ContractError("P2 Rust qualified owner digest drifted")
        query = {
            "binding_id": expected_query_id,
            "binding_kind": "use",
            "crate_target_id": resolution["crate_target_id"],
            "local_name": f"__qualified_owner_{symbol['site_id']}",
            "namespace": "type",
            "reason_code": "rust_binding_path_resolution_pending",
            "resolution": "unresolved",
            "scope_id": lexical_scope["scope_id"],
            "source_node_id": syntax_site["source_node_id"],
            "surface_target_path": resolution["query_path"],
            "syntax_site_id": symbol["syntax_site_id"],
            "target_scope_id": None,
            "target_symbol_site_id": None,
            "visibility": "private",
            "visibility_path": None,
        }
        replay_record = {
            **resolution,
            "binding_ids": [expected_query_id, *resolution["binding_ids"]],
            "source_binding_id": expected_query_id,
            "status": resolution["path_status"],
        }
        rust_bindings[expected_query_id] = query
        replay_key = (query["crate_target_id"], query["scope_id"], query["local_name"])
        rust_path_replay_index["bindings_by_scope_name"][replay_key] = [expected_query_id]
        try:
            replay_rust_type_path_resolution(
                replay_record, rust_scopes, rust_bindings, rust_path_replay_index
            )
        finally:
            del rust_bindings[expected_query_id]
            del rust_path_replay_index["bindings_by_scope_name"][replay_key]
        qualified_resolutions_by_symbol.setdefault(symbol["site_id"], []).append(resolution)
    expected_qualified_symbols = {
        site_id
        for site_id, site in symbol_sites.items()
        if site["language"] == "rust"
        and site["role"] == "call_target"
        and site["qualified_owner_signature"] is not None
    }
    if set(qualified_resolutions_by_symbol) != expected_qualified_symbols:
        raise ContractError("P2 Rust qualified owner coverage is incomplete")
    for symbol_id in expected_qualified_symbols:
        symbol = symbol_sites[symbol_id]
        syntax_site = syntax[symbol["syntax_site_id"]]
        expected_targets = {
            scope["crate_target_id"]
            for scope in scopes_by_source[syntax_site["source_node_id"]]
            if scope["byte_start"] <= syntax_site["byte_start"]
            and syntax_site["byte_end"] <= scope["byte_end"]
        }
        actual_targets = {
            resolution["crate_target_id"]
            for resolution in qualified_resolutions_by_symbol[symbol_id]
        }
        if actual_targets != expected_targets or len(
            qualified_resolutions_by_symbol[symbol_id]
        ) != len(expected_targets):
            raise ContractError("P2 Rust qualified owner target coverage is incomplete")

    unresolved_symbols = sum(
        site["resolution"] == "unresolved" for site in symbol_sites.values()
    )
    all_site_ids = set(syntax) | set(symbol_sites) | set(data_sites)
    call_edges = _unique_index(
        manifests["call_edges"]["records"], "edge_id", "P2 call edges"
    )
    dataflow_edges = _unique_index(
        manifests["dataflow_edges"]["records"], "edge_id", "P2 dataflow edges"
    )
    for relation_name, edges in (
        ("call", call_edges),
        ("dataflow", dataflow_edges),
    ):
        if not edges or any(
            edge["from_site_id"] not in all_site_ids
            or edge["to_site_id"] not in all_site_ids
            for edge in edges.values()
        ):
            raise ContractError(f"P2 {relation_name} graph has missing endpoints")
    call_targets: dict[str, set[str]] = {site_id: set() for site_id in symbol_sites}
    for edge in call_edges.values():
        source = symbol_sites.get(edge["from_site_id"])
        target = symbol_sites.get(edge["to_site_id"])
        if (
            source is None
            or target is None
            or source["resolution"] != "local"
            or target["resolution"] != "declaration"
        ):
            raise ContractError("P2 call edge lacks local symbol resolution")
        call_targets[source["site_id"]].add(target["site_id"])
    owner_definitions_by_key: dict[tuple[str, str, str], set[str]] = {}
    for resolution in owner_resolutions.values():
        owner_site_id = resolution["canonical_type_definition_site_id"]
        if resolution["status"] != "resolved_local_type" or owner_site_id is None:
            continue
        declaration = symbol_sites[resolution["source_symbol_site_id"]]
        owner_definitions_by_key.setdefault(
            (resolution["crate_target_id"], owner_site_id, declaration["signature"]), set()
        ).add(declaration["site_id"])
    module_definitions_by_key: dict[tuple[str, str, str], set[str]] = {}
    for declaration in symbol_sites.values():
        if (
            declaration["language"] != "rust"
            or declaration["role"] != "declaration"
            or declaration["owner_signature"] is not None
        ):
            continue
        syntax_site = syntax[declaration["syntax_site_id"]]
        by_target: dict[str, list[dict[str, Any]]] = {}
        for scope in scopes_by_source.get(syntax_site["source_node_id"], []):
            if (
                scope["byte_start"] <= syntax_site["byte_start"]
                and syntax_site["byte_end"] <= scope["byte_end"]
            ):
                by_target.setdefault(scope["crate_target_id"], []).append(scope)
        for target_id, target_scopes in by_target.items():
            target_scopes.sort(
                key=lambda scope: (
                    scope["byte_end"] - scope["byte_start"],
                    -scope["byte_start"],
                    scope["scope_id"],
                )
            )
            lexical_scope = target_scopes[0]
            if lexical_scope["scope_kind"] == "block":
                continue
            module_definitions_by_key.setdefault(
                (target_id, lexical_scope["scope_id"], declaration["signature"]), set()
            ).add(declaration["site_id"])
    expected_rust_targets: dict[str, set[str]] = {}
    for resolution in receiver_resolutions.values():
        owner_site_id = resolution["canonical_type_definition_site_id"]
        if resolution["status"] != "resolved_local_type" or owner_site_id is None:
            continue
        call = symbol_sites[resolution["source_symbol_site_id"]]
        method_name = call["signature"].rsplit("::", 1)[-1]
        definitions = owner_definitions_by_key.get(
            (resolution["crate_target_id"], owner_site_id, method_name), set()
        )
        if len(definitions) == 1:
            expected_rust_targets.setdefault(call["site_id"], set()).update(definitions)
    for resolution in qualified_resolutions.values():
        call = symbol_sites[resolution["source_symbol_site_id"]]
        method_name = call["signature"].rsplit("::", 1)[-1]
        definitions: set[str] = set()
        if resolution["status"] == "resolved_local_type":
            owner_site_id = resolution["canonical_type_definition_site_id"]
            if owner_site_id is not None:
                definitions = owner_definitions_by_key.get(
                    (resolution["crate_target_id"], owner_site_id, method_name), set()
                )
        elif resolution["status"] == "resolved_local_module":
            module_scope_id = resolution["canonical_module_scope_id"]
            if module_scope_id is not None:
                definitions = module_definitions_by_key.get(
                    (resolution["crate_target_id"], module_scope_id, method_name), set()
                )
        if len(definitions) == 1:
            expected_rust_targets.setdefault(call["site_id"], set()).update(definitions)
    for site_id, site in symbol_sites.items():
        if site["language"] == "rust" and call_targets[site_id] != expected_rust_targets.get(
            site_id, set()
        ):
            raise ContractError("P2 Rust call edge bypasses canonical owner identity")
        if call_targets[site_id] != set(site["definition_site_ids"]):
            raise ContractError("P2 symbol definition set disagrees with call edges")

    def source_id_for_site(site_id: str) -> str:
        if site_id in syntax:
            return syntax[site_id]["source_node_id"]
        if site_id in symbol_sites:
            return syntax[symbol_sites[site_id]["syntax_site_id"]]["source_node_id"]
        return syntax[data_sites[site_id]["syntax_site_id"]]["source_node_id"]

    receipts = _unique_index(
        manifests["toolchain_receipts"]["records"],
        "receipt_id",
        "P2 toolchain receipts",
    )
    expected_receipt_keys = {
        (language, role, platform)
        for language in EXPECTED_LANGUAGES
        for role, platform in (
            ("parser", "common"),
            ("platform_slice", "linux"),
            ("platform_slice", "macos"),
        )
    }
    actual_receipt_keys = {
        (receipt["language"], receipt["receipt_role"], receipt["platform"])
        for receipt in receipts.values()
    }
    if actual_receipt_keys != expected_receipt_keys:
        raise ContractError("P2 parser/platform receipts do not cover the toolchain matrix")
    projection_ids_by_key: dict[tuple[str, str], set[str]] = {}
    for projection_id, projection in projections.items():
        if projection["projection_kind"] == "config":
            projection_ids_by_key.setdefault(
                (projection["language"], projection["platform"]), set()
            ).add(projection_id)
    candidate_source = {
        candidate["candidate_id"]: candidate["source_node_id"] for candidate in candidates
    }
    for receipt in receipts.values():
        source_ids = {
            source_id
            for source_id, source in sources.items()
            if source["status"] == "current" and source["language"] == receipt["language"]
        }
        if set(receipt["completed_source_node_ids"]) != source_ids:
            raise ContractError("P2 tool receipt completed source set is incomplete")
        if set(receipt["config_projection_ids"]) != projection_ids_by_key[
            (receipt["language"], receipt["platform"])
        ]:
            raise ContractError("P2 tool receipt config projection set is incomplete")
        observed_sets = {
            "expected_source_node": source_ids,
            "completed_source_node": source_ids,
            "syntax_site": {
                site_id for site_id in syntax if source_id_for_site(site_id) in source_ids
            },
            "candidate_record": {
                candidate_id
                for candidate_id, source_id in candidate_source.items()
                if source_id in source_ids
            },
            "call_edge": {
                edge_id
                for edge_id, edge in call_edges.items()
                if source_id_for_site(edge["from_site_id"]) in source_ids
            },
            "dataflow_edge": {
                edge_id
                for edge_id, edge in dataflow_edges.items()
                if source_id_for_site(edge["from_site_id"]) in source_ids
            },
        }
        for prefix, identifiers in observed_sets.items():
            if receipt[f"{prefix}_count"] != len(identifiers) or receipt[
                f"{prefix}_ids_digest"
            ] != stable_id_set_digest(
                f"decodex/lane-authority-v2-tool-receipt-{prefix}/1", identifiers
            ):
                raise ContractError(f"P2 tool receipt {prefix} evidence drifted")

    parser_errors = sum(source["parser_error_count"] for source in sources.values())
    return {
        **p1,
        "candidate_site_edge_count": len(candidate_edges),
        "cfg_projection_count": len(projections),
        "call_edge_count": len(call_edges),
        "data_site_count": len(data_sites),
        "dataflow_edge_count": len(dataflow_edges),
        "parser_error_count": parser_errors,
        "phase": "P2",
        "rust_module_scope_count": len(rust_scopes),
        "rust_name_binding_count": len(rust_bindings),
        "rust_path_resolution_count": len(rust_resolutions),
        "rust_receiver_type_resolution_count": len(receiver_resolutions),
        "rust_method_owner_resolution_count": len(owner_resolutions),
        "rust_qualified_owner_resolution_count": len(qualified_resolutions),
        "source_node_count": len(sources),
        "symbol_site_count": len(symbol_sites),
        "syntax_site_count": len(syntax),
        "toolchain_receipt_count": len(receipts),
        "unresolved_count": (
            parser_errors
            + len(candidate_ids)
            + len(dataflow_edges)
            + unresolved_symbols
        ),
        "unresolved_symbol_count": unresolved_symbols,
    }


def verify_p3(root: Path) -> dict[str, Any]:
    p2 = verify_p2(root)
    catalog = load_json(root, CATALOG_PATH)
    policy = load_json(root, EXTERNAL_SYMBOL_POLICY_PATH)
    authority_policy = load_json(root, AUTHORITY_SYMBOL_POLICY_PATH)
    validate_catalog_p3_policy_projection(catalog, policy, authority_policy)
    disposition_manifest = load_json(root, CATALOG_DISPOSITIONS_PATH)
    relation_schema = load_json(
        root, Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json")
    )
    validate_typed_relation_manifest(
        disposition_manifest, "catalog_entry_dispositions", relation_schema
    )
    symbol_manifest = load_json(
        root, Path("tools/lane-authority-inventory/manifests/relations/symbol_sites.json")
    )
    symbols = _unique_index(symbol_manifest["records"], "site_id", "P3 symbol sites")
    policy_sets = (
        ("reviewed_non_authority_external_symbols", policy),
        ("external_symbols", authority_policy),
    )
    policy_by_identity: dict[tuple[str, str], dict[str, Any]] = {}
    policy_by_id: dict[str, dict[str, Any]] = {}
    policy_digest_by_id: dict[str, str] = {}
    catalog_entries: dict[str, dict[str, Any]] = {}
    for section, section_policy in policy_sets:
        for entry in section_policy["entries"]:
            identity = (entry["language"], entry["signature"])
            if identity in policy_by_identity or entry["id"] in policy_by_id:
                raise ContractError("P3 authority policies overlap")
            policy_by_identity[identity] = entry
            policy_by_id[entry["id"]] = entry
            policy_digest_by_id[entry["id"]] = section_policy[
                "policy_semantic_digest"
            ]
        for entry in catalog[section]:
            if entry["id"] in catalog_entries:
                raise ContractError("P3 catalog contains a duplicate policy id")
            catalog_entries[entry["id"]] = entry
    matched_by_entry = {entry_id: set() for entry_id in policy_by_id}
    external_sites: set[str] = set()
    for site in symbols.values():
        if site["resolution"] == "external":
            raise ContractError("P3 observed a mutated P2 external symbol")
        identity = (site["language"], site["signature"])
        policy_entry = policy_by_identity.get(identity)
        if (
            policy_entry is not None
            and site["role"] == "call_target"
            and site["resolution"] == "unresolved"
        ):
            matched_by_entry[policy_entry["id"]].add(site["site_id"])
            external_sites.add(site["site_id"])
    if any(not site_ids for site_ids in matched_by_entry.values()):
        raise ContractError("P3 external policy contains an unused entry")

    for entry_id, site_ids in matched_by_entry.items():
        if set(catalog_entries[entry_id]["consumer_ids"]) != site_ids:
            raise ContractError("P3 catalog consumers disagree with exact policy matches")

    dispositions = _unique_index(
        disposition_manifest["records"],
        "disposition_id",
        "P3 catalog dispositions",
    )
    expected_dispositions: dict[str, dict[str, Any]] = {}
    for entry_id, site_ids in matched_by_entry.items():
        policy_entry = policy_by_id[entry_id]
        for site_id in site_ids:
            disposition_id = stable_parts_id(
                "decodex/lane-authority-v2-catalog-disposition/1", entry_id, site_id
            )
            expected_dispositions[disposition_id] = {
                "catalog_entry_id": entry_id,
                "disposition": "matched_site",
                "disposition_id": disposition_id,
                "evidence_digest": stable_parts_id(
                    "decodex/lane-authority-v2-external-policy-binding/1",
                    policy_digest_by_id[entry_id],
                    entry_id,
                    site_id,
                    symbols[site_id]["signature_digest"],
                ),
                "reason_code": policy_entry["reason_code"],
                "site_id": site_id,
            }
    if dispositions != expected_dispositions:
        raise ContractError("P3 catalog dispositions are not the exact policy projection")
    expected_used_digest = stable_id_set_digest(
        "decodex/lane-authority-v2-used-external-symbol-sites/1", external_sites
    )
    if catalog["used_external_symbol_set_digest"] != expected_used_digest:
        raise ContractError("P3 used external symbol digest disagrees")
    authority_entry_ids = {entry["id"] for entry in authority_policy["entries"]}
    authority_sites = set().union(
        *(matched_by_entry[entry_id] for entry_id in authority_entry_ids)
    )
    return {
        **p2,
        "authority_policy_entry_count": len(authority_policy["entries"]),
        "authority_symbol_count": len(authority_sites),
        "catalog_disposition_count": len(dispositions),
        "catalog_status": catalog["catalog_status"],
        "external_policy_entry_count": len(policy["entries"]),
        "external_symbol_count": len(external_sites),
        "non_authority_external_symbol_count": len(external_sites - authority_sites),
        "phase": "P3",
    }


def readiness_rejection(root: Path) -> dict[str, Any]:
    catalog = load_json(root, CATALOG_PATH)
    evidence = (
        verify_p3(root)
        if catalog.get("catalog_status") == "p3_machine_validated_incomplete"
        else verify_p2(root)
    )
    report = {
        "advancement_state": "C1I_INCOMPLETE",
        "analysis_input_digest": evidence["analysis_cut_digest"],
        "counts": evidence["candidate_anchors"],
        "contract_digests": evidence["contract_digests"],
        "phase": evidence["phase"],
        "rejections": [
            {
                "actual_digest": None,
                "candidate_ids": [],
                "expected_digest": None,
                "reason_code": "c1i_phase_incomplete",
                "site_ids": [],
                "tool_receipt_ids": [],
            }
        ],
        "schema": "decodex/lane-authority-v2-c1i-rejection-report/1",
        "status": "rejected",
    }
    validate_rejection_report(root, report)
    return report


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--phase", choices=["P0", "P1", "P2", "P3"])
    mode.add_argument("--review-preimage", action="store_true")
    mode.add_argument("--readiness", choices=["C1I"])
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    root = Path(run_git(Path.cwd(), "rev-parse", "--show-toplevel"))
    try:
        if args.phase == "P0":
            print(canonical_json(verify_p0(root)), end="")
            return 0
        if args.phase == "P1":
            print(canonical_json(verify_p1(root)), end="")
            return 0
        if args.phase == "P2":
            print(canonical_json(verify_p2(root)), end="")
            return 0
        if args.phase == "P3":
            print(canonical_json(verify_p3(root)), end="")
            return 0
        if args.review_preimage:
            evidence = verify_p0(root, require_review=False)
            cut = load_json(root, CHECKPOINT_PATH)["provisional_analysis_cut_anchor"]
            evidence["review_input_digest"] = review_scope_digest(
                root, cut["provisional_pr_base_commit"]
            )
            print(canonical_json(evidence), end="")
            return 0
        report = readiness_rejection(root)
        rendered = canonical_json(report)
        print(rendered, end="")
        return 1
    except (ContractError, FileNotFoundError, KeyError, TypeError, ValueError) as error:
        print(f"C1I P0 contract invalid: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
