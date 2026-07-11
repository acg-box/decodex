#!/usr/bin/env python3
"""Project exact external-symbol policy decisions onto the P2 symbol universe."""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

import verify_contract as contract


SYMBOLS_PATH = Path(
    "tools/lane-authority-inventory/manifests/relations/symbol_sites.json"
)
P3_OUTPUT_PATHS = {
    contract.CATALOG_PATH,
    contract.CATALOG_DISPOSITIONS_PATH,
    contract.CFG_COVERAGE_PATH,
    contract.DATAFLOW_PROOFS_PATH,
    Path("tools/lane-authority-inventory/manifests/inventory_composition.json"),
    Path("tools/lane-authority-inventory/manifests/relations/candidate_adjudications.json"),
    Path("tools/lane-authority-inventory/manifests/relations/site_classifications.json"),
    Path("tools/lane-authority-inventory/manifests/relations/symbol_dispositions.json"),
}


def relation(name: str, records: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "records": records,
        "relation": name,
        "schema": f"decodex/lane-authority-v2-c1i-{name.replace('_', '-')}/1",
    }


def write_json(root: Path, path: Path, value: dict[str, Any]) -> None:
    destination = root / path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(contract.canonical_json(value), encoding="utf-8")


def empty_catalog_projection() -> dict[str, Any]:
    return {
        "catalog_semantic_digest": "0" * 64,
        "catalog_status": "p3_machine_validated_incomplete",
        "dynamic_capability_roots": [],
        "executable_declarative_paths": [],
        "external_symbols": [],
        "languages": ["python", "rust", "shell", "swift", "toml", "yaml"],
        "local_closure_boundaries": [],
        "persistent_data_roots": [],
        "provider_and_config_roots": [],
        "review_gate": {
            "architecture_review_complete": True,
            "p0_p4_machine_validation_only": True,
            "p5_integrated_digest_requires_fresh_review": True,
            "semantic_change_invalidates_ready_review": True,
        },
        "reviewed_non_authority_external_symbols": [],
        "schema": "decodex/lane-authority-v2-authority-surface-catalog/1",
        "toolchain_matrix": [],
        "used_external_symbol_set_digest": "0" * 64,
    }


def immutable_input_digests(root: Path) -> dict[Path, str]:
    roots = (
        root / "tools/lane-authority-inventory/catalog",
        root / "tools/lane-authority-inventory/manifests",
    )
    paths = {
        path.relative_to(root)
        for input_root in roots
        for path in input_root.rglob("*.json")
        if path.relative_to(root) not in P3_OUTPUT_PATHS
    }
    return {path: contract.sha256_path(root, path) for path in sorted(paths)}


def assert_immutable_inputs(root: Path, before: dict[Path, str]) -> None:
    after = immutable_input_digests(root)
    if after != before:
        changed = sorted(
            str(path)
            for path in set(before) | set(after)
            if before.get(path) != after.get(path)
        )
        raise contract.ContractError(
            f"P3 mutated authored or P1/P2 inputs: {changed}"
        )


