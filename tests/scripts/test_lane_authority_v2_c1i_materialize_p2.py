from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_ROOT = REPO_ROOT / "tools/lane-authority-inventory"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


class LaneAuthorityV2C1IMaterializeP2Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        load_module("verify_contract", TOOL_ROOT / "verify_contract.py")
        cls.materializer = load_module(
            "lane_authority_v2_c1i_materialize_p2", TOOL_ROOT / "materialize_p2.py"
        )

    def test_clean_swift_sources_do_not_require_native_parser(self):
        parsed = {
            "source_nodes": [
                {
                    "language": "swift",
                    "parser_error_count": 0,
                    "source_node_id": "source:swift",
                }
            ]
        }

        with mock.patch.object(subprocess, "run") as run:
            result = self.materializer.resolve_swift_recovery(Path("/source"), parsed)

        self.assertEqual((None, set()), result)
        run.assert_not_called()

    def test_cargo_targets_are_normalized_to_exact_cut_source_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "apps/example/Cargo.toml"
            source = root / "apps/example/src/lib.rs"
            metadata = {
                "packages": [
                    {
                        "dependencies": [
                            {"name": "serde-json", "rename": "json"},
                            {"name": "time", "rename": None},
                        ],
                        "manifest_path": str(manifest),
                        "name": "example",
                        "version": "1.2.3",
                        "targets": [
                            {
                                "crate_types": ["lib", "lib"],
                                "edition": "2024",
                                "kind": ["lib", "lib"],
                                "name": "example",
                                "src_path": str(source),
                            }
                        ],
                    }
                ]
            }
            sources = {
                "apps/example/Cargo.toml": {
                    "language": "toml",
                    "source_node_id": "source:manifest",
                },
                "apps/example/src/lib.rs": {
                    "language": "rust",
                    "source_node_id": "source:lib",
                },
            }

            targets = self.materializer.normalize_cargo_metadata_targets(
                root, metadata, sources
            )

        self.assertEqual(1, len(targets))
        self.assertEqual(["lib"], targets[0]["crate_types"])
        self.assertEqual(["lib"], targets[0]["target_kinds"])
        self.assertEqual(
            ["alloc", "core", "json", "proc_macro", "std", "time"],
            targets[0]["extern_crate_names"],
        )
        self.assertEqual("apps/example/Cargo.toml", targets[0]["manifest_path"])
        self.assertEqual("apps/example/src/lib.rs", targets[0]["target_root_path"])
        self.assertEqual("source:lib", targets[0]["target_root_source_node_id"])
        self.assertEqual(64, len(targets[0]["crate_target_id"]))

    def test_cargo_target_outside_exact_cut_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            metadata = {
                "packages": [
                    {
                        "manifest_path": "/outside/Cargo.toml",
                        "name": "escape",
                        "version": "1.0.0",
                        "targets": [],
                    }
                ]
            }
            with self.assertRaisesRegex(
                Exception, "cargo metadata path escapes the exact source cut"
            ):
                self.materializer.normalize_cargo_metadata_targets(root, metadata, {})

    def test_cargo_target_root_must_be_an_inventory_rust_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            metadata = {
                "packages": [
                    {
                        "manifest_path": str(root / "Cargo.toml"),
                        "name": "missing",
                        "version": "1.0.0",
                        "targets": [
                            {
                                "crate_types": ["bin"],
                                "edition": "2024",
                                "kind": ["bin"],
                                "name": "missing",
                                "src_path": str(root / "src/main.rs"),
                            }
                        ],
                    }
                ]
            }
            sources = {
                "Cargo.toml": {
                    "language": "toml",
                    "source_node_id": "source:manifest",
                }
            }
            with self.assertRaisesRegex(
                Exception, "cargo target root is not an exact-cut Rust source"
            ):
                self.materializer.normalize_cargo_metadata_targets(
                    root, metadata, sources
                )

    def test_rust_module_scopes_follow_unique_target_qualified_mod_edges(self):
        parsed = {
            "source_nodes": [
                {
                    "language": "rust",
                    "path": "src/lib.rs",
                    "source_node_id": "source:lib",
                    "status": "current",
                },
                {
                    "language": "rust",
                    "path": "src/child.rs",
                    "source_node_id": "source:child",
                    "status": "current",
                },
            ],
            "rust_module_declaration_facts": [
                {
                    "body_scope_syntax_site_id": None,
                    "declaration_syntax_site_id": "syntax:mod-child",
                    "lexical_scope_syntax_site_id": "syntax:lib-root",
                    "module_name": "child",
                    "source_node_id": "source:lib",
                }
            ],
            "rust_scope_facts": [
                {
                    "byte_end": 20,
                    "byte_start": 0,
                    "parent_scope_syntax_site_id": None,
                    "scope_kind": "source_file",
                    "source_node_id": "source:lib",
                    "syntax_site_id": "syntax:lib-root",
                },
                {
                    "byte_end": 19,
                    "byte_start": 10,
                    "parent_scope_syntax_site_id": "syntax:lib-root",
                    "scope_kind": "block",
                    "source_node_id": "source:lib",
                    "syntax_site_id": "syntax:block",
                },
                {
                    "byte_end": 11,
                    "byte_start": 0,
                    "parent_scope_syntax_site_id": None,
                    "scope_kind": "source_file",
                    "source_node_id": "source:child",
                    "syntax_site_id": "syntax:child-root",
                },
            ],
            "syntax_sites": [
                {"site_id": "syntax:lib-root", "source_node_id": "source:lib"},
                {"site_id": "syntax:block", "source_node_id": "source:lib"},
                {"site_id": "syntax:mod-child", "source_node_id": "source:lib"},
                {"site_id": "syntax:child-root", "source_node_id": "source:child"},
            ],
        }
        target = {
            "crate_target_id": "target:one",
            "extern_crate_names": ["alloc", "core", "proc_macro", "std"],
            "manifest_path": "Cargo.toml",
            "target_kinds": ["lib"],
            "target_name": "example",
            "target_root_path": "src/lib.rs",
            "target_root_source_node_id": "source:lib",
        }
        projections = [
            {"projection_id": "projection:lib", "site_id": "syntax:lib-root"},
            {
                "projection_id": "projection:child",
                "site_id": "syntax:child-root",
            },
        ]

        scopes = self.materializer.materialize_rust_module_scopes(
            parsed, [target], projections
        )

        self.assertEqual(3, len(scopes))
        root = next(scope for scope in scopes if scope["scope_kind"] == "crate_root")
        block = next(scope for scope in scopes if scope["scope_kind"] == "block")
        child = next(scope for scope in scopes if scope["scope_kind"] == "file_module")
        self.assertEqual(root["scope_id"], block["parent_scope_id"])
        self.assertEqual(root["canonical_module_path"], block["canonical_module_path"])
        self.assertEqual(root["scope_id"], child["parent_scope_id"])
        self.assertEqual(
            f"{root['canonical_module_path']}::child",
            child["canonical_module_path"],
        )
        self.assertEqual("syntax:mod-child", child["declaration_syntax_site_id"])

    def test_ambiguous_external_rust_module_files_are_not_guessed(self):
        parsed = {
            "source_nodes": [
                {
                    "language": "rust",
                    "path": path,
                    "source_node_id": source_id,
                    "status": "current",
                }
                for path, source_id in (
                    ("src/lib.rs", "source:lib"),
                    ("src/child.rs", "source:flat"),
                    ("src/child/mod.rs", "source:nested"),
                )
            ],
            "rust_module_declaration_facts": [
                {
                    "body_scope_syntax_site_id": None,
                    "declaration_syntax_site_id": "syntax:mod-child",
                    "lexical_scope_syntax_site_id": "syntax:lib-root",
                    "module_name": "child",
                    "source_node_id": "source:lib",
                }
            ],
            "rust_scope_facts": [
                {
                    "byte_end": 1,
                    "byte_start": 0,
                    "parent_scope_syntax_site_id": None,
                    "scope_kind": "source_file",
                    "source_node_id": source_id,
                    "syntax_site_id": syntax_id,
                }
                for source_id, syntax_id in (
                    ("source:lib", "syntax:lib-root"),
                    ("source:flat", "syntax:flat-root"),
                    ("source:nested", "syntax:nested-root"),
                )
            ],
            "syntax_sites": [
                {"site_id": "syntax:lib-root", "source_node_id": "source:lib"},
                {"site_id": "syntax:mod-child", "source_node_id": "source:lib"},
                {"site_id": "syntax:flat-root", "source_node_id": "source:flat"},
                {"site_id": "syntax:nested-root", "source_node_id": "source:nested"},
            ],
        }
        target = {
            "crate_target_id": "target:one",
            "extern_crate_names": ["alloc", "core", "proc_macro", "std"],
            "manifest_path": "Cargo.toml",
            "target_kinds": ["lib"],
            "target_name": "example",
            "target_root_path": "src/lib.rs",
            "target_root_source_node_id": "source:lib",
        }
        projections = [
            {"projection_id": "projection:lib", "site_id": "syntax:lib-root"},
            {"projection_id": "projection:flat", "site_id": "syntax:flat-root"},
            {"projection_id": "projection:nested", "site_id": "syntax:nested-root"},
        ]

        scopes = self.materializer.materialize_rust_module_scopes(
            parsed, [target], projections
        )

        self.assertEqual(["crate_root"], [scope["scope_kind"] for scope in scopes])

    def test_rust_name_bindings_resolve_only_exact_declarations(self):
        parsed = {
            "rust_name_binding_facts": [
                {
                    "binding_kind": "module",
                    "lexical_scope_syntax_site_id": "syntax:root",
                    "local_name": "state",
                    "source_node_id": "source:lib",
                    "surface_target_path": None,
                    "syntax_site_id": "syntax:module",
                    "visibility": "private",
                    "visibility_path": None,
                },
                {
                    "binding_kind": "type_declaration",
                    "lexical_scope_syntax_site_id": "syntax:root",
                    "local_name": "Store",
                    "source_node_id": "source:lib",
                    "surface_target_path": None,
                    "syntax_site_id": "syntax:type",
                    "visibility": "public",
                    "visibility_path": None,
                },
                {
                    "binding_kind": "use",
                    "lexical_scope_syntax_site_id": "syntax:root",
                    "local_name": "Row",
                    "source_node_id": "source:lib",
                    "surface_target_path": "rusqlite::Row",
                    "syntax_site_id": "syntax:use",
                    "visibility": "private",
                    "visibility_path": None,
                },
            ]
        }
        scopes = [
            {
                "crate_target_id": "target:one",
                "declaration_syntax_site_id": None,
                "parent_scope_id": None,
                "scope_id": "scope:root",
                "scope_syntax_site_id": "syntax:root",
                "source_node_id": "source:lib",
            },
            {
                "crate_target_id": "target:one",
                "declaration_syntax_site_id": "syntax:module",
                "parent_scope_id": "scope:root",
                "scope_id": "scope:state",
                "scope_syntax_site_id": "syntax:state-root",
                "source_node_id": "source:state",
            },
        ]
        symbols = [
            {
                "language": "rust",
                "role": "declaration",
                "signature": "Store",
                "site_id": "symbol:store",
                "syntax_site_id": "syntax:type",
            }
        ]

        bindings = self.materializer.materialize_rust_name_bindings(
            parsed, scopes, symbols
        )

        module = next(binding for binding in bindings if binding["local_name"] == "state")
        store = next(binding for binding in bindings if binding["local_name"] == "Store")
        row = next(binding for binding in bindings if binding["local_name"] == "Row")
        self.assertEqual(("resolved", "scope:state"), (module["resolution"], module["target_scope_id"]))
        self.assertEqual(
            ("resolved", "symbol:store"),
            (store["resolution"], store["target_symbol_site_id"]),
        )
        self.assertEqual("unresolved", row["resolution"])
        self.assertEqual("rust_binding_path_resolution_pending", row["reason_code"])

    def test_rust_path_resolution_follows_crate_self_and_reexport_chain(self):
        scopes = [
            {
                "canonical_module_path": "target::one",
                "crate_target_id": "target:one",
                "parent_scope_id": None,
                "scope_id": "scope:root",
                "scope_kind": "crate_root",
                "target_extern_crate_names": ["alloc", "core", "proc_macro", "std"],
            },
            {
                "canonical_module_path": "target::one::state",
                "crate_target_id": "target:one",
                "parent_scope_id": "scope:root",
                "scope_id": "scope:state",
                "scope_kind": "file_module",
            },
            {
                "canonical_module_path": "target::one::state::store",
                "crate_target_id": "target:one",
                "parent_scope_id": "scope:state",
                "scope_id": "scope:store",
                "scope_kind": "file_module",
            },
        ]

        def binding(
            binding_id,
            kind,
            scope_id,
            local_name,
            surface=None,
            target_scope=None,
            target_symbol=None,
        ):
            return {
                "binding_id": binding_id,
                "binding_kind": kind,
                "crate_target_id": "target:one",
                "local_name": local_name,
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "resolved" if target_scope or target_symbol else "unresolved",
                "scope_id": scope_id,
                "surface_target_path": surface,
                "target_scope_id": target_scope,
                "target_symbol_site_id": target_symbol,
                "visibility": "crate",
            }

        bindings = [
            binding("binding:state", "module", "scope:root", "state", target_scope="scope:state"),
            binding("binding:store", "module", "scope:state", "store", target_scope="scope:store"),
            binding(
                "binding:type",
                "type_declaration",
                "scope:store",
                "StateStore",
                target_symbol="symbol:state-store",
            ),
            binding(
                "binding:reexport",
                "reexport",
                "scope:state",
                "StateStore",
                "self::store::StateStore",
            ),
            binding(
                "binding:use",
                "use",
                "scope:root",
                "StateStore",
                "crate::state::StateStore",
            ),
        ]

        resolutions = self.materializer.resolve_rust_binding_paths(scopes, bindings)

        by_source = {resolution["source_binding_id"]: resolution for resolution in resolutions}
        resolved = by_source["binding:use"]
        self.assertEqual("resolved_local_type", resolved["status"])
        self.assertEqual("symbol:state-store", resolved["canonical_type_definition_site_id"])
        self.assertEqual(
            "target::one::state::store::StateStore", resolved["canonical_path"]
        )
        self.assertEqual(
            [
                "binding:use",
                "binding:state",
                "binding:reexport",
                "binding:store",
                "binding:type",
            ],
            resolved["binding_ids"],
        )

    def test_rust_path_resolution_uses_module_boundary_and_cargo_extern_attestation(self):
        scopes = [
            {
                "canonical_module_path": "target::one",
                "crate_target_id": "target:one",
                "parent_scope_id": None,
                "scope_id": "scope:root",
                "scope_kind": "crate_root",
                "target_extern_crate_names": ["alloc", "core", "proc_macro", "std", "time"],
            },
            {
                "canonical_module_path": "target::one::time",
                "crate_target_id": "target:one",
                "parent_scope_id": "scope:root",
                "scope_id": "scope:time",
                "scope_kind": "file_module",
            },
        ]
        bindings = [
            {
                "binding_id": "binding:module-time",
                "binding_kind": "module",
                "crate_target_id": "target:one",
                "local_name": "time",
                "namespace": "type",
                "reason_code": "rust_binding_exact_module_declaration",
                "resolution": "resolved",
                "scope_id": "scope:root",
                "surface_target_path": None,
                "target_scope_id": "scope:time",
                "target_symbol_site_id": None,
                "visibility": "private",
            },
            {
                "binding_id": "binding:duration",
                "binding_kind": "use",
                "crate_target_id": "target:one",
                "local_name": "Duration",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": "scope:time",
                "surface_target_path": "time::Duration",
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
            },
            {
                "binding_id": "binding:read-anonymous",
                "binding_kind": "use",
                "crate_target_id": "target:one",
                "local_name": "_",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": "scope:time",
                "surface_target_path": "std::io::Read",
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
            },
            {
                "binding_id": "binding:write-anonymous",
                "binding_kind": "use",
                "crate_target_id": "target:one",
                "local_name": "_",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": "scope:time",
                "surface_target_path": "std::io::Write",
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
            },
            {
                "binding_id": "binding:time-value-reexport",
                "binding_kind": "reexport",
                "crate_target_id": "target:one",
                "local_name": "time",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": "scope:root",
                "surface_target_path": "self::time::time",
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "crate",
            },
            {
                "binding_id": "binding:unknown-crate",
                "binding_kind": "use",
                "crate_target_id": "target:one",
                "local_name": "Unknown",
                "namespace": "type",
                "reason_code": "rust_binding_path_resolution_pending",
                "resolution": "unresolved",
                "scope_id": "scope:time",
                "surface_target_path": "mystery::Unknown",
                "target_scope_id": None,
                "target_symbol_site_id": None,
                "visibility": "private",
            },
        ]

        resolutions = self.materializer.resolve_rust_binding_paths(scopes, bindings)

        by_source = {resolution["source_binding_id"]: resolution for resolution in resolutions}
        self.assertEqual("external", by_source["binding:duration"]["status"])
        self.assertEqual("external", by_source["binding:read-anonymous"]["status"])
        self.assertEqual("external", by_source["binding:write-anonymous"]["status"])
        self.assertEqual("unresolved", by_source["binding:time-value-reexport"]["status"])
        self.assertEqual("unresolved", by_source["binding:unknown-crate"]["status"])

    def test_native_swift_parser_resolves_tree_sitter_recovery(self):
        parsed = {
            "source_nodes": [
                {
                    "language": "swift",
                    "parser_error_count": 7,
                    "path": "Sources/Valid.swift",
                    "source_node_id": "source:swift",
                }
            ]
        }
        version = subprocess.CompletedProcess(
            ["swiftc", "--version"], 0, stdout="Swift version 6.4\n"
        )
        parsed_file = subprocess.CompletedProcess(
            ["swiftc", "-frontend", "-parse", "/source/Sources/Valid.swift"],
            0,
            stdout="",
            stderr="",
        )

        with mock.patch.object(subprocess, "run", side_effect=[version, parsed_file]) as run:
            version_digest, sources = self.materializer.resolve_swift_recovery(
                Path("/source"), parsed
            )

        self.assertEqual({"source:swift"}, sources)
        self.assertEqual(0, parsed["source_nodes"][0]["parser_error_count"])
        self.assertEqual(64, len(version_digest))
        self.assertEqual(
            ["swiftc", "-frontend", "-parse", "/source/Sources/Valid.swift"],
            run.call_args_list[1].args[0],
        )

    def test_native_swift_parser_failure_remains_a_hard_failure(self):
        parsed = {
            "source_nodes": [
                {
                    "language": "swift",
                    "parser_error_count": 1,
                    "path": "Sources/Invalid.swift",
                    "source_node_id": "source:swift",
                }
            ]
        }
        version = subprocess.CompletedProcess(
            ["swiftc", "--version"], 0, stdout="Swift version 6.4\n"
        )
        failure = subprocess.CalledProcessError(
            1, ["swiftc", "-frontend", "-parse", "/source/Sources/Invalid.swift"]
        )

        with mock.patch.object(subprocess, "run", side_effect=[version, failure]):
            with self.assertRaises(subprocess.CalledProcessError):
                self.materializer.resolve_swift_recovery(Path("/source"), parsed)

        self.assertEqual(1, parsed["source_nodes"][0]["parser_error_count"])


if __name__ == "__main__":
    unittest.main()
