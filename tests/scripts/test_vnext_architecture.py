"""Static architecture checks for the local SQLite product slice."""

from pathlib import Path
import plistlib
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def toml(path: str) -> dict[str, object]:
    with (ROOT / path).open("rb") as source:
        return tomllib.load(source)


class LocalSqliteArchitectureTests(unittest.TestCase):
    """Protect the SQLite authority and protocol-only desktop client."""

    def test_database_is_one_discoverable_workspace_owner(self) -> None:
        workspace = toml("Cargo.toml")["workspace"]
        members = set(workspace["members"])
        dependencies = workspace["dependencies"]
        self.assertIn("database", members)
        self.assertNotIn("database/transfer", members)
        self.assertIn("bundled", dependencies["rusqlite"]["features"])

    def test_current_store_has_no_retired_storage_dependency(self) -> None:
        runtime = toml("crates/decodex-runtime/Cargo.toml")
        dependencies = runtime["dependencies"]
        self.assertIn("decodex-database", dependencies)
        self.assertNotIn("redb", dependencies)
        self.assertNotIn("redb", toml("Cargo.toml")["workspace"]["dependencies"])

    def test_clients_remain_protocol_only(self) -> None:
        for manifest_path in ("apps/decodex-gpui/Cargo.toml",):
            dependencies = toml(manifest_path)["dependencies"]
            with self.subTest(manifest=manifest_path):
                self.assertIn("decodex-protocol", dependencies)
                self.assertNotIn("decodex-database", dependencies)
                self.assertNotIn("rusqlite", dependencies)
                self.assertNotIn("redb", dependencies)
        cli_dependencies = toml("apps/decodex-cli/Cargo.toml")["dependencies"]
        self.assertIn("decodex-protocol", cli_dependencies)
        self.assertIn("decodex-runtime", cli_dependencies)
        self.assertNotIn("decodex-database", cli_dependencies)
        self.assertNotIn("rusqlite", cli_dependencies)
        self.assertNotIn("redb", cli_dependencies)

    def test_desktop_has_one_foreground_app_bundle(self) -> None:
        manifests = sorted(ROOT.glob("apps/*/packaging/Info.plist"))
        self.assertEqual(len(manifests), 1)
        with manifests[0].open("rb") as source:
            info = plistlib.load(source)
        self.assertEqual(info["CFBundleIdentifier"], "box.acg.decodex")
        self.assertFalse(info.get("LSBackgroundOnly", False))

    def test_shared_auth_observation_must_not_terminate_other_apps(self) -> None:
        coordinator = read("crates/decodex-runtime/src/shared_auth_coordinator.rs")
        for forbidden in ("std::process::Command", "libc::kill", "SIGTERM", "SIGKILL"):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, coordinator)


if __name__ == "__main__":
    unittest.main()