def materialize(root: Path) -> dict[str, Any]:
    contract.verify_p2(root, allow_pending_authority_projection=True)
    immutable_before = immutable_input_digests(root)
    policy = contract.load_json(root, contract.EXTERNAL_SYMBOL_POLICY_PATH)
    authority_policy = contract.load_json(root, contract.AUTHORITY_SYMBOL_POLICY_PATH)
    contract.validate_external_symbol_policy(policy)
    contract.validate_authority_symbol_policy(authority_policy)
    symbol_manifest = contract.load_json(root, SYMBOLS_PATH)
    policy_sets = (
        (
            "reviewed_non_authority_external_symbols",
            policy,
            contract.policy_catalog_entry,
        ),
        ("external_symbols", authority_policy, contract.authority_policy_catalog_entry),
    )
    policy_by_identity: dict[tuple[str, str], dict[str, Any]] = {}
    policy_by_id: dict[str, dict[str, Any]] = {}
    policy_digest_by_id: dict[str, str] = {}
    for _section, section_policy, _builder in policy_sets:
        for entry in section_policy["entries"]:
            identity = (entry["language"], entry["signature"])
            if identity in policy_by_identity or entry["id"] in policy_by_id:
                raise contract.ContractError("external symbol policies overlap")
            policy_by_identity[identity] = entry
            policy_by_id[entry["id"]] = entry
            policy_digest_by_id[entry["id"]] = section_policy[
                "policy_semantic_digest"
            ]
    consumers = {entry_id: set() for entry_id in policy_by_id}
    symbols = symbol_manifest["records"]
    for site in symbols:
        if site["role"] != "call_target":
            continue
        entry = policy_by_identity.get((site["language"], site["signature"]))
        if entry is None or site["resolution"] != "unresolved":
            continue
        consumers[entry["id"]].add(site["site_id"])
    unused_entries = sorted(entry_id for entry_id, sites in consumers.items() if not sites)
    if unused_entries:
        raise contract.ContractError(
            f"external symbol policy entries have no exact consumers: {unused_entries}"
        )

    catalog = empty_catalog_projection()
    for section, section_policy, builder in policy_sets:
        section_ids = {entry["id"] for entry in section_policy["entries"]}
        catalog[section] = [
            builder(policy_by_id[entry_id], consumers[entry_id])
            for entry_id in sorted(section_ids)
        ]
    external_site_ids = set().union(*consumers.values())
    catalog["used_external_symbol_set_digest"] = contract.stable_id_set_digest(
        "decodex/lane-authority-v2-used-external-symbol-sites/1", external_site_ids
    )
    catalog["catalog_semantic_digest"] = contract.catalog_semantic_digest(catalog)
    contract.validate_catalog_p3_policy_projection(catalog, policy, authority_policy)

    symbols_by_id = {site["site_id"]: site for site in symbols}
    dispositions: list[dict[str, Any]] = []
    for entry_id in sorted(policy_by_id):
        entry = policy_by_id[entry_id]
        for site_id in sorted(consumers[entry_id]):
            disposition_id = contract.stable_parts_id(
                "decodex/lane-authority-v2-catalog-disposition/1", entry_id, site_id
            )
            dispositions.append(
                {
                    "catalog_entry_id": entry_id,
                    "disposition": "matched_site",
                    "disposition_id": disposition_id,
                    "evidence_digest": contract.stable_parts_id(
                        "decodex/lane-authority-v2-external-policy-binding/1",
                        policy_digest_by_id[entry_id],
                        entry_id,
                        site_id,
                        symbols_by_id[site_id]["signature_digest"],
                    ),
                    "reason_code": entry["reason_code"],
                    "site_id": site_id,
                }
            )
    dispositions.sort(key=lambda disposition: disposition["disposition_id"])

    call_edges = contract.load_json(
        root,
        Path("tools/lane-authority-inventory/manifests/relations/call_edges.json"),
    )["records"]
    call_edge_ids_by_site: dict[str, list[str]] = {}
    for edge in call_edges:
        call_edge_ids_by_site.setdefault(edge["from_site_id"], []).append(edge["edge_id"])
    catalog_disposition_by_site = {
        disposition["site_id"]: disposition for disposition in dispositions
    }
    authority_policy_ids = {entry["id"] for entry in authority_policy["entries"]}
    symbol_dispositions: list[dict[str, Any]] = []
    for site in symbols:
        if site["role"] != "call_target":
            continue
        policy_entry = policy_by_identity.get((site["language"], site["signature"]))
        call_edge_ids = sorted(call_edge_ids_by_site.get(site["site_id"], []))
        catalog_disposition = catalog_disposition_by_site.get(site["site_id"])
        policy_entry_id = None
        catalog_disposition_id = None
        dataflow_proof_id = None
        if site["resolution"] == "local":
            if not call_edge_ids:
                raise contract.ContractError("local symbol lacks canonical call edges")
            disposition = "resolved_local"
            reason_code = "canonical_local_call"
        elif policy_entry is not None and site["resolution"] == "unresolved":
            if catalog_disposition is None or call_edge_ids:
                raise contract.ContractError(
                    "cataloged external symbol has inconsistent edge projection"
                )
            policy_entry_id = policy_entry["id"]
            catalog_disposition_id = catalog_disposition["disposition_id"]
            if policy_entry_id in authority_policy_ids:
                disposition = "cataloged_authority_external"
                reason_code = "authority_external_policy"
            else:
                disposition = "cataloged_non_authority_external"
                reason_code = "reviewed_non_authority_external_policy"
        else:
            disposition = "rejected_dynamic_target"
            reason_code = "dynamic_target_not_finite"
        disposition_id = contract.stable_parts_id(
            "decodex/lane-authority-v2-symbol-disposition/1", site["site_id"]
        )
        evidence_digest = contract.stable_parts_id(
            "decodex/lane-authority-v2-symbol-disposition-evidence/1",
            site["site_id"],
            site["signature_digest"],
            disposition,
            *(call_edge_ids or [""]),
            policy_entry_id or "",
            catalog_disposition_id or "",
            dataflow_proof_id or "",
        )
        symbol_dispositions.append(
            {
                "call_edge_ids": call_edge_ids,
                "catalog_disposition_id": catalog_disposition_id,
                "dataflow_proof_id": dataflow_proof_id,
                "disposition": disposition,
                "disposition_id": disposition_id,
                "evidence_digest": evidence_digest,
                "policy_entry_id": policy_entry_id,
                "reason_code": reason_code,
                "site_id": site["site_id"],
            }
        )
    symbol_dispositions.sort(key=lambda item: item["disposition_id"])

    write_json(root, contract.CATALOG_PATH, catalog)
    write_json(
        root,
        contract.CATALOG_DISPOSITIONS_PATH,
        relation("catalog_entry_dispositions", dispositions),
    )
    write_json(
        root,
        contract.SYMBOL_DISPOSITIONS_PATH,
        relation("symbol_dispositions", symbol_dispositions),
    )
    assert_immutable_inputs(root, immutable_before)
    evidence = contract.verify_p3(root)
    return {
        "catalog_dispositions": evidence["catalog_disposition_count"],
        "catalog_status": evidence["catalog_status"],
        "authority_policy_entries": evidence["authority_policy_entry_count"],
        "authority_symbols": evidence["authority_symbol_count"],
        "external_policy_entries": evidence["external_policy_entry_count"],
        "external_symbols": evidence["external_symbol_count"],
        "phase": evidence["phase"],
        "unresolved_symbols": evidence["unresolved_symbol_count"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.parse_args()
    root = Path(contract.run_git(Path.cwd(), "rev-parse", "--show-toplevel"))
    print(contract.canonical_json(materialize(root)), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
