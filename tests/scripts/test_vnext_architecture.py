import json
import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

VNEXT_CONTRACT_ROOTS = (
    ROOT / "crates/decodex-core/src",
    ROOT / "crates/decodex-postgres/src",
    ROOT / "crates/decodex-protocol/src",
    ROOT / "crates/decodex-runtime/src",
    ROOT / "crates/decodex-app-client-ffi/src",
    ROOT / "apps/decodexd/src",
    ROOT / "apps/decodex-cli/src",
    ROOT / "apps/decodex-gpui/src",
)
CONTRACT_SUFFIXES = {".rs", ".sql", ".proto", ".json"}
GOAL_IDENTIFIER = re.compile(
    r"\b(?:goal|goals|goal_[A-Za-z0-9_]*|[A-Za-z0-9_]*Goal[A-Za-z0-9_]*)\b",
    re.IGNORECASE,
)
RUST_CONTRACT_STRING = re.compile(
    r"(?:serde\s*\([^)]*(?:rename|alias)|route|path|operation_id|schema)"
    r"[^\n]*[\"'][^\"']*\bgoals?\b",
    re.IGNORECASE,
)


def _strip_rust_prose(source):
    source = re.sub(r"/\*.*?\*/", " ", source, flags=re.DOTALL)
    source = re.sub(r"//[^\n]*", " ", source)
    return re.sub(r'r#*".*?"#*|"(?:\\.|[^"\\])*"', '""', source, flags=re.DOTALL)


def _strip_sql_prose(source):
    source = re.sub(r"--[^\n]*", " ", source)
    source = re.sub(r"/\*.*?\*/", " ", source, flags=re.DOTALL)
    return re.sub(r"'(?:''|[^'])*'", "''", source, flags=re.DOTALL)


def forbidden_goal_contracts(source, suffix, ownership="active_vnext"):
    """Return product Goal authority, not ordinary prose or bounded external ownership."""
    if ownership in {"frozen_legacy", "external_codex_adapter"}:
        return []
    findings = []
    if suffix == ".rs":
        findings.extend(match.group(0) for match in GOAL_IDENTIFIER.finditer(_strip_rust_prose(source)))
        findings.extend(match.group(0) for match in RUST_CONTRACT_STRING.finditer(source))
    elif suffix == ".sql":
        findings.extend(match.group(0) for match in GOAL_IDENTIFIER.finditer(_strip_sql_prose(source)))
    elif suffix in {".proto", ".json"}:
        findings.extend(match.group(0) for match in GOAL_IDENTIFIER.finditer(source))
    return findings

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
    "decodex-app-client-ffi": {"decodex-protocol"},
}

EXPECTED_POSTGRES_EXTERNAL_DEPENDENCIES = {
    "deadpool-postgres",
    "libc",
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
    "serde_json",
    "sha2",
    "tempfile",
    "toml",
}

EXPECTED_PROTOCOL_EXTERNAL_DEPENDENCIES = {
    "futures-util",
    "libc",
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
    "tempfile",
    "tokio",
    "toml_edit",
}

EXPECTED_APP_CLIENT_FFI_EXTERNAL_DEPENDENCIES = {
    "libc",
    "serde",
    "serde_json",
    "tempfile",
    "tokio",
    "toml_edit",
}

PRODUCTION_TARGET_KINDS = {"bin", "lib", "rlib", "dylib", "cdylib", "staticlib"}

