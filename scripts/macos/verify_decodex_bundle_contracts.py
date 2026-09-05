#!/usr/bin/env python3
"""Verify the live executable and ABI contracts in a staged Decodex bundle."""

from __future__ import annotations

import argparse
import ctypes
import json
import plistlib
import re
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


def executable_identity(service_path: Path) -> dict[str, object]:
	try:
		completed = subprocess.run(
			[str(service_path), "--output", "json", "build-info"],
			check=True,
			capture_output=True,
			text=True,
		)
	except (OSError, subprocess.CalledProcessError) as error:
		raise ContractError("service version is unavailable") from error
	try:
		document = json.loads(completed.stdout)
	except json.JSONDecodeError as error:
		raise ContractError("service version is malformed") from error
	if (
		not isinstance(document, dict)
		or set(document) != {"schema", "version", "commit", "dirty"}
		or document.get("schema") != "decodex/build-info/1"
		or not isinstance(document.get("version"), str)
		or not document["version"]
		or not isinstance(document.get("commit"), str)
		or re.fullmatch(r"[0-9a-f]{40}", document["commit"]) is None
		or not isinstance(document.get("dirty"), bool)
	):
		raise ContractError("service version is malformed")
	return document


def read_app_info(info_path: Path) -> dict[str, object]:
	try:
		with info_path.open("rb") as info_file:
			document = plistlib.load(info_file)
	except (OSError, plistlib.InvalidFileException) as error:
		raise ContractError("application version is unavailable") from error
	if not isinstance(document, dict):
		raise ContractError("application version is malformed")
	return document


def stamp_app_identity(info_path: Path, identity: dict[str, object]) -> None:
	document = read_app_info(info_path)
	document["CFBundleShortVersionString"] = identity["version"]
	document["DecodexBuildCommit"] = identity["commit"]
	document["DecodexBuildDirty"] = identity["dirty"]
	try:
		with info_path.open("wb") as info_file:
			plistlib.dump(document, info_file, fmt=plistlib.FMT_XML, sort_keys=True)
	except OSError as error:
		raise ContractError("application version could not be stamped") from error


def verify(
	service_path: Path,
	app_info_path: Path,
	native_client_path: Path,
	menu_bar_path: Path,
) -> None:
	identity = executable_identity(service_path)
	app_info = read_app_info(app_info_path)
	if (
		app_info.get("CFBundleShortVersionString") != identity["version"]
		or app_info.get("DecodexBuildCommit") != identity["commit"]
		or app_info.get("DecodexBuildDirty") != identity["dirty"]
	):
		raise ContractError("application and service build identities differ")
	native_abi = exported_u32(
		native_client_path, "decodex_app_native_client_abi_version"
	)
	if native_abi != NATIVE_CLIENT_ABI_VERSION:
		raise ContractError("native client ABI does not match the app")
	menu_bar_abi = exported_u32(menu_bar_path, "decodex_menu_bar_abi_version")
	if menu_bar_abi != MENU_BAR_ABI_VERSION:
		raise ContractError("menu-bar ABI does not match the app")


def main() -> int:
	parser = argparse.ArgumentParser()
	parser.add_argument("--service", required=True, type=Path)
	parser.add_argument("--app-info", required=True, type=Path)
	parser.add_argument("--native-client", required=True, type=Path)
	parser.add_argument("--menu-bar", required=True, type=Path)
	parser.add_argument("--stamp-app-info", action="store_true")
	args = parser.parse_args()
	try:
		if args.stamp_app_info:
			stamp_app_identity(args.app_info, executable_identity(args.service))
		verify(args.service, args.app_info, args.native_client, args.menu_bar)
	except ContractError as error:
		print(f"Decodex bundle contract check failed: {error}", file=sys.stderr)
		return 1
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
