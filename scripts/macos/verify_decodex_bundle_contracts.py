#!/usr/bin/env python3
"""Verify the live compatibility exports in a staged Decodex bundle."""

from __future__ import annotations

import argparse
import ctypes
import json
import subprocess
import sys
from pathlib import Path


NATIVE_CLIENT_ABI_VERSION = 1
MENU_BAR_ABI_VERSION = 1


class ContractError(RuntimeError):
	"""The staged native components do not implement one compatible contract."""


def exported_u32(library_path: Path, symbol_name: str) -> int:
	try:
		library = ctypes.CDLL(str(library_path))
	except OSError as error:
		raise ContractError(f"cannot load {library_path.name}: {error}") from error
	try:
		symbol = getattr(library, symbol_name)
	except AttributeError as error:
		raise ContractError(
			f"{library_path.name} does not export {symbol_name}"
		) from error
	symbol.argtypes = []
	symbol.restype = ctypes.c_uint32
	return int(symbol())


def daemon_artifact_cohort(daemon_path: Path) -> int:
	try:
		completed = subprocess.run(
			[str(daemon_path), "artifact-cohort"],
			check=True,
			capture_output=True,
			text=True,
		)
	except (OSError, subprocess.CalledProcessError) as error:
		raise ContractError("daemon artifact contract is unavailable") from error
	try:
		document = json.loads(completed.stdout)
		cohort = document["artifact_cohort"]
	except (json.JSONDecodeError, KeyError, TypeError) as error:
		raise ContractError("daemon artifact contract is malformed") from error
	if not isinstance(cohort, int) or isinstance(cohort, bool) or cohort < 1:
		raise ContractError("daemon artifact contract is malformed")
	return cohort


def verify(daemon_path: Path, native_client_path: Path, menu_bar_path: Path) -> None:
	daemon_cohort = daemon_artifact_cohort(daemon_path)
	native_abi = exported_u32(
		native_client_path, "decodex_app_native_client_abi_version"
	)
	if native_abi != NATIVE_CLIENT_ABI_VERSION:
		raise ContractError("native client ABI does not match the app")
	native_cohort = exported_u32(
		native_client_path, "decodex_app_native_client_artifact_cohort"
	)
	if native_cohort != daemon_cohort:
		raise ContractError("native client and daemon artifact contracts differ")
	menu_bar_abi = exported_u32(menu_bar_path, "decodex_menu_bar_abi_version")
	if menu_bar_abi != MENU_BAR_ABI_VERSION:
		raise ContractError("menu-bar ABI does not match the app")


def main() -> int:
	parser = argparse.ArgumentParser()
	parser.add_argument("--daemon", required=True, type=Path)
	parser.add_argument("--native-client", required=True, type=Path)
	parser.add_argument("--menu-bar", required=True, type=Path)
	args = parser.parse_args()
	try:
		verify(args.daemon, args.native_client, args.menu_bar)
	except ContractError as error:
		print(f"Decodex bundle contract check failed: {error}", file=sys.stderr)
		return 1
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
