#!/usr/bin/env python3
"""Generate or verify the Lane Authority v2 C0 baseline manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


BASELINE_DEFAULT = "d57553bc1bcdceebe1d0c7ec5ad5dc492b695348"
SOURCE_ROOTS = (
    ".github",
    "apps/decodex/src",
    "apps/decodex-app/Sources",
    "apps/decodex-publisher/src",
    "apps/radar/src",
    "automations",
    "plugins/decodex",
    "scripts",
)
SOURCE_SUFFIXES = {".rs", ".py", ".swift", ".sh", ".bash", ".zsh", ".toml", ".yml", ".yaml"}
GENERATED_PATHS = {
    "apps/decodex/src/bootstrap/tests/fixtures/lane_authority_v2/launcher_inventory.json",
    "apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json",
    "apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json",
    "apps/decodex/src/state/tests/fixtures/lane_authority_v2/legacy_authority_inventory.json",
}
C0_ALLOWED_PATHS = GENERATED_PATHS | {
    "openwiki/decisions/lane-authority-v2.md",
    "openwiki/evidence/lane-authority-v2-checkpoints.md",
    "openwiki/quickstart.md",
    "openwiki/specs/lane-authority-v2-effects.md",
    "openwiki/specs/lane-authority-v2-gates.md",
    "openwiki/specs/lane-authority-v2.md",
    "scripts/lane_authority_v2_baseline.py",
    "scripts/verify_lane_authority_v2_baseline.sh",
}
SCENARIO_SPEC = Path("openwiki/specs/lane-authority-v2.md")
EFFECT_SPEC = Path("openwiki/specs/lane-authority-v2-effects.md")
SCENARIO_FREEZE_COUNT = 129
SCENARIO_FREEZE_DIGEST = "43b5a08e8c196d8253af341db92858717610c67f77c2718d2bdd973b342ac127"


@dataclass(frozen=True)
class Pattern:
    category: str
    access: str
    expression: re.Pattern[str]
    adapter_owner: str
    replacement_kind: str
    removal_checkpoint: str


PATTERNS = (
    Pattern(
        "sqlite_write",
        "write",
        re.compile(r"\b(?:execute|execute_batch|execute_named|transaction|savepoint)\s*\(|\b(?:INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|REPLACE)\b", re.I),
        "lane_authority_v2::state_adapter",
        "runtime.commit_transition",
        "C7",
    ),
    Pattern(
        "sqlite_read_or_schema_discovery",
        "read_discovery",
        re.compile(r"\b(?:query|query_row|prepare|pragma|sqlite_master|user_version)\b", re.I),
        "lane_authority_v2::state_adapter",
        "runtime.authority_read",
        "C7",
    ),
    Pattern(
        "filesystem_mutation",
        "write",
        re.compile(r"(?:fs::(?:write|remove|rename|copy|create_dir)|File::create|OpenOptions|write_all|set_permissions|\.unlink\s*\(|\.write_(?:text|bytes)\s*\(|shutil\.(?:copy|move|rmtree)|FileManager\.default\.(?:removeItem|moveItem|copyItem|createDirectory))"),
        "lane_authority_v2::filesystem_adapter",
        "filesystem.registered_effect",
        "C7",
    ),
    Pattern(
        "process_or_signal_mutation",
        "write",
        re.compile(r"(?:Command::new|std::process::Command|subprocess\.|os\.kill|libc::kill|Process\s*\(|\.terminate\s*\(|\.interrupt\s*\(|kill\s+-)"),
        "lane_authority_v2::process_adapter",
        "process.registered_effect",
        "C7",
    ),
    Pattern(
        "git_mutation_or_discovery",
        "read_write_discovery",
        re.compile(r"(?:\bgit\b|GitCommand|worktree|refs/|rev-parse|ls-remote|merge-base|cherry-pick|rebase|push|fetch)"),
        "lane_authority_v2::git_adapter",
        "git.registered_effect_or_raw_object_read",
        "C7",
    ),
    Pattern(
        "provider_mutation",
        "write",
        re.compile(r"(?:\.post\s*\(|\.patch\s*\(|\.delete\s*\(|mutation\s+[A-Za-z_]|create_comment|update_issue|add_issue|remove_issue|archive|merge_pull|close_pull)", re.I),
        "lane_authority_v2::provider_adapter",
        "provider.registered_effect",
        "C7",
    ),
    Pattern(
        "provider_authority_read",
        "read_discovery",
        re.compile(r"(?:get_issue|list_issues|list_comments|pull_request|review|labels_complete|updated_at|pageInfo|hasNextPage)", re.I),
        "lane_authority_v2::provider_adapter",
        "provider.versioned_readback",
        "C7",
    ),
    Pattern(
        "authority_state_or_path",
        "read_write_discovery",
        re.compile(r"(?:lease|attempt|lane|worktree|review_lifecycle|execution_program|private_event|activity_marker|terminal_guard|run_control|closeout|supersed|\.codex|decodex)", re.I),
        "lane_authority_v2::authority_adapter",
        "runtime.typed_authority_or_diagnostic",
        "C7",
    ),
    Pattern(
        "credential_config_or_automation",
        "read_write_discovery",
        re.compile(r"(?:credential|token|auth|config|keychain|secret service|automation|launchd|systemd)", re.I),
        "lane_authority_v2::host_adapter",
        "host.registered_effect_or_attestation",
        "C7",
    ),
)

LAUNCH_PATTERN = re.compile(
    r"(?:\bdecodex\b|Command::new|std::process::Command|subprocess\.|Process\s*\(|launchd|systemd|automation)",
    re.I,
)
SCENARIO_PATTERN = re.compile(
    r"^\|\s*((?:ID|MIG|QUA|ADM|EFX|SUP|TEL|ADJ)-\d{2})\s*\|\s*(C\d[A-Z]?)\s*\|"
)


def run_git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=root, check=True, text=True, stdout=subprocess.PIPE
    )
    return result.stdout


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_digest(records: list[tuple[str, str]]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-source-tree/1\0")
    for path, content_digest in sorted(records):
        path_bytes = path.encode("utf-8")
        digest.update(len(path_bytes).to_bytes(4, "big"))
        digest.update(path_bytes)
        digest.update(bytes.fromhex(content_digest))
    return digest.hexdigest()


def candidate_digest(records: list[tuple[int, str]]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-candidates/1\0")
    for line_number, line_digest in records:
        digest.update(line_number.to_bytes(4, "big"))
        digest.update(bytes.fromhex(line_digest))
    return digest.hexdigest()


def language_for(path: str) -> str:
    suffix = Path(path).suffix
    return {
        ".rs": "rust",
        ".py": "python",
        ".swift": "swift",
        ".sh": "shell",
        ".bash": "shell",
        ".zsh": "shell",
        ".toml": "toml",
        ".yml": "yaml",
        ".yaml": "yaml",
    }[suffix]


def is_test_path(path: str) -> bool:
    parts = Path(path).parts
    return "tests" in parts or "fixtures" in parts or Path(path).name.startswith("test_")


def baseline_files(root: Path, baseline: str) -> list[str]:
    output = run_git(root, "ls-tree", "-r", "--name-only", baseline, "--", *SOURCE_ROOTS)
    return [
        path
        for path in output.splitlines()
        if Path(path).suffix in SOURCE_SUFFIXES and path not in GENERATED_PATHS
    ]


def ignored_source_files(root: Path) -> set[str]:
    output = run_git(
        root,
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--",
        *SOURCE_ROOTS,
    )
    return {
        path
        for path in output.splitlines()
        if Path(path).suffix in SOURCE_SUFFIXES and path not in GENERATED_PATHS
    }


def validate_c0_scope(root: Path, baseline: str) -> None:
    subprocess.run(
        ["git", "merge-base", "--is-ancestor", baseline, "HEAD"], cwd=root, check=True
    )
    changed = set(run_git(root, "diff", "--name-only", baseline, "--").splitlines())
    untracked = set(
        run_git(root, "ls-files", "--others", "--exclude-standard").splitlines()
    )
    unexpected = sorted((changed | untracked) - C0_ALLOWED_PATHS)
    if unexpected:
        rendered = "\n".join(f"  {path}" for path in unexpected)
        raise ValueError(f"C0 contains paths outside the frozen scope:\n{rendered}")
    ignored_sources = sorted(ignored_source_files(root))
    if ignored_sources:
        rendered = "\n".join(f"  {path}" for path in ignored_sources)
        raise ValueError(
            "ignored untracked source/config paths evade the frozen baseline:\n"
            f"{rendered}"
        )


def inspect_sources(
    root: Path, baseline: str
) -> tuple[
    dict[str, object],
    list[dict[str, object]],
    list[dict[str, object]],
    list[list[object]],
]:
    file_records: list[tuple[str, str]] = []
    root_records: dict[str, list[tuple[str, str]]] = {source_root: [] for source_root in SOURCE_ROOTS}
    authority_nodes: list[dict[str, object]] = []
    launcher_nodes: list[dict[str, object]] = []
    source_files: list[list[object]] = []

    for path in baseline_files(root, baseline):
        content = (root / path).read_bytes()
        content_digest = sha256_bytes(content)
        file_records.append((path, content_digest))
        source_root = next(item for item in SOURCE_ROOTS if path == item or path.startswith(f"{item}/"))
        root_records[source_root].append((path, content_digest))
        language = language_for(path)
        scope = "test" if is_test_path(path) else "production"
        source_files.append([path, language, scope, content_digest])
        text = content.decode("utf-8", errors="replace")

        pattern_hits: dict[Pattern, list[tuple[int, str]]] = {pattern: [] for pattern in PATTERNS}
        launcher_hits: list[tuple[int, str]] = []
        for line_number, line in enumerate(text.splitlines(), 1):
            line_digest = sha256_bytes(line.encode("utf-8"))
            for pattern in PATTERNS:
                if pattern.expression.search(line):
                    pattern_hits[pattern].append((line_number, line_digest))
            if LAUNCH_PATTERN.search(line):
                launcher_hits.append((line_number, line_digest))

        for pattern, hits in pattern_hits.items():
            if not hits:
                continue
            node_key = f"{path}\0{pattern.category}".encode("utf-8")
            authority_nodes.append(
                {
                    "access": pattern.access,
                    "adapter_owner": pattern.adapter_owner,
                    "candidate_digest": candidate_digest(hits),
                    "candidate_line_count": len(hits),
                    "category": pattern.category,
                    "first_line": hits[0][0],
                    "language": language,
                    "mandatory_removal_checkpoint": pattern.removal_checkpoint,
                    "path": path,
                    "replacement_kind": pattern.replacement_kind,
                    "runtime_generation": "v12_legacy",
                    "scope": scope,
                    "source_node_id": sha256_bytes(node_key),
                }
            )
        if launcher_hits:
            node_key = f"launcher\0{path}".encode("utf-8")
            launcher_nodes.append(
                {
                    "candidate_digest": candidate_digest(launcher_hits),
                    "candidate_line_count": len(launcher_hits),
                    "first_line": launcher_hits[0][0],
                    "language": language,
                    "path": path,
                    "runtime_generation": "v12_legacy",
                    "scope": scope,
                    "source_node_id": sha256_bytes(node_key),
                    "v2_owner": "lane_authority_v2::version_pinned_supervisor",
                }
            )

    source = {
        "baseline_commit": baseline,
        "file_count": len(file_records),
        "root_digests": {
            source_root: canonical_digest(records)
            for source_root, records in sorted(root_records.items())
        },
        "source_tree_digest": canonical_digest(file_records),
    }
    authority_nodes.sort(key=lambda item: (str(item["path"]), str(item["category"])))
    launcher_nodes.sort(key=lambda item: str(item["path"]))
    return source, authority_nodes, launcher_nodes, source_files


def scenario_records(root: Path) -> list[dict[str, str]]:
    scenarios: list[dict[str, str]] = []
    seen: set[str] = set()
    for line in (root / SCENARIO_SPEC).read_text(encoding="utf-8").splitlines():
        match = SCENARIO_PATTERN.match(line)
        if match is None:
            continue
        scenario_id, checkpoint = match.groups()
        if scenario_id in seen:
            raise ValueError(f"duplicate scenario id: {scenario_id}")
        seen.add(scenario_id)
        test_name = f"lane_authority_v2_{checkpoint.lower()}_{scenario_id.lower().replace('-', '_')}"
        scenarios.append(
            {"checkpoint": checkpoint, "id": scenario_id, "test_name": test_name}
        )
    scenarios.sort(key=lambda item: item["id"])
    if not scenarios:
        raise ValueError("no Lane Authority v2 scenarios found")
    return scenarios


def scenario_freeze_digest(scenarios: list[dict[str, str]]) -> str:
    digest = hashlib.sha256(b"decodex/lane-authority-v2-scenario-freeze/1\0")
    for scenario in sorted(scenarios, key=lambda item: item["id"]):
        for key in ("id", "checkpoint", "test_name"):
            value = scenario[key].encode("utf-8")
            digest.update(len(value).to_bytes(4, "big"))
            digest.update(value)
    return digest.hexdigest()


def validate_scenario_freeze(
    scenarios: list[dict[str, str]],
    *,
    expected_count: int = SCENARIO_FREEZE_COUNT,
    expected_digest: str = SCENARIO_FREEZE_DIGEST,
) -> str:
    actual_digest = scenario_freeze_digest(scenarios)
    if len(scenarios) != expected_count or actual_digest != expected_digest:
        raise ValueError(
            "scenario freeze mismatch: "
            f"expected count={expected_count} digest={expected_digest}; "
            f"actual count={len(scenarios)} digest={actual_digest}"
        )
    return actual_digest


def scenario_manifest(root: Path, baseline: str, source_digest: str) -> dict[str, object]:
    scenarios = scenario_records(root)
    freeze_digest = validate_scenario_freeze(scenarios)
    return {
        "baseline_commit": baseline,
        "scenarios": scenarios,
        "schema": "decodex/lane-authority-v2-scenario-manifest/1",
        "scenario_freeze_digest": freeze_digest,
        "source_tree_digest": source_digest,
    }


def run_self_tests(root: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="lane-authority-v2-") as temporary:
        fixture = Path(temporary)
        subprocess.run(["git", "init", "--quiet"], cwd=fixture, check=True)
        (fixture / "scripts").mkdir()
        (fixture / ".gitignore").write_text("scripts/probe.py\n", encoding="utf-8")
        (fixture / "scripts/probe.py").write_text("probe = True\n", encoding="utf-8")
        if "scripts/probe.py" not in ignored_source_files(fixture):
            raise AssertionError("ignored source/config self-test did not detect probe")

    scenarios = scenario_records(root)
    validate_scenario_freeze(scenarios)
    mutated = [dict(item) for item in scenarios]
    mutated[0]["checkpoint"] = "C0" if mutated[0]["checkpoint"] != "C0" else "C7"
    try:
        validate_scenario_freeze(mutated)
    except ValueError:
        pass
    else:
        raise AssertionError("scenario freeze self-test accepted checkpoint drift")
    print("verified ignored-source and scenario-freeze negative controls")


def expand_effect_kinds(kind_cell: str) -> list[str]:
    tokens = re.findall(r"`([^`]+)`", kind_cell)
    if not tokens:
        return []
    base = tokens[0]
    prefix = base.rsplit(".", 1)[0]
    return [token if "." in token else f"{prefix}.{token}" for token in tokens]


def split_markdown_table_row(line: str) -> list[str]:
    cells: list[str] = []
    current: list[str] = []
    in_code = False
    for character in line.strip().strip("|"):
        if character == "`":
            in_code = not in_code
            current.append(character)
        elif character == "|" and not in_code:
            cells.append("".join(current).strip())
            current = []
        else:
            current.append(character)
    cells.append("".join(current).strip())
    return cells


def effect_kind_registry(root: Path) -> list[dict[str, object]]:
    effects: list[dict[str, object]] = []
    seen: set[str] = set()
    for line in (root / EFFECT_SPEC).read_text(encoding="utf-8").splitlines():
        if not line.startswith("| `"):
            continue
        cells = split_markdown_table_row(line)
        if len(cells) != 4:
            raise ValueError(f"invalid effect registry row: {line}")
        kind_cell, class_cell, readback, stop_rule = cells
        effect_kinds = expand_effect_kinds(kind_cell)
        if not effect_kinds:
            continue
        class_tokens = re.findall(r"`([^`]+)`", class_cell)
        if len(class_tokens) == 1:
            semantic_classes = [class_cell.replace("`", "")] * len(effect_kinds)
        elif len(class_tokens) == len(effect_kinds):
            semantic_classes = class_tokens
        else:
            raise ValueError(
                f"effect alias/class cardinality mismatch: {kind_cell} | {class_cell}"
            )
        for effect_kind, semantic_class in zip(effect_kinds, semantic_classes, strict=True):
            if effect_kind in seen:
                raise ValueError(f"duplicate effect kind: {effect_kind}")
            seen.add(effect_kind)
            target = effect_kind.split(".", 1)[0]
            provider_requirement = "not_applicable"
            if target in {"linear", "github"}:
                provider_requirement = (
                    "exact_provider_capability_and_preconditions_in_class_readback_stop_rule"
                )
            effects.append(
                {
                    "adapter_owner": f"lane_authority_v2::{target}_adapter",
                    "compensation_class": semantic_class,
                    "compensation_stop_rule": stop_rule,
                    "desired_state_readback": readback,
                    "kind": effect_kind,
                    "mandatory_removal_checkpoint": "retained_v2",
                    "provider_capability_requirement": provider_requirement,
                    "reconciliation_policy": "desired_state_readback_before_retry_then_apply_stop_rule",
                    "replacement_kind": effect_kind,
                    "replacement_owner": f"lane_authority_v2::{target}_adapter",
                    "runtime_generation": "v2",
                    "semantic_digest": sha256_bytes(
                        "\0".join(
                            [effect_kind, semantic_class, readback, stop_rule, provider_requirement]
                        ).encode("utf-8")
                    ),
                }
            )
    effects.sort(key=lambda item: str(item["kind"]))
    if not effects:
        raise ValueError("no normative effect kinds found")
    return effects


def group_authority_nodes(
    nodes: list[dict[str, object]], *, mutation_only: bool
) -> list[dict[str, object]]:
    grouped: dict[str, dict[str, object]] = {}
    for node in nodes:
        if mutation_only and node["access"] == "read_discovery":
            continue
        path = str(node["path"])
        entry = grouped.setdefault(
            path,
            {
                "classifications": [],
                "language": node["language"],
                "path": path,
                "scope": node["scope"],
                "source_file_id": sha256_bytes(f"source-file\0{path}".encode("utf-8")),
            },
        )
        classifications = entry["classifications"]
        assert isinstance(classifications, list)
        classifications.append(
            [
                node["category"],
                node["candidate_line_count"],
                node["first_line"],
                node["candidate_digest"],
                node["source_node_id"],
            ]
        )
    return [grouped[path] for path in sorted(grouped)]


def manifests(root: Path, baseline: str) -> dict[str, dict[str, object]]:
    source, authority_nodes, launcher_nodes, source_files = inspect_sources(root, baseline)
    effect_kinds = effect_kind_registry(root)
    category_definitions = {
        pattern.category: {
            "access": pattern.access,
            "adapter_owner": pattern.adapter_owner,
            "mandatory_removal_checkpoint": pattern.removal_checkpoint,
            "replacement_kind": pattern.replacement_kind,
            "runtime_generation": "v12_legacy",
        }
        for pattern in PATTERNS
    }
    return {
        "apps/decodex/src/bootstrap/tests/fixtures/lane_authority_v2/launcher_inventory.json": {
            "baseline": source,
            "entries": launcher_nodes,
            "schema": "decodex/lane-authority-v2-launcher-inventory/1",
        },
        "apps/decodex/src/state/tests/fixtures/lane_authority_v2/legacy_authority_inventory.json": {
            "baseline": source,
            "category_definitions": category_definitions,
            "classification_tuple": [
                "category",
                "candidate_line_count",
                "first_line",
                "candidate_digest",
                "source_node_id",
            ],
            "source_file_tuple": ["path", "language", "scope", "sha256"],
            "source_files": source_files,
            "nodes": group_authority_nodes(authority_nodes, mutation_only=False),
            "schema": "decodex/lane-authority-v2-legacy-authority-inventory/1",
        },
        "apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json": {
            "baseline": source,
            "category_definitions": category_definitions,
            "classification_tuple": [
                "category",
                "candidate_line_count",
                "first_line",
                "candidate_digest",
                "source_node_id",
            ],
            "effect_kinds": effect_kinds,
            "entries": group_authority_nodes(authority_nodes, mutation_only=True),
            "schema": "decodex/lane-authority-v2-mutation-registry/1",
        },
        "apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json": scenario_manifest(
            root, baseline, str(source["source_tree_digest"])
        ),
    }


def encoded(value: dict[str, object]) -> bytes:
    collection_key = "entries" if "entries" in value else "nodes" if "nodes" in value else None
    if collection_key is None:
        return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    collection = value[collection_key]
    assert isinstance(collection, list)
    def compact(item: object) -> str:
        return json.dumps(item, separators=(",", ":"), sort_keys=True)
    lines = ["{"]
    for key in sorted(key for key in value if key != collection_key):
        lines.append(f"{json.dumps(key)}:{compact(value[key])},")
    lines.append(f"{json.dumps(collection_key)}:[")
    for index, item in enumerate(collection):
        suffix = "," if index + 1 < len(collection) else ""
        lines.append(f"{compact(item)}{suffix}")
    lines.extend(["]", "}"])
    return ("\n".join(lines) + "\n").encode("utf-8")


def write_manifests(root: Path, values: dict[str, dict[str, object]]) -> None:
    for relative_path, value in values.items():
        path = root / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(encoded(value))
        print(f"wrote {relative_path}")


def verify_manifests(root: Path, values: dict[str, dict[str, object]]) -> None:
    failed = False
    for relative_path, value in values.items():
        path = root / relative_path
        expected = encoded(value)
        actual = path.read_bytes() if path.exists() else b""
        if actual == expected:
            print(f"verified {relative_path}")
            continue
        failed = True
        print(f"stale or missing: {relative_path}", file=sys.stderr)
    if failed:
        raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", default=BASELINE_DEFAULT)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    root = Path(run_git(Path.cwd(), "rev-parse", "--show-toplevel").strip())
    if args.self_test:
        run_self_tests(root)
        return
    baseline = run_git(root, "rev-parse", args.baseline).strip()
    validate_c0_scope(root, baseline)
    values = manifests(root, baseline)
    if args.write:
        write_manifests(root, values)
    else:
        verify_manifests(root, values)


if __name__ == "__main__":
    main()
