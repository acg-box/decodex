import json
import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

VNEXT_CONTRACT_ROOTS = (
    ROOT / "crates/decodex-core/src",
    ROOT / "crates/decodex-postgres/src",
    ROOT / "crates/decodex-postgres/migrations",
    ROOT / "crates/decodex-protocol/src",
    ROOT / "crates/decodex-runtime/src",
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


def _rust_braced_body_after(source, marker):
    start = source.index(marker) + len(marker)
    opening = source.index("{", start)
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[opening + 1 : index]
    raise AssertionError(f"unclosed Rust body after {marker}")


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

    def test_project_policy_identity_and_exact_revision_have_one_inert_owner(self):
        core_lib = (ROOT / "crates/decodex-core/src/lib.rs").read_text()
        policy = (ROOT / "crates/decodex-core/src/policy.rs").read_text()
        postgres = (ROOT / "crates/decodex-postgres/src/policies.rs").read_text()
        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V6__project_policy_authority.sql"
        ).read_text()

        self.assertIn("PolicyId", core_lib)
        self.assertIn("PolicyRevisionId", core_lib)
        self.assertIn("pub struct PolicyId", policy)
        self.assertIn("pub struct PolicyRevisionId", policy)
        self.assertIn("use decodex_core", postgres)
        self.assertNotIn("pub struct PolicyId", postgres)
        self.assertNotIn("pub struct PolicyRevisionId", postgres)
        self.assertIn("CREATE TABLE decodex.policies", migration)
        self.assertIn("CREATE TABLE decodex.policy_revisions", migration)
        self.assertIn("policy_revisions_policy_project_fk", migration)
        self.assertIn("policy_revisions_accepting_agent_project_fk", migration)
        self.assertIn("policy_revisions_supersedes_fk", migration)
        self.assertIn("policy_revisions_immutable", migration)
        self.assertEqual(migration.count("CREATE FUNCTION decodex.create_policy("), 1)
        self.assertEqual(
            migration.count("CREATE FUNCTION decodex.accept_policy_revision("), 1
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

        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V6__project_policy_authority.sql"
        ).read_text().lower()
        for live_token in (
            "approval routing",
            "budget enforcement",
            "quiet period",
            "tool enforcement",
            "path enforcement",
            "network enforcement",
            "dispatch",
            "wakeup",
            "codex process",
        ):
            self.assertNotIn(live_token, migration)

    def test_v16_decision_and_v17_handoff_have_one_stateless_dispatch_disabled_path(
        self,
    ):
        core_routing = _strip_rust_prose(
            (ROOT / "crates/decodex-core/src/routing.rs").read_text()
        )
        postgres_routing = _strip_rust_prose(
            (ROOT / "crates/decodex-postgres/src/routing_decisions.rs").read_text()
        )
        orchestration = _strip_rust_prose(
            (ROOT / "crates/decodex-runtime/src/routing_orchestration.rs").read_text()
        )

        self.assertEqual(core_routing.count("fn decide_routing"), 1)
        self.assertEqual(postgres_routing.count("decide_routing("), 1)
        runtime_invokers = {
            path.relative_to(ROOT).as_posix()
            for path in (ROOT / "crates/decodex-runtime/src").rglob("*.rs")
            if ".route_account(" in _strip_rust_prose(path.read_text())
        }
        self.assertEqual(
            runtime_invokers,
            {"crates/decodex-runtime/src/routing_orchestration.rs"},
        )
        self.assertIn("pub struct ExecutionCoordinator", orchestration)
        self.assertIn("pub(crate) async fn coordinate", orchestration)
        self.assertNotIn("DisabledRoutingOrchestration", orchestration)

        selected_start = orchestration.index("RoutingDecisionKind::Selected =>")
        selected_end = orchestration.index(
            "RoutingDecisionKind::WaitingUsage =>", selected_start
        )
        selected = orchestration[selected_start:selected_end]
        waiting = _rust_braced_body_after(
            orchestration, "RoutingDecisionKind::WaitingUsage =>"
        )
        reconciliation = _rust_braced_body_after(
            orchestration, "RoutingDecisionKind::WaitingReconciliation =>"
        )
        no_route = _rust_braced_body_after(
            orchestration, "RoutingDecisionKind::NoRoute =>"
        )
        self.assertIn("plan_and_prepare", selected)
        self.assertNotIn("plan_and_prepare", waiting)
        self.assertNotIn("plan_and_prepare", reconciliation)
        self.assertNotIn("plan_and_prepare", no_route)
        self.assertIn("WaitingUsageHandoff", waiting)
        self.assertNotIn("WaitingUsageHandoff", reconciliation)
        self.assertIn("WaitingReconciliationHandoff", reconciliation)
        self.assertNotIn("WaitingUsageWake", waiting)

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
            self.assertNotIn(identifier, isolated_source)

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
        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V7__program_objective_authority.sql"
        ).read_text()

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
            self.assertNotIn(parallel_identity, migration)
        for runtime_identity in (
            "RuntimeSessionId",
            "ConversationId",
            "ThreadId",
            "create_conversation",
            "create_runtime_session",
        ):
            self.assertNotIn(runtime_identity, core)
            self.assertNotIn(runtime_identity, postgres)
            self.assertNotIn(runtime_identity, migration)

        self.assertIn("REFERENCES decodex.projects", migration)
        self.assertIn("REFERENCES decodex.agents(agent_id, project_id)", migration)
        self.assertIn("REFERENCES decodex.policy_revisions", migration)
        self.assertIn("objective_completion_evidence", migration)
        self.assertIn("DEFERRABLE INITIALLY DEFERRED", migration)
        self.assertIn("p_project_id decodex.canonical_uuid_v4_text", migration)
        self.assertIn("stored.project_id<>canonical_project_id", migration)
        self.assertIn("evidence_row.objective_updated_at <> OLD.updated_at", migration)
        self.assertIn("ON CONFLICT DO NOTHING", migration)
        self.assertNotIn("WorkItem", migration)

    def test_active_vnext_contracts_have_no_product_goal_authority(self):
        findings = {}
        for root in VNEXT_CONTRACT_ROOTS:
            for path in sorted(root.rglob("*")):
                if path.is_file() and path.suffix in CONTRACT_SUFFIXES:
                    path_findings = forbidden_goal_contracts(
                        path.read_text(), path.suffix
                    )
                    if path_findings:
                        findings[str(path.relative_to(ROOT))] = path_findings

        self.assertEqual(findings, {})

    def test_v22_retained_title_bridge_is_two_effect_and_production_inert(self):
        migration = (
            ROOT
            / "crates/decodex-postgres/migrations/V22__retained_title_experiment_bridge.sql"
        ).read_text()
        runner = (
            ROOT
            / "crates/decodex-runtime/src/account_launch/retained_title_experiment.rs"
        ).read_text()
        runtime_manifest = (ROOT / "crates/decodex-runtime/Cargo.toml").read_text()
        runtime_lib = (ROOT / "crates/decodex-runtime/src/lib.rs").read_text()
        daemon_manifest = (ROOT / "apps/decodexd/Cargo.toml").read_text()
        daemon_source = "\n".join(
            path.read_text()
            for path in sorted((ROOT / "apps/decodexd/src").rglob("*.rs"))
        )

        for exact_fact in (
            "start_request_id",
            "start_request_digest",
            "request_cwd",
            "request_marker",
            "request_ephemeral",
            "start_response_id",
            "start_response_digest",
            "response_ephemeral",
            "returned_name",
            "requested_title",
            "read_request_digest",
            "read_response_digest",
            "returned_cwd",
        ):
            self.assertIn(exact_fact, migration)
        self.assertIn("p_returned_name IS NOT NULL", migration)
        self.assertIn("returned_name text CHECK (returned_name IS NULL)", migration)
        self.assertIn("codex_experiment_title_set_attempts", migration)
        self.assertIn("codex_experiment_retained_title_attestations", migration)
        self.assertIn("codex_experiment_attested_observations", migration)
        self.assertIn("attestation.attested_at<=observation.observed_at", migration)
        self.assertIn(
            "REVOKE EXECUTE ON FUNCTION decodex.bind_codex_experiment_thread_exact",
            migration,
        )
        self.assertIn(
            "REVOKE EXECUTE ON FUNCTION decodex.record_codex_experiment_observation_exact",
            migration,
        )

        ordered_calls = (
            ".prepare_codex_experiment(",
            ".mark_codex_experiment_creation_possible(",
            ".start_retained_title_thread(",
            ".bind_codex_experiment_start(",
            ".mark_codex_experiment_title_set_possible(",
            ".set_retained_title(",
            ".read_retained_title_thread(",
            ".attest_codex_experiment_retained_title(",
            ".record_attested_codex_experiment_observation(",
        )
        positions = [runner.index(call) for call in ordered_calls]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("const START_REQUEST_ID: i64 = 3", runner)
        self.assertIn("const TITLE_SET_REQUEST_ID: i64 = 4", runner)
        self.assertIn("const READ_REQUEST_ID: i64 = 5", runner)
        self.assertRegex(
            runner,
            r"Err\(RpcError::MethodRejected\(_\)\)\s*=>\s*(?:"
            r"return Err\(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous\)\s*,?"
            r"|\{\s*return Err\("
            r"ManualRetainedTitleExperimentError::RetainedTitleAmbiguous"
            r"\);\s*\})",
        )
        self.assertNotIn("Err(RpcError::MethodRejected(_)) => false", runner)
        self.assertIn("Err(RpcError::Supervision(_)) => false", runner)
        self.assertIn('format!("v22:retained-title:{experiment_id}:{operation}")', runner)
        for forbidden_method in (
            "thread/list",
            "thread/search",
            "thread/archive",
            "turn/start",
            "thread/resume",
        ):
            self.assertNotIn(forbidden_method, runner)

        self.assertIn('retained-title-experiment = []', runtime_manifest)
        self.assertIn('required-features = ["retained-title-experiment"]', runtime_manifest)
        self.assertIn('#[cfg(feature = "retained-title-experiment")]', runtime_lib)
        self.assertNotIn("retained-title-experiment", daemon_manifest)
        self.assertNotIn("run_manual_retained_title_experiment", daemon_source)

        production_roots = (
            ROOT / "crates/decodex-protocol/src",
            ROOT / "apps/decodexd/src",
        )
        production_files = (
            ROOT / "crates/decodex-runtime/src/application.rs",
            ROOT / "crates/decodex-runtime/src/routing_orchestration.rs",
        )
        production_source = "\n".join(
            path.read_text()
            for root in production_roots
            for path in sorted(root.rglob("*.rs"))
        ) + "\n".join(path.read_text() for path in production_files)
        self.assertNotIn("run_manual_retained_title_experiment", production_source)
        self.assertNotIn("FreshCodexExperimentCreation", production_source)

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
