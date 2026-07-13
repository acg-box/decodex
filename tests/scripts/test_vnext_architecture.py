import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

EXPECTED_DEPENDENCIES = {
    "decodex-core": set(),
    "decodex-protocol": {"decodex-core"},
    "decodex-postgres": {"decodex-core"},
    "decodex-codex": {"decodex-core"},
    "decodex-runtime": {
        "decodex-codex",
        "decodex-core",
        "decodex-postgres",
        "decodex-protocol",
    },
    "decodexd": {"decodex-runtime"},
    "decodex-cli": {"decodex-protocol"},
    "decodex-gpui": {"decodex-protocol"},
}

EXPECTED_WORKSPACE_MANIFESTS = {
    "apps/decodex-cli/Cargo.toml",
    "apps/decodex-gpui/Cargo.toml",
    "apps/decodex-publisher/Cargo.toml",
    "apps/decodexd/Cargo.toml",
    "apps/radar/Cargo.toml",
    "crates/decodex-codex/Cargo.toml",
    "crates/decodex-core/Cargo.toml",
    "crates/decodex-postgres/Cargo.toml",
    "crates/decodex-protocol/Cargo.toml",
    "crates/decodex-runtime/Cargo.toml",
    "spikes/vnext-storage/Cargo.toml",
}


class VnextArchitectureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        cls.metadata = json.loads(result.stdout)
        cls.packages = {package["name"]: package for package in cls.metadata["packages"]}
        cls.packages_by_id = {
            package["id"]: package for package in cls.metadata["packages"]
        }

    def test_vnext_dependency_direction_is_exact(self):
        for package_name, expected in EXPECTED_DEPENDENCIES.items():
            with self.subTest(package=package_name):
                actual = {
                    dependency["name"]
                    for dependency in self.packages[package_name]["dependencies"]
                }
                self.assertEqual(actual, expected)

    def test_workspace_owner_set_is_exact(self):
        actual = {
            str(
                Path(self.packages_by_id[package_id]["manifest_path"])
                .resolve()
                .relative_to(ROOT)
            )
            for package_id in self.metadata["workspace_members"]
        }

        self.assertEqual(actual, EXPECTED_WORKSPACE_MANIFESTS)

    def test_legacy_runtime_is_preserved_but_not_an_active_workspace_member(self):
        member_manifests = {
            Path(package["manifest_path"]).resolve()
            for package in self.metadata["packages"]
            if package["id"] in self.metadata["workspace_members"]
        }
        legacy_manifest = (ROOT / "apps/decodex/Cargo.toml").resolve()

        self.assertTrue(legacy_manifest.is_file())
        self.assertNotIn(legacy_manifest, member_manifests)
        self.assertNotIn("decodex", self.packages)

    def test_clients_cannot_reach_runtime_or_infrastructure_owners(self):
        forbidden = {"decodex-runtime", "decodex-postgres", "decodex-codex"}

        for package_name in ("decodex-cli", "decodex-gpui"):
            dependencies = {
                dependency["name"]
                for dependency in self.packages[package_name]["dependencies"]
            }
            self.assertTrue(dependencies.isdisjoint(forbidden))


if __name__ == "__main__":
    unittest.main()
