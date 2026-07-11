from __future__ import annotations

import copy
import hashlib
import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from unittest import mock

from jsonschema import Draft202012Validator
from jsonschema.exceptions import ValidationError


REPO_ROOT = Path(__file__).resolve().parents[2]
VERIFIER_PATH = REPO_ROOT / "tools/lane-authority-inventory/verify_contract.py"


def load_verifier_module():
    spec = importlib.util.spec_from_file_location("lane_authority_v2_c1i_contract", VERIFIER_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules["lane_authority_v2_c1i_contract"] = module
    spec.loader.exec_module(module)
    return module


class LaneAuthorityV2C1IContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.verifier = load_verifier_module()

    def p0_catalog(self):
        catalog = self.verifier.load_json(REPO_ROOT, self.verifier.CATALOG_PATH)
        catalog["catalog_status"] = "p0_schema_only_incomplete"
        catalog["catalog_semantic_digest"] = None
        catalog["used_external_symbol_set_digest"] = None
        for section in (
            "dynamic_capability_roots",
            "executable_declarative_paths",
            "external_symbols",
            "local_closure_boundaries",
            "persistent_data_roots",
            "provider_and_config_roots",
            "reviewed_non_authority_external_symbols",
            "toolchain_matrix",
        ):
            catalog[section] = []
        return catalog

    def test_p0_foundation_verifies_frozen_anchors_under_later_catalog(self):
        result = self.verifier.verify_p0(
            REPO_ROOT, require_review=False, allow_later_catalog=True
        )

        self.assertEqual("P0", result["phase"])
        self.assertEqual("C1I_INCOMPLETE", result["advancement_state"])
        self.assertEqual(
            "p3_machine_validated_incomplete", result["catalog_status"]
        )
        self.assertEqual(
            {
                "c0_source_files": 3363,
                "launcher_candidate_line_hits": 203,
                "legacy_candidate_line_hits": 40854,
                "mutation_candidate_line_hits": 39516,
            },
            result["candidate_anchors"],
        )

    def test_p0_phase_rejects_a_later_catalog(self):
        with self.assertRaisesRegex(self.verifier.ContractError, "P0 catalog"):
            self.verifier.verify_p0(REPO_ROOT, require_review=False)

    def test_c0_candidate_observations_are_reconstructed_from_frozen_artifacts(self):
        observations = self.verifier.expected_c0_candidate_observations(REPO_ROOT)
        counts = {"launcher": 0, "legacy": 0, "mutation": 0}
        for observation in observations.values():
            counts[observation["origin"]] += observation["candidate_line_count"]
        self.assertEqual(
            {"launcher": 203, "legacy": 40854, "mutation": 39516}, counts
        )

    def test_readiness_rejection_is_deterministic_and_reason_coded(self):
        evidence = {
            "analysis_cut_digest": "a" * 64,
            "candidate_anchors": {
                "c0_source_files": 3363,
                "launcher_candidate_line_hits": 203,
                "legacy_candidate_line_hits": 40854,
                "mutation_candidate_line_hits": 39516,
            },
            "contract_digests": {},
            "phase": "P1",
        }
        with mock.patch.object(self.verifier, "verify_p2", return_value=evidence):
            first = self.verifier.canonical_json(self.verifier.readiness_rejection(REPO_ROOT))
            second = self.verifier.canonical_json(self.verifier.readiness_rejection(REPO_ROOT))

        self.assertEqual(first, second)
        self.assertNotIn(str(REPO_ROOT), first)
        self.assertIn('"advancement_state":"C1I_INCOMPLETE"', first)
        self.assertIn('"reason_code":"c1i_phase_incomplete"', first)
        self.assertIn('"status":"rejected"', first)

    def test_p1_materialization_replays_the_exact_git_cut(self):
        result = self.verifier.verify_p1(REPO_ROOT)

        self.assertEqual("P1", result["phase"])
        self.assertEqual(3377, result["analysis_source_count"])
        self.assertGreaterEqual(result["tool_source_count"], 5)
        self.assertEqual(0, result["deleted_tombstone_count"])
        self.assertEqual(41057, result["candidate_record_count"])

    def test_p2_rejects_rust_module_scope_identity_tampering(self):
        relation_path = Path(
            "tools/lane-authority-inventory/manifests/relations/rust_module_scopes.json"
        )
        manifest = self.verifier.load_json(REPO_ROOT, relation_path)
        tampered = copy.deepcopy(manifest)
        scope = next(
            record
            for record in tampered["records"]
            if record["scope_kind"] == "block"
        )
        scope["canonical_module_path"] += "::forged"
        original_load = self.verifier.load_json

        def load_tampered(root, path):
            if path == relation_path:
                return copy.deepcopy(tampered)
            return original_load(root, path)

        with mock.patch.object(
            self.verifier, "load_json", side_effect=load_tampered
        ), self.assertRaisesRegex(
            self.verifier.ContractError, "Rust module scope id drifted"
        ):
            self.verifier.verify_p2(REPO_ROOT)

    def test_p2_rejects_rust_name_binding_identity_tampering(self):
        relation_path = Path(
            "tools/lane-authority-inventory/manifests/relations/rust_name_bindings.json"
        )
        manifest = self.verifier.load_json(REPO_ROOT, relation_path)
        tampered = copy.deepcopy(manifest)
        binding = next(
            record
            for record in tampered["records"]
            if record["resolution"] == "resolved"
        )
        binding["local_name"] += "Forged"
        original_load = self.verifier.load_json

        def load_tampered(root, path):
            if path == relation_path:
                return copy.deepcopy(tampered)
            return original_load(root, path)

        with mock.patch.object(
            self.verifier, "load_json", side_effect=load_tampered
        ), self.assertRaisesRegex(
            self.verifier.ContractError, "Rust name binding id drifted"
        ):
            self.verifier.verify_p2(REPO_ROOT)

    def test_p2_rejects_rehashed_rust_path_middle_hop_tampering(self):
        resolution_path = Path(
            "tools/lane-authority-inventory/manifests/relations/rust_path_resolutions.json"
        )
        binding_path = Path(
            "tools/lane-authority-inventory/manifests/relations/rust_name_bindings.json"
        )
        manifest = self.verifier.load_json(REPO_ROOT, resolution_path)
        bindings = self.verifier.load_json(REPO_ROOT, binding_path)["records"]
        tampered = copy.deepcopy(manifest)
        resolution = next(
            record
            for record in tampered["records"]
            if record["status"].startswith("resolved_local_")
            and len(record["binding_ids"]) >= 3
        )
        replacement = next(
            binding["binding_id"]
            for binding in bindings
            if binding["binding_id"] not in resolution["binding_ids"]
        )
        resolution["binding_ids"][1] = replacement
        resolution["resolution_digest"] = hashlib.sha256(
            self.verifier.canonical_json(
                {
                    key: value
                    for key, value in resolution.items()
                    if key != "resolution_digest"
                }
            ).encode("utf-8")
        ).hexdigest()
        original_load = self.verifier.load_json

        def load_tampered(root, path):
            if path == resolution_path:
                return copy.deepcopy(tampered)
            return original_load(root, path)

        with mock.patch.object(
            self.verifier, "load_json", side_effect=load_tampered
        ), self.assertRaisesRegex(
            self.verifier.ContractError, "binding chain is not replayable"
        ):
            self.verifier.verify_p2(REPO_ROOT)

    def test_changed_path_policy_rejects_runtime_source(self):
        self.assertEqual(
            ["apps/decodex/src/lib.rs"],
            self.verifier.unexpected_changed_paths(
                [
                    "apps/decodex/src/lib.rs",
                    "openwiki/specs/lane-authority-v2-inventory.md",
                    "tools/lane-authority-inventory/contracts/dataflow_contract.json",
                ]
            ),
        )

    def test_dataflow_contract_rejects_missing_top(self):
        contract = self.verifier.load_json(REPO_ROOT, self.verifier.DATAFLOW_PATH)
        weakened = copy.deepcopy(contract)
        weakened["value_lattice"].remove("Top")

        with self.assertRaisesRegex(self.verifier.ContractError, "Bottom-to-Top"):
            self.verifier.validate_dataflow_contract(weakened)

    def test_dataflow_contract_rejects_rule_set_drift_and_top_input(self):
        contract = self.verifier.load_json(REPO_ROOT, self.verifier.DATAFLOW_PATH)
        missing_rule = copy.deepcopy(contract)
        missing_rule["allowed_transfer_rules"].pop()
        with self.assertRaisesRegex(self.verifier.ContractError, "allowed_transfer_rules"):
            self.verifier.validate_dataflow_contract(missing_rule)

        duplicate_rule = copy.deepcopy(contract)
        duplicate_rule["allowed_transfer_rules"][-1]["id"] = duplicate_rule[
            "allowed_transfer_rules"
        ][0]["id"]
        with self.assertRaisesRegex(self.verifier.ContractError, "allowed_transfer_rules"):
            self.verifier.validate_dataflow_contract(duplicate_rule)

        top_input = copy.deepcopy(contract)
        top_input["allowed_transfer_rules"][0]["inputs"].append("Top")
        with self.assertRaisesRegex(self.verifier.ContractError, "allowed_transfer_rules"):
            self.verifier.validate_dataflow_contract(top_input)

    def test_dataflow_contract_rejects_receipt_and_top_transition_drift(self):
        contract = self.verifier.load_json(REPO_ROOT, self.verifier.DATAFLOW_PATH)
        missing_receipt = copy.deepcopy(contract)
        missing_receipt["accepted_proof_receipt_fields"].remove("fixed_point_digest")
        with self.assertRaisesRegex(self.verifier.ContractError, "receipt_fields"):
            self.verifier.validate_dataflow_contract(missing_receipt)

        weakened_top = copy.deepcopy(contract)
        weakened_top["top_transition"]["result"] = "Unknown"
        with self.assertRaisesRegex(self.verifier.ContractError, "top_transition"):
            self.verifier.validate_dataflow_contract(weakened_top)

    def test_p0_catalog_rejects_early_population(self):
        catalog = self.p0_catalog()
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
            with self.subTest(section=section):
                populated = copy.deepcopy(catalog)
                populated[section] = [{"id": "premature"}]
                with self.assertRaisesRegex(
                    self.verifier.ContractError, "must be empty until P3"
                ):
                    self.verifier.validate_catalog_p0(populated)

    def test_external_symbol_policy_is_exact_unique_and_review_pending(self):
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )

        self.verifier.validate_external_symbol_policy(policy)

        self.assertEqual(
            "p3_machine_validated_review_pending", policy["policy_status"]
        )
        self.assertTrue(
            all(
                entry["authority_relevance"] == "reviewed_non_authority"
                for entry in policy["entries"]
            )
        )

    def test_authority_symbol_policy_is_exact_unique_and_observed(self):
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.AUTHORITY_SYMBOL_POLICY_PATH
        )
        symbols = self.verifier.load_json(
            REPO_ROOT,
            Path(
                "tools/lane-authority-inventory/manifests/relations/symbol_sites.json"
            ),
        )["records"]

        self.verifier.validate_authority_symbol_policy(policy)

        observed_identities = {
            (site["language"], site["signature"])
            for site in symbols
            if site["role"] == "call_target"
        }
        self.assertGreater(len(policy["entries"]), 0)
        self.assertTrue(
            all(
                (entry["language"], entry["signature"]) in observed_identities
                for entry in policy["entries"]
            )
        )

    def test_authority_symbol_policy_rejects_duplicate_signature_and_wildcard(self):
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.AUTHORITY_SYMBOL_POLICY_PATH
        )
        duplicate = copy.deepcopy(policy)
        duplicate["entries"].insert(1, copy.deepcopy(duplicate["entries"][0]))
        duplicate["policy_semantic_digest"] = (
            self.verifier.authority_symbol_policy_semantic_digest(duplicate)
        )
        with self.assertRaisesRegex(
            self.verifier.ContractError, "duplicate language/signature"
        ):
            self.verifier.validate_authority_symbol_policy(duplicate)

        wildcard = copy.deepcopy(policy)
        exit_entry = next(
            entry
            for entry in wildcard["entries"]
            if entry["signature"] == "std::process::exit"
        )
        exit_entry["signature"] = "std::process::exit*"
        wildcard["policy_semantic_digest"] = (
            self.verifier.authority_symbol_policy_semantic_digest(wildcard)
        )
        with self.assertRaisesRegex(self.verifier.ContractError, "wildcard"):
            self.verifier.validate_authority_symbol_policy(wildcard)

    def test_external_symbol_policy_rejects_duplicate_signature_and_wildcard(self):
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        duplicate = copy.deepcopy(policy)
        duplicate["entries"].insert(1, copy.deepcopy(duplicate["entries"][0]))
        duplicate["policy_semantic_digest"] = (
            self.verifier.external_symbol_policy_semantic_digest(duplicate)
        )
        with self.assertRaisesRegex(
            self.verifier.ContractError, "duplicate language/signature"
        ):
            self.verifier.validate_external_symbol_policy(duplicate)

        wildcard = copy.deepcopy(policy)
        wildcard["entries"][15]["signature"] = "panic*"
        wildcard["policy_semantic_digest"] = (
            self.verifier.external_symbol_policy_semantic_digest(wildcard)
        )
        with self.assertRaisesRegex(self.verifier.ContractError, "wildcard"):
            self.verifier.validate_external_symbol_policy(wildcard)

    def test_external_symbol_policy_schema_excludes_authority_capabilities(self):
        schema = self.verifier.load_json(
            REPO_ROOT,
            Path(
                "tools/lane-authority-inventory/contracts/"
                "external_symbol_policy.schema.json"
            ),
        )
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        unsafe = copy.deepcopy(policy)
        unsafe["entries"][0]["capability_class"] = "filesystem"

        with self.assertRaises(ValidationError):
            Draft202012Validator(schema).validate(unsafe)

    def test_catalog_semantic_digest_excludes_generated_consumer_projection(self):
        catalog = self.p0_catalog()
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        catalog["catalog_status"] = "p3_machine_validated_incomplete"
        catalog["used_external_symbol_set_digest"] = "a" * 64
        catalog["reviewed_non_authority_external_symbols"] = [
            self.verifier.policy_catalog_entry(policy["entries"][0], {"site:one"})
        ]
        original = self.verifier.catalog_semantic_digest(catalog)

        catalog["used_external_symbol_set_digest"] = "b" * 64
        catalog["reviewed_non_authority_external_symbols"][0]["consumer_ids"] = [
            "site:two"
        ]
        catalog["reviewed_non_authority_external_symbols"][0][
            "used_site_set_digest"
        ] = "c" * 64
        self.assertEqual(original, self.verifier.catalog_semantic_digest(catalog))

        catalog["reviewed_non_authority_external_symbols"][0]["signature"] = "Other"
        self.assertNotEqual(original, self.verifier.catalog_semantic_digest(catalog))

    def test_p3_external_policy_projection_is_exact_and_incomplete(self):
        result = self.verifier.verify_p3(REPO_ROOT)

        self.assertEqual("P3", result["phase"])
        self.assertEqual("p3_machine_validated_incomplete", result["catalog_status"])
        external_policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        authority_policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.AUTHORITY_SYMBOL_POLICY_PATH
        )
        self.assertEqual(
            len(external_policy["entries"]), result["external_policy_entry_count"]
        )
        self.assertEqual(
            len(authority_policy["entries"]), result["authority_policy_entry_count"]
        )
        self.assertGreater(result["authority_symbol_count"], 0)
        self.assertGreater(result["non_authority_external_symbol_count"], 0)
        self.assertEqual(
            result["authority_symbol_count"]
            + result["non_authority_external_symbol_count"],
            result["external_symbol_count"],
        )
        self.assertEqual(
            result["external_symbol_count"], result["catalog_disposition_count"]
        )
        self.assertGreater(result["unresolved_symbol_count"], 0)

    def test_pending_authority_projection_is_only_allowed_for_materializer_preflight(self):
        catalog = self.verifier.load_json(REPO_ROOT, self.verifier.CATALOG_PATH)
        external_policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        authority_policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.AUTHORITY_SYMBOL_POLICY_PATH
        )
        stale = copy.deepcopy(catalog)
        stale["external_symbols"].pop()
        stale["catalog_semantic_digest"] = self.verifier.catalog_semantic_digest(stale)
        with self.assertRaisesRegex(self.verifier.ContractError, "authority policy"):
            self.verifier.validate_catalog_p3_policy_projection(
                stale, external_policy, authority_policy
            )
        self.verifier.validate_catalog_p3_policy_projection(
            stale,
            external_policy,
            authority_policy,
            allow_pending_authority_projection=True,
        )
        pending = copy.deepcopy(catalog)
        pending["external_symbols"] = []
        pending["catalog_semantic_digest"] = self.verifier.catalog_semantic_digest(
            pending
        )

        with self.assertRaisesRegex(self.verifier.ContractError, "authority policy entries"):
            self.verifier.validate_catalog_p3_policy_projection(
                pending, external_policy, authority_policy
            )
        self.verifier.validate_catalog_p3_policy_projection(
            pending,
            external_policy,
            authority_policy,
            allow_pending_authority_projection=True,
        )

    def test_p3_rejects_consumer_projection_tampering(self):
        catalog = self.verifier.load_json(REPO_ROOT, self.verifier.CATALOG_PATH)
        tampered = copy.deepcopy(catalog)
        entry = tampered["reviewed_non_authority_external_symbols"][0]
        entry["consumer_ids"].pop()
        entry["used_site_set_digest"] = self.verifier.stable_id_set_digest(
            "decodex/lane-authority-v2-catalog-entry-used-sites/1",
            set(entry["consumer_ids"]),
        )
        original_load = self.verifier.load_json

        def load_tampered(root, path):
            if path == self.verifier.CATALOG_PATH:
                return copy.deepcopy(tampered)
            return original_load(root, path)

        with (
            mock.patch.object(self.verifier, "verify_p2", return_value={}),
            mock.patch.object(self.verifier, "load_json", side_effect=load_tampered),
            self.assertRaisesRegex(self.verifier.ContractError, "consumers disagree"),
        ):
            self.verifier.verify_p3(REPO_ROOT)

    def test_p3_rejects_an_external_symbol_without_policy_authority(self):
        symbols_path = Path(
            "tools/lane-authority-inventory/manifests/relations/symbol_sites.json"
        )
        symbol_manifest = self.verifier.load_json(REPO_ROOT, symbols_path)
        tampered = copy.deepcopy(symbol_manifest)
        policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.EXTERNAL_SYMBOL_POLICY_PATH
        )
        authority_policy = self.verifier.load_json(
            REPO_ROOT, self.verifier.AUTHORITY_SYMBOL_POLICY_PATH
        )
        policy_identities = {
            (entry["language"], entry["signature"]) for entry in policy["entries"]
        } | {
            (entry["language"], entry["signature"])
            for entry in authority_policy["entries"]
        }
        site = next(
            site
            for site in tampered["records"]
            if site["role"] == "call_target"
            and site["resolution"] == "unresolved"
            and (site["language"], site["signature"]) not in policy_identities
        )
        site["external"] = True
        site["resolution"] = "external"
        original_load = self.verifier.load_json

        def load_tampered(root, path):
            if path == symbols_path:
                return copy.deepcopy(tampered)
            return original_load(root, path)

        with (
            mock.patch.object(self.verifier, "verify_p2", return_value={}),
            mock.patch.object(self.verifier, "load_json", side_effect=load_tampered),
            self.assertRaisesRegex(self.verifier.ContractError, "policy authority"),
        ):
            self.verifier.verify_p3(REPO_ROOT)

    def test_gate_distinguishes_valid_incomplete_from_invalid_contract(self):
        gate = subprocess.run(
            [str(REPO_ROOT / "scripts/verify_lane_authority_v2_gates.sh"), "C1I"],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(1, gate.returncode)
        self.assertIn('"reason_code":"c1i_phase_incomplete"', gate.stdout)

        with mock.patch.object(
            self.verifier,
            "verify_p0",
            side_effect=self.verifier.ContractError("synthetic invalid contract"),
        ):
            with redirect_stderr(io.StringIO()):
                self.assertEqual(2, self.verifier.main(["--phase", "P0"]))

    def test_origin_main_drift_invalidates_the_p0_anchor(self):
        with mock.patch.object(self.verifier, "run_git", return_value="different-main"):
            with self.assertRaisesRegex(self.verifier.ContractError, "origin/main"):
                self.verifier.validate_origin_main_anchor(REPO_ROOT, "reviewed-base")

    def test_normative_validator_rejects_a_malformed_nested_schema(self):
        original = self.verifier.load_json
        analysis_schema_path = self.verifier.SCHEMA_PATHS[0]

        def load_with_malformed_schema(root, path):
            value = original(root, path)
            if path == analysis_schema_path:
                value = copy.deepcopy(value)
                value["properties"]["deleted_tombstone_count"]["minimum"] = "invalid"
            return value

        with mock.patch.object(self.verifier, "load_json", side_effect=load_with_malformed_schema):
            with self.assertRaisesRegex(self.verifier.ContractError, "invalid JSON Schema"):
                self.verifier.validate_json_schema_documents(REPO_ROOT)

        with tempfile.TemporaryDirectory() as tmp:
            duplicate_json = Path(tmp) / "duplicate.json"
            duplicate_json.write_text('{"schema":"one","schema":"two"}', encoding="utf-8")
            with self.assertRaisesRegex(self.verifier.ContractError, "duplicate JSON key"):
                self.verifier.load_json(Path(tmp), Path("duplicate.json"))

    def test_catalog_sections_enforce_semantic_type(self):
        schema = self.verifier.load_json(
            REPO_ROOT,
            Path(
                "tools/lane-authority-inventory/contracts/authority_surface_catalog.schema.json"
            ),
        )
        catalog = self.p0_catalog()
        catalog["catalog_status"] = "p3_populated_approved"
        catalog["catalog_semantic_digest"] = "a" * 64
        catalog["used_external_symbol_set_digest"] = "b" * 64
        catalog["toolchain_matrix"] = [
            {
                "config_projection_ids": ["cfg:one"],
                "language": "rust",
                "platform": "common",
                "receipt_role": "parser",
                "tool": "rustc",
                "tool_identity_digest": "c" * 64,
            }
        ]
        catalog["external_symbols"] = [
            {
                "authority_relevance": "authority_surface",
                "consumer_ids": [],
                "entry_kind": "local_closure_boundary",
                "id": "catalog:wrong-section",
                "language": "rust",
                "match_mode": "exact_local_boundary",
                "owner": "orchestrator",
                "ownership": "legacy_direct",
                "reason_code": "legacy",
                "removal_checkpoint": "C1B",
                "replacement_kind": "broker",
                "semantic_kinds": ["authority_read"],
                "signature": "wrong::section",
                "used_site_set_digest": "d" * 64,
            }
        ]
        with self.assertRaises(ValidationError):
            Draft202012Validator(schema).validate(catalog)

    def test_cross_relation_totality_and_candidate_equality_are_executable(self):
        digest = "a" * 64
        records = {relation: [] for relation in self.verifier.EXPECTED_RELATIONS}
        records["source_nodes"] = [
            {
                "byte_length": 1,
                "content_digest": digest,
                "language": "rust",
                "parser_receipt_id": "tool:rust",
                "parser_error_count": 0,
                "parser_node_count": 1,
                "parser_node_digest": digest,
                "path": "src/lib.rs",
                "predecessor_source_node_id": None,
                "provenance": "c1i_head",
                "scope": "production",
                "source_node_id": "source:one",
                "status": "current",
                "syntax_site_count": 1,
                "syntax_site_ids_digest": self.verifier.stable_id_set_digest(
                    "decodex/lane-authority-v2-source-syntax-sites/1", {"site:one"}
                ),
                "zero_syntax_reason_code": None,
            },
        ]
        records["syntax_sites"] = [
            {
                "byte_end": 1,
                "byte_start": 0,
                "is_parser_root": True,
                "node_kind": "source_file",
                "recovery_state": "clean",
                "site_id": "site:one",
                "source_node_id": "source:one",
            }
        ]
        records["cfg_projections"] = [
            {
                "cfg_expression_digest": digest,
                "disposition": "active_supported",
                "evidence_digest": digest,
                "language": "rust",
                "platform": "common",
                "projection_id": "cfg:config",
                "projection_kind": "config",
                "site_id": "site:one",
            },
            {
                "cfg_expression_digest": digest,
                "disposition": "active_supported",
                "evidence_digest": digest,
                "language": "rust",
                "platform": "common",
                "projection_id": "cfg:target",
                "projection_kind": "target",
                "site_id": "site:one",
            }
        ]
        records["site_classifications"] = [
            {
                "authority_relevance": "reviewed_non_authority",
                "config_projection": "cfg:config",
                "owner": None,
                "ownership": "not_applicable",
                "reason_code": "data-only",
                "removal_checkpoint": None,
                "replacement_kind": None,
                "runtime_generation": "not_runtime",
                "scope": "production",
                "semantic_kinds": ["data_only"],
                "site_id": "site:one",
                "target_projection": "cfg:target",
            }
        ]
        records["symbol_sites"] = [
            {
                "definition_site_ids": [],
                "external": False,
                "language": "rust",
                "owner_signature": None,
                "receiver_type_evidence": None,
                "receiver_type_signature": None,
                "resolution": "declaration",
                "resolution_hint": "exact",
                "role": "declaration",
                "signature": "run",
                "signature_digest": hashlib.sha256(b"run").hexdigest(),
                "site_id": "symbol:definition",
                "syntax_site_id": "site:one",
            },
            {
                "definition_site_ids": ["symbol:definition"],
                "external": False,
                "language": "rust",
                "owner_signature": None,
                "receiver_type_evidence": None,
                "receiver_type_signature": None,
                "resolution": "local",
                "resolution_hint": "exact",
                "role": "call_target",
                "signature": "run",
                "signature_digest": hashlib.sha256(b"run").hexdigest(),
                "site_id": "symbol:call",
                "syntax_site_id": "site:one",
            },
        ]
        records["site_classifications"].extend(
            {
                **records["site_classifications"][0],
                "site_id": site_id,
            }
            for site_id in ("symbol:definition", "symbol:call")
        )
        candidate = {
            "candidate_category": "legacy_read",
            "candidate_digest": "0" * 64,
            "candidate_id": "0" * 64,
            "c0_observation_ids": ["launcher:one", "legacy:one", "mutation:one"],
            "c0_origin_artifacts": ["launcher", "legacy", "mutation"],
            "line_digest": digest,
            "line_number": 1,
            "provenance": "c0_replay",
            "source_node_id": "source:one",
        }
        candidate["candidate_id"] = self.verifier.canonical_candidate_id(candidate)
        candidate["candidate_digest"] = self.verifier.candidate_record_digest(candidate)
        records["candidate_records"] = [candidate]
        records["candidate_site_edges"] = [
            {"candidate_id": candidate["candidate_id"], "edge_digest": digest, "site_id": "site:one"}
        ]
        records["candidate_adjudications"] = [
            {
                "candidate_category": "legacy_read",
                "candidate_id": candidate["candidate_id"],
                "disposition": "covered_by_sites",
                "evidence_digest": digest,
                "reason_code": "parsed",
                "related_site_ids": ["site:one"],
                "review_receipt_digest": digest,
            }
        ]
        catalog = self.p0_catalog()
        catalog["toolchain_matrix"] = []
        records["toolchain_receipts"] = []
        for language in sorted(self.verifier.EXPECTED_LANGUAGES):
            source_id = f"source:tool:{language}"
            site_id = f"site:tool:{language}"
            config_id = f"cfg:tool:{language}:config"
            target_id = f"cfg:tool:{language}:target"
            records["source_nodes"].append(
                {
                    "byte_length": 1,
                    "content_digest": digest,
                    "language": language,
                    "parser_receipt_id": f"tool:{language}",
                    "parser_error_count": 0,
                    "parser_node_count": 1,
                    "parser_node_digest": digest,
                    "path": f"tools/scanner.{language}",
                    "predecessor_source_node_id": None,
                    "provenance": "tool",
                    "scope": "tool",
                    "source_node_id": source_id,
                    "status": "current",
                    "syntax_site_count": 1,
                    "syntax_site_ids_digest": self.verifier.stable_id_set_digest(
                        "decodex/lane-authority-v2-source-syntax-sites/1", {site_id}
                    ),
                    "zero_syntax_reason_code": None,
                }
            )
            records["syntax_sites"].append(
                {
                    "byte_end": 1,
                    "byte_start": 0,
                    "is_parser_root": True,
                    "node_kind": self.verifier.PARSER_ROOT_KINDS[language],
                    "recovery_state": "clean",
                    "site_id": site_id,
                    "source_node_id": source_id,
                }
            )
            for projection_id, projection_kind in (
                (config_id, "config"),
                (target_id, "target"),
            ):
                records["cfg_projections"].append(
                    {
                        "cfg_expression_digest": digest,
                        "disposition": "active_supported",
                        "evidence_digest": digest,
                        "language": language,
                        "platform": "common",
                        "projection_id": projection_id,
                        "projection_kind": projection_kind,
                        "site_id": site_id,
                    }
                )
            records["site_classifications"].append(
                {
                    "authority_relevance": "reviewed_non_authority",
                    "config_projection": config_id,
                    "owner": None,
                    "ownership": "not_applicable",
                    "reason_code": "tool-source",
                    "removal_checkpoint": None,
                    "replacement_kind": None,
                    "runtime_generation": "not_runtime",
                    "scope": "tool",
                    "semantic_kinds": ["data_only"],
                    "site_id": site_id,
                    "target_projection": target_id,
                }
            )
            entry = {
                "config_projection_ids": [config_id],
                "language": language,
                "platform": "common",
                "receipt_role": "parser",
                "tool": f"{language}-analyzer",
                "tool_identity_digest": digest,
            }
            catalog["toolchain_matrix"].append(entry)
            records["toolchain_receipts"].append(
                {
                    **entry,
                    "completed": True,
                    "receipt_id": f"tool:{language}",
                }
            )
        for platform in ("linux", "macos"):
            projection_id = f"cfg:rust:{platform}"
            records["cfg_projections"].append(
                {
                    "cfg_expression_digest": digest,
                    "disposition": "active_supported",
                    "evidence_digest": digest,
                    "language": "rust",
                    "platform": platform,
                    "projection_id": projection_id,
                    "projection_kind": "config",
                    "site_id": "site:one",
                }
            )
            entry = {
                "config_projection_ids": [projection_id],
                "language": "rust",
                "platform": platform,
                "receipt_role": "platform_slice",
                "tool": "rust-analyzer",
                "tool_identity_digest": digest,
            }
            catalog["toolchain_matrix"].append(entry)
            records["toolchain_receipts"].append(
                {
                    **entry,
                    "completed": True,
                    "receipt_id": f"tool:rust:{platform}",
                }
            )
        records["call_edges"] = [
            {
                "edge_id": "call:one",
                "from_site_id": "symbol:call",
                "to_site_id": "symbol:definition",
            }
        ]
        records["dataflow_edges"] = [
            {"edge_id": "flow:one", "from_site_id": "site:one", "to_site_id": "site:one"}
        ]
        expected_candidate_observations = {
            f"{origin}:one": {
                "candidate_digest": self.verifier.c0_candidate_digest([(1, digest)]),
                "candidate_line_count": 1,
                "category": "legacy_read",
                "first_line": 1,
                "origin": origin,
                "path": "src/lib.rs",
            }
            for origin in ("launcher", "legacy", "mutation")
        }
        catalog["catalog_status"] = "p3_populated_approved"
        catalog["used_external_symbol_set_digest"] = self.verifier.stable_id_set_digest(
            "decodex/lane-authority-v2-used-external-symbol-sites/1", set()
        )
        catalog["catalog_semantic_digest"] = self.verifier.catalog_semantic_digest(catalog)
        syntax_by_source = {
            source["source_node_id"]: {
                site["site_id"]
                for site in records["syntax_sites"]
                if site["source_node_id"] == source["source_node_id"]
            }
            for source in records["source_nodes"]
        }
        for receipt in records["toolchain_receipts"]:
            if receipt["receipt_role"] == "parser":
                source_ids = {
                    source["source_node_id"]
                    for source in records["source_nodes"]
                    if source["parser_receipt_id"] == receipt["receipt_id"]
                }
            else:
                projection_sites = {
                    projection["site_id"]
                    for projection in records["cfg_projections"]
                    if projection["projection_id"] in receipt["config_projection_ids"]
                }
                source_ids = {
                    site["source_node_id"]
                    for site in records["syntax_sites"]
                    if site["site_id"] in projection_sites
                }
            receipt["completed_source_node_ids"] = sorted(source_ids)
            syntax_ids = set().union(*(syntax_by_source[source_id] for source_id in source_ids))
            candidate_ids = {
                candidate["candidate_id"]
                for candidate in records["candidate_records"]
                if candidate["source_node_id"] in source_ids
            }
            call_ids = {
                edge["edge_id"] for edge in records["call_edges"]
                if edge["from_site_id"] in syntax_ids
                or any(
                    symbol["site_id"] == edge["from_site_id"]
                    and symbol["syntax_site_id"] in syntax_ids
                    for symbol in records["symbol_sites"]
                )
            }
            dataflow_ids = {
                edge["edge_id"] for edge in records["dataflow_edges"]
                if edge["from_site_id"] in syntax_ids
            }
            for prefix, identifiers in {
                "expected_source_node": source_ids,
                "completed_source_node": source_ids,
                "syntax_site": syntax_ids,
                "candidate_record": candidate_ids,
                "call_edge": call_ids,
                "dataflow_edge": dataflow_ids,
            }.items():
                receipt[f"{prefix}_count"] = len(identifiers)
                receipt[f"{prefix}_ids_digest"] = self.verifier.stable_id_set_digest(
                    f"decodex/lane-authority-v2-tool-receipt-{prefix}/1", identifiers
                )
            receipt["rejection_reason_codes"] = []
            receipt["unresolved_count"] = 0
        expected_source_partitions = {"analysis": 1, "deleted_tombstone": 0, "tool": 6}
        expected_source_partition_digests = self.verifier.source_partition_digests(
            records["source_nodes"]
        )
        self.verifier.validate_cross_relation_records(
            records,
            expected_source_partitions=expected_source_partitions,
            expected_source_partition_digests=expected_source_partition_digests,
            expected_candidate_observations=expected_candidate_observations,
            catalog=catalog,
        )

        wrong_observation_digest = copy.deepcopy(expected_candidate_observations)
        wrong_observation_digest["launcher:one"]["candidate_digest"] = "b" * 64
        with self.assertRaisesRegex(self.verifier.ContractError, "observation digest"):
            self.verifier.validate_cross_relation_records(
                records,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=wrong_observation_digest,
                catalog=catalog,
            )

        cloned_candidate = copy.deepcopy(records)
        clone = copy.deepcopy(cloned_candidate["candidate_records"][0])
        clone["candidate_id"] = "f" * 64
        clone["candidate_digest"] = self.verifier.candidate_record_digest(clone)
        cloned_candidate["candidate_records"].append(clone)
        with self.assertRaisesRegex(self.verifier.ContractError, "canonical"):
            self.verifier.validate_cross_relation_records(
                cloned_candidate,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        unresolved = copy.deepcopy(records)
        unresolved["site_classifications"][0]["reason_code"] = "unresolved-target"
        with self.assertRaisesRegex(self.verifier.ContractError, "unresolved reason code"):
            self.verifier.validate_cross_relation_records(
                unresolved,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        incomplete_receipt = copy.deepcopy(records)
        incomplete_receipt["toolchain_receipts"][0]["syntax_site_count"] = 0
        with self.assertRaisesRegex(self.verifier.ContractError, "syntax_site count"):
            self.verifier.validate_cross_relation_records(
                incomplete_receipt,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        hidden_source_syntax = copy.deepcopy(records)
        hidden_source_syntax["source_nodes"][0]["syntax_site_count"] = 0
        with self.assertRaisesRegex(self.verifier.ContractError, "syntax site count"):
            self.verifier.validate_cross_relation_records(
                hidden_source_syntax,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=self.verifier.source_partition_digests(
                    hidden_source_syntax["source_nodes"]
                ),
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        wrong_scope = copy.deepcopy(records)
        wrong_scope["site_classifications"][0]["scope"] = "test"
        with self.assertRaisesRegex(self.verifier.ContractError, "scope"):
            self.verifier.validate_cross_relation_records(
                wrong_scope,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        missing_parser_root = copy.deepcopy(records)
        missing_parser_root["syntax_sites"][0]["is_parser_root"] = False
        with self.assertRaisesRegex(self.verifier.ContractError, "parser root"):
            self.verifier.validate_cross_relation_records(
                missing_parser_root,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        partial_parser_root = copy.deepcopy(records)
        partial_parser_root["syntax_sites"][0]["byte_end"] = 0
        with self.assertRaisesRegex(self.verifier.ContractError, "full-byte parser root"):
            self.verifier.validate_cross_relation_records(
                partial_parser_root,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        parser_recovery = copy.deepcopy(records)
        parser_recovery["syntax_sites"][0]["recovery_state"] = "error"
        with self.assertRaisesRegex(self.verifier.ContractError, "ERROR or MISSING"):
            self.verifier.validate_cross_relation_records(
                parser_recovery,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        wrong_projection_language = copy.deepcopy(records)
        wrong_projection_language["cfg_projections"][0]["language"] = "python"
        with self.assertRaisesRegex(self.verifier.ContractError, "projection language"):
            self.verifier.validate_cross_relation_records(
                wrong_projection_language,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        relabeled_source = copy.deepcopy(records)
        relabeled_source["source_nodes"][1]["provenance"] = "c1i_head"
        relabeled_source["source_nodes"][1]["scope"] = "production"
        with self.assertRaisesRegex(self.verifier.ContractError, "partitions"):
            self.verifier.validate_cross_relation_records(
                relabeled_source,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        for field, value in (("scope", "test"), ("provenance", "post_c0_base")):
            with self.subTest(source_relabel=field):
                relabeled_within_analysis = copy.deepcopy(records)
                relabeled_within_analysis["source_nodes"][0][field] = value
                with self.assertRaisesRegex(self.verifier.ContractError, "partition digests"):
                    self.verifier.validate_cross_relation_records(
                        relabeled_within_analysis,
                        expected_source_partitions=expected_source_partitions,
                        expected_source_partition_digests=expected_source_partition_digests,
                        expected_candidate_observations=expected_candidate_observations,
                        catalog=catalog,
                    )

        wrong_projection_kind = copy.deepcopy(records)
        wrong_projection_kind["site_classifications"][0]["target_projection"] = "cfg:config"
        with self.assertRaisesRegex(self.verifier.ContractError, "another site or kind"):
            self.verifier.validate_cross_relation_records(
                wrong_projection_kind,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        wrong_projection_platform = copy.deepcopy(records)
        wrong_projection_platform["cfg_projections"][1]["platform"] = "linux"
        with self.assertRaisesRegex(self.verifier.ContractError, "platforms disagree"):
            self.verifier.validate_cross_relation_records(
                wrong_projection_platform,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        missing_cfg = copy.deepcopy(records)
        missing_cfg["cfg_projections"] = []
        with self.assertRaisesRegex(self.verifier.ContractError, "cfg projection"):
            self.verifier.validate_cross_relation_records(
                missing_cfg,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        wrong_category = copy.deepcopy(records)
        wrong_category["candidate_adjudications"][0]["candidate_category"] = "mutation"
        with self.assertRaisesRegex(self.verifier.ContractError, "category"):
            self.verifier.validate_cross_relation_records(
                wrong_category,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        missing_toolchain = copy.deepcopy(records)
        missing_toolchain["toolchain_receipts"].pop()
        with self.assertRaisesRegex(self.verifier.ContractError, "toolchain"):
            self.verifier.validate_cross_relation_records(
                missing_toolchain,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        empty_macos_slice = copy.deepcopy(records)
        empty_macos_slice["cfg_projections"] = [
            projection
            for projection in empty_macos_slice["cfg_projections"]
            if projection["platform"] != "macos"
        ]
        empty_macos_slice["toolchain_receipts"] = [
            receipt
            for receipt in empty_macos_slice["toolchain_receipts"]
            if receipt["platform"] != "macos"
        ]
        empty_macos_catalog = copy.deepcopy(catalog)
        empty_macos_catalog["toolchain_matrix"] = [
            entry
            for entry in empty_macos_catalog["toolchain_matrix"]
            if entry["platform"] != "macos"
        ]
        empty_macos_catalog["catalog_semantic_digest"] = self.verifier.catalog_semantic_digest(
            empty_macos_catalog
        )
        with self.assertRaisesRegex(self.verifier.ContractError, "linux and macos"):
            self.verifier.validate_cross_relation_records(
                empty_macos_slice,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=empty_macos_catalog,
            )

        unbound_supporting = copy.deepcopy(records)
        unbound_supporting["supporting_inputs"] = [
            {
                "authority_capability": "authority_capable",
                "config_projection_ids": ["cfg:config"],
                "consumer_site_ids": ["site:one"],
                "content_digest": digest,
                "input_id": "input:one",
                "materialized_source_node_id": "source:missing",
                "path": "generated/authority.json",
                "producer": "rust-analyzer",
                "producer_receipt_id": "tool:rust",
                "scope": "generated",
            }
        ]
        with self.assertRaisesRegex(self.verifier.ContractError, "current materialized source"):
            self.verifier.validate_cross_relation_records(
                unbound_supporting,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        external_signature = "std::env::var"
        missing_catalog_entry = copy.deepcopy(records)
        missing_catalog_entry["symbol_sites"].append(
            {
                "definition_site_ids": [],
                "external": True,
                "language": "rust",
                "owner_signature": None,
                "receiver_type_evidence": None,
                "receiver_type_signature": None,
                "resolution": "external",
                "resolution_hint": "qualified",
                "role": "call_target",
                "signature": external_signature,
                "signature_digest": hashlib.sha256(
                    external_signature.encode("utf-8")
                ).hexdigest(),
                "site_id": "symbol:one",
                "syntax_site_id": "site:one",
            }
        )
        missing_catalog_entry["site_classifications"].append(
            {
                **missing_catalog_entry["site_classifications"][0],
                "site_id": "symbol:one",
            }
        )
        missing_catalog_entry["catalog_entry_dispositions"] = [
            {
                "catalog_entry_id": "catalog:missing",
                "disposition": "matched_site",
                "disposition_id": "disposition:one",
                "evidence_digest": digest,
                "reason_code": "resolved",
                "site_id": "symbol:one",
            }
        ]
        with self.assertRaisesRegex(self.verifier.ContractError, "catalog entry"):
            self.verifier.validate_cross_relation_records(
                missing_catalog_entry,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

        wrong_section_catalog = copy.deepcopy(catalog)
        wrong_section_catalog["persistent_data_roots"] = [
            {
                "consumer_ids": ["symbol:one"],
                "entry_kind": "persistent_data_root",
                "id": "catalog:wrong-section",
                "language": "rust",
                "used_site_set_digest": self.verifier.stable_id_set_digest(
                    "decodex/lane-authority-v2-catalog-entry-used-sites/1", {"symbol:one"}
                ),
            }
        ]
        wrong_section_disposition = copy.deepcopy(missing_catalog_entry)
        wrong_section_disposition["catalog_entry_dispositions"][0][
            "catalog_entry_id"
        ] = "catalog:wrong-section"
        with self.assertRaisesRegex(self.verifier.ContractError, "site kind"):
            self.verifier.validate_cross_relation_records(
                wrong_section_disposition,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=wrong_section_catalog,
            )

        matched_external = copy.deepcopy(missing_catalog_entry)
        matched_catalog = copy.deepcopy(catalog)
        matched_catalog["external_symbols"] = [
            {
                "consumer_ids": ["symbol:one"],
                "entry_kind": "external_symbol",
                "id": "catalog:external",
                "language": "rust",
                "signature": external_signature,
                "used_site_set_digest": self.verifier.stable_id_set_digest(
                    "decodex/lane-authority-v2-catalog-entry-used-sites/1",
                    {"symbol:one"},
                ),
            }
        ]
        matched_catalog["used_external_symbol_set_digest"] = self.verifier.stable_id_set_digest(
            "decodex/lane-authority-v2-used-external-symbol-sites/1", {"symbol:one"}
        )
        matched_catalog["catalog_semantic_digest"] = self.verifier.catalog_semantic_digest(
            matched_catalog
        )
        matched_external["catalog_entry_dispositions"][0][
            "catalog_entry_id"
        ] = "catalog:external"
        self.verifier.validate_cross_relation_records(
            matched_external,
            expected_source_partitions=expected_source_partitions,
            expected_source_partition_digests=expected_source_partition_digests,
            expected_candidate_observations=expected_candidate_observations,
            catalog=matched_catalog,
        )

        wrong_language_catalog = copy.deepcopy(matched_catalog)
        wrong_language_catalog["external_symbols"][0]["language"] = "python"
        with self.assertRaisesRegex(self.verifier.ContractError, "language"):
            self.verifier.validate_cross_relation_records(
                matched_external,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=wrong_language_catalog,
            )

        duplicate_catalog_id = copy.deepcopy(catalog)
        duplicate_catalog_id["dynamic_capability_roots"] = [{"id": "catalog:duplicate"}]
        duplicate_catalog_id["persistent_data_roots"] = [{"id": "catalog:duplicate"}]
        with self.assertRaisesRegex(self.verifier.ContractError, "duplicate id"):
            self.verifier.validate_cross_relation_records(
                records,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=duplicate_catalog_id,
            )

        duplicate_toolchain = copy.deepcopy(records)
        duplicate_receipt = copy.deepcopy(duplicate_toolchain["toolchain_receipts"][0])
        duplicate_receipt["receipt_id"] = "tool:duplicate"
        duplicate_toolchain["toolchain_receipts"].append(duplicate_receipt)
        with self.assertRaisesRegex(self.verifier.ContractError, "duplicate semantic"):
            self.verifier.validate_cross_relation_records(
                duplicate_toolchain,
                expected_source_partitions=expected_source_partitions,
                expected_source_partition_digests=expected_source_partition_digests,
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

    def test_relation_schema_rejects_covered_candidate_without_sites(self):
        schema = self.verifier.load_json(
            REPO_ROOT,
            Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json"),
        )
        manifest = {
            "schema": "decodex/lane-authority-v2-c1i-candidate-adjudications/1",
            "relation": "candidate_adjudications",
            "records": [
                {
                    "candidate_category": "legacy_read",
                    "candidate_id": "candidate:one",
                    "disposition": "covered_by_sites",
                    "evidence_digest": "a" * 64,
                    "reason_code": "parsed",
                    "related_site_ids": [],
                    "review_receipt_digest": "b" * 64,
                }
            ],
        }
        with self.assertRaises(ValidationError):
            Draft202012Validator(schema).validate(manifest)

    def test_symbol_relation_represents_unresolved_without_false_external_claim(self):
        schema = self.verifier.load_json(
            REPO_ROOT,
            Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json"),
        )
        symbol = {
            "definition_site_ids": [],
            "external": None,
            "language": "rust",
            "owner_signature": None,
            "receiver_type_evidence": None,
            "receiver_type_signature": None,
            "resolution": "unresolved",
            "resolution_hint": "dynamic",
            "role": "call_target",
            "signature": "<dynamic:call_expression>",
            "signature_digest": "a" * 64,
            "site_id": "symbol:one",
            "syntax_site_id": "syntax:one",
        }
        manifest = {
            "records": [symbol],
            "relation": "symbol_sites",
            "schema": "decodex/lane-authority-v2-c1i-symbol-sites/1",
        }
        Draft202012Validator(schema).validate(manifest)

        invalid_local = copy.deepcopy(manifest)
        invalid_local["records"][0]["external"] = False
        invalid_local["records"][0]["resolution"] = "local"
        with self.assertRaises(ValidationError):
            Draft202012Validator(schema).validate(invalid_local)

    def test_cfg_projection_relation_schema_is_satisfiable(self):
        schema = self.verifier.load_json(
            REPO_ROOT,
            Path("tools/lane-authority-inventory/contracts/relation_manifest.schema.json"),
        )
        manifest = {
            "schema": "decodex/lane-authority-v2-c1i-cfg-projections/1",
            "relation": "cfg_projections",
            "records": [
                {
                    "cfg_expression_digest": "a" * 64,
                    "disposition": "active_supported",
                    "evidence_digest": "b" * 64,
                    "language": "rust",
                    "platform": "common",
                    "projection_id": "cfg:one",
                    "projection_kind": "config",
                    "site_id": "site:one",
                }
            ],
        }
        Draft202012Validator(schema).validate(manifest)

    def test_cfg_and_dataflow_proof_schemas_have_concrete_instances(self):
        digest = "a" * 64
        cfg_schema = self.verifier.load_json(
            REPO_ROOT,
            Path("tools/lane-authority-inventory/contracts/cfg_coverage.schema.json"),
        )
        cfg = {
            "analysis_cut_digest": digest,
            "cfg_coverage_digest": "0" * 64,
            "cfg_relation_digest": digest,
            "covered_syntax_site_count": 1,
            "covered_syntax_site_ids_digest": digest,
            "platform_slice_digests": {"common": digest, "linux": digest, "macos": digest},
            "schema": "decodex/lane-authority-v2-c1i-cfg-coverage/1",
            "syntax_site_count": 1,
            "syntax_site_ids_digest": digest,
            "unresolved_count": 0,
        }
        cfg["cfg_coverage_digest"] = self.verifier.self_bound_artifact_digest(
            cfg, "cfg_coverage_digest", "decodex/lane-authority-v2-cfg-coverage/1"
        )
        Draft202012Validator(cfg_schema).validate(cfg)

        proof_schema = self.verifier.load_json(
            REPO_ROOT,
            Path("tools/lane-authority-inventory/contracts/dataflow_proofs.schema.json"),
        )
        dataflow = {
            "analysis_cut_digest": digest,
            "call_relation_digest": digest,
            "dataflow_contract_digest": digest,
            "dataflow_proofs_digest": "0" * 64,
            "dataflow_relation_digest": digest,
            "fixed_point_digest": digest,
            "proofs": [
                {
                    "call_edge_ids": ["call:one"],
                    "catalog_entry_ids": ["catalog:one"],
                    "config_projection_ids": ["cfg:one"],
                    "dataflow_edge_ids": ["flow:one"],
                    "fixed_point_digest": digest,
                    "proof_id": "proof:one",
                    "result_value": {"kind": "Constant", "value_digest": digest},
                    "sink_site_id": "site:one",
                    "source_site_ids": ["site:source"],
                    "tool_receipt_ids": ["tool:one"],
                    "transfer_rule_ids": ["constant_construct"],
                }
            ],
            "schema": "decodex/lane-authority-v2-c1i-dataflow-proofs/1",
            "sink_count": 1,
            "top_reaching_sink_count": 0,
            "unresolved_count": 0,
        }
        dataflow["dataflow_proofs_digest"] = self.verifier.self_bound_artifact_digest(
            dataflow,
            "dataflow_proofs_digest",
            "decodex/lane-authority-v2-dataflow-proofs/1",
        )
        Draft202012Validator(proof_schema).validate(dataflow)

    def test_dataflow_proof_requires_a_closed_path_and_bound_fixed_point(self):
        proof = {
            "call_edge_ids": ["call:path"],
            "catalog_entry_ids": ["catalog:one"],
            "config_projection_ids": ["cfg:source"],
            "dataflow_edge_ids": [],
            "fixed_point_digest": "0" * 64,
            "proof_id": "proof:one",
            "result_value": {"kind": "Constant", "value_digest": "a" * 64},
            "sink_site_id": "site:sink",
            "source_site_ids": ["site:source"],
            "tool_receipt_ids": ["tool:one"],
            "transfer_rule_ids": ["constant_construct"],
        }
        proof["fixed_point_digest"] = self.verifier.dataflow_fixed_point_digest(proof)
        call_edges = {
            "call:path": {
                "edge_id": "call:path",
                "from_site_id": "site:source",
                "to_site_id": "site:sink",
            }
        }
        kwargs = {
            "call_edges": call_edges,
            "dataflow_edges": {},
            "site_ids": {"site:source", "site:sink", "site:other", "site:dead"},
            "sink_ids": {"site:sink"},
            "syntax_ids": {"site:source", "site:sink", "site:other", "site:dead"},
            "syntax_sources": {
                "site:source": "source:one",
                "site:sink": "source:one",
                "site:other": "source:other",
                "site:dead": "source:other",
            },
            "derived_syntax": {},
            "projections": {
                "cfg:source": {
                    "projection_kind": "config",
                    "site_id": "site:source",
                }
            },
        }
        self.verifier.validate_dataflow_proof_path(proof, **kwargs)

        disconnected = copy.deepcopy(proof)
        disconnected["call_edge_ids"].append("call:dead")
        disconnected["fixed_point_digest"] = self.verifier.dataflow_fixed_point_digest(
            disconnected
        )
        disconnected_kwargs = copy.deepcopy(kwargs)
        disconnected_kwargs["call_edges"]["call:dead"] = {
            "edge_id": "call:dead",
            "from_site_id": "site:other",
            "to_site_id": "site:dead",
        }
        with self.assertRaisesRegex(self.verifier.ContractError, "outside every"):
            self.verifier.validate_dataflow_proof_path(disconnected, **disconnected_kwargs)

        tampered = copy.deepcopy(proof)
        tampered["fixed_point_digest"] = "f" * 64
        with self.assertRaisesRegex(self.verifier.ContractError, "fixed-point digest"):
            self.verifier.validate_dataflow_proof_path(tampered, **kwargs)

    def test_empty_relation_universe_cannot_pass_vacuously(self):
        records = {relation: [] for relation in self.verifier.EXPECTED_RELATIONS}
        catalog = self.p0_catalog()
        expected_candidate_observations = {}
        with self.assertRaisesRegex(self.verifier.ContractError, "analysis cut"):
            self.verifier.validate_cross_relation_records(
                records,
                expected_source_partitions={
                    "analysis": 3363,
                    "deleted_tombstone": 0,
                    "tool": 1,
                },
                expected_source_partition_digests=self.verifier.source_partition_digests([]),
                expected_candidate_observations=expected_candidate_observations,
                catalog=catalog,
            )

    def test_output_policy_rejects_semantic_downgrade(self):
        original = self.verifier.load_json

        def load_with_downgrade(root, path):
            value = original(root, path)
            if path == self.verifier.OUTPUT_POLICY_PATH:
                value = copy.deepcopy(value)
                cfg = next(
                    artifact
                    for artifact in value["artifacts"]
                    if artifact["role"] == "cfg_coverage"
                )
                cfg["accepted_authority"] = False
                cfg["binding"] = "diagnostic_only"
            return value

        with mock.patch.object(self.verifier, "load_json", side_effect=load_with_downgrade):
            with self.assertRaisesRegex(self.verifier.ContractError, "closure drifted"):
                self.verifier.validate_output_policy(REPO_ROOT)

    def test_review_preimage_excludes_only_the_receipt(self):
        base = "51f553fd32c8f75eed925afe87f99931844fffec"
        reviewed_path = "tools/lane-authority-inventory/README.md"
        with mock.patch.object(
            self.verifier, "changed_paths", return_value=[reviewed_path]
        ):
            without_receipt = self.verifier.review_scope_digest(REPO_ROOT, base)
        with mock.patch.object(
            self.verifier,
            "changed_paths",
            return_value=[reviewed_path, str(self.verifier.REVIEW_RECEIPT_PATH)],
        ):
            with_receipt = self.verifier.review_scope_digest(REPO_ROOT, base)
        with mock.patch.object(
            self.verifier, "changed_paths", return_value=[reviewed_path]
        ):
            different_base = self.verifier.review_scope_digest(
                REPO_ROOT, "d57553bc1bcdceebe1d0c7ec5ad5dc492b695348"
            )
        self.assertEqual(without_receipt, with_receipt)
        self.assertNotEqual(without_receipt, different_base)


if __name__ == "__main__":
    unittest.main()
