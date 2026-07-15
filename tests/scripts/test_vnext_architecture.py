import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

EXPECTED_OWNER_DEPENDENCIES = {
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

EXPECTED_POSTGRES_EXTERNAL_DEPENDENCIES = {
    "deadpool-postgres",
    "libc",
    "refinery",
    "serde_json",
    "sha2",
    "time",
    "tokio",
    "tokio-postgres",
}

EXPECTED_CORE_EXTERNAL_DEPENDENCIES = {
    "getrandom",
    "libc",
    "regex",
    "serde",
    "sha2",
    "tempfile",
    "toml",
}

EXPECTED_PROTOCOL_EXTERNAL_DEPENDENCIES = {
    "futures-util",
    "serde",
    "serde_json",
    "tempfile",
    "tokio",
    "tokio-tungstenite",
}

EXPECTED_CLI_EXTERNAL_DEPENDENCIES = {
    "clap",
    "serde",
    "serde_json",
    "tokio",
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
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        for package_name, expected in EXPECTED_OWNER_DEPENDENCIES.items():
            with self.subTest(package=package_name):
                actual = {
                    dependency["name"]
                    for dependency in self.packages[package_name]["dependencies"]
                    if dependency["name"] in owned_packages
                }
                self.assertEqual(actual, expected)

    def test_postgres_dependency_stack_is_exact(self):
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        actual = {
            dependency["name"]
            for dependency in self.packages["decodex-postgres"]["dependencies"]
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_POSTGRES_EXTERNAL_DEPENDENCIES)

    def test_core_configuration_and_storage_dependencies_are_exact(self):
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        actual = {
            dependency["name"]
            for dependency in self.packages["decodex-core"]["dependencies"]
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_CORE_EXTERNAL_DEPENDENCIES)

    def test_protocol_client_transport_dependencies_are_exact(self):
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        actual = {
            dependency["name"]
            for dependency in self.packages["decodex-protocol"]["dependencies"]
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_PROTOCOL_EXTERNAL_DEPENDENCIES)

    def test_cli_external_dependencies_are_exact(self):
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        actual = {
            dependency["name"]
            for dependency in self.packages["decodex-cli"]["dependencies"]
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_CLI_EXTERNAL_DEPENDENCIES)

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

    def test_production_targets_reach_both_adapters_only_through_runtime(self):
        workspace_names = set(self.packages)
        production_graph = {
            package["name"]: {
                dependency["name"]
                for dependency in package["dependencies"]
                if dependency["kind"] is None
                and dependency["name"] in workspace_names
            }
            for package in self.metadata["packages"]
        }

        def reachable(package_name):
            pending = list(production_graph[package_name])
            result = set()
            while pending:
                dependency = pending.pop()
                if dependency in result:
                    continue
                result.add(dependency)
                pending.extend(production_graph.get(dependency, set()))
            return result

        covered_targets = set()
        reaches_both = set()
        for package in self.metadata["packages"]:
            production_targets = {
                target["name"]
                for target in package["targets"]
                if {"lib", "bin"}.intersection(target["kind"])
            }
            self.assertTrue(production_targets, package["name"])
            covered_targets.update(
                (package["name"], target) for target in production_targets
            )
            dependencies = reachable(package["name"])
            if {"decodex-postgres", "decodex-codex"}.issubset(dependencies):
                reaches_both.add(package["name"])

        expected_targets = {
            (package["name"], target["name"])
            for package in self.metadata["packages"]
            for target in package["targets"]
            if {"lib", "bin"}.intersection(target["kind"])
        }

        self.assertEqual(covered_targets, expected_targets)
        self.assertEqual(reaches_both, {"decodex-runtime", "decodexd"})
        self.assertEqual(
            production_graph["decodex-runtime"].intersection(
                {"decodex-postgres", "decodex-codex"}
            ),
            {"decodex-postgres", "decodex-codex"},
        )
        self.assertEqual(
            production_graph["decodexd"].intersection(
                {"decodex-runtime", "decodex-postgres", "decodex-codex"}
            ),
            {"decodex-runtime"},
        )
        self.assertNotIn("decodex-postgres", reachable("decodex-codex"))

    def test_synthetic_account_binding_features_are_not_enabled_by_production_edges(self):
        forbidden_features = {"account-binding-fixtures", "test-support"}

        for package in self.metadata["packages"]:
            with self.subTest(package=package["name"]):
                defaults = set(package["features"].get("default", []))
                self.assertTrue(defaults.isdisjoint(forbidden_features))

                for dependency in package["dependencies"]:
                    if dependency["kind"] is None:
                        self.assertTrue(
                            set(dependency["features"]).isdisjoint(forbidden_features)
                        )

    def test_cli_source_has_no_direct_mutation_or_infrastructure_escape(self):
        cli_root = ROOT / "apps/decodex-cli"
        source = "\n".join(
            path.read_text()
            for path in sorted(cli_root.rglob("*.rs"))
        )
        forbidden = {
            "decodex_core",
            "decodex_runtime",
            "decodex_postgres",
            "decodex_codex",
            "tokio_postgres",
            "rusqlite",
            "std::fs",
            "OpenOptions",
            "std::process::Command",
        }

        self.assertEqual({token for token in forbidden if token in source}, set())

    def test_project_and_agent_identity_have_one_canonical_inert_authority(self):
        core_lib = (ROOT / "crates/decodex-core/src/lib.rs").read_text()
        project = (ROOT / "crates/decodex-core/src/project.rs").read_text()
        agent = (ROOT / "crates/decodex-core/src/agent.rs").read_text()
        postgres = (ROOT / "crates/decodex-postgres/src/project_agents.rs").read_text()
        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V5__project_agent_authority.sql"
        ).read_text()

        self.assertIn("ProjectId", core_lib)
        self.assertIn("AgentId", core_lib)
        self.assertIn("use decodex_core", postgres)
        self.assertNotIn("pub struct ProjectId", postgres)
        self.assertNotIn("pub struct AgentId", postgres)
        self.assertIn("CREATE TABLE decodex.projects", migration)
        self.assertIn("CREATE TABLE decodex.agents", migration)
        self.assertIn("agents_one_global_advisor_idx", migration)
        self.assertIn("agents_one_lead_per_project_idx", migration)
        self.assertEqual(
            postgres.count(
                "::pg_catalog.text::decodex.canonical_uuid_v4_text"
            ),
            4,
        )
        self.assertNotIn("::text::decodex.canonical_uuid_v4_text", postgres)
        self.assertEqual(
            migration.count("CREATE FUNCTION decodex.bootstrap_advisor("), 1
        )
        self.assertEqual(
            migration.count("CREATE FUNCTION decodex.create_project("), 1
        )
        self.assertEqual(
            migration.count("CREATE FUNCTION decodex.transition_project("), 1
        )

        production = project.split("#[cfg(test)]", 1)[0] + agent.split("#[cfg(test)]", 1)[0]
        for placeholder in (
            "project-placeholder",
            "lead-placeholder",
            "agent-placeholder",
            "00000000-0000-0000-0000-000000000000",
        ):
            self.assertNotIn(placeholder, production)
            self.assertNotIn(placeholder, migration)

    def test_project_and_agent_slice_enables_no_live_behavior(self):
        live_roots = [
            ROOT / "crates/decodex-codex/src",
            ROOT / "crates/decodex-protocol/src",
            ROOT / "crates/decodex-runtime/src",
            ROOT / "apps/decodexd/src",
            ROOT / "apps/decodex-cli/src",
            ROOT / "apps/decodex-gpui/src",
        ]
        source = "\n".join(
            path.read_text()
            for root in live_roots
            for path in sorted(root.rglob("*.rs"))
        )
        forbidden = {
            "bootstrap_advisor",
            "create_project",
            "ProjectAuthority",
            "ProjectRepository",
            "AgentRepository",
        }

        self.assertEqual({token for token in forbidden if token in source}, set())

        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V5__project_agent_authority.sql"
        ).read_text().lower()
        for live_token in (
            "prompt",
            "model",
            "delegate",
            "schedule",
            "wakeup",
            "message",
        ):
            self.assertNotIn(live_token, migration)
        for conversation_surface in (
            "create table decodex.conversations",
            "insert into decodex.conversations",
            "conversation_id",
        ):
            self.assertNotIn(conversation_surface, migration)


if __name__ == "__main__":
    unittest.main()
