#!/usr/bin/env python3
"""Attest and compare the read-only Rust vstyle audit against its accepted baseline."""

from collections import Counter
from datetime import date
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


REPO_ROOT = Path(__file__).resolve().parents[1]
CONTRACT_PATH = REPO_ROOT / "config" / "vstyle-rust-audit.json"
FINDING_PATTERN = re.compile(
    r"^(?P<path>.+?):(?P<line>[1-9][0-9]*):(?P<column>[1-9][0-9]*): "
    r"\[(?P<rule>RUST-STYLE-[A-Z0-9-]+)\] (?P<message>.+)$"
)
CHECKED_PATTERN = re.compile(r"^Checked (?P<count>[0-9]+) file\(s\)\.$")
MANUAL_PATTERN = re.compile(r"^(?P<count>[0-9]+) violation\(s\) require manual fixes\.$")
FOUND_PATTERN = re.compile(r"^Found (?P<count>[0-9]+) style violation\(s\)\.$")


class AuditError(RuntimeError):
    """The audit input or provenance did not satisfy the closed contract."""


def load_contract(path=CONTRACT_PATH):
    """Load the checked-in audit contract."""
    with Path(path).open(encoding="utf-8") as contract_file:
        contract = json.load(contract_file)
    if contract.get("schema") != "decodex/vstyle-rust-audit/1":
        raise AuditError("unsupported vstyle audit contract schema")
    tool = contract["tool"]
    if not re.fullmatch(r"[0-9a-f]{40}", tool["commit"]):
        raise AuditError("vstyle source commit is not a full Git object ID")
    if tool["git_short"] != tool["commit"][:7] or tool["commit"] not in tool["install"]:
        raise AuditError("vstyle install and version identities do not match the pinned commit")
    if contract["rust_rules"] != sorted(set(contract["rust_rules"])):
        raise AuditError("vstyle Rust rule contract is not sorted and unique")
    baseline = baseline_counter(contract)
    accepted = contract["accepted_baseline"]
    findings = sum(baseline.values())
    manual = sum(count for (*_, fixable), count in baseline.items() if not fixable)
    if findings != accepted["findings"] or manual != accepted["manual"]:
        raise AuditError(
            "checked-in vstyle baseline summary mismatch: "
            f"normalized={findings}/{manual}, accepted={accepted['findings']}/{accepted['manual']}"
        )
    return contract


def validate_governance(contract, today=None):
    """Fail when the accepted baseline has passed its mandatory review date."""
    review_by = date.fromisoformat(contract["governance"]["review_by"])
    if (today or date.today()) > review_by:
        raise AuditError(f"vstyle audit baseline review expired on {review_by.isoformat()}")


def parse_host(rustc_output):
    """Extract the active Rust host triple."""
    for line in rustc_output.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    raise AuditError("rustc did not report a host triple")


def validate_version(actual, contract, host):
    """Attest the exact package version, source revision, and build target."""
    tool = contract["tool"]
    expected = f"vibe-style {tool['version']}-{tool['git_short']}-{host}"
    if actual.strip() != expected:
        raise AuditError(f"vstyle identity mismatch: expected {expected!r}, got {actual.strip()!r}")


def parse_coverage(output):
    """Normalize the implemented Rust rule inventory."""
    rules = []
    for line in output.splitlines():
        if not line:
            continue
        fields = line.split("\t")
        if len(fields) != 2 or fields[1] != "implemented":
            raise AuditError(f"unexpected vstyle coverage output: {line!r}")
        if fields[0].startswith("RUST-"):
            rules.append(fields[0])
    if len(rules) != len(set(rules)):
        raise AuditError("vstyle coverage contains duplicate Rust rules")
    return sorted(rules)


def validate_rules(actual, contract):
    """Attest the complete implemented Rust rule inventory."""
    expected = sorted(contract["rust_rules"])
    if actual != expected:
        missing = sorted(set(expected) - set(actual))
        added = sorted(set(actual) - set(expected))
        raise AuditError(f"vstyle Rust rule inventory mismatch: missing={missing}, added={added}")


def finding_key(path, rule, message, fixable):
    """Build a location-stable finding signature."""
    return (path, rule, message, fixable)