EXPECTED_WORKSPACE_MANIFESTS = {
    "apps/decodex-cli/Cargo.toml",
    "apps/decodex-gpui/Cargo.toml",
    "apps/decodex-publisher/Cargo.toml",
    "apps/decodexd/Cargo.toml",
    "apps/radar/Cargo.toml",
    "crates/decodex-codex/Cargo.toml",
    "crates/decodex-app-client-ffi/Cargo.toml",
    "crates/decodex-core/Cargo.toml",
    "crates/decodex-postgres/Cargo.toml",
    "crates/decodex-protocol/Cargo.toml",
    "crates/decodex-runtime/Cargo.toml",
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
        dependencies = self.packages["decodex-core"]["dependencies"]
        actual = {
            dependency["name"]
            for dependency in dependencies
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_CORE_EXTERNAL_DEPENDENCIES)
        self.assertEqual(
            {
                dependency["name"]
                for dependency in dependencies
                if dependency["kind"] == "dev"
            },
            {"serde_json", "tempfile"},
        )

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
        dependencies = self.packages["decodex-cli"]["dependencies"]
        actual = {
            dependency["name"]
            for dependency in dependencies
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_CLI_EXTERNAL_DEPENDENCIES)
        self.assertEqual(
            {
                dependency["name"]
                for dependency in dependencies
                if dependency["kind"] == "dev"
            },
            {"tempfile"},
        )

    def test_app_client_ffi_external_dependencies_are_exact(self):
        owned_packages = set(EXPECTED_OWNER_DEPENDENCIES)
        dependencies = self.packages["decodex-app-client-ffi"]["dependencies"]
        actual = {
            dependency["name"]
            for dependency in dependencies
            if dependency["name"] not in owned_packages
        }

        self.assertEqual(actual, EXPECTED_APP_CLIENT_FFI_EXTERNAL_DEPENDENCIES)
        self.assertEqual(
            {
                dependency["name"]
                for dependency in dependencies
                if dependency["kind"] == "dev"
            },
            {"tempfile"},
        )

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

        for package_name in (
            "decodex-cli",
            "decodex-gpui",
            "decodex-app-client-ffi",
        ):
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
                if PRODUCTION_TARGET_KINDS.intersection(target["kind"])
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
            if PRODUCTION_TARGET_KINDS.intersection(target["kind"])
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
        local_authority_modules = {"fast_mode.rs", "git_hook.rs", "local_git.rs"}
        source = "\n".join(
            path.read_text()
            for path in sorted(cli_root.rglob("*.rs"))
            if path.name not in local_authority_modules
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

    def test_cli_fast_mode_is_one_local_config_authority(self):
        source = (ROOT / "apps/decodex-cli/src/fast_mode.rs").read_text()

        for required in (
            '"decodex/fast-mode-cli/1"',
            '"fast_mode"',
            '"config.toml"',
            'env::var_os("HOME")',
            "toml_edit",
        ):
            self.assertIn(required, source)

        forbidden = {
            "decodex_protocol",
            "decodex_runtime",
            "decodex_postgres",
            "AccountStore",
            "WebSocket",
            "TcpStream",
            "UnixStream",
            "reqwest",
            "access_token",
            "CODEX_HOME",
            "127.0.0.1",
            "8192",
        }
        self.assertEqual({token for token in forbidden if token in source}, set())

    def test_project_and_agent_identity_have_one_canonical_inert_authority(self):
        core_lib = (ROOT / "crates/decodex-core/src/lib.rs").read_text()
        project = (ROOT / "crates/decodex-core/src/project.rs").read_text()
        agent = (ROOT / "crates/decodex-core/src/agent.rs").read_text()
        postgres = (ROOT / "crates/decodex-postgres/src/project_agents.rs").read_text()
        schema = (ROOT / "crates/decodex-postgres/schema.sql").read_text()

        self.assertIn("ProjectId", core_lib)
        self.assertIn("AgentId", core_lib)
        self.assertIn("use decodex_core", postgres)
        self.assertNotIn("pub struct ProjectId", postgres)
        self.assertNotIn("pub struct AgentId", postgres)
        self.assertIn("CREATE TABLE decodex.projects", schema)
        self.assertIn("CREATE TABLE decodex.agents", schema)
        self.assertIn("agents_one_global_advisor_idx", schema)
        self.assertIn("agents_one_lead_per_project_idx", schema)
        self.assertEqual(
            postgres.count(
                "::pg_catalog.text::decodex.canonical_uuid_v4_text"
            ),
            4,
        )
        self.assertNotIn("::text::decodex.canonical_uuid_v4_text", postgres)
        self.assertEqual(
            schema.count("CREATE FUNCTION decodex.bootstrap_advisor("), 1
        )
        self.assertEqual(
            schema.count("CREATE FUNCTION decodex.create_project("), 1
        )
        self.assertEqual(
            schema.count("CREATE FUNCTION decodex.transition_project("), 1
        )

        production = project.split("#[cfg(test)]", 1)[0] + agent.split("#[cfg(test)]", 1)[0]
        for placeholder in (
            "project-placeholder",
            "lead-placeholder",
            "agent-placeholder",
            "00000000-0000-0000-0000-000000000000",
        ):
            self.assertNotIn(placeholder, production)
            self.assertNotIn(placeholder, schema)

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

    def test_project_policy_identity_and_exact_revision_have_one_inert_owner(self):
        core_lib = (ROOT / "crates/decodex-core/src/lib.rs").read_text()
        policy = (ROOT / "crates/decodex-core/src/policy.rs").read_text()
        postgres = (ROOT / "crates/decodex-postgres/src/policies.rs").read_text()
        schema = (ROOT / "crates/decodex-postgres/schema.sql").read_text()

        self.assertIn("PolicyId", core_lib)
        self.assertIn("PolicyRevisionId", core_lib)
        self.assertIn("pub struct PolicyId", policy)
        self.assertIn("pub struct PolicyRevisionId", policy)
        self.assertIn("use decodex_core", postgres)
        self.assertNotIn("pub struct PolicyId", postgres)
        self.assertNotIn("pub struct PolicyRevisionId", postgres)
        self.assertIn("CREATE TABLE decodex.policies", schema)
        self.assertIn("CREATE TABLE decodex.policy_revisions", schema)
        self.assertIn("policy_revisions_policy_project_fk", schema)
        self.assertIn("policy_revisions_accepting_agent_project_fk", schema)
        self.assertIn("policy_revisions_supersedes_fk", schema)
        self.assertIn("policy_revisions_immutable", schema)
        self.assertEqual(schema.count("CREATE FUNCTION decodex.create_policy("), 1)
        self.assertEqual(
            schema.count("CREATE FUNCTION decodex.accept_policy_revision("), 1
        )

        for path in (ROOT / "crates").rglob("*.rs"):
            if path == ROOT / "crates/decodex-core/src/policy.rs":
                continue
            self.assertNotIn("pub struct PolicyId", path.read_text(), str(path))
            self.assertNotIn("pub struct PolicyRevisionId", path.read_text(), str(path))

    def test_project_policy_slice_enables_no_effective_policy_behavior(self):
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

        for token in (
            "PolicyRepository",
            "accept_policy_revision",
            "policies_for_project",
        ):
            self.assertNotIn(token, source)

    def test_stateless_execution_coordinator_is_crate_private(
        self,
    ):
        core_routing = _strip_rust_prose(
            (ROOT / "crates/decodex-core/src/routing.rs").read_text()
        )
        postgres_routing = _strip_rust_prose(
            (ROOT / "crates/decodex-postgres/src/routing_decisions.rs").read_text()
        )
        postgres_snapshots = _strip_rust_prose(
            (ROOT / "crates/decodex-postgres/src/routing.rs").read_text()
        )
        quick_task_routing_path = (
            ROOT / "crates/decodex-postgres/src/quick_task_routing.rs"
        )
        postgres_quick_task_routing = _strip_rust_prose(
            quick_task_routing_path.read_text()
        )
        orchestration = _strip_rust_prose(
            (ROOT / "crates/decodex-runtime/src/routing_orchestration.rs").read_text()
        )
        runtime_quick_task = _strip_rust_prose(
            (ROOT / "crates/decodex-runtime/src/quick_task.rs").read_text()
        )
        exact_commands = _strip_rust_prose(
            (ROOT / "crates/decodex-postgres/src/exact_commands.rs").read_text()
        )
        runtime_root = _strip_rust_prose(
            (ROOT / "crates/decodex-runtime/src/lib.rs").read_text()
        )
        schema = (ROOT / "crates/decodex-postgres/schema.sql").read_text()

        self.assertEqual(core_routing.count("fn decide_routing"), 1)
        self.assertEqual(core_routing.count("fn decide_account_registry_routing"), 1)
        self.assertEqual(postgres_routing.count("decide_routing("), 1)
        self.assertEqual(postgres_quick_task_routing.count("decide_account_registry_routing("), 1)
        self.assertIn("client.transaction()", postgres_quick_task_routing)
        self.assertIn("parse_snapshot_envelope", postgres_quick_task_routing)
        self.assertIn("parse_initial_route_effect", postgres_quick_task_routing)
        self.assertIn("parse_continuation_binding", postgres_quick_task_routing)
        self.assertIn("pub async fn route_quick_task_initial", postgres_quick_task_routing)
        self.assertIn("pub async fn bind_quick_task_continuation", postgres_quick_task_routing)
        self.assertIn(
            "split routing decisions are reserved for ManagedRun execution",
            (ROOT / "crates/decodex-postgres/src/routing_decisions.rs").read_text(),
        )
        self.assertIn(
            "split routing snapshots are reserved for ManagedRun execution",
            (ROOT / "crates/decodex-postgres/src/routing.rs").read_text(),
        )
        self.assertNotIn("conversation_account_registry", postgres_routing)
        self.assertNotIn("conversation_account_registry", postgres_snapshots)

        expected_orchestration_invokers = {
            ".route_quick_task_initial(": "crates/decodex-runtime/src/routing_orchestration.rs",
            ".read_quick_task_initial_route(": "crates/decodex-runtime/src/routing_orchestration.rs",
            ".create_quick_task_routing_successor(": "crates/decodex-runtime/src/routing_orchestration.rs",
            ".bind_quick_task_continuation(": "crates/decodex-runtime/src/routing_orchestration.rs",
            ".plan_initial_thread_continuation(": "crates/decodex-runtime/src/routing_orchestration.rs",
            ".plan_continuation(": "crates/decodex-runtime/src/routing_orchestration.rs",
        }
        for call, owner in expected_orchestration_invokers.items():
            invokers = {
                path.relative_to(ROOT).as_posix()
                for path in (ROOT / "crates/decodex-runtime/src").rglob("*.rs")
                if call in _strip_rust_prose(path.read_text())
            }
            self.assertEqual(invokers, {owner}, call)

        for forbidden in (
            "route_quick_task_initial(",
            "bind_quick_task_continuation(",
            "resolve_routing_snapshot(",
            "route_account(",
            "decide_account_registry_routing(",
            "BEGIN_INITIAL_ROUTE_SQL",
            "COMPLETE_INITIAL_ROUTE_SQL",
            "BIND_CONTINUATION_SQL",
        ):
            self.assertNotIn(forbidden, runtime_quick_task)
        for routing_adapter in (
            "RouteQuickTaskInitial",
            "BindQuickTaskContinuation",
            "CreateQuickTaskRoutingSuccessor",
            "QuickTaskRoutingSuccessor",
            "successor_to_route(",
        ):
            self.assertNotIn(routing_adapter, runtime_quick_task)
        self.assertNotIn("QuickTask", exact_commands)

        for function in (
            "begin_quick_task_initial_route_exact",
            "complete_quick_task_initial_route_exact",
            "read_quick_task_initial_route_exact",
            "bind_quick_task_continuation_exact",
        ):
            self.assertEqual(schema.count(f"CREATE FUNCTION decodex.{function}("), 1)
        postgres_sql = sorted(
            path.relative_to(ROOT / "crates/decodex-postgres").as_posix()
            for path in (ROOT / "crates/decodex-postgres").rglob("*.sql")
        )
        self.assertEqual(postgres_sql, ["schema.sql"])

        self.assertIn("pub(crate) struct ExecutionCoordinator", orchestration)
        self.assertIn("pub(crate) async fn pre_process", orchestration)
        self.assertIn("pub(crate) async fn resume_establishment", orchestration)
        self.assertIn("pub(crate) async fn successor_to_route", orchestration)
        self.assertIn("pub(crate) async fn continuation_bind_to_plan", orchestration)
        self.assertIn("pub(crate) async fn post_process", orchestration)
        self.assertNotIn("DisabledRoutingOrchestration", orchestration)
        self.assertNotIn("pub use routing_orchestration", runtime_root)
        for identifier in (
            "ContinuationCoordinates",
            "ExecutionOutcome",
            "WaitingUsageHandoff",
            "WaitingReconciliationHandoff",
            "RoutingAuthorityRejection",
        ):
            self.assertIsNone(
                re.search(rf"\b{re.escape(identifier)}\b", orchestration),
                f"removed Rust identifier remains: {identifier}",
            )
            self.assertIsNone(
                re.search(rf"\b{re.escape(identifier)}\b", runtime_root),
                f"removed Rust identifier remains: {identifier}",
            )

        public_quick_task_source = "\n".join(
            path.read_text()
            for root in (
                ROOT / "crates/decodex-protocol/src",
                ROOT / "crates/decodex-runtime/src",
                ROOT / "apps/decodex-gpui/src",
            )
            for path in sorted(root.rglob("*.rs"))
        )
        self.assertNotIn("RetryRouting", public_quick_task_source)
        self.assertNotIn("RetryQuickTaskRouting", public_quick_task_source)

    def test_stateless_routing_coordinator_has_no_live_consumer_or_v18_composition(
        self,
    ):
        isolated_roots = (
            ROOT / "crates/decodex-protocol/src",
            ROOT / "crates/decodex-codex/src",
            ROOT / "apps/decodexd/src",
            ROOT / "apps/decodex-cli/src",
            ROOT / "apps/decodex-gpui/src",
        )
        isolated_source = "\n".join(
            _strip_rust_prose(path.read_text())
            for root in isolated_roots
            for path in sorted(root.rglob("*.rs"))
        )
        for identifier in (
            "ExecutionCoordinator",
            "ExecutionCommand",
            "ExecutionOutcome",
            "RouteAccount",
            "RoutingDecisionSnapshot",
            "PlanContinuation",
            "WaitingUsageHandoff",
            "WaitingUsageWake",
        ):
            self.assertIsNone(
                re.search(rf"\b{re.escape(identifier)}\b", isolated_source),
                f"removed Rust identifier remains: {identifier}",
            )

        runtime_source = "\n".join(
            _strip_rust_prose(path.read_text())
            for path in sorted((ROOT / "crates/decodex-runtime/src").rglob("*.rs"))
        )
        for identifier in (
            "RegisterWaitingUsageWake",
            "ClaimDueWaitingUsageWake",
            "FireWaitingUsageWake",
            "CancelWaitingUsageWake",
            "WaitingUsageWakeTransition",
        ):
            self.assertNotIn(identifier, runtime_source)

    def test_program_objective_identity_and_effect_boundaries_are_exact(self):
        core = (ROOT / "crates/decodex-core/src/program.rs").read_text()
        core_lib = (ROOT / "crates/decodex-core/src/lib.rs").read_text()
        postgres = (ROOT / "crates/decodex-postgres/src/programs.rs").read_text()
        schema = (ROOT / "crates/decodex-postgres/schema.sql").read_text()

        self.assertIn("stable_id!(ProgramId", core)
        self.assertIn("stable_id!(ObjectiveId", core)
        self.assertIn("mod program", core_lib)
        self.assertIn("ProgramContextInput", core_lib)
        self.assertIn("use decodex_core", postgres)
        for parallel_identity in (
            "struct ProjectId",
            "struct LeadId",
            "struct PolicyId",
            "struct WorkItemId",
            "work_item_ids",
        ):
            self.assertNotIn(parallel_identity, core)
            self.assertNotIn(parallel_identity, postgres)
        for runtime_identity in (
            "RuntimeSessionId",
            "ConversationId",
            "ThreadId",
            "create_conversation",
            "create_runtime_session",
        ):
            self.assertNotIn(runtime_identity, core)
            self.assertNotIn(runtime_identity, postgres)

        self.assertIn("REFERENCES decodex.projects", schema)
        self.assertIn("REFERENCES decodex.agents(agent_id, project_id)", schema)
        self.assertIn("REFERENCES decodex.policy_revisions", schema)
        self.assertIn("objective_completion_evidence", schema)
        self.assertIn("DEFERRABLE INITIALLY DEFERRED", schema)
        self.assertIn("p_project_id decodex.canonical_uuid_v4_text", schema)
        self.assertIn("stored.project_id<>canonical_project_id", schema)
        self.assertIn("evidence_row.objective_updated_at <> OLD.updated_at", schema)
        self.assertIn("ON CONFLICT DO NOTHING", schema)

    def test_active_vnext_contracts_have_no_product_goal_authority(self):
        findings = {}
        paths = [ROOT / "crates/decodex-postgres/schema.sql"]
        for root in VNEXT_CONTRACT_ROOTS:
            paths.extend(sorted(root.rglob("*")))
        for path in paths:
            if path.is_file() and path.suffix in CONTRACT_SUFFIXES:
                path_findings = forbidden_goal_contracts(path.read_text(), path.suffix)
                if path_findings:
                    findings[str(path.relative_to(ROOT))] = path_findings

        self.assertEqual(findings, {})

    def test_retained_title_runtime_experiment_is_absent(self):
        for path in (
            ROOT / "crates/decodex-runtime/src/account_launch/retained_title_experiment.rs",
            ROOT / "crates/decodex-runtime/src/bin/decodex-retained-title-experiment.rs",
        ):
            self.assertFalse(path.exists(), str(path))

        process = (
            ROOT / "crates/decodex-runtime/src/account_launch/process.rs"
        ).read_text()
        for token in (
            "RETAINED_TITLE_DEVELOPER_INSTRUCTIONS",
            "start_retained_title_thread",
            "set_retained_title",
            "read_retained_title_thread",
            "launch_retained_title_process",
        ):
            self.assertNotIn(token, process)

        schema = (ROOT / "crates/decodex-postgres/schema.sql").read_text()
        for durable_authority in (
            "prepare_codex_experiment_exact",
            "attest_codex_experiment_retained_title_exact",
            "record_attested_codex_experiment_observation_exact",
        ):
            self.assertIn(durable_authority, schema)

    def test_managed_run_success_requires_work_item_acceptance_authority(self):
        required = (
            "ManagedRun may reach successful terminal completion only from explicit "
            "authoritative\nWorkItem acceptance and validation"
        )
        exclusions = (
            "Objective achievement or evidence",
            "Codex Goal state cannot establish WorkItem acceptance or ManagedRun success",
        )
        for relative in (
            "openwiki/specs/vnext-authority.md",
            "openwiki/specs/vnext-gates.md",
        ):
            contract = (ROOT / relative).read_text()

            self.assertIn(required, contract)
            for exclusion in exclusions:
                self.assertIn(exclusion, contract)

    def test_goal_vocabulary_audit_is_contract_aware_and_adversarial(self):
        forbidden = {
            ".rs": [
                "pub struct Goal { id: GoalId }",
                "pub goal_id: String",
                '#[serde(rename = "goal")] pub objective: String',
                'route("/vnext/goals")',
            ],
            ".sql": [
                "CREATE TABLE decodex.goals (goal_id uuid PRIMARY KEY);",
                'CREATE TYPE decodex."GoalState" AS ENUM (\'active\');',
            ],
            ".proto": ["message Goal { string goal_id = 1; }"],
            ".json": ['{"properties":{"goal":{"type":"string"}}}'],
        }
        for suffix, fixtures in forbidden.items():
            for fixture in fixtures:
                with self.subTest(suffix=suffix, fixture=fixture):
                    self.assertTrue(forbidden_goal_contracts(fixture, suffix))

        allowed = [
            ("// Current non-goals remain explicit.", ".rs", "active_vnext"),
            ('let prose = "ordinary goals and non-goals";', ".rs", "active_vnext"),
            ("-- ordinary goals are discussed here", ".sql", "active_vnext"),
            ("pub struct GoalHandle;", ".rs", "external_codex_adapter"),
            ("CREATE TABLE legacy_goals(id uuid);", ".sql", "frozen_legacy"),
        ]
        for fixture, suffix, ownership in allowed:
            with self.subTest(suffix=suffix, ownership=ownership):
                self.assertEqual(
                    forbidden_goal_contracts(fixture, suffix, ownership), []
                )

if __name__ == "__main__":
    unittest.main()
