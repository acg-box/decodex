#!/usr/bin/env python3
"""Run the workspace Clippy contract one package at a time."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Sequence


HEADLESS_EXCLUDED_PACKAGE = "decodex-gpui"
DENY_FLAGS = (
	"-D",
	"clippy::all",
	"-D",
	"clippy::too_many_lines",
	"-D",
	"clippy::unwrap_used",
	"-D",
	"clippy::use_self",
	"-D",
	"clippy::wildcard_imports",
	"-D",
	"missing-docs",
	"-D",
	"unused-crate-dependencies",
	"-D",
	"warnings",
)


def workspace_package_names(root: Path) -> list[str]:
	"""Return all workspace package names in deterministic order."""
	command = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
	completed = subprocess.run(
		command,
		cwd=root,
		check=False,
		stdout=subprocess.PIPE,
		text=True,
	)
	if completed.returncode != 0:
		raise RuntimeError(f"cargo metadata failed with exit code {completed.returncode}")

	metadata = json.loads(completed.stdout)
	package_names = {package["id"]: package["name"] for package in metadata["packages"]}
	return sorted(package_names[package_id] for package_id in metadata["workspace_members"])


def clippy_command(package_name: str) -> list[str]:
	"""Build the repository Clippy command for one package."""
	return [
		"cargo",
		"clippy",
		"--package",
		package_name,
		"--all-features",
		"--all-targets",
		"--keep-going",
		"--",
		"--no-deps",
		*DENY_FLAGS,
	]


def lint_workspace(root: Path, *, headless: bool) -> int:
	"""Lint selected workspace packages and aggregate their results."""
	package_names = workspace_package_names(root)
	if headless:
		package_names = [
			package_name
			for package_name in package_names
			if package_name != HEADLESS_EXCLUDED_PACKAGE
		]

	results: list[tuple[str, str, bool]] = []
	package_count = len(package_names)
	for index, package_name in enumerate(package_names, start=1):
		print(f"==> Rust lint [{index}/{package_count}]: {package_name}", flush=True)
		try:
			completed = subprocess.run(
				clippy_command(package_name),
				cwd=root,
				check=False,
				stderr=subprocess.STDOUT,
			)
		except OSError as error:
			status = f"SPAWN FAILURE ({error})"
			failed = True
		else:
			status = "PASS" if completed.returncode == 0 else f"FAIL (exit {completed.returncode})"
			failed = completed.returncode != 0
		results.append((package_name, status, failed))
		print(f"<== Rust lint [{index}/{package_count}]: {package_name}: {status}", flush=True)

	print("Rust lint summary:")
	for package_name, status, _ in results:
		print(f"  {package_name}: {status}")

	failed_count = sum(failed for _, _, failed in results)
	passed_count = package_count - failed_count
	print(f"Rust lint result: {passed_count} passed; {failed_count} failed; {package_count} total.")
	return int(failed_count != 0)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
	"""Parse the full-workspace or headless selection boundary."""
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument(
		"--headless",
		action="store_true",
		help="exclude only decodex-gpui from the workspace lint",
	)
	return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
	"""Run the workspace lint owner."""
	args = parse_args(argv)
	root = Path(__file__).resolve().parents[1]
	try:
		return lint_workspace(root, headless=args.headless)
	except OSError as error:
		print(f"Rust lint setup failed: cargo metadata spawn failure: {error}", file=sys.stderr)
		return 2
	except (json.JSONDecodeError, KeyError, RuntimeError, TypeError) as error:
		print(f"Rust lint setup failed: {error}", file=sys.stderr)
		return 2


if __name__ == "__main__":
	raise SystemExit(main())