def parse_curate(output, allowed_rules):
    """Parse vstyle's text boundary and normalize findings as a multiset."""
    findings = Counter()
    checked_files = None
    manual_summary = 0
    found_summary = None
    for line in output.splitlines():
        if not line:
            continue
        match = FINDING_PATTERN.match(line)
        if match:
            path = PurePosixPath(match["path"])
            if path.is_absolute() or ".." in path.parts:
                raise AuditError(f"vstyle reported an unsafe path: {match['path']!r}")
            if match["rule"] not in allowed_rules:
                raise AuditError(f"vstyle reported an unattested rule: {match['rule']}")
            message = match["message"]
            fixable = message.endswith(" (fixable)")
            if fixable:
                message = message.removesuffix(" (fixable)")
            findings[finding_key(path.as_posix(), match["rule"], message, fixable)] += 1
            continue
        match = CHECKED_PATTERN.match(line)
        if match:
            checked_files = int(match["count"])
            continue
        match = MANUAL_PATTERN.match(line)
        if match:
            manual_summary = int(match["count"])
            continue
        match = FOUND_PATTERN.match(line)
        if match:
            found_summary = int(match["count"])
            continue
        raise AuditError(f"unexpected vstyle curate output: {line!r}")
    total = sum(findings.values())
    manual = sum(count for (*_, fixable), count in findings.items() if not fixable)
    if checked_files is None or found_summary is None:
        raise AuditError("vstyle curate output omitted its summary")
    if total != found_summary or manual != manual_summary:
        raise AuditError(
            "vstyle curate summary mismatch: "
            f"parsed total/manual={total}/{manual}, reported={found_summary}/{manual_summary}"
        )
    return findings, {"checked_files": checked_files, "total": total, "manual": manual}


def baseline_counter(contract):
    """Load the reviewed baseline as a multiset."""
    baseline = Counter()
    for finding in contract["baseline"]:
        key = finding_key(
            finding["path"],
            finding["rule"],
            finding["message"],
            finding["fixable"],
        )
        baseline[key] += finding["count"]
    return baseline


def compare_findings(current, baseline):
    """Return new regressions and resolved baseline findings."""
    return current - baseline, baseline - current


def render_delta(prefix, delta):
    """Render a deterministic normalized delta."""
    lines = []
    for (path, rule, message, fixable), count in sorted(delta.items()):
        disposition = "fixable" if fixable else "manual"
        lines.append(f"{prefix} {count}x {path} [{rule}] ({disposition}) {message}")
    return lines


def run(command):
    """Run one read-only tool command from the repository root."""
    return subprocess.run(
        command,
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def audit():
    """Execute the closed read-only audit contract."""
    contract = load_contract()
    validate_governance(contract)

    rustc = run(["rustc", "-vV"])
    if rustc.returncode != 0:
        raise AuditError(f"rustc host query failed: {rustc.stderr.strip()}")
    host = parse_host(rustc.stdout)

    version = run(["cargo", "vstyle", "--version"])
    if version.returncode != 0:
        raise AuditError(f"vstyle version query failed: {version.stderr.strip()}")
    validate_version(version.stdout, contract, host)

    coverage = run(["cargo", "vstyle", "coverage"])
    if coverage.returncode != 0:
        raise AuditError(f"vstyle coverage query failed: {coverage.stderr.strip()}")
    rules = parse_coverage(coverage.stdout)
    validate_rules(rules, contract)

    curate = run(
        [
            "cargo",
            "vstyle",
            "curate",
            "--language",
            "rust",
            "--workspace",
            "--all-features",
        ]
    )
    if curate.returncode not in {0, 1}:
        raise AuditError(f"vstyle curate failed unexpectedly: {curate.stderr.strip()}")
    findings, summary = parse_curate(curate.stdout + curate.stderr, set(rules))
    if summary["checked_files"] < contract["accepted_baseline"]["checked_files"]:
        raise AuditError(
            "vstyle audit scope shrank: "
            f"checked {summary['checked_files']} files, "
            f"accepted at least {contract['accepted_baseline']['checked_files']}"
        )
    added, resolved = compare_findings(findings, baseline_counter(contract))

    print(
        "vstyle audit: "
        f"{version.stdout.strip()}; {len(rules)} Rust rules; "
        f"{summary['checked_files']} files; {summary['total']} findings; "
        f"{summary['manual']} manual"
    )
    for line in render_delta("-", resolved):
        print(line)
    for line in render_delta("+", added):
        print(line)
    if added:
        print(f"vstyle audit: FAILED with {sum(added.values())} new regression(s)")
        return 1
    if resolved:
        print(
            "vstyle audit: baseline has "
            f"{sum(resolved.values())} resolved finding(s); reviewed refresh required"
        )
    else:
        print("vstyle audit: accepted baseline matched exactly")
    return 0


def main():
    """CLI entrypoint."""
    try:
        return audit()
    except (AuditError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"vstyle audit: provenance or contract failure: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
