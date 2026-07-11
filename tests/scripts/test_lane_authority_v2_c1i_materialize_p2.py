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
