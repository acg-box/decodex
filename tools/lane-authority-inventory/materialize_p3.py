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


def relation(records: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "records": records,
        "relation": "catalog_entry_dispositions",
        "schema": "decodex/lane-authority-v2-c1i-catalog-entry-dispositions/1",
    }


def write_json(root: Path, path: Path, value: dict[str, Any]) -> None:
    destination = root / path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(contract.canonical_json(value), encoding="utf-8")


def materialize(root: Path) -> dict[str, Any]:
    contract.verify_p2(root, allow_pending_authority_projection=True)
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
        if site["resolution"] == "external":
            if (
                entry is None
                or site["external"] is not True
                or site["definition_site_ids"]
            ):
                raise contract.ContractError(
                    "existing external symbol lacks exact policy authority"
                )
            consumers[entry["id"]].add(site["site_id"])
            continue
        if entry is None or site["resolution"] != "unresolved":
            continue
        site["external"] = True
        site["resolution"] = "external"
        consumers[entry["id"]].add(site["site_id"])
    unused_entries = sorted(entry_id for entry_id, sites in consumers.items() if not sites)
    if unused_entries:
        raise contract.ContractError(
            f"external symbol policy entries have no exact consumers: {unused_entries}"
        )

    catalog = contract.load_json(root, contract.CATALOG_PATH)
    if catalog["catalog_status"] not in {
        "p0_schema_only_incomplete",
        "p3_machine_validated_incomplete",
    }:
        raise contract.ContractError("P3 materialization requires a P0 or P3 policy catalog")
    catalog["catalog_status"] = "p3_machine_validated_incomplete"
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

    write_json(root, SYMBOLS_PATH, symbol_manifest)
    write_json(root, contract.CATALOG_PATH, catalog)
    write_json(root, contract.CATALOG_DISPOSITIONS_PATH, relation(dispositions))
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
