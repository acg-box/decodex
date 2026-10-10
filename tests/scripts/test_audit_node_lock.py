from __future__ import annotations

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "audit_node_lock", ROOT / "scripts/audit_node_lock.py"
)
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class NodeAuditInputTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.site = Path(self.temporary.name) / "site"
        self.site.mkdir()
        for name in ("package.json", "package-lock.json", ".nvmrc"):
            shutil.copyfile(ROOT / "site" / name, self.site / name)

    def run_audit(self, site: Path) -> tuple[int, dict]:
        output = io.StringIO()
        with (
            mock.patch.object(audit, "node_version", return_value=audit.REQUIRED_NODE),
            mock.patch.object(
                audit,
                "npm_version",
                return_value=audit.EXPECTED_PACKAGE_MANAGER.split("@")[1],
            ),
            contextlib.redirect_stdout(output),
        ):
            try:
                result = audit.main(["--lock-only", "--site", str(site)])
            except SystemExit as error:
                result = error.code
        return result, json.loads(output.getvalue())

    def test_regular_site_preserves_lock_graph(self) -> None:
        code, receipt = self.run_audit(self.site)
        self.assertEqual(code, 0)
        self.assertEqual(receipt["status"], "pass")
        self.assertEqual(receipt["scope"], "lock_graph")
        packages = json.loads((self.site / "package-lock.json").read_text())["packages"]
        self.assertEqual(receipt["audited_packages"], len(packages) - 1)

    def test_cli_rejects_symlink_site(self) -> None:
        link = self.site.parent / "linked-site"
        link.symlink_to(self.site, target_is_directory=True)
        self.assertEqual(
            self.run_audit(link),
            (1, {"status": "failed", "error_code": "node_site_root_invalid"}),
        )

    def test_bundled_package_requires_parent_membership(self) -> None:
        path = self.site / "package-lock.json"
        lock = json.loads(path.read_text())
        parent = lock["packages"]["node_modules/@tailwindcss/oxide-wasm32-wasi"]
        parent["bundleDependencies"].remove("tslib")
        path.write_text(json.dumps(lock))
        self.assertEqual(
            self.run_audit(self.site),
            (1, {"status": "failed", "error_code": "node_lock_bundle_provenance_invalid"}),
        )

    def test_bundled_package_requires_registry_parent_integrity(self) -> None:
        path = self.site / "package-lock.json"
        lock = json.loads(path.read_text())
        parent = lock["packages"]["node_modules/@tailwindcss/oxide-wasm32-wasi"]
        parent["integrity"] = "invalid"
        path.write_text(json.dumps(lock))
        self.assertEqual(
            self.run_audit(self.site),
            (1, {"status": "failed", "error_code": "node_lock_provenance_invalid"}),
        )

    def test_non_utf8_inputs_return_structured_failure(self) -> None:
        for name, expected in (
            ("package.json", "node_manifest_unavailable"),
            ("package-lock.json", "node_manifest_unavailable"),
            (".nvmrc", "node_toolchain_contract_invalid"),
        ):
            with self.subTest(name=name):
                path = self.site / name
                original = path.read_bytes()
                try:
                    path.write_bytes(b"\xff")
                    self.assertEqual(
                        self.run_audit(self.site),
                        (1, {"status": "failed", "error_code": expected}),
                    )
                finally:
                    path.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
